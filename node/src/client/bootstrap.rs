use crate::dht::DhtBuckets;
use crate::id::NodeId;
use crate::net::{node::NodeError, queue::ConnectionData};
use crate::node::Node;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[derive(Deserialize, Debug)]
pub struct Bootstrapper {
    #[serde(deserialize_with = "string_to_node_id")]
    pub id: NodeId,
    pub ip: SocketAddr,
}

fn string_to_node_id<'de, D>(de: D) -> Result<NodeId, D::Error>
where
    D: Deserializer<'de>,
{
    let node_id_string = <String>::deserialize(de)?;
    let node_id = NodeId::try_from(node_id_string.as_str()).map_err(serde::de::Error::custom)?;
    Ok(node_id)
}

pub struct Bootstrappers<'a> {
    pub bootstrapper_path: &'a Path,
    pub light_bootstrapper_path: &'a Path,
}

impl<'a> Bootstrappers<'a> {
    pub fn new(bootstrapper_path: &'a Path, light_bootstrapper_path: &'a Path) -> Self {
        Self {
            bootstrapper_path,
            light_bootstrapper_path,
        }
    }

    fn read_bootsrappers(&self) -> HashMap<String, Vec<Bootstrapper>> {
        let mut content = String::new();
        let mut file = File::open(self.bootstrapper_path).unwrap();
        File::read_to_string(&mut file, &mut content).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    fn read_light_bootsrappers(&self) -> HashMap<String, Vec<Bootstrapper>> {
        let mut content = String::new();
        let mut file = File::open(self.light_bootstrapper_path).unwrap();
        File::read_to_string(&mut file, &mut content).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    fn read_all_bootsrappers(&self, network_name: &str) -> Vec<Bootstrapper> {
        let mut content = String::new();
        let mut file = File::open(self.bootstrapper_path).unwrap();
        File::read_to_string(&mut file, &mut content).unwrap();
        let mut bootstrappers: HashMap<String, Vec<Bootstrapper>> = serde_json::from_str(&content).unwrap();
        let mut bootstrappers = bootstrappers
            .remove(network_name)
            .expect("this network is not listed in the bootstrappers file");

        let mut content2 = String::new();
        let mut file2 = File::open(self.light_bootstrapper_path).unwrap();
        File::read_to_string(&mut file2, &mut content2).unwrap();
        let mut light_bootstrappers: HashMap<String, Vec<Bootstrapper>> =
            serde_json::from_str(&content2).unwrap();
        let light_bootstrappers = light_bootstrappers
            .remove(network_name)
            .expect("this network is not listed in the bootstrappers file");

        bootstrappers.extend(light_bootstrappers);
        bootstrappers
    }

    pub fn bootstrappers(&self, network_name: &str) -> HashMap<NodeId, Option<DhtBuckets>> {
        let bootstrappers = self.read_bootsrappers();
        let bootstrappers = bootstrappers
            .get(network_name)
            .expect("this network is not listed in the bootstrappers file");
        let mut ret: HashMap<_, _> = bootstrappers
            .iter()
            .map(|bootstrapper| (bootstrapper.id, None))
            .collect();
        let light_bootstrappers = self.read_light_bootsrappers();
        let light_bootstrappers = light_bootstrappers
            .get(network_name)
            .expect("this network is not listed in the bootstrappers file");
        ret.extend(
            light_bootstrappers
                .iter()
                .map(|bootstrapper| {
                    (
                        bootstrapper.id,
                        Some(DhtBuckets {
                            block: Default::default(),
                        }),
                    )
                })
                .collect::<HashMap<_, _>>(),
        );
        ret
    }

    pub async fn bootstrap_all(
        &self,
        node: &Arc<Node>,
        max_connections: usize,
        network_name: &str,
    ) -> Vec<Result<NodeId, NodeError>> {
        log::debug!("bootstrapping nodes");

        // TODO error handling
        let bootstrappers = self.read_all_bootsrappers(network_name);

        // TODO should not create a new semaphore but use the common one
        let semaphore = Arc::new(Semaphore::new(max_connections));

        let mut set = JoinSet::new();
        for bootstrapper in bootstrappers {
            let socket = bootstrapper.ip;
            let node_id = bootstrapper.id;

            let socket_addr = (socket.ip(), socket.port())
                .to_socket_addrs()
                .expect("invalid ip/port")
                .next()
                .expect("empty socket");

            let node2 = node.clone();
            let semaphore = semaphore.clone();
            set.spawn(async move {
                node2
                    .create_connection(
                        semaphore,
                        // TODO this should not be a timestamp of 0
                        // nor an empty cert.
                        // how to get the cert of bootstrappers ?
                        // maybe we should get it from the connection directly
                        ConnectionData {
                            node_id,
                            socket_addr,
                            timestamp: 0,
                            x509_certificate: vec![],
                        },
                    )
                    .await
                    .map(|_| node_id)
            });
        }

        let mut res = Vec::new();
        while let Some(fut) = set.join_next().await {
            res.push(fut.unwrap());
        }
        res
    }
}
