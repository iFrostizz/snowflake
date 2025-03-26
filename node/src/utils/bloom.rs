// A bloom filter that updates every time a new node is added to the set

use proto_lib::p2p;
use rand::{random, Rng};
use sha2::{Digest, Sha256};
use std::f64::consts::LN_2;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BloomError {
    #[error("too few hashes")]
    TooFewHashes,
    #[error("too many hashes")]
    TooManyHashes,
    #[error("invalid num hashes")]
    InvalidNumHashes,
    #[error("too few entries")]
    TooFewEntries,
}

// TODO the implementation might be wrong, because the filter is not very effective
// most of the peers got from a PeerList are already known.
// I'm getting between 0-3 for each PeerList which is really low.
// TODO reiterate this?
#[derive(Clone, Debug)]
pub struct Filter {
    read: ReadFilter,
    count: u64,
    max_count: u64,
    last_regen: Instant,
}

/// Bloom Filter used to ask peers for new peers
impl Filter {
    const MIN_HASHES: u64 = 1;
    const MAX_HASHES: u64 = 16;
    const MIN_ENTRIES: u64 = 1;
    const LN2_SQ: f64 = LN_2 * LN_2;
    const MAX_REGEN: Duration = Duration::from_secs(60);

    /// create a new Filter with the specified number of hashes and bytes for entries
    pub fn new(num_hashes: u64, num_entries: u64) -> Result<Self, BloomError> {
        if num_entries < Self::MIN_ENTRIES {
            return Err(BloomError::TooFewEntries);
        }

        if num_hashes < Self::MIN_HASHES {
            return Err(BloomError::TooFewHashes);
        } else if num_hashes > Self::MAX_HASHES {
            return Err(BloomError::TooManyHashes);
        }

        let mut rng = rand::thread_rng();
        let seeds = (0..num_hashes).map(|_| rng.gen()).collect();

        let read = ReadFilter {
            salt: random(),
            num_bits: num_entries * 8,
            seeds,
            entries: vec![0; num_entries as usize],
        };

        Ok(Self {
            read,
            count: 0,
            max_count: Self::estimate_count(num_hashes, num_entries),
            last_regen: Instant::now(),
        })
    }

    pub fn is_sub_optimal(&self) -> bool {
        Instant::now().duration_since(self.last_regen) > Self::MAX_REGEN
            || self.count >= self.max_count
    }

    /// Create a new bloom filter with optimal parameter for the amount of tracked IPs
    pub fn new_optimal(tracked: u64) -> Result<Self, BloomError> {
        let count = tracked;
        let false_positive_p = 0.001;
        let optimal_entries = Self::optimal_entries(count, false_positive_p);
        let optimal_hashes = Self::optimal_hashes(optimal_entries, count);
        Self::new(optimal_hashes, optimal_entries)
    }

    pub fn as_proto(&self) -> p2p::BloomFilter {
        let read = &self.read;
        p2p::BloomFilter {
            filter: read.bytes(),
            salt: read.salt.to_vec(),
        }
    }

    fn gossip_id_to_hash(gossip_id: [u8; 32], salt: [u8; ReadFilter::SALT_SIZE]) -> u64 {
        let mut hasher = <Sha256 as Digest>::new();
        hasher.update(gossip_id);
        hasher.update(salt);
        let entry = hasher.finalize();

        Self::big_endian_u64(&entry)
    }

    fn big_endian_u64(data: &[u8]) -> u64 {
        // source: https://tip.golang.org/src/vendor/golang.org/x/sys/cpu/byteorder.go
        (0..8).fold(0, |acc, i| acc | (data[7 - i] as u64) << (i * 8))
    }

    pub fn feed(&mut self, gossip_id: [u8; 32]) {
        let hash = Self::gossip_id_to_hash(gossip_id, self.read.salt);
        self.feed_entry(hash);
    }

    fn feed_entry(&mut self, mut hash: u64) {
        for seed in &self.read.seeds {
            // rot left 17
            hash = Self::rot17(hash) ^ seed;
            let idx = hash % self.read.num_bits;
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            self.read.entries[byte_idx as usize] |= 1 << bit_idx;
        }

        self.count += 1;
    }

    fn rot17(hash: u64) -> u64 {
        hash.rotate_left(17)
    }

    fn optimal_entries(count: u64, false_positive_p: f64) -> u64 {
        if count == 0 || false_positive_p >= 1.0 {
            return Self::MIN_ENTRIES;
        } else if false_positive_p <= 0.0 {
            return u64::MAX;
        }

        let entries_in_bits = -(count as f64) * f64::ln(false_positive_p) / Self::LN2_SQ;
        let entries = (entries_in_bits + 8.0 - 1.0) / 8.0;

        if entries >= u64::MAX as f64 {
            return u64::MAX;
        }

        std::cmp::max(entries as u64, 1)
    }

    fn optimal_hashes(num_entries: u64, count: u64) -> u64 {
        if num_entries < count {
            return Self::MIN_HASHES;
        } else if count == 0 {
            return Self::MAX_HASHES;
        }

        let num_hashes = f64::ceil(num_entries as f64 * 8.0 * LN_2 / count as f64);

        if num_hashes >= Self::MAX_HASHES as f64 {
            return Self::MAX_HASHES;
        }

        std::cmp::max(num_hashes as u64, Self::MIN_HASHES)
    }

    fn estimate_count(num_hashes: u64, num_entries: u64) -> u64 {
        let false_positive_p: f64 = 0.01;

        if num_hashes < Self::MIN_HASHES || num_entries < Self::MIN_ENTRIES {
            return 0;
        }

        let inv_num_hashes = 1.0 / num_hashes as f64;
        let num_bits = num_entries * 8;
        let exp = 1.0 - false_positive_p.powf(inv_num_hashes);
        let count = f64::ceil(-f64::ln(exp) * num_bits as f64 * inv_num_hashes);

        if count >= u64::MAX as f64 {
            return u64::MAX;
        }

        count as u64
    }
}

pub trait ViewFilter {
    fn bytes(&self) -> Vec<u8>;
    fn contains(&self, gossip_id: [u8; 32]) -> bool;
}

#[derive(Clone, Debug)]
pub struct ReadFilter {
    salt: [u8; ReadFilter::SALT_SIZE],
    num_bits: u64,
    seeds: Vec<u64>,
    entries: Vec<u8>,
}

impl ViewFilter for ReadFilter {
    fn bytes(&self) -> Vec<u8> {
        let num_hashes = self.seeds.len();
        let bytes_len = 1 + num_hashes * 8 + self.entries.len();
        let mut bytes = Vec::with_capacity(bytes_len);
        bytes.push(num_hashes as u8); // safe because of the bounds on seeds
        for seed in &self.seeds {
            bytes.append(&mut seed.to_be_bytes().to_vec());
        }
        bytes.append(&mut self.entries.clone());

        bytes
    }

    fn contains(&self, gossip_id: [u8; 32]) -> bool {
        let hash = Filter::gossip_id_to_hash(gossip_id, self.salt);
        self.contains_entry(hash)
    }
}

impl ReadFilter {
    const SALT_SIZE: usize = 32;

    fn contains_entry(&self, mut hash: u64) -> bool {
        let mut accumulator = 1;

        for seed in &self.seeds {
            if accumulator == 0 {
                break;
            }

            hash = Filter::rot17(hash) ^ seed;
            let idx = hash % self.num_bits;
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            accumulator &= self.entries[byte_idx as usize] >> bit_idx;
        }

        accumulator != 0
    }
}

impl TryFrom<&[u8]> for ReadFilter {
    type Error = BloomError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(BloomError::InvalidNumHashes);
        }

        let num_hashes = *value.first().unwrap() as u64;
        let entries_offset = 1 + num_hashes * 8;

        if num_hashes < Filter::MIN_HASHES {
            return Err(BloomError::TooFewHashes);
        } else if num_hashes > Filter::MAX_HASHES {
            return Err(BloomError::TooManyHashes);
        } else if (value.len() as u64) < entries_offset + Filter::MIN_ENTRIES {
            return Err(BloomError::TooFewEntries);
        }

        let seeds = value
            .get(1..((num_hashes as usize * 8) + 1))
            .unwrap()
            .chunks(8)
            .map(Filter::big_endian_u64)
            .collect();

        let entries: Vec<u8> = value.get((entries_offset as usize)..).unwrap().to_owned();

        Ok(Self {
            salt: rand::thread_rng().gen(),
            num_bits: entries.len() as u64 * 8,
            seeds,
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Filter, ReadFilter, ViewFilter};

    #[test]
    fn optimal_hashes() {
        assert_eq!(Filter::optimal_hashes(0, 1024), Filter::MIN_HASHES);
        assert_eq!(Filter::optimal_hashes(1024, 0), Filter::MAX_HASHES);
        assert_eq!(Filter::optimal_hashes(u64::MAX, 1), Filter::MAX_HASHES);
        assert_eq!(Filter::optimal_hashes(1, u64::MAX), Filter::MIN_HASHES);
        assert_eq!(Filter::optimal_hashes(1024, 1024), 6);
    }

    #[test]
    fn optimal_entries() {
        assert_eq!(Filter::optimal_entries(0, 0.5), Filter::MIN_ENTRIES);
        assert_eq!(Filter::optimal_entries(1, 0.0), u64::MAX);
        assert_eq!(Filter::optimal_entries(1, 1.0), Filter::MIN_ENTRIES);
        assert_eq!(Filter::optimal_entries(1024, 0.01), 1227);
    }

    #[test]
    fn add_contains() {
        let mut filter = Filter::new_optimal(10).unwrap();
        let hash1 = 1;
        let hash2 = 2;

        assert!(!filter.read.contains_entry(hash1));
        assert!(!filter.read.contains_entry(hash2));
        assert_eq!(filter.count, 0);

        filter.feed_entry(hash1);
        assert!(filter.read.contains_entry(hash1));
        assert!(!filter.read.contains_entry(hash2));
        assert_eq!(filter.count, 1);

        filter.feed_entry(hash2);
        assert!(filter.read.contains_entry(hash1));
        assert!(filter.read.contains_entry(hash2));
        assert_eq!(filter.count, 2);
    }

    #[test]
    fn add_contains_gossip_id() {
        let mut filter = Filter::new_optimal(10).unwrap();
        let id1 = [1; 32];
        let id2 = [2; 32];

        assert!(!filter.read.contains(id1));
        assert!(!filter.read.contains(id2));
        assert_eq!(filter.count, 0);

        filter.feed(id1);
        assert!(filter.read.contains(id1));
        assert!(!filter.read.contains(id2));
        assert_eq!(filter.count, 1);

        filter.feed(id2);
        assert!(filter.read.contains(id1));
        assert!(filter.read.contains(id2));
        assert_eq!(filter.count, 2);
    }

    #[test]
    fn add_equivalence() {
        let mut filter = Filter::new(2, 5).unwrap();
        filter.read.seeds = vec![0; 2];

        let hash1 = 1;
        let hash2 = 2;

        assert_eq!(filter.read.entries, vec![0, 0, 0, 0, 0]);

        filter.feed_entry(hash1);
        assert_eq!(filter.read.entries, vec![0, 0, 0, 1, 1]);

        filter.feed_entry(hash2);
        assert_eq!(filter.read.entries, vec![0, 1, 0, 1, 1]);
        assert_eq!(
            filter.read.bytes(),
            vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1]
        );
    }

    #[test]
    fn read_filter() {
        let read_filter = ReadFilter::try_from(
            &[
                8, 117, 188, 204, 192, 163, 248, 254, 26, 202, 94, 108, 23, 25, 77, 139, 210, 196,
                205, 113, 17, 89, 76, 138, 17, 7, 171, 57, 41, 34, 55, 40, 225, 21, 51, 228, 125,
                167, 255, 205, 247, 194, 97, 48, 217, 146, 207, 246, 100, 89, 220, 26, 77, 98, 24,
                209, 60, 110, 31, 223, 66, 248, 223, 206, 44, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ][..],
        )
        .unwrap();
        assert!(read_filter.entries.iter().all(|e| *e == 0));

        let read_filter = ReadFilter::try_from(
            &[
                8, 189, 94, 17, 27, 168, 6, 237, 15, 151, 68, 134, 64, 194, 175, 177, 242, 73, 234,
                11, 196, 188, 232, 145, 241, 166, 121, 141, 54, 170, 90, 34, 13, 131, 51, 100, 187,
                238, 87, 134, 72, 182, 133, 225, 121, 117, 197, 153, 15, 223, 207, 12, 250, 165,
                78, 87, 208, 118, 177, 121, 16, 223, 73, 128, 121, 128, 128, 8, 160, 0, 8, 0, 96,
                4, 192, 0, 224, 8, 35, 0, 128, 0, 20, 8, 66, 8, 128, 0, 4, 4, 16, 2, 130, 192, 160,
                10, 224, 0, 64, 0, 16, 40, 128, 8, 32, 0, 0, 4, 128, 1, 128, 16, 96, 8, 48,
            ][..],
        )
        .unwrap();
        assert!(read_filter.contains_entry(987654321));
        assert!(read_filter.contains_entry(887654321));
        assert!(read_filter.contains_entry(787654321));
        assert!(read_filter.contains_entry(687654321));
        assert!(read_filter.contains_entry(587654321));
        assert!(read_filter.contains_entry(487654321));
        assert!(read_filter.contains_entry(37654321));
        assert!(read_filter.contains_entry(27654321));
        assert!(read_filter.contains_entry(17654321));
    }

    #[test]
    fn filter_bytes() {
        let bytes = vec![
            8, 189, 94, 17, 27, 168, 6, 237, 15, 151, 68, 134, 64, 194, 175, 177, 242, 73, 234, 11,
            196, 188, 232, 145, 241, 166, 121, 141, 54, 170, 90, 34, 13, 131, 51, 100, 187, 238,
            87, 134, 72, 182, 133, 225, 121, 117, 197, 153, 15, 223, 207, 12, 250, 165, 78, 87,
            208, 118, 177, 121, 16, 223, 73, 128, 121, 128, 128, 8, 160, 0, 8, 0, 96, 4, 192, 0,
            224, 8, 35, 0, 128, 0, 20, 8, 66, 8, 128, 0, 4, 4, 16, 2, 130, 192, 160, 10, 224, 0,
            64, 0, 16, 40, 128, 8, 32, 0, 0, 4, 128, 1, 128, 16, 96, 8, 48,
        ];

        let read_filter = ReadFilter::try_from(bytes.as_slice()).unwrap();
        assert_eq!(read_filter.bytes(), bytes);
    }
}
