use std::time::Duration;
use crate::types::{HandlerConfig, NetworkId, Tracking, Rpc};

#[derive(Debug, Clone)]
pub struct NormalizedConfig {
    /// The network ID to use for RPC calls
    pub network_id: NetworkId,
    /// The level of data you are okay with providers tracking
    pub tracking: Tracking,
    /// List of injected RPCs (localhost, anvil, etc)
    pub injected_rpcs: Vec<Rpc>,
    /// Retry settings for failed RPC calls
    pub retry: RetryConfig,
    /// General settings
    pub settings: SettingsConfig,
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Number of retry attempts for failed RPC calls
    pub retry_count: u32,
    /// Delay between retry attempts
    pub retry_delay: Duration,
}

#[derive(Debug, Clone)]
pub struct SettingsConfig {
    /// Timeout for RPC latency testing
    pub rpc_timeout: Duration,
    /// Timeout for individual RPC calls
    pub rpc_call_timeout: Duration,
    /// Whether to use browser localStorage for persisting latency cache
    pub browser_local_storage: bool,
    /// Log level for this package including RPC calls
    pub log_level: String,
    /// If true, prune dynamic data to only the configured networkId during init
    pub prune_unused_data: bool,
}

pub fn resolve_config(config: HandlerConfig) -> NormalizedConfig {
    let settings = config.settings.unwrap_or_default();
    
    NormalizedConfig {
        network_id: config.network_id,
        tracking: settings.tracking,
        injected_rpcs: settings.network_rpcs,
        retry: RetryConfig {
            retry_count: settings.proxy_settings
                .as_ref()
                .map(|p| p.retry_count)
                .unwrap_or(3),
            retry_delay: Duration::from_millis(
                settings.proxy_settings
                    .as_ref()
                    .map(|p| p.retry_delay_ms)
                    .unwrap_or(100),
            ),
        },
        settings: SettingsConfig {
            rpc_timeout: Duration::from_millis(settings.rpc_probe_timeout_ms),
            rpc_call_timeout: Duration::from_millis(
                settings.proxy_settings
                    .as_ref()
                    .map(|p| p.rpc_call_timeout_ms)
                    .unwrap_or(10000),
            ),
            browser_local_storage: false, // Not applicable for Rust
            log_level: match settings.log_level {
                crate::types::LogLevel::Error => "error".to_string(),
                crate::types::LogLevel::Warn => "warn".to_string(),
                crate::types::LogLevel::Info => "info".to_string(),
                crate::types::LogLevel::Debug => "debug".to_string(),
                crate::types::LogLevel::Trace => "trace".to_string(),
            },
            prune_unused_data: false, // Can be made configurable later
        },
    }
}
