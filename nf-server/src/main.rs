mod certs;
mod notification;
mod tls;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use tracing::Level;

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::{self, error::RecvError};
use tokio_rustls::TlsAcceptor;

use notification::Notification;

/// Forwards desktop notifications to connected clients over mutual TLS.
#[derive(Parser)]
struct Cli {
    /// Address to listen on.
    #[arg(long, default_value = "0.0.0.0")]
    address: IpAddr,

    /// Port to listen on.
    #[arg(short, long, default_value_t = 7878)]
    port: u16,

    /// Log verbosity: error, warn, info, debug or trace.
    #[arg(long, default_value_t = Level::INFO)]
    log_level: Level,

    #[command(flatten)]
    tls: tls::TlsArgs,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a private CA plus server and client certificates for mutual TLS.
    GenCerts {
        /// Directory to write ca.{pem,key}, server.{pem,key} and client.{pem,key} to.
        #[arg(long, default_value = "certs")]
        out: PathBuf,
        /// Validity of the generated certificates, in days.
        #[arg(long, default_value_t = 825)]
        days: u32,
    },
}

/// Logs where clients can reach the server. When listening on all interfaces
/// the bound address alone is not useful, so list the machine's addresses too.
fn log_listening(addr: SocketAddr) {
    tracing::info!("listening on {addr}");
    if !addr.ip().is_unspecified() {
        return;
    }
    let reachable: Vec<String> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|i| i.ip())
        .filter(|ip| ip.is_ipv4() == addr.ip().is_ipv4() && !ip.is_loopback())
        .filter(|ip| !matches!(ip, IpAddr::V6(v6) if v6.segments()[0] & 0xffc0 == 0xfe80))
        .map(|ip| SocketAddr::new(ip, addr.port()).to_string())
        .collect();
    if !reachable.is_empty() {
        tracing::info!("reachable at {}", reachable.join(", "));
    }
}

/// Each notification is sent as one BSON document, which is self-delimiting
/// (its first 4 bytes are its length), so no extra framing is needed.
async fn serve_client(
    stream: TcpStream,
    acceptor: TlsAcceptor,
    mut rx: broadcast::Receiver<Notification>,
) -> Result<()> {
    let mut tls = acceptor.accept(stream).await?;
    loop {
        match rx.recv().await {
            Ok(n) => {
                tls.write_all(&bson::serialize_to_vec(&n)?).await?;
                tls.flush().await?;
            }
            Err(RecvError::Lagged(skipped)) => tracing::warn!("client lagging, dropped {skipped}"),
            Err(RecvError::Closed) => return Ok(()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt().with_max_level(cli.log_level).init();

    if let Some(Command::GenCerts { out, days }) = cli.command {
        certs::generate(&out, days)?;
        println!(
            "Wrote ca.{{pem,key}}, server.{{pem,key}} and client.{{pem,key}} to {}\n\
             Copy ca.pem + server.* to the server host and ca.pem + client.* to each client host.\n\
             Keep ca.key offline; it is not needed by either binary.",
            out.display()
        );
        return Ok(());
    }

    let acceptor = tls::acceptor(&cli.tls)?;
    let listener = TcpListener::bind((cli.address, cli.port)).await?;
    log_listening(listener.local_addr()?);

    let (tx, _) = broadcast::channel(64);
    let mut watcher = tokio::spawn(notification::watch(tx.clone()));

    loop {
        tokio::select! {
            res = &mut watcher => return res?,
            _ = tokio::signal::ctrl_c() => return Ok(()),
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let (acceptor, rx) = (acceptor.clone(), tx.subscribe());
                tokio::spawn(async move {
                    tracing::info!(%peer, "client connected");
                    if let Err(e) = serve_client(stream, acceptor, rx).await {
                        tracing::info!(%peer, "client disconnected: {e:#}");
                    }
                });
            }
        }
    }
}
