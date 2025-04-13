use crate::utils::{bls::Bls, ip::ip_octets, packer::Packer};
use openssl::{
    error::ErrorStack,
    hash::MessageDigest,
    pkey::{PKey, Private, Public},
    rsa::Rsa,
    sign::{Signer, Verifier},
};
use sha2::{self, Digest};
use std::{net::IpAddr, path::Path, time};

#[derive(Debug)]
pub struct UnsignedIp {
    ip: Vec<u8>,
    port: u16,
    pub timestamp: u64,
}

#[derive(Debug)]
pub struct SignedIp {
    pub unsigned_ip: UnsignedIp,
    pub ip_sig: Vec<u8>,
    pub ip_bls_sig: Vec<u8>,
}

impl UnsignedIp {
    const IPV6_LEN: usize = 16;
    const SHORT_LEN: usize = 2;
    const LONG_LEN: usize = 8;

    pub fn new(ip_addr: IpAddr, port: u16, timestamp: u64) -> Self {
        let ip = ip_octets(ip_addr);

        Self {
            ip,
            port,
            timestamp,
        }
    }

    fn capacity(&self) -> usize {
        // defaults to ipv6 since it's mapped
        Self::IPV6_LEN + Self::SHORT_LEN + Self::LONG_LEN
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut packer = Packer::new_with_capacity(self.capacity());
        packer.pack_fixed_bytes(self.ip.as_slice());
        packer.pack_short(&self.port);
        packer.pack_long(&self.timestamp);
        packer.finish()
    }

    pub fn sign_with_key(self, bls: &Bls, pem_key_path: &Path) -> SignedIp {
        let pem_private_key = std::fs::read(pem_key_path).unwrap();
        let pem_private_key = openssl::rsa::Rsa::private_key_from_pem(&pem_private_key).unwrap();
        let pem_private_key = openssl::pkey::PKey::from_rsa(pem_private_key).unwrap();

        self.sign(&pem_private_key, bls)
    }

    /// Sign the sha256 of the public IP given the staker RSA private key
    fn sign(self, keypair: &PKey<Private>, bls: &Bls) -> SignedIp {
        let mut signer = Signer::new(MessageDigest::sha256(), keypair).unwrap();
        let ip_bytes = self.bytes();
        assert_eq!(ip_bytes.len(), self.capacity());
        signer.update(&ip_bytes).unwrap();
        let ip_sig = signer.sign_to_vec().unwrap(); // TODO error handling if necessary
        let ip_bls_sig = bls.sign_pop(&ip_bytes);

        SignedIp {
            unsigned_ip: self,
            ip_sig,
            ip_bls_sig,
        }
    }

    // TODO handle certificate from the TLS connection
    #[allow(unused)]
    pub fn verify(&self, signature: &[u8], pkey: Rsa<Public>) -> Result<bool, ErrorStack> {
        let max_timestamp = time::SystemTime::now()
            .duration_since(time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if max_timestamp < self.timestamp {
            return Ok(false);
        }

        let pkey = &PKey::from_rsa(pkey).unwrap();
        let mut verifier = Verifier::new(MessageDigest::sha256(), pkey).unwrap();

        let as_bytes = self.bytes();
        let mut hasher = sha2::Sha256::new();
        hasher.update(as_bytes);
        let hashed_ip = hasher.finalize();
        verifier.update(&hashed_ip).unwrap();

        verifier.verify(signature)
    }
}

#[cfg(test)]
mod tests {
    use crate::net::ip::UnsignedIp;
    use openssl::{
        hash::MessageDigest,
        pkey::PKey,
        rsa::Padding,
        sign::{Signer, Verifier},
        x509,
    };
    use sha2::Digest;
    use std::{fs, path::Path, str::FromStr};

    #[test]
    fn correct_ip_bytes_repr() {
        let ip_addr =
            std::net::IpAddr::from_str("4187:e8e5:6129:f9a8:7e88:5b66:4255:e34b").unwrap();
        let port = 6969;
        let timestamp = 69420;
        let unsigned_ip = UnsignedIp::new(ip_addr, port, timestamp);
        let as_bytes = unsigned_ip.bytes();

        assert_eq!(
            as_bytes,
            [
                65, 135, 232, 229, 97, 41, 249, 168, 126, 136, 91, 102, 66, 85, 227, 75, 27, 57, 0,
                0, 0, 0, 0, 1, 15, 44
            ]
        );

        let mut hasher = sha2::Sha256::new();
        hasher.update(as_bytes.clone());
        let ip_hash = hasher.finalize();

        assert_eq!(
            ip_hash.to_vec(),
            [
                78, 206, 134, 62, 131, 68, 102, 55, 164, 146, 192, 6, 169, 154, 156, 225, 229, 25,
                149, 104, 39, 205, 233, 5, 98, 143, 22, 125, 170, 120, 31, 22
            ]
        );

        let credentials_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/credentials/");
        let private_key_path = credentials_path.join("staker.key");
        let cert_path = credentials_path.join("staker.crt");

        let private_key = fs::read(private_key_path).unwrap();
        let private_key_pem = openssl::rsa::Rsa::private_key_from_pem(&private_key).unwrap();
        let keypair = PKey::from_rsa(private_key_pem).unwrap();

        let mut signer = Signer::new(MessageDigest::sha256(), &keypair).unwrap();
        signer.set_rsa_padding(Padding::PKCS1).unwrap();
        signer.update(&ip_hash).unwrap();
        let signature = signer.sign_to_vec().unwrap();

        let cert_raw = fs::read(cert_path).unwrap();
        let x509 = x509::X509::from_pem(&cert_raw).unwrap();
        let public_key = x509.public_key().unwrap();

        let mut verifier = Verifier::new(MessageDigest::sha256(), public_key.as_ref()).unwrap();
        verifier.set_rsa_padding(Padding::PKCS1).unwrap();
        verifier.update(&ip_hash).unwrap();
        let verified = verifier.verify(&signature).unwrap();
        assert!(verified);
    }
}
