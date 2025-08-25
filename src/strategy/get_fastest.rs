use std::time::Duration;
use crate::{performance::measure_rpcs, Rpc, Result};

pub async fn get_fastest(rpcs: &[Rpc], timeout: Duration) -> Result<(Option<String>, std::collections::HashMap<String, u64>)> {
    let (latencies, _check_results) = measure_rpcs(rpcs, timeout).await?;
    
    let fastest = latencies
        .iter()
        .min_by_key(|(_, latency)| *latency)
        .map(|(url, _)| url.clone());
    
    Ok((fastest, latencies))
}
