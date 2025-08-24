use serde::{Deserialize, Serialize};
use url::Url;

pub type NetworkId = u64;
pub type NetworkName = String;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rpc {
    pub url: Url,
    pub tracking: Option<Tracking>,
    pub tracking_details: Option<String>,
    pub is_open_source: Option<bool>
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum Tracking {
    Yes,
    Limited,
    None
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LatencyRecord {
    pub latency_ms: u64,
    #[serde(with = "system_time_serde")]
    pub last_tested: std::time::SystemTime,
    pub failure_count: u32
}

// structs are effectively data objects

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HandlerConfig {
    pub network_id: NetworkId,
    pub settings: {
        pub log_level: LogLevel,
        pub tracking: Tracking,
        pub network_rpcs: Vec<Rpc>,
        pub rpc_probe_timeout_ms: u64,
        pub proxy_settings: Option<ProxySettings>,
        pub wipe_chain_data: WipeChainData
    }
}

/**
 * Think of `impl xyz`` as a class, with `new()` being the constructor.
 * 
 * Any methods defined here are built into the struct which it represents, in this case,
 * HandlerConfig above can be created using the `new()` method.
 */

impl HandlerConfig {
    pub fn new(network_id: NetworkId) -> Self {
        Self {
            network_id,
            settings: {
                log_level: LogLevel::Info,
                tracking: Tracking::Limited,
                network_rpcs: vec![], // TODO: chainlist.rs
                network_name: String::new(), // TODO: chainlist.rs 
                rpc_probe_timeout_ms: 3000,
                proxy_settings: None,
                wipe_chain_data: WipeChainData::new(false, vec![]) // TODO: chainlist.rs methods
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WipeChainData {
    pub clear_data: bool,
    pub retain_these_chains: Vec<NetworkId>
}

impl WipeChainData {
    pub fn new(clear_data: bool, retain_these_chains: Vec<NetworkId>) -> Self {
        Self { clear_data, retain_these_chains }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxySettings {
    pub struct retry_count: u32,
    pub retry_delay_ms: u64,
}

/**
 * Think of `impl Default for xyz` as the default constructor for the struct,
 * effectively allowing Option<T> to be initialized with default values.
 */

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            retry_count: 3,
            retry_delay_ms: 1000,
            rpc_call_timeout_ms: 5000
        }
    }
}

/**
 * This is a custom module for serializing and deserializing SystemTime.
 * 
 * It provides functions to convert SystemTime to and from a format suitable for
 * JSON since it is not natively supported.
 */

mod system_time_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{SystemTime, UNIX_EPOCH};

    pub fn serialize<S: Serializer>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
        let duration = time
            .duration_since(UNIX_EPOCH)
            .map_err(serde::ser::Error::custom)?;
        duration.as_secs().serialize(serializer)
    }


    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
        let secs: u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + std::time::Duration::from_secs(secs))
    }
}