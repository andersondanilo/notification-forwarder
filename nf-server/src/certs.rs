use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use time::{Duration, OffsetDateTime};

/// Fixed name in the server certificate. Clients verify the server against this
/// name rather than the address they connect to, so certificates stay valid
/// when the server's IP or hostname changes. Must match `SERVER_NAME` in nf-client.
const SERVER_NAME: &str = "nf-server";

fn params(
    cn: &str,
    names: Vec<String>,
    days: u32,
    usages: Vec<KeyUsagePurpose>,
) -> Result<CertificateParams> {
    let mut p = CertificateParams::new(names)?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, cn);
    p.distinguished_name = dn;
    // Backdate slightly to tolerate small clock differences between hosts.
    p.not_before = OffsetDateTime::now_utc() - Duration::hours(1);
    p.not_after = OffsetDateTime::now_utc() + Duration::days(days.into());
    p.key_usages = usages;
    Ok(p)
}

/// Writes a new file, refusing to overwrite. Private files are created 0600.
fn write(dir: &Path, name: &str, contents: &str, private: bool) -> Result<()> {
    let path = dir.join(name);
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(if private { 0o600 } else { 0o644 })
        .open(&path)
        .with_context(|| format!("creating {} (refusing to overwrite)", path.display()))?
        .write_all(contents.as_bytes())?;
    Ok(())
}

/// Generates a private CA and a server and client certificate signed by it,
/// for mutual TLS. Nothing is written if any target file already exists.
pub fn generate(dir: &Path, days: u32) -> Result<()> {
    const FILES: [&str; 6] = ["ca.pem", "ca.key", "server.pem", "server.key", "client.pem", "client.key"];
    if let Some(f) = FILES.iter().find(|f| dir.join(f).exists()) {
        bail!("{} already exists; remove it (or pick another --out) to regenerate", dir.join(f).display());
    }
    std::fs::create_dir_all(dir)?;

    let mut ca_params = params(
        "notification-forwarder CA",
        vec![],
        days,
        vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign],
    )?;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate()?)?;

    let server_key = KeyPair::generate()?;
    let mut server = params("nf-server", vec![SERVER_NAME.to_owned()], days, vec![KeyUsagePurpose::DigitalSignature])?;
    server.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server_cert = server.signed_by(&server_key, &ca)?;

    let client_key = KeyPair::generate()?;
    let mut client = params("nf-client", vec![], days, vec![KeyUsagePurpose::DigitalSignature])?;
    client.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let client_cert = client.signed_by(&client_key, &ca)?;

    write(dir, "ca.pem", &ca.pem(), false)?;
    write(dir, "ca.key", &ca.key().serialize_pem(), true)?;
    write(dir, "server.pem", &server_cert.pem(), false)?;
    write(dir, "server.key", &server_key.serialize_pem(), true)?;
    write(dir, "client.pem", &client_cert.pem(), false)?;
    write(dir, "client.key", &client_key.serialize_pem(), true)?;
    Ok(())
}
