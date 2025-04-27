use crate::{id::NodeId, stats, utils::FIFO};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

#[derive(Debug)]
struct Latency {
    records: FIFO<Duration>,
}

impl Latency {
    fn get_lat_nanos(&self) -> u128 {
        let records = &self.records;
        let sum = records
            .last_elements(usize::MAX)
            .fold(Duration::default(), |acc, dur| acc.saturating_add(*dur));
        let len = records.len();

        sum.as_nanos() / len as u128
    }
}

#[derive(Debug)]
pub struct PeersLatency {
    max_records: usize,
    lat_to_peers: BTreeMap<u128, Vec<NodeId>>,
    peers: HashMap<NodeId, Latency>,
}

impl PeersLatency {
    pub fn new(max_records: usize) -> Self {
        if max_records == 0 {
            panic!("cannot set empty cache");
        }

        Self {
            max_records,
            lat_to_peers: BTreeMap::new(),
            peers: HashMap::new(),
        }
    }

    pub fn record(&mut self, node_id: NodeId, lat: Duration) {
        self.remove_lat(&node_id);
        self.push_lat(node_id, lat);

        stats::latency::record_latency(lat.as_secs_f64());
    }

    pub fn fastest_n(&self, n: usize) -> impl Iterator<Item = &NodeId> {
        self.lat_to_peers.values().flatten().take(n)
    }

    fn remove_lat(&mut self, node_id: &NodeId) -> Option<()> {
        let lat_nanos = self.get_lat_nanos(node_id)?;
        let ids = self
            .lat_to_peers
            .get_mut(&lat_nanos)
            .expect("lat_to_peers invariant broken");

        debug_assert!(!ids.is_empty());
        if ids.len() == 1 {
            self.lat_to_peers.remove(&lat_nanos);
        } else {
            let index = ids.iter().position(|el| el == node_id)?;

            ids.swap_remove(index);
        }

        Some(())
    }

    fn push_lat(&mut self, node_id: NodeId, lat: Duration) {
        self.peers
            .entry(node_id)
            .and_modify(|latency| {
                latency.records.push(lat);
            })
            .or_insert_with(|| {
                let mut records = FIFO::new(self.max_records);
                records.push(lat);
                Latency { records }
            });

        let new_lat_nanos = self.get_lat_nanos(&node_id).unwrap(); // modified above
        self.lat_to_peers
            .entry(new_lat_nanos)
            .and_modify(|ids| ids.push(node_id))
            .or_insert_with(|| vec![node_id]);
    }

    fn get_lat_nanos(&self, node_id: &NodeId) -> Option<u128> {
        let lat = self.peers.get(node_id)?;
        Some(lat.get_lat_nanos())
    }
}

#[cfg(test)]
mod tests {
    use super::PeersLatency;
    use crate::id::NodeId;
    use std::time::Duration;

    #[test]
    fn add_latency() {
        let node_id = NodeId::default();
        let mut latency = PeersLatency::new(2);

        let dur1 = Duration::from_millis(101);
        let dur2 = Duration::from_millis(102);
        let dur3 = Duration::from_millis(103);

        latency.push_lat(node_id, dur1);
        let records = &latency.peers.get(&node_id).unwrap().records;
        assert_eq!(records.len(), 1);
        let elements = records.last_elements(1).collect::<Vec<_>>();
        assert_eq!(elements, vec![&dur1]);

        latency.push_lat(node_id, dur2);
        let records = &latency.peers.get(&node_id).unwrap().records;
        assert_eq!(records.len(), 2);
        let elements = records.last_elements(2).collect::<Vec<_>>();
        assert_eq!(elements, vec![&dur2, &dur1]);

        latency.push_lat(node_id, dur3);
        let records = &latency.peers.get(&node_id).unwrap().records;
        assert_eq!(records.len(), 2);
        let elements = records.last_elements(2).collect::<Vec<_>>();
        assert_eq!(elements, vec![&dur3, &dur2]);
    }

    #[test]
    fn fastest_peers() {
        let node_id1 = NodeId::try_from("NodeID-NpagUxt6KQiwPch9Sd4osv8kD1TZnkjdk").unwrap();
        let node_id2 = NodeId::try_from("NodeID-EzGaipqomyK9UKx9DBHV6Ky3y68hoknrF").unwrap();

        let mut latency = PeersLatency::new(2);

        latency.record(node_id1, Duration::from_millis(100));
        let lat1 = latency.get_lat_nanos(&node_id1).unwrap();
        let ids = latency.lat_to_peers.get(&lat1).unwrap();
        assert_eq!(ids, &vec![node_id1]);
        latency.record(node_id1, Duration::from_millis(200));
        let lat1 = latency.get_lat_nanos(&node_id1).unwrap();
        let ids = latency.lat_to_peers.get(&lat1).unwrap();
        assert_eq!(ids, &vec![node_id1]);

        latency.record(node_id2, Duration::from_millis(200));
        latency.record(node_id2, Duration::from_millis(200));

        let fastest = latency.fastest_n(2).collect::<Vec<_>>();

        assert_eq!(fastest, vec![&node_id1, &node_id2]);
    }
}
