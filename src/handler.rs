use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

use crate::{
    config::{resolve_config, NormalizedConfig},
    provider::{create_provider, wrap_with_retry, RetryOptions},
    provider::retry_proxy::RetryProvider,
    rpc::select_base_rpc_set,
    strategy::{get_fastest, get_first_healthy, Strategy},
    JsonRpcRequest, JsonRpcResponse, NetworkId, Result, RpcHandlerError, Rpc,
};

pub struct RpcHandler {
    pub config: NormalizedConfig,
    pub network_id: NetworkId,
    pub rpcs: Vec<Rpc>,
    latencies: Arc<RwLock<HashMap<String, u64>>>,
    provider: Arc<RwLock<Option<RetryProvider>>>,
    strategy: Strategy,
    client: reqwest::Client,
}

impl RpcHandler {
    pub async fn new(config: crate::HandlerConfig, strategy: Option<Strategy>) -> Result<Arc<Self>> {
        let normalized_config = resolve_config(config);
        let strategy = strategy.unwrap_or(Strategy::Fastest);
        
        // Select base RPC set
        let rpcs = select_base_rpc_set(
            normalized_config.network_id,
            normalized_config.tracking.clone(),
            normalized_config.injected_rpcs.clone(),
        );

        let handler = Arc::new(Self {
            network_id: normalized_config.network_id,
            rpcs,
            latencies: Arc::new(RwLock::new(HashMap::new())),
            provider: Arc::new(RwLock::new(None)),
            strategy,
            client: reqwest::Client::new(),
            config: normalized_config,
        });

        Ok(handler)
    }

    pub async fn init(self: &Arc<Self>) -> Result<()> {
        match self.strategy {
            Strategy::Fastest => {
                let (fastest, latencies) = get_fastest(&self.rpcs, self.config.settings.rpc_timeout).await?;
                
                if let Some(fastest_url) = fastest {
                    {
                        let mut latencies_lock = self.latencies.write().await;
                        *latencies_lock = latencies;
                    }
                    
                    let provider = self.build_provider(fastest_url).await?;
                    {
                        let mut provider_lock = self.provider.write().await;
                        *provider_lock = Some(provider);
                    }
                    
                    self.log("info", "Initialized fastest provider", None).await;
                } else {
                    return Err(RpcHandlerError::NoAvailableRpcs { 
                        network_id: self.network_id 
                    });
                }
            }
            Strategy::FirstHealthy => {
                let first_healthy = get_first_healthy(&self.rpcs, self.config.settings.rpc_timeout, Some(false)).await?;
                
                if let Some(url) = first_healthy {
                    let provider = self.build_provider(url).await?;
                    {
                        let mut provider_lock = self.provider.write().await;
                        *provider_lock = Some(provider);
                    }
                    
                    self.log("info", "Initialized first healthy provider", None).await;
                } else {
                    return Err(RpcHandlerError::NoAvailableRpcs { 
                        network_id: self.network_id 
                    });
                }
            }
        }
        
        Ok(())
    }

    pub async fn get_provider(&self) -> Result<RetryProvider> {
        let provider_lock = self.provider.read().await;
        provider_lock
            .clone()
            .ok_or_else(|| RpcHandlerError::NoAvailableRpcs { network_id: self.network_id })
    }

    pub async fn get_provider_url(&self) -> Result<String> {
        let provider = self.get_provider().await?;
        Ok(provider.base_url)
    }

    pub async fn get_latencies(&self) -> HashMap<String, u64> {
        self.latencies.read().await.clone()
    }

    pub async fn refresh(self: &Arc<Self>) -> Result<()> {
        match self.strategy {
            Strategy::Fastest => {
                let (fastest, latencies) = get_fastest(&self.rpcs, self.config.settings.rpc_timeout).await?;
                
                if let Some(fastest_url) = fastest {
                    {
                        let mut latencies_lock = self.latencies.write().await;
                        *latencies_lock = latencies;
                    }
                    
                    let provider = self.build_provider(fastest_url).await?;
                    {
                        let mut provider_lock = self.provider.write().await;
                        *provider_lock = Some(provider);
                    }
                    
                    self.log("info", "Refreshed fastest provider", None).await;
                } else {
                    self.log("warn", "No fastest provider found", None).await;
                }
            }
            Strategy::FirstHealthy => {
                let first_healthy = get_first_healthy(&self.rpcs, self.config.settings.rpc_timeout, Some(false)).await?;
                
                if let Some(url) = first_healthy {
                    let provider = self.build_provider(url).await?;
                    {
                        let mut provider_lock = self.provider.write().await;
                        *provider_lock = Some(provider);
                    }
                    
                    self.log("info", "Refreshed first healthy provider", None).await;
                } else {
                    self.log("warn", "No healthy provider found", None).await;
                }
            }
        }
        
        Ok(())
    }

    async fn build_provider(self: &Arc<Self>, url: String) -> Result<RetryProvider> {
        let _base_provider = create_provider(url.clone(), self.network_id)?;
        
        let latencies = Arc::clone(&self.latencies);
        
        let retry_options = RetryOptions {
            retry_count: self.config.retry.retry_count,
            retry_delay: self.config.retry.retry_delay,
            get_ordered_urls: Arc::new(move || {
                let latencies_guard = futures::executor::block_on(latencies.read());
                let mut ordered: Vec<_> = latencies_guard
                    .iter()
                    .map(|(url, &latency)| (url.clone(), latency))
                    .collect();
                ordered.sort_by_key(|(_, latency)| *latency);
                ordered.into_iter().map(|(url, _)| url).collect()
            }),
            chain_id: self.network_id,
            rpc_call_timeout: self.config.settings.rpc_call_timeout,
            on_log: Some(Arc::new(move |level, msg, meta| {
                match level {
                    "error" => tracing::error!(message = %msg, metadata = ?meta, "RPC log"),
                    "warn" => tracing::warn!(message = %msg, metadata = ?meta, "RPC log"),
                    "info" => tracing::info!(message = %msg, metadata = ?meta, "RPC log"),
                    "debug" => tracing::debug!(message = %msg, metadata = ?meta, "RPC log"),
                    _ => tracing::trace!(message = %msg, metadata = ?meta, "RPC log"),
                }
            })),
            refresh: Arc::new(|| {
                Box::pin(async move {
                    // Simple refresh - just return Ok for now
                    // In a real implementation, you might want to trigger a refresh
                    Ok(())
                })
            }),
        };
        
        Ok(wrap_with_retry(url, self.network_id, retry_options))
    }

    pub async fn try_proxy_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse<serde_json::Value>> {
        let provider = self.get_provider().await?;
        provider.send_request(&request).await
    }

    async fn log(&self, level: &str, message: &str, metadata: Option<serde_json::Value>) {
        let log_level = &self.config.settings.log_level;
        
        // Simple level filtering
        let should_log = match (log_level.as_str(), level) {
            ("error", "error") => true,
            ("warn", "error" | "warn") => true,
            ("info", "error" | "warn" | "info") => true,
            ("debug", "error" | "warn" | "info" | "debug") => true,
            ("trace", _) => true,
            _ => false,
        };
        
        if should_log {
            match level {
                "error" => tracing::error!(
                    network_id = %self.network_id,
                    metadata = ?metadata,
                    "{}", message
                ),
                "warn" => tracing::warn!(
                    network_id = %self.network_id,
                    metadata = ?metadata,
                    "{}", message
                ),
                "info" => tracing::info!(
                    network_id = %self.network_id,
                    metadata = ?metadata,
                    "{}", message
                ),
                "debug" => tracing::debug!(
                    network_id = %self.network_id,
                    metadata = ?metadata,
                    "{}", message
                ),
                _ => tracing::trace!(
                    network_id = %self.network_id,
                    metadata = ?metadata,
                    "{}", message
                ),
            }
        }
    }
}
