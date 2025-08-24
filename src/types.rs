use serde::{Deserialize, Serialize};
use url::Url;

use crate::chainlist::{get_chain_info};

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

impl LogLevel {
    // Returns true if a configured log level (self) should allow emitting an event of 'event_level'.
    pub fn allows(&self, event_level: &LogLevel) -> bool {
        use LogLevel::*;
        match self {
            Error => matches!(event_level, Error),
            Warn => matches!(event_level, Error | Warn),
            Info => matches!(event_level, Error | Warn | Info),
            Debug => matches!(event_level, Error | Warn | Info | Debug),
            Trace => true,
        }
    }
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
    pub settings: Option<HandlerSettings>
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HandlerSettings {
        pub log_level: LogLevel,
        pub tracking: Tracking,
        pub network_rpcs: Vec<Rpc>,
        pub network_name: NetworkName,
        pub rpc_probe_timeout_ms: u64,
        pub proxy_settings: Option<ProxySettings>,
        pub wipe_chain_data: WipeChainData
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
            settings: Some(HandlerSettings {
                log_level: LogLevel::Error,
                tracking: Tracking::Limited,
                network_rpcs: Vec::new(), 
                network_name: get_chain_info(network_id).unwrap().name,
                rpc_probe_timeout_ms: 3000,
                proxy_settings: Some(ProxySettings::default()),
                wipe_chain_data: WipeChainData::new(network_id)
            })
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WipeChainData {
    pub clear_data: bool,
    pub retain_these_chains: Vec<NetworkId>
}

impl WipeChainData {
    fn new(network_id: NetworkId) -> Self {
        Self { clear_data: true, retain_these_chains: [network_id].to_vec() }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxySettings {
    pub retry_count: u32,
    pub retry_delay_ms: u64,
    pub rpc_call_timeout_ms: u64
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

    pub fn serialize<S: Serializer>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
        {
            let duration = time
                .duration_since(UNIX_EPOCH)
                .map_err(serde::ser::Error::custom)?;
            duration.as_secs().serialize(serializer)
        }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error>
        {
            let secs = u64::deserialize(deserializer)?;
            Ok(UNIX_EPOCH + std::time::Duration::from_secs(secs))
        }
}