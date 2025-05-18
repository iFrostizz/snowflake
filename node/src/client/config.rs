use super::tls::danger::NoCertificateVerification;
use rustls::{crypto::aws_lc_rs, RootCertStore};
use std::{fs, io::BufReader, path::Path, sync::Arc};
use tokio_rustls::rustls::ClientConfig;

pub fn client_config(cert_path: &Path, key_path: &Path) -> ClientConfig {
    let certfile = fs::File::open(cert_path).expect("Cannot open CA file");
    let mut reader = BufReader::new(certfile);
    let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
        .map(|result| result.unwrap())
        .collect();
    assert_eq!(certs.len(), 1, "got more than one certificate");
    let pkeyfile = fs::File::open(key_path).expect("Cannot open key file");
    let mut reader = BufReader::new(pkeyfile);
    let private_key = rustls_pemfile::private_key(&mut reader).unwrap().unwrap();

    let root_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.into(),
    };

    let mut config = ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS12, &rustls::version::TLS13])
        .with_root_certificates(root_store)
        .with_client_auth_cert(certs, private_key).unwrap();

    // disable client certificate verification
    config
        .dangerous()
        .set_certificate_verifier(Arc::new(NoCertificateVerification::new(
            aws_lc_rs::default_provider(),
        )));
    
    config
}
