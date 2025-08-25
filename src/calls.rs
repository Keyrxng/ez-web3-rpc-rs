use std::{collections::HashMap, sync::Arc, time::{Duration, Instant}};
use crate::{JsonRpcRequest, JsonRpcResponse, RpcHandler, Result, RpcHandlerError};
use serde_json::Value;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct ConsensusOptions {
    pub timeout_ms: Option<u64>,
    pub concurrency: Option<usize>,
    pub cooldown_ms: Option<u64>,
}

impl Default for ConsensusOptions {
    fn default() -> Self {
        Self {
            timeout_ms: Some(8000),
            concurrency: Some(4),
            cooldown_ms: Some(30000),
        }
    }
}

#[derive(Debug, Clone)]
struct CooldownInfo {
    until: Instant,
    strikes: u32,
}

pub struct RpcCalls {
    handler: Arc<RpcHandler>,
    cooldowns: Arc<RwLock<HashMap<String, CooldownInfo>>>,
    client: reqwest::Client,
}

impl RpcCalls {
    pub fn new(handler: Arc<RpcHandler>) -> Self {
        Self {
            handler,
            cooldowns: Arc::new(RwLock::new(HashMap::new())),
            client: reqwest::Client::new(),
        }
    }
    
    /// Basic consensus: require a quorum of identical responses across providers.
    pub async fn consensus<T>(
        &self,
        req: &JsonRpcRequest,
        quorum_threshold: f64, // e.g., 0.66 for 66%
        options: Option<ConsensusOptions>,
    ) -> Result<T> 
    where
        T: serde::de::DeserializeOwned,
    {
        let opts = options.unwrap_or_default();
        let attempt = self.consensus_attempt(req, quorum_threshold, &opts, true).await?;
        
        if attempt.success {
            if let Some(value) = attempt.value {
                return serde_json::from_value(value)
                    .map_err(|e| RpcHandlerError::SerializationError(e.to_string()));
            }
        }
        
        Err(RpcHandlerError::ConsensusFailure {
            most_common: attempt.most_common_key.unwrap_or_else(|| "n/a".to_string()),
        })
    }
    
    /// BFT-style consensus: iteratively lowers quorum requirement if initial threshold fails.
    pub async fn bft_consensus<T>(
        &self,
        req: &JsonRpcRequest,
        quorum_threshold: f64,
        min_threshold: f64,
        options: Option<ConsensusOptions>,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let opts = options.unwrap_or_default();
        let base_attempt = self.consensus_attempt(req, quorum_threshold, &opts, false).await?;
        
        if base_attempt.success {
            if let Some(value) = base_attempt.value {
                return serde_json::from_value(value)
                    .map_err(|e| RpcHandlerError::SerializationError(e.to_string()));
            }
        }
        
        if base_attempt.results.is_empty() {
            return Err(RpcHandlerError::ConsensusFailure {
                most_common: "No successful RPC responses for BFT consensus".to_string(),
            });
        }
        
        // Descend thresholds
        let mut curr = quorum_threshold - 0.05;
        while curr >= min_threshold {
            let needed = (base_attempt.results.len() as f64 * curr).ceil() as usize;
            if needed == 0 {
                break;
            }
            
            if let Some(ref most_key) = base_attempt.most_common_key {
                if base_attempt.counts.get(most_key).unwrap_or(&0) >= &needed {
                    return serde_json::from_value(base_attempt.key_to_value.get(most_key).unwrap().clone())
                        .map_err(|e| RpcHandlerError::SerializationError(e.to_string()));
                }
            }
            
            curr = (curr - 0.05).max(0.0);
        }
        
        Err(RpcHandlerError::ConsensusFailure {
            most_common: "Could not reach BFT consensus down to minimum threshold".to_string(),
        })
    }
    
    /// Attempt an RPC call using the active provider (with proxy retries).
    pub async fn try_rpc_call(&self, req: &JsonRpcRequest) -> Result<JsonRpcResponse<Value>> {
        self.handler.try_proxy_request(req.clone()).await
    }
    
    async fn consensus_attempt(
        &self,
        req: &JsonRpcRequest,
        quorum_threshold: f64,
        options: &ConsensusOptions,
        allow_early_abort: bool,
    ) -> Result<ConsensusAttemptResult> {
        let timeout_ms = options.timeout_ms.unwrap_or(8000);
        let concurrency = options.concurrency.unwrap_or(4);
        let cooldown_ms = options.cooldown_ms.unwrap_or(30000);
        
        let now = Instant::now();
        let cooldowns = self.cooldowns.read().await;
        
        let mut rpc_urls: Vec<String> = self.handler.rpcs
            .iter()
            .map(|rpc| rpc.url.to_string())
            .filter(|url| !url.starts_with("wss://"))
            .filter(|url| {
                if let Some(cd) = cooldowns.get(url) {
                    cd.until <= now
                } else {
                    true
                }
            })
            .collect();
        
        drop(cooldowns);
        
        if rpc_urls.is_empty() {
            return Err(RpcHandlerError::NoAvailableRpcs { 
                network_id: self.handler.network_id 
            });
        }
        
        if rpc_urls.len() == 1 {
            return Err(RpcHandlerError::ConsensusFailure {
                most_common: "Only one RPC available, could not reach consensus".to_string(),
            });
        }
        
        // Randomize ordering
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        rpc_urls.shuffle(&mut rng);
        
        let mut results = Vec::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut key_to_value: HashMap<String, Value> = HashMap::new();
        let mut aborted = false;
        
        let maybe_abort_early = |counts: &HashMap<String, usize>, results_len: usize, key: &str| {
            if !allow_early_abort {
                return false;
            }
            let dynamic_quorum = (results_len as f64 * quorum_threshold).ceil() as usize;
            counts.get(key).unwrap_or(&0) >= &dynamic_quorum
        };
        
        let run_request = move |url: String, req: JsonRpcRequest, client: reqwest::Client| async move {
            let result = tokio::time::timeout(
                Duration::from_millis(timeout_ms),
                client.post(&url).json(&req).send()
            ).await;
            
            match result {
                Ok(Ok(response)) if response.status().is_success() => {
                    match response.json::<JsonRpcResponse<Value>>().await {
                        Ok(json_response) => {
                            if let Some(result) = json_response.result {
                                Ok((url, result))
                            } else {
                                Err((url, "No result in response".to_string()))
                            }
                        }
                        Err(e) => Err((url, format!("JSON parse error: {}", e)))
                    }
                }
                Ok(Ok(_)) => Err((url, "HTTP error".to_string())),
                Ok(Err(e)) => Err((url, format!("Request error: {}", e))),
                Err(_) => Err((url, "Timeout".to_string())),
            }
        };
        
        // Process URLs with concurrency limit
        let mut index = 0;
        let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
        let mut tasks = Vec::new();
        
        while index < rpc_urls.len() && !aborted {
            let url = rpc_urls[index].clone();
            let req = req.clone();
            let client = self.client.clone();
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            
            let task = tokio::spawn(async move {
                let _permit = permit;
                run_request(url, req, client).await
            });
            
            tasks.push(task);
            index += 1;
            
            // Check if we can process some results
            if tasks.len() >= concurrency || index >= rpc_urls.len() {
                for task in tasks.drain(..) {
                    match task.await {
                        Ok(Ok((_url, result))) => {
                            results.push(result.clone());
                            let key = self.stable_string(&result);
                            let count = counts.entry(key.clone()).or_insert(0);
                            *count += 1;
                            key_to_value.insert(key.clone(), result);
                            
                            if maybe_abort_early(&counts, results.len(), &key) {
                                aborted = true;
                                break;
                            }
                        }
                        Ok(Err((url, error))) => {
                            self.apply_cooldown(&url, cooldown_ms, error.contains("429")).await;
                        }
                        Err(_) => {
                            // Task panicked
                        }
                    }
                }
            }
        }
        
        if results.is_empty() {
            return Ok(ConsensusAttemptResult {
                success: false,
                value: None,
                counts,
                results,
                most_common_key: None,
                key_to_value,
            });
        }
        
        let final_quorum = (results.len() as f64 * quorum_threshold).ceil() as usize;
        let most_common_key = counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(key, _)| key.clone());
        
        if let Some(ref key) = most_common_key {
            if counts.get(key).unwrap_or(&0) >= &final_quorum {
                return Ok(ConsensusAttemptResult {
                    success: true,
                    value: key_to_value.get(key).cloned(),
                    counts,
                    results,
                    most_common_key,
                    key_to_value,
                });
            }
        }
        
        Ok(ConsensusAttemptResult {
            success: false,
            value: None,
            counts,
            results,
            most_common_key,
            key_to_value,
        })
    }
    
    fn stable_string(&self, val: &Value) -> String {
        // Create a stable string representation for comparison
        match val {
            Value::String(s) => s.clone(),
            _ => {
                // Sort object keys for consistent comparison
                let sorted = self.sort_value(val.clone());
                serde_json::to_string(&sorted).unwrap_or_else(|_| "invalid".to_string())
            }
        }
    }
    
    fn sort_value(&self, val: Value) -> Value {
        match val {
            Value::Object(mut obj) => {
                let mut sorted_obj = serde_json::Map::new();
                let mut keys: Vec<_> = obj.keys().cloned().collect();
                keys.sort();
                for key in keys {
                    if let Some(value) = obj.remove(&key) {
                        sorted_obj.insert(key, self.sort_value(value));
                    }
                }
                Value::Object(sorted_obj)
            }
            Value::Array(arr) => {
                Value::Array(arr.into_iter().map(|v| self.sort_value(v)).collect())
            }
            _ => val,
        }
    }
    
    async fn apply_cooldown(&self, url: &str, base_ms: u64, is_rate_limit: bool) {
        let mut cooldowns = self.cooldowns.write().await;
        let existing = cooldowns.get(url);
        let strikes = existing.map(|cd| cd.strikes).unwrap_or(0) + 1;
        
        let factor: f64 = if is_rate_limit { 2.0 } else { 1.5 };
        let delay = ((base_ms as f64) * factor.powi(strikes as i32 - 1)) as u64;
        let delay = delay.min(5 * 60 * 1000); // Cap at 5 minutes
        
        cooldowns.insert(url.to_string(), CooldownInfo {
            strikes,
            until: Instant::now() + Duration::from_millis(delay),
        });
        
        // Log cooldown if handler has logging
        tracing::warn!(
            url = %url,
            strikes = strikes,
            delay_ms = delay,
            "Cooling down provider"
        );
    }
}

#[derive(Debug)]
struct ConsensusAttemptResult {
    success: bool,
    value: Option<Value>,
    counts: HashMap<String, usize>,
    results: Vec<Value>,
    most_common_key: Option<String>,
    key_to_value: HashMap<String, Value>,
}
