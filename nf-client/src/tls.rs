use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Args;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::pem::PemObject;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};

/// Where to find the TLS files. Each file defaults to a fixed name inside the
/// config directory, which itself defaults to `notification-forwarder` inside
/// the platform config directory (e.g. `~/.config`).
#[derive(Args)]
pub struct TlsArgs {
    /// Directory holding ca.pem, client.pem and client.key.
    #[arg(long)]
    config_dir: Option<PathBuf>,
    /// CA certificate used to verify the server [default: <config-dir>/ca.pem].
    #[arg(long)]
    ca: Option<PathBuf>,
    /// Client certificate [default: <config-dir>/client.pem].
    #[arg(long)]
    cert: Option<PathBuf>,
    /// Client private key [default: <config-dir>/client.key].
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .with_context(|| format!("reading {}", path.display()))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            tracing::warn!("{} is accessible by other users (mode {:o}); run chmod 600", path.display(), mode & 0o777);
        }
    }
    PrivateKeyDer::from_pem_file(path).with_context(|| format!("parsing {}", path.display()))
}

/// Builds a TLS connector that trusts only servers certified by the CA and
/// authenticates with the client certificate.
pub fn connector(args: &TlsArgs) -> Result<TlsConnector> {
    let ca = args.resolve(&args.ca, "ca.pem")?;
    let cert = args.resolve(&args.cert, "client.pem")?;
    let key = args.resolve(&args.key, "client.key")?;

    let mut roots = RootCertStore::empty();
    for c in CertificateDer::pem_file_iter(&ca).with_context(|| format!("reading {}", ca.display()))? {
        roots.add(c?)?;
    }
    let certs = CertificateDer::pem_file_iter(&cert)
        .with_context(|| format!("reading {}", cert.display()))?
        .collect::<Result<Vec<_>, _>>()?;

    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_client_auth_cert(certs, load_key(&key)?)?;
    Ok(TlsConnector::from(Arc::new(config)))
}
