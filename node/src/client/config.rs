use super::tls::danger::NoCertificateVerification;
use rustls::{
    crypto::aws_lc_rs,
    pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
    RootCertStore,
};
use std::{
    fs,
    io::{self, BufReader},
    path::Path,
    sync::Arc,
};
use tokio_rustls::rustls::ClientConfig;

fn load_private_key(key_path: &Path) -> io::Result<PrivateKeyDer<'static>> {
    let key_bytes = fs::read(key_path)?;
    let mut reader = io::Cursor::new(&key_bytes);
    if let Some(key) = rustls_pemfile::private_key(&mut reader)? {
        return Ok(key);
    }
    if key_bytes.starts_with(b"-----BEGIN") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "no private key found in PEM file",
        ));
    }
    Ok(PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_bytes)))
}

pub fn client_config(cert_path: &Path, key_path: &Path) -> ClientConfig {
    let certfile = fs::File::open(cert_path).expect("Cannot open CA file");
    let mut reader = BufReader::new(certfile);
    let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
        .map(|result| result.unwrap())
        .collect();
    assert_eq!(certs.len(), 1, "got more than one certificate");
    let private_key = load_private_key(key_path)
        .unwrap_or_else(|err| panic!("Cannot parse key file {}: {err}", key_path.display()));

    let root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };

    let mut config = ClientConfig::builder_with_protocol_versions(&[
        &rustls::version::TLS12,
        &rustls::version::TLS13,
    ])
    .with_root_certificates(root_store)
    .with_client_auth_cert(certs, private_key)
    .unwrap();

    // disable client certificate verification
    config
        .dangerous()
        .set_certificate_verifier(Arc::new(NoCertificateVerification::new(
            aws_lc_rs::default_provider(),
        )));

    config
}
