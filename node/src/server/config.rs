use super::tls::danger::NoCertificateVerification;
use rustls::{
    crypto::aws_lc_rs,
    pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
    ServerConfig,
};
use std::{
    fs,
    io::{self, BufReader},
    path::Path,
    sync::Arc,
};

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

pub fn server_config(cert_path: &Path, key_path: &Path) -> ServerConfig {
    let certfile = fs::File::open(cert_path).expect("Cannot open CA file");
    let mut reader = BufReader::new(certfile);
    let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
        .map(|result| result.unwrap())
        .collect();
    let private_key = load_private_key(key_path).unwrap_or_else(|err| {
        panic!("Cannot parse key file {}: {err}", key_path.display())
    });

    ServerConfig::builder()
        .with_client_cert_verifier(Arc::new(NoCertificateVerification::new(
            aws_lc_rs::default_provider(),
        )))
        .with_single_cert(certs, private_key)
        .unwrap()
}
