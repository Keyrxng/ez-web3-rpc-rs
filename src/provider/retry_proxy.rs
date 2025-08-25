use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use crate::{NetworkId, JsonRpcRequest, JsonRpcResponse, Result, RpcHandlerError};

#[derive(Clone)]
pub struct RetryOptions {
    pub retry_count: u32,
    pub retry_delay: Duration,
    pub get_ordered_urls: Arc<dyn Fn() -> Vec<String> + Send + Sync>,
    pub chain_id: NetworkId,
    pub rpc_call_timeout: Duration,
    pub on_log: Option<Arc<dyn Fn(&str, &str, Option<serde_json::Value>) + Send + Sync>>,
    pub refresh: Arc<dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send>> + Send + Sync>,
}

impl std::fmt::Debug for RetryOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetryOptions")
            .field("retry_count", &self.retry_count)
            .field("retry_delay", &self.retry_delay)
            .field("chain_id", &self.chain_id)
            .field("rpc_call_timeout", &self.rpc_call_timeout)
            .field("has_get_ordered_urls", &true)
            .field("has_on_log", &self.on_log.is_some())
            .field("has_refresh", &true)
            .finish()
    }
}

#[derive(Clone)]
pub struct RetryProvider {
    pub base_url: String,
    pub chain_id: NetworkId,
    pub options: Arc<RwLock<RetryOptions>>,
    client: reqwest::Client,
}

impl RetryProvider {
    pub fn new(base_url: String, chain_id: NetworkId, options: RetryOptions) -> Self {
        Self {
            base_url,
            chain_id,
            options: Arc::new(RwLock::new(options)),
            client: reqwest::Client::new(),
        }
    }
    
    pub async fn send_request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse<serde_json::Value>> {
        let options = self.options.read().await;
        let ordered_urls = (options.get_ordered_urls)();
        
        // Ensure base URL is in the list
        let mut urls = ordered_urls;
        if !urls.contains(&self.base_url) {
            urls.insert(0, self.base_url.clone());
        }
        
        if urls.is_empty() {
            if let Some(ref logger) = options.on_log {
                logger("error", "No RPCs available", None);
            }
            return Err(RpcHandlerError::NoAvailableRpcs { network_id: self.chain_id });
        }
        
        let mut loops = options.retry_count;
        while loops > 0 {
            // Process URLs in batches of 3
            for chunk in urls.chunks(3) {
                let batch_result = self.race_batch(chunk, request, &options).await;
                
                match batch_result {
                    Ok(response) => {
                        // Non-blocking refresh after successful call
                        let refresh_fn = Arc::clone(&options.refresh);
                        tokio::spawn(async move {
                            if let Err(_e) = refresh_fn().await {
                                // Log refresh failure if needed
                            }
                        });
                        
                        return Ok(response);
                    }
                    Err(batch_err) => {
                        let is_last_batch = chunk.len() < 3 || chunk.as_ptr() == urls.chunks(3).last().unwrap().as_ptr();
                        if loops == 1 && is_last_batch {
                            if let Some(ref logger) = options.on_log {
                                logger("error", "Failed after all retries", Some(serde_json::json!({
                                    "error": format!("{:?}", batch_err)
                                })));
                            }
                            return Err(batch_err);
                        }
                        
                        if let Some(ref logger) = options.on_log {
                            logger("debug", "Batch failed, backing off", Some(serde_json::json!({
                                "delay_ms": options.retry_delay.as_millis()
                            })));
                        }
                        
                        tokio::time::sleep(options.retry_delay).await;
                    }
                }
            }
            loops -= 1;
        }
        
        Err(RpcHandlerError::AllEndpointsFailed)
    }
    
    async fn race_batch(
        &self,
        urls: &[String],
        request: &JsonRpcRequest,
        options: &RetryOptions,
    ) -> Result<JsonRpcResponse<serde_json::Value>> {
        let tasks: Vec<_> = urls.iter().map(|url| {
            let url = url.clone();
            let request = request.clone();
            let client = self.client.clone();
            let timeout = options.rpc_call_timeout;
            
            async move {
                self.attempt_rpc(&client, &url, &request, timeout).await
            }
        }).collect();
        
        // Race the requests and return the first successful one
        let results = futures::future::join_all(tasks).await;
        
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(response) => {
                    if let Some(ref logger) = options.on_log {
                        logger("debug", "Successfully called provider method", Some(serde_json::json!({
                            "url": urls[i]
                        })));
                    }
                    return Ok(response);
                }
                Err(e) => {
                    if let Some(ref logger) = options.on_log {
                        logger("debug", "Provider attempt failed", Some(serde_json::json!({
                            "url": urls[i],
                            "error": format!("{:?}", e)
                        })));
                    }
                }
            }
        }
        
        Err(RpcHandlerError::AllEndpointsFailed)
    }
    
    async fn attempt_rpc(
        &self,
        client: &reqwest::Client,
        url: &str,
        request: &JsonRpcRequest,
        timeout: Duration,
    ) -> Result<JsonRpcResponse<serde_json::Value>> {
        let response = tokio::time::timeout(
            timeout,
            client.post(url).json(request).send()
        ).await?;
        
        let response = response?;
        
        if response.status().is_success() {
            let json_response = response.json().await?;
            Ok(json_response)
        } else {
            Err(RpcHandlerError::JsonRpc(url.to_string()))
        }
    }
}

pub fn wrap_with_retry(
    url: String,
    chain_id: NetworkId,
    options: RetryOptions,
) -> RetryProvider {
    RetryProvider::new(url, chain_id, options)
}
