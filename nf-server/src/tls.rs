use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Args;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{RootCertStore, ServerConfig};

/// Where to find the TLS files. Each file defaults to a fixed name inside the
/// config directory, which itself defaults to `notification-forwarder` inside
/// the platform config directory (e.g. `~/.config`).
#[derive(Args)]
pub struct TlsArgs {
    /// Directory holding ca.pem, server.pem and server.key.
    #[arg(long)]
    config_dir: Option<PathBuf>,
    /// CA certificate used to verify clients [default: <config-dir>/ca.pem].
    #[arg(long)]
    ca: Option<PathBuf>,
    /// Server certificate [default: <config-dir>/server.pem].
    #[arg(long)]
    cert: Option<PathBuf>,
    /// Server private key [default: <config-dir>/server.key].
    #[arg(long)]
    key: Option<PathBuf>,
}

impl TlsArgs {
    fn resolve(&self, explicit: &Option<PathBuf>, default: &str) -> Result<PathBuf> {
        if let Some(p) = explicit {
            return Ok(p.clone());
        }
        let dir = match &self.config_dir {
            Some(dir) => dir.clone(),
            None => dirs::config_dir()
                .context("cannot determine the config directory; pass --config-dir")?
                .join("notification-forwarder"),
        };
        Ok(dir.join(default))
    }
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let mode = std::fs::metadata(path)
        .with_context(|| format!("reading {}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        tracing::warn!("{} is accessible by other users (mode {:o}); run chmod 600", path.display(), mode & 0o777);
    }
    PrivateKeyDer::from_pem_file(path).with_context(|| format!("parsing {}", path.display()))
}

/// Builds a TLS acceptor that presents the server certificate and only accepts
/// clients holding a certificate signed by the CA.
pub fn acceptor(args: &TlsArgs) -> Result<TlsAcceptor> {
    let ca = args.resolve(&args.ca, "ca.pem")?;
    let cert = args.resolve(&args.cert, "server.pem")?;
    let key = args.resolve(&args.key, "server.key")?;

    let mut roots = RootCertStore::empty();
    for c in CertificateDer::pem_file_iter(&ca).with_context(|| format!("reading {}", ca.display()))? {
        roots.add(c?)?;
    }
    let certs = CertificateDer::pem_file_iter(&cert)
        .with_context(|| format!("reading {}", cert.display()))?
        .collect::<Result<Vec<_>, _>>()?;

    let config = ServerConfig::builder()
        .with_client_cert_verifier(WebPkiClientVerifier::builder(Arc::new(roots)).build()?)
        .with_single_cert(certs, load_key(&key)?)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
