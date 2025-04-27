use crate::id::{ChainId, FUJI_C_CHAIN_ID, MAINNET_C_CHAIN_ID};
use phf::phf_map;

pub const CLIENT: &str = "snowflake";

pub const MAINNET_ID: u32 = 1;
pub const FUJI_ID: u32 = 5;

pub const ETH_MAINNET_ID: u64 = 43114;
pub const ETH_FUJI_ID: u64 = 43113;

/// max clock difference from peer in seconds
#[allow(unused)] // TODO deny connections if their timestamp is too skewed
pub const MAX_CLOCK_DIFF: u32 = 60;

pub static NETWORK: phf::Map<&'static str, u32> = phf_map! {
    "mainnet" => MAINNET_ID,
    "fuji" => FUJI_ID,
};

pub static ETH_NETWORK: phf::Map<&'static str, u64> = phf_map! {
    "mainnet" => ETH_MAINNET_ID,
    "fuji" => ETH_FUJI_ID,
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

pub const AVALANCHEGO_HANDLER_ID: u64 = 0;
pub const SNOWFLAKE_HANDLER_ID: u64 = 127;

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
