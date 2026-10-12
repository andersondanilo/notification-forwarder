mod tls;

use std::time::Duration;

use anyhow::{Result, bail};
use clap::Parser;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tracing::Level;

/// Name the server certificate is verified against, regardless of the address
/// we connect to, so the server's IP or hostname can change freely. Must match
/// `SERVER_NAME` in nf-server.
const SERVER_NAME: &str = "nf-server";

/// Upper bound on a single BSON document, to avoid unbounded allocations.
const MAX_DOC_LEN: usize = 1024 * 1024;

/// Shows notifications forwarded by nf-server as desktop notifications.
#[derive(Parser)]
struct Cli {
    /// IP address or hostname of the server.
    #[arg(long, default_value = "localhost")]
    host: String,

    /// Port of the server.
    #[arg(short, long, default_value_t = 7878)]
    port: u16,

    /// Seconds to wait before reconnecting after the connection drops or fails.
    /// The client retries forever.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    reconnect_interval: u64,

    /// Log verbosity: error, warn, info, debug or trace.
    #[arg(long, default_value_t = Level::INFO)]
    log_level: Level,

    #[command(flatten)]
    tls: tls::TlsArgs,
}

#[derive(Debug, Deserialize)]
struct Notification {
    summary: String,
    body: String,
    icon: String,
    appname: String,
}

/// Reads one length-prefixed BSON document.
async fn read_document(r: &mut (impl AsyncRead + Unpin)) -> Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let total = i32::from_le_bytes(len);
    if !(5..=MAX_DOC_LEN as i32).contains(&total) {
        bail!("invalid BSON document length {total}");
    }
    let mut doc = vec![0u8; total as usize];
    doc[..4].copy_from_slice(&len);
    r.read_exact(&mut doc[4..]).await?;
    Ok(doc)
}

async fn show(n: Notification) {
    let res = tokio::task::spawn_blocking(move || {
        let mut notification = notify_rust::Notification::new();
        notification.summary(&n.summary).body(&n.body);
        // Freedesktop notification daemons show the app name and resolve icon
        // names/paths. Windows and macOS have no equivalent for a foreign app,
        // so show the app name as the subtitle and drop the icon.
        #[cfg(all(unix, not(target_os = "macos")))]
        notification.appname(&n.appname).icon(&n.icon);
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        if !n.appname.is_empty() {
            notification.subtitle(&n.appname);
        }
        notification.show().map(|_| ())
    })
    .await;
    match res {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!("failed to show notification: {e}"),
        Err(e) => tracing::warn!("notification task failed: {e}"),
    }
}

async fn run(
    addr: (&str, u16),
    name: ServerName<'static>,
    connector: &TlsConnector,
) -> Result<()> {
    let stream = TcpStream::connect(addr).await?;
    let mut tls = connector.connect(name, stream).await?;
    tracing::info!("connected to {}:{}", addr.0, addr.1);
    loop {
        let doc = read_document(&mut tls).await?;
        match bson::deserialize_from_slice::<Notification>(&doc) {
            Ok(n) => show(n).await,
            Err(e) => tracing::warn!("ignoring malformed notification: {e}"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt().with_max_level(cli.log_level).init();

    let name = ServerName::try_from(SERVER_NAME)?;
    let connector = tls::connector(&cli.tls)?;
    let addr = (cli.host.as_str(), cli.port);

    let interval = Duration::from_secs(cli.reconnect_interval);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            res = run(addr, name.clone(), &connector) => {
                if let Err(e) = res {
                    tracing::warn!("connection lost: {e:#}, retrying in {interval:?}");
                }
                tokio::time::sleep(interval).await;
            }
        }
    }
}
