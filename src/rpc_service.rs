use std::time::{Duration, Instant};

use futures::future::join_all;
use tokio::time::timeout;

use crate::{JsonRpcRequest, LatencyRecord, Result, Rpc, RpcHandlerError};

pub struct RpcTestingService {
    timeout_duration: Duration,
    pub client: reqwest::Client,
}

impl RpcTestingService {
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            timeout_duration: Duration::from_millis(timeout_ms),
            client: reqwest::Client::new(),
        }
    }

    pub async fn test_rpc_latency(&self, rpc: &Rpc) -> Result<LatencyRecord> {
        let start = Instant::now();

        let test_req = JsonRpcRequest {
            id: 1,
            jsonrpc: "2.0".to_string(),
            method: "eth_blockNumber".to_string(),
            params: serde_json::Value::Array(vec![]),
        };

        let response = timeout(
            self.timeout_duration,
            self.client.post(rpc.url.clone()).json(&test_req).send(),
        )
        .await;

        match response {
            Ok(Ok(resp)) if resp.status().is_success() => {
                let latency = start.elapsed().as_millis() as u64;
                Ok(LatencyRecord {
                    latency_ms: latency,
                    last_tested: std::time::SystemTime::now(),
                    failure_count: 0,
                })
            }
            _ => Err(RpcHandlerError::Timeout {
                duration_ms: self.timeout_duration.as_millis() as u64,
            }),
        }
    }

    pub async fn race_rpcs(&self, rpcs: &[Rpc]) -> Vec<(usize, Result<LatencyRecord>)> {
        let futures: Vec<_> = rpcs
            .iter()
            .enumerate()
            .map(|(idx, rpc)| async move { (idx, self.test_rpc_latency(rpc).await) })
            .collect();

        join_all(futures).await
    }
}
