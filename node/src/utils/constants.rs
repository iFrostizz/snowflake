use crate::id::{ChainId, FUJI_C_CHAIN_ID, MAINNET_C_CHAIN_ID};
use phf::phf_map;

pub const MAINNET_ID: u32 = 1;
pub const FUJI_ID: u32 = 5;

/// max clock difference from peer in seconds
#[allow(unused)] // TODO deny connections if their timestamp is too skewed
pub const MAX_CLOCK_DIFF: u32 = 60;

pub static NETWORK: phf::Map<&'static str, u32> = phf_map! {
    "mainnet" => MAINNET_ID,
    "fuji" => FUJI_ID,
};

#[allow(unused)]
pub static C_CHAIN_ID_STR: phf::Map<&'static str, &str> = phf_map! {
    "mainnet" => "2q9e4r6Mu3U68nU1fYjgbR6JvwrRx36CohpAX5UQxse55x1Q5",
    "fuji" => "yH8D7ThNJkxmtkuv2jgBa4P1Rn3Qpr4pPr7QYNfcdoS6k6HWp",
};

pub const C_CHAIN_ID: phf::Map<&'static str, ChainId> = phf_map! {
    "mainnet" => MAINNET_C_CHAIN_ID,
    "fuji" => FUJI_C_CHAIN_ID,
};

pub const DEFAULT_DEADLINE: u64 = 10_000_000_000; // <10s

pub const APP_PREFIX: u8 = 0;

#[cfg(feature = "dhat-heap")]
pub(crate) const DHAT_TIME_S: u64 = 600;

#[cfg(test)]
mod tests {
    use super::{C_CHAIN_ID, C_CHAIN_ID_STR};
    use crate::id::{ChainId, Id};

    #[test]
    fn correct_compile_time_chain_id() {
        assert_eq!(
            C_CHAIN_ID["mainnet"],
            ChainId(Id::try_from(C_CHAIN_ID_STR["mainnet"]).unwrap())
        );

        assert_eq!(
            C_CHAIN_ID["fuji"],
            ChainId(Id::try_from(C_CHAIN_ID_STR["fuji"]).unwrap())
        );
    }
}
