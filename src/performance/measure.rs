use std::{collections::HashMap, time::{Duration, Instant}};
use crate::{JsonRpcRequest, Rpc, Result};
use futures::future::join_all;
use serde_json::{json, Value};

pub type LatencyMap = HashMap<String, u64>;

#[derive(Debug, Clone)]
pub struct RpcCheckResult {
    pub url: String,
    pub success: bool,
    pub duration: u64,
    pub block_number: Option<String>,
    pub bytecode_ok: bool,
}

const PERMIT2_ADDRESS: &str = "0x000000000022D473030F116dDEE9F6B43aC78BA3";

fn is_permit2_bytecode_valid(bytecode: Option<&str>) -> bool {
    if let Some(code) = bytecode {
        let expected = "0x604060808152600";
        code.starts_with(expected)
    } else {
        false
    }
}

async fn post_request(
    client: &reqwest::Client,
    url: &str,
    payload: &JsonRpcRequest,
    timeout: Duration,
) -> Result<(bool, Option<Value>, u64)> {
    let start = Instant::now();
    
    let response = tokio::time::timeout(
        timeout,
        client.post(url)
            .json(payload)
            .send()
    ).await;
    
    let duration = start.elapsed().as_millis() as u64;
    
    match response {
        Ok(Ok(res)) => {
            if res.status().is_success() {
                match res.json::<Value>().await {
                    Ok(json_data) => {
                        let has_result = json_data.get("result").is_some();
                        Ok((has_result, Some(json_data), duration))
                    }
                    Err(_) => Ok((false, None, duration))
                }
            } else {
                Ok((false, None, duration))
            }
        }
        Ok(Err(_)) | Err(_) => Ok((false, None, duration))
    }
}

/// Measure RPCs: run block + code requests in parallel, validate common block number logic later externally.
pub async fn measure_rpcs(rpcs: &[Rpc], timeout: Duration) -> Result<(LatencyMap, Vec<RpcCheckResult>)> {
    let client = reqwest::Client::new();
    
    let block_payload = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "eth_getBlockByNumber".to_string(),
        params: json!(["latest", false]),
        id: Some(1),
    };
    
    let code_payload = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "eth_getCode".to_string(),
        params: json!([PERMIT2_ADDRESS, "latest"]),
        id: Some(1),
    };
    
    let tasks: Vec<_> = rpcs.iter().map(|rpc| {
        let url = rpc.url.to_string();
        let client = &client;
        let block_req = &block_payload;
        let code_req = &code_payload;
        
        async move {
            let block_future = post_request(client, &url, block_req, timeout);
            let code_future = post_request(client, &url, code_req, timeout);
            
            let (block_result, code_result) = tokio::join!(block_future, code_future);
            
            let mut block_number: Option<String> = None;
            let mut block_ok = false;
            let mut block_duration = 0u64;
            
            if let Ok((ok, data, dur)) = block_result {
                block_ok = ok;
                block_duration = dur;
                if let Some(json_data) = data {
                    if let Some(result) = json_data.get("result") {
                        if let Some(number) = result.get("number") {
                            if let Some(num_str) = number.as_str() {
                                block_number = Some(num_str.to_string());
                            }
                        }
                    }
                }
            }
            
            let mut code_ok = false;
            let mut code_duration = 0u64;
            let mut bytecode: Option<String> = None;
            
            if let Ok((ok, data, dur)) = code_result {
                code_ok = ok;
                code_duration = dur;
                if let Some(json_data) = data {
                    if let Some(result) = json_data.get("result") {
                        if let Some(code_str) = result.as_str() {
                            bytecode = Some(code_str.to_string());
                        }
                    }
                }
            }
            
            let bytecode_ok = is_permit2_bytecode_valid(bytecode.as_deref());
            let success = block_ok && code_ok && bytecode_ok;
            let duration = std::cmp::max(block_duration, code_duration);
            
            RpcCheckResult {
                url,
                success,
                duration,
                block_number,
                bytecode_ok,
            }
        }
    }).collect();
    
    let results = join_all(tasks).await;
    
    // Determine most common block number
    let mut counts: HashMap<String, usize> = HashMap::new();
    for result in &results {
        if let Some(ref block_num) = result.block_number {
            *counts.entry(block_num.clone()).or_insert(0) += 1;
        }
    }
    
    let most_common = counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(block_num, _)| block_num);
    
    // Build latency map excluding out-of-sync RPCs
    let mut latencies = HashMap::new();
    for result in &results {
        if !result.success {
            continue;
        }
        
        // Skip if out of sync with most common block number
        if let (Some(block_num), Some(common)) = (&result.block_number, &most_common) {
            if block_num != common {
                continue;
            }
        }
        
        latencies.insert(result.url.clone(), result.duration);
    }
    
    Ok((latencies, results))
}
