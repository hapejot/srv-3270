//! TLS-Anbindung (Feature `tls`): Zertifikat und Schluessel aus PEM-Dateien.
//!
//! Die Verschluesselung liegt unter der Telnet-Verhandlung. Nach dem Handshake
//! laufen dieselben 3270-Records wie bei TCP.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};

/// Baut einen Akzeptor aus Zertifikatskette und privatem Schluessel (PEM).
pub fn acceptor(cert: &Path, key: &Path) -> Result<TlsAcceptor> {
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut std::io::BufReader::new(
            std::fs::File::open(cert).with_context(|| format!("Zertifikat {}", cert.display()))?,
        ))
        .collect::<std::result::Result<_, _>>()
        .context("Zertifikat lesen")?;
    if certs.is_empty() {
        return Err(anyhow!("kein Zertifikat in {}", cert.display()));
    }
    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut std::io::BufReader::new(
        std::fs::File::open(key).with_context(|| format!("Schluessel {}", key.display()))?,
    ))
    .context("Schluessel lesen")?
    .ok_or_else(|| anyhow!("kein Schluessel in {}", key.display()))?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("TLS-Konfiguration")?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
