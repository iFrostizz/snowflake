use super::tls::danger::NoCertificateVerification;
use rustls::{crypto::aws_lc_rs, ServerConfig};
use std::{fs, io::BufReader, path::Path, sync::Arc};

pub fn server_config(cert_path: &Path, key_path: &Path) -> ServerConfig {
    let certfile = fs::File::open(cert_path).expect("Cannot open CA file");
    let mut reader = BufReader::new(certfile);
    let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
        .map(|result| result.unwrap())
        .collect();
    let pkeyfile = fs::File::open(key_path).expect("Cannot open key file");
    let mut reader = BufReader::new(pkeyfile);
    let private_key = rustls_pemfile::private_key(&mut reader).unwrap().unwrap();

    ServerConfig::builder()
        .with_client_cert_verifier(Arc::new(NoCertificateVerification::new(
            aws_lc_rs::default_provider(),
        )))
        .with_single_cert(certs, private_key)
        .unwrap()
}
