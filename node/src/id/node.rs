use super::{Id, IdError};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};
use std::fmt::{Debug, Display, Formatter};

const NODE_PREFIX: &str = "NodeID-";

#[derive(Clone, Copy, Hash, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct NodeId {
    id: Id<{ Self::LEN }>,
}

impl AsRef<[u8]> for NodeId {
    fn as_ref(&self) -> &[u8] {
        self.id.as_slice()
    }
}

impl From<NodeId> for [u8; 20] {
    fn from(value: NodeId) -> Self {
        value.id.into()
    }
}

impl From<[u8; 20]> for NodeId {
    fn from(value: [u8; 20]) -> Self {
        Self {
            id: Id::from(value),
        }
    }
}

impl Display for NodeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", NODE_PREFIX, self.id)
    }
}

impl Debug for NodeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", NODE_PREFIX, self.id)
    }
}

pub struct VecNodeIds<'a>(&'a Vec<NodeId>);

impl<'a> From<&'a Vec<NodeId>> for VecNodeIds<'a> {
    fn from(value: &'a Vec<NodeId>) -> Self {
        Self(value)
    }
}

impl Display for VecNodeIds<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[")?;
        let mut first = true;
        for node_id in self.0 {
            if first {
                write!(f, "{}", node_id)?;
            } else {
                write!(f, ", {}", node_id)?;
                first = false;
            }
        }
        write!(f, "]")
    }
}

impl TryFrom<String> for NodeId {
    type Error = IdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        <NodeId as TryFrom<&str>>::try_from(&value)
    }
}

impl TryFrom<&str> for NodeId {
    type Error = IdError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if let Some(suffix) = value.strip_prefix(NODE_PREFIX) {
            Ok(Self {
                id: Id::try_from(suffix)?,
            })
        } else {
            Err(IdError::MissingPrefix)
        }
    }
}

impl NodeId {
    pub const LEN: usize = 20;

    pub fn from_cert(cert: Vec<u8>) -> Self {
        let mut hasher = <Sha256 as Digest>::new();
        hasher.update(cert);
        let inner = hasher.finalize();

        let mut hasher = <Ripemd160 as Digest>::new();
        hasher.update(inner);
        let node_id = hasher.finalize();

        let mut fixed_node_id = [0; Self::LEN];
        fixed_node_id.copy_from_slice(&node_id);

        Self {
            id: Id {
                inner: fixed_node_id,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use openssl::x509;

    use super::NodeId;

    #[test]
    fn node_id_conversion() {
        let node_id_str = "NodeID-A8X5CqiZhsqSEGNUNqZifHqskMvSUJBG";
        let node_id = NodeId::try_from(node_id_str).unwrap();
        let actual_node_id_str = node_id.to_string();
        assert_eq!(actual_node_id_str, node_id_str);
    }

    #[test]
    fn node_id_from_cert() {
        let test_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/id/testdata/");
        for entry in fs::read_dir(test_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() && path.extension().unwrap() == "crt" {
                let file_name = path.file_name().unwrap().to_str().unwrap();
                let file_name = file_name.get(..(file_name.len() - 4)).unwrap();
                println!("{}....", file_name);

                let bytes = fs::read(&path).unwrap();
                let x509 = x509::X509::from_pem(&bytes).unwrap();
                let cert = x509.to_der().unwrap();
                println!("{:?}", &cert);
                assert!(!cert.is_empty());

                let node_id = NodeId::from_cert(cert);

                assert_eq!(node_id.to_string(), format!("NodeID-{}", file_name));
            }
        }
    }
}
