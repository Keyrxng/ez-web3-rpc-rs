use std::time::Duration;
use crate::{performance::measure_rpcs, Rpc, Result};

/// Find first healthy RPC by running health checks sequentially after parallel pre-flight.
/// 
/// If no healthy RPC is found, returns None.
/// 
/// Note: HTTP RPCs are only checked if the `http` option is enabled. (i.e localhost)
pub async fn get_first_healthy(rpcs: &[Rpc], timeout: Duration, http: Option<bool>) -> Result<Option<String>> {
    let http_allowed = http.unwrap_or(false);
    
    let filtered_rpcs: Vec<&Rpc> = rpcs
        .iter()
        .filter(|rpc| {
            let url = rpc.url.as_str();
            url.starts_with("https://") || (http_allowed && url.starts_with("http://"))
        })
        .collect();
    
    if filtered_rpcs.is_empty() {
        return Ok(None);
    }
    
    // Shuffle to avoid always hitting the same RPC first
    let mut shuffled = filtered_rpcs.clone();
    {
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        shuffled.shuffle(&mut rng);
    } // rng is dropped here, so it won't be across await points
    
    for rpc in shuffled {
        let single_rpc = vec![rpc.clone()];
        if let Ok((latencies, _)) = measure_rpcs(&single_rpc, timeout).await {
            if !latencies.is_empty() {
                return Ok(Some(rpc.url.to_string()));
            }
        }
    }
    
    Ok(None)
}
