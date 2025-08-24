use std::{sync::Arc, time::Duration};

use dashmap::DashMap;

use crate::{
    chainlist, rpc_service::RpcTestingService, HandlerConfig, JsonRpcRequest, JsonRpcResponse, LatencyRecord, LogLevel, NetworkId, ProxySettings, Result, Rpc, RpcHandlerError
};

pub struct RpcHandler {
    config: HandlerConfig,
    network_id: NetworkId,
    rpcs: Vec<Rpc>,
    latencies: Arc<DashMap<String, LatencyRecord>>,
    rpc_service: Arc<RpcTestingService>,
    refresh_count: Arc<parking_lot::RwLock<u32>>,
    proxy_settings: ProxySettings,
}

impl RpcHandler {
    pub async fn new(
        config: Option<HandlerConfig>,
        network_id: NetworkId,
    ) -> Result<Self> {
        let handler_config: HandlerConfig = config.unwrap_or(HandlerConfig::new(network_id)).clone();
        let latencies = Arc::new(DashMap::new());

        let settings = handler_config.settings.clone().unwrap();
        let proxy_settings = settings.proxy_settings.clone();
        let rpc_service = Arc::new(RpcTestingService::new(settings.rpc_probe_timeout_ms));
        let wipe_data = settings.wipe_chain_data.clone();
        let network_id = handler_config.network_id.clone();

        let chainlist_rpcs = chainlist::get_extra_rpcs(network_id);
        let rpcs = [settings.network_rpcs.clone(), chainlist_rpcs]
            .into_iter()
            .flatten()
            .collect();

        if wipe_data.clear_data {
            chainlist::initialize_chain_data(wipe_data.retain_these_chains.clone());
        }

        let handler = Self {
            network_id,
            rpcs,
            latencies,
            config: handler_config,
            rpc_service,
            proxy_settings: proxy_settings.unwrap_or_default(),
            refresh_count: Arc::new(parking_lot::RwLock::new(0)),
        };

        handler.test_rpc_performance().await?;

        Ok(handler)
    }

    pub fn get_latencies(&self) -> Arc<DashMap<String, LatencyRecord>> {
        Arc::clone(&self.latencies)
    }

    pub async fn get_fastest_rpc(&self, with_update: Option<bool>) -> Result<String> {
        if self.rpcs.len() == 0 || with_update.unwrap_or(false) {
            self.test_rpc_performance().await?;
        }

        if let Some(entry) = self.latencies .iter().min_by_key(|entry| entry.value().latency_ms) {
            Ok(entry.key().clone())
        } else{
            Err(RpcHandlerError::NoAvailableRpcs { network_id: self.network_id } )
        }
    }

    async fn test_rpc_performance(&self) -> Result<()> {
        let should_refresh = {
            let count = *self.refresh_count.read();
            self.latencies.len() <= 1 || count >= 3
        };

        if should_refresh {
            let results = self.rpc_service.race_rpcs(&self.rpcs).await;

            for (idx, result) in results {
                if let Ok(latency_record) = result {
                    self.latencies
                        .insert(self.rpcs[idx].url.to_string(), latency_record);
                }
            }

            *self.refresh_count.write() = 0;
        }

        Ok(())
    }

    pub async fn try_proxy_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse<serde_json::Value>> {
        let mut last_error = None;

        let fastest_rpc = self.get_fastest_rpc(None).await?;

        for attempt in 0..self.proxy_settings.retry_count {
            match self.try_send_request(&request, &fastest_rpc).await {
                Ok(response) => {
                    self.log(attempt, &fastest_rpc, "RPC request succeed");
                    return Ok(response);
            }
            Err(e) => {
                last_error = Some(e);
                    if attempt < self.proxy_settings.retry_count - 1 {
                        tokio::time::sleep(Duration::from_millis(self.proxy_settings.retry_delay_ms)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| RpcHandlerError::AllEndpointsFailed))
    }

    pub async fn try_send_request(
        &self, 
        request: &JsonRpcRequest,
        url: &str
    ) -> Result<JsonRpcResponse<serde_json::Value>> {
        let response = self.rpc_service.client
            .post(url)
            .json(request)
            .send()
            .await?;

        if response.status().is_success() {
            let json_response = response.json().await?;
            Ok(json_response)
        }else{
            Err(RpcHandlerError::JsonRpc(url.to_owned()))
        }
    }
    fn log(&self, attempt: u32, url: &str, msg: &str) {
        let settings = self.config.settings.as_ref().unwrap();
        match settings.log_level {
            LogLevel::Info => tracing::info!(
                network = %settings.network_name,
                attempt = attempt + 1,
                url = %url,
                "{msg}"
            ),
            LogLevel::Error => tracing::error!(
                network = %settings.network_name,
                attempt = attempt + 1,
                url = %url,
                "{msg}"
            ),
            LogLevel::Debug => tracing::debug!(
                network = %settings.network_name,
                attempt = attempt + 1,
                url = %url,
                "{msg}"
            ),
            LogLevel::Trace => tracing::trace!(
                network = %settings.network_name,
                attempt = attempt + 1,
                url = %url,
                "{msg}"
            ),
            LogLevel::Warn => tracing::warn!(
                network = %settings.network_name,
                attempt = attempt + 1,
                url = %url,
                "{msg}"
            )
        }
    }
}
