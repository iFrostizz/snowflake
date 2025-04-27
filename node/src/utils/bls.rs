use blst::{
    blst_fp, blst_fp2, blst_hash_to_g2, blst_p1_affine, blst_p1_affine_compress, blst_p1_uncompress, blst_p2,
    blst_p2_affine, blst_p2_affine_compress, blst_p2_uncompress, blst_scalar, blst_sign_pk2_in_g1,
    blst_sk_to_pk2_in_g1, BLST_ERROR,
};
use std::{fs, path::Path};

#[allow(unused)]
#[derive(Debug)]
pub enum BlsError {
    NotCompressSize,
    UncompressFailed,
}

pub struct Bls {
    secret_key: blst_scalar,
}

impl Bls {
    // const IKM_LEN: usize = 32;
    const BLST_FP_BYTES: usize = 384 / 8;
    const BLST_P1_COMPRESS_BYTES: usize = Self::BLST_FP_BYTES;
    const BLST_P2_COMPRESS_BYTES: usize = Self::BLST_FP_BYTES * 2;
    pub const PUBLIC_KEY_BYTES: usize = Self::BLST_P1_COMPRESS_BYTES;

    pub fn new(file_path: &Path) -> Self {
        let bytes = fs::read(file_path).expect("failed to read bls key file");
        let len = bytes.len();

        let secret_key: [u8; 32] = bytes
            .try_into()
            .unwrap_or_else(|_| panic!("too much bytes, expected 32, got: {}", len));
        let secret_key = blst_scalar { b: secret_key };

        Self { secret_key }
    }

    #[cfg(test)]
    pub fn from_private_key(private_key: [u8; 32]) -> Self {
        Self {
            secret_key: blst_scalar { b: private_key },
        }
    }

    fn _sign(&self, msg: &[u8], dst: &[u8]) -> [u8; Self::BLST_P2_COMPRESS_BYTES] {
        let zero_coordinate = blst_fp2 {
            fp: [blst_fp { l: [0; 6] }; 2],
        };

        let mut out = blst_p2 {
            x: zero_coordinate,
            y: zero_coordinate,
            z: zero_coordinate,
        };

        unsafe {
            let aug = std::ptr::null();
            blst_hash_to_g2(&mut out, msg.as_ptr(), msg.len(), dst.as_ptr(), dst.len(), aug, 0);
        }

        let mut sig = blst_p2_affine {
            x: zero_coordinate,
            y: zero_coordinate,
        };

        unsafe {
            let hash = out;
            blst_sign_pk2_in_g1(std::ptr::null_mut(), &mut sig, &hash, &self.secret_key);
        }

        let mut out = [0; Self::BLST_P2_COMPRESS_BYTES];
        unsafe {
            blst_p2_affine_compress(out.as_mut_ptr(), &sig);
        }

        out
    }

    #[allow(unused)]
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let cipher = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";
        self._sign(msg, cipher).to_vec()
    }

    pub fn sign_pop(&self, msg: &[u8]) -> Vec<u8> {
        let cipher = b"BLS_POP_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";
        self._sign(msg, cipher).to_vec()
    }

    pub fn public_key(&self) -> [u8; Self::PUBLIC_KEY_BYTES] {
        let zero_coordinate = blst_fp { l: [0; 6] };
        let mut out_pk = blst_p1_affine {
            x: zero_coordinate,
            y: zero_coordinate,
        };

        unsafe {
            blst_sk_to_pk2_in_g1(std::ptr::null_mut(), &mut out_pk, &self.secret_key);
        }

        Self::compress(out_pk)
    }

    #[allow(unused)]
    fn uncompress_g1(sig: &[u8]) -> Result<blst_p1_affine, BlsError> {
        if sig.len() != Self::BLST_P1_COMPRESS_BYTES {
            return Err(BlsError::NotCompressSize);
        }

        let zero_coordinate = blst_fp { l: [0; 6] };

        let mut out = blst_p1_affine {
            x: zero_coordinate,
            y: zero_coordinate,
        };

        let err = unsafe { blst_p1_uncompress(&mut out, sig.as_ptr()) };
        if err != BLST_ERROR::BLST_SUCCESS {
            return Err(BlsError::UncompressFailed);
        }

        Ok(out)
    }

    #[allow(unused)]
    fn uncompress_g2(sig: &[u8]) -> Result<blst_p2_affine, BlsError> {
        if sig.len() != Self::BLST_P2_COMPRESS_BYTES {
            return Err(BlsError::NotCompressSize);
        }

        let zero_coordinate = blst_fp2 {
            fp: [blst_fp { l: [0; 6] }; 2],
        };

        let mut out = blst_p2_affine {
            x: zero_coordinate,
            y: zero_coordinate,
        };

        let err = unsafe { blst_p2_uncompress(&mut out, sig.as_ptr()) };
        if err != BLST_ERROR::BLST_SUCCESS {
            return Err(BlsError::UncompressFailed);
        }

        Ok(out)
    }

    fn compress(p1: blst_p1_affine) -> [u8; Self::BLST_P1_COMPRESS_BYTES] {
        let mut out = [0; Self::BLST_P1_COMPRESS_BYTES];
        unsafe { blst_p1_affine_compress(out.as_mut_ptr(), &p1) }
        out
    }

    #[allow(unused)]
    pub fn verify_pop(sig: &[u8], public_key: &[u8]) -> Result<(), BlsError> {
        Self::_verify(sig, public_key)
    }

    fn _verify(sig: &[u8], public_key: &[u8]) -> Result<(), BlsError> {
        let _uncompressed_public_key = Self::uncompress_g1(public_key)?;
        let _sig = Self::uncompress_g2(sig)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Bls;
    use rand::random;

    #[test]
    fn bls_sig_verify() {
        let bls = Bls::from_private_key(random());
        let msg = random::<[u8; 32]>();
        let sig = bls.sign(&msg);
        assert!(Bls::_verify(&sig, &bls.public_key()).is_ok());
    }

    #[test]
    fn bls_public_key() {
        let bls = Bls::from_private_key([99; 32]);
        let public_key = bls.public_key();
        assert_eq!(public_key.to_vec(), hex::decode("8b0aed989d8c40f8a18090e3c2067104cb909dfc96022935903f354a604ab4240bf55fe60d1a4260d4ee0b07e3da64dc").unwrap());
        assert_eq!(bls.sign_pop(&public_key), hex::decode("906db213b6297c0cb1ba60376881db4fd170ed4fd1b05c1282e2d5774d8df92146615969cde6388268219bfd207f75160280758e59cffeb353943a3709a2b2790e766fe7962e0588cb739f60a4cc3be81b2a490302ea26f67499a18ed7cfcfeb").unwrap());
    }
}
