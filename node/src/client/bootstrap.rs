use crate::dht::DhtBuckets;
use crate::id::NodeId;
use crate::net::{node::NodeError, queue::ConnectionData};
use crate::node::Node;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::Read;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::sync::Arc;

#[derive(Deserialize, Debug)]
pub struct Bootstrapper {
    pub id: NodeId,
    pub ip: SocketAddr,
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

    fn read_bootsrappers(&self) -> io::Result<HashMap<String, Vec<Bootstrapper>>> {
        let mut content = String::new();
        let mut file = File::open(self.bootstrapper_path)?;
        File::read_to_string(&mut file, &mut content)?;
        Ok(serde_json::from_str(&content)?)
    }

    fn read_light_bootsrappers(&self) -> io::Result<HashMap<String, Vec<Bootstrapper>>> {
        let mut content = String::new();
        let mut file = File::open(self.light_bootstrapper_path)?;
        File::read_to_string(&mut file, &mut content)?;
        Ok(serde_json::from_str(&content)?)
    }

    fn read_all_bootsrappers(&self, network_name: &str) -> io::Result<Vec<Bootstrapper>> {
        let mut content = String::new();
        let mut file = File::open(self.bootstrapper_path)?;
        File::read_to_string(&mut file, &mut content)?;
        let mut bootstrappers: HashMap<String, Vec<Bootstrapper>> = serde_json::from_str(&content)?;
        let mut bootstrappers = bootstrappers
            .remove(network_name)
            .expect("this network is not listed in the bootstrappers file");

        let mut content2 = String::new();
        let mut file2 = File::open(self.light_bootstrapper_path)?;
        File::read_to_string(&mut file2, &mut content2)?;
        let mut light_bootstrappers: HashMap<String, Vec<Bootstrapper>> =
            serde_json::from_str(&content2)?;
        let light_bootstrappers = light_bootstrappers
            .remove(network_name)
            .expect("this network is not listed in the bootstrappers file");

        bootstrappers.extend(light_bootstrappers);
        Ok(bootstrappers)
    }

    pub fn bootstrappers(
        &self,
        network_name: &str,
    ) -> io::Result<HashMap<NodeId, Option<DhtBuckets>>> {
        let bootstrappers = self.read_bootsrappers()?;
        let bootstrappers = bootstrappers
            .get(network_name)
            .expect("this network is not listed in the bootstrappers file");
        let mut ret: HashMap<_, _> = bootstrappers
            .iter()
            .map(|bootstrapper| (bootstrapper.id, None))
            .collect();
        let light_bootstrappers = self.read_light_bootsrappers()?;
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
        Ok(ret)
    }

    pub async fn bootstrap_all(
        &self,
        node: Arc<Node>,
        network_name: &str,
    ) -> io::Result<Vec<Result<NodeId, NodeError>>> {
        log::debug!("bootstrapping nodes");

        let bootstrappers = self.read_all_bootsrappers(network_name)?;

        let mut set = Vec::new();
        for bootstrapper in bootstrappers {
            let socket = bootstrapper.ip;
            let node_id = bootstrapper.id;

            let socket_addr = (socket.ip(), socket.port())
                .to_socket_addrs()
                .expect("invalid ip/port")
                .next()
                .expect("empty socket");

            let connection_queue = node.network.connection_queue.clone();
            set.push((
                node_id,
                connection_queue.connect_peer(
                    node.clone(),
                    ConnectionData {
                        node_id,
                        socket_addr,
                        timestamp: 0,
                        x509_certificate: vec![],
                    },
                    None,
                ),
            ));
        }

        let mut res = Vec::new();
        let iter = set.into_iter();
        for (node_id, fut) in iter {
            let Some(handle) = fut else {
                continue;
            };
            let Ok(ret) = handle.await else {
                continue;
            };
            res.push(ret.map(|_| node_id));
        }
        Ok(res)
    }
}
