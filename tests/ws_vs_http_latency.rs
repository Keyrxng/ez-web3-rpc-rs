//! Ad-hoc comparative latency harness: existing HTTP handler vs raw WebSocket calls.
//!
//! Scope (Gnosis network):
//! - Methods: eth_blockNumber, eth_gasPrice, single "heavy" eth_getBlockByNumber(true) request
//! - Re-uses a single persistent WebSocket connection
//! - Simple sequential sends (no pipelining) to keep logic minimal
//! - Collects: mean, median, p95, min, max, stddev
//!
//! Environment knobs (set via env vars before running):
//! - WS_BENCH_ITER=50 (default 30): iterations per lightweight method
//! - WS_BENCH_HEAVY=1 (include heavy block fetch; default 1)
//! - WS_BENCH_BLOCK (override block tag for heavy fetch; default "latest")
//! - WS_BENCH_WARMUP=3 (warmup iterations per method ignored in stats; default 3)
//!
//! Run manually (ignored by default):
//!     cargo test ws_vs_http_latency -- --ignored --nocapture
//!
//! Caveats:
//! - Not statistically rigorous; no outlier rejection beyond p95 summary.
//! - Sequential pattern may favor connection keep-alive vs real-world concurrent scenarios.
//! - HTTP handler latency already benefits from persistent connections under reqwest's pool.
//! - WebSocket implementation here is minimal and doesn't batch / pipeline / compress.
//! - Heavy block fetch may be cached at provider edge depending on block freshness.

use ez_web3_rpc::{HandlerConfig, JsonRpcRequest, RpcHandler};
use serde_json::{json, Value};
use tokio_tungstenite::connect_async;
use futures::{SinkExt, StreamExt};
use tokio::time::{Instant, Duration};
use rand::{rngs::StdRng, SeedableRng, Rng};
use std::{env, collections::HashMap};

#[ignore]
#[tokio::test]
async fn ws_vs_http_latency() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();

    // Config
    let iterations: usize = env::var("WS_BENCH_ITER").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
    let warmup: usize = env::var("WS_BENCH_WARMUP").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let include_heavy: bool = env::var("WS_BENCH_HEAVY").ok().map(|v| v != "0").unwrap_or(true);
    let heavy_block_tag = env::var("WS_BENCH_BLOCK").unwrap_or_else(|_| "latest".to_string());

    println!("Config => iterations: {iterations}, warmup: {warmup}, heavy: {include_heavy}, block_tag: {heavy_block_tag}");

    // HTTP via existing handler
    let config = HandlerConfig::new(100); // Gnosis
    let handler = RpcHandler::new(Some(config), 100).await?;

    // Methods to probe (lightweight)
    let lightweight_methods = ["eth_blockNumber", "eth_gasPrice"];
    let mut http_samples: HashMap<&'static str, Vec<Duration>> = HashMap::new();
    for m in &lightweight_methods { http_samples.insert(*m, Vec::with_capacity(iterations)); }

    for m in &lightweight_methods {
        // warmup
        for _ in 0..warmup { let _ = handler.try_proxy_request(JsonRpcRequest { jsonrpc: "2.0".into(), method: (*m).into(), params: json!([]), id: Some(1) }).await?; }
        for _ in 0..iterations { let start = Instant::now(); let _ = handler.try_proxy_request(JsonRpcRequest { jsonrpc: "2.0".into(), method: (*m).into(), params: json!([]), id: Some(1) }).await?; http_samples.get_mut(m).unwrap().push(start.elapsed()); }
    }

    // WebSocket raw baseline
    let url = url::Url::parse("wss://gnosis-rpc.publicnode.com")?;
    let (ws_stream, _resp) = connect_async(url).await?;
    let (mut write, mut read) = ws_stream.split();

    let mut ws_samples: HashMap<&'static str, Vec<Duration>> = HashMap::new();
    for m in &lightweight_methods { ws_samples.insert(*m, Vec::with_capacity(iterations)); }
    let mut next_id: u64 = 1;
    // Warmup per method (WebSocket)
    for m in &lightweight_methods {
        for _ in 0..warmup { single_ws_roundtrip(*m, &mut write, &mut read, &mut next_id).await?; }
        for _ in 0..iterations { let d = single_ws_roundtrip(*m, &mut write, &mut read, &mut next_id).await?; ws_samples.get_mut(m).unwrap().push(d); }
    }

    // Optional heavy block fetch comparison (HTTP vs WS) once
    let mut heavy_http: Option<Duration> = None;
    let mut heavy_ws: Option<Duration> = None;
    if include_heavy {
        // Choose a block tag (latest) or potentially random recent block for HTTP & WS parity
        let tag_param = heavy_block_tag.clone();
        // HTTP heavy
        let req = JsonRpcRequest { jsonrpc: "2.0".into(), method: "eth_getBlockByNumber".into(), params: json!([tag_param, true]), id: Some(777) };
        let start = Instant::now();
        let _ = handler.try_proxy_request(req).await?;
        heavy_http = Some(start.elapsed());
        // WS heavy
    let heavy_dur = single_ws_custom(json!({"jsonrpc":"2.0","id": next_id, "method":"eth_getBlockByNumber","params":[heavy_block_tag, true]}), &mut write, &mut read, next_id).await?; next_id += 1; heavy_ws = Some(heavy_dur);
    }

    fn summarize(label: &str, samples: &[Duration]) -> (f64, f64) {
        let mut ms: Vec<f64> = samples.iter().map(|d| d.as_secs_f64()*1000.0).collect();
        ms.sort_by(|a,b| a.partial_cmp(b).unwrap());
        if ms.is_empty() { println!("{label}: no samples"); return (0.0,0.0); }
        let mean = ms.iter().sum::<f64>() / ms.len() as f64;
        let median = if ms.len()%2==1 { ms[ms.len()/2] } else { (ms[ms.len()/2-1]+ms[ms.len()/2])/2.0 };
        let p95_idx = ((ms.len() as f64)*0.95).ceil() as usize - 1; // len>=1
        let p95 = ms[p95_idx.min(ms.len()-1)];
        let min = ms.first().copied().unwrap();
        let max = ms.last().copied().unwrap();
        let variance = ms.iter().map(|v| (v-mean).powi(2)).sum::<f64>() / (ms.len().saturating_sub(1) as f64).max(1.0);
        let stddev = variance.sqrt();
        println!("{label}: mean {:.2} | median {:.2} | p95 {:.2} | min {:.2} | max {:.2} | std {:.2} | n {}", mean, median, p95, min, max, stddev, ms.len());
        (mean, median)
    }

    println!("\nLightweight method latency comparison ({} iterations each, warmup {}):", iterations, warmup);
    let mut rng = StdRng::seed_from_u64(42);
    for m in &lightweight_methods { // stable order
        let http_stats = summarize(&format!("HTTP {m}"), http_samples.get(m).unwrap());
        let ws_stats = summarize(&format!("WS   {m}"), ws_samples.get(m).unwrap());
        println!("  Speed ratio HTTP/WS (mean): {:.2}x", http_stats.0 / ws_stats.0.max(1e-9));
        // Random small spacer jitter to mimic human-read log spacing (irrelevant to logic)
    if rng.gen_range(0.0f32..1.0) < 0.3 { println!(); }
    }

    if let (Some(hh), Some(hw)) = (heavy_http, heavy_ws) {
        let hh_ms = hh.as_secs_f64()*1000.0; let hw_ms = hw.as_secs_f64()*1000.0;
        println!("\nHeavy eth_getBlockByNumber(true) one-shot: HTTP {:.2} ms | WS {:.2} ms | ratio {:.2}x", hh_ms, hw_ms, hh_ms / hw_ms.max(1e-9));
    }

    Ok(())
}

// --- Helpers ---

async fn single_ws_roundtrip(method: &'static str, write: &mut (impl SinkExt<tokio_tungstenite::tungstenite::Message> + Unpin), read: &mut (impl StreamExt<Item=Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>> + Unpin), next_id: &mut u64) -> anyhow::Result<Duration> {
    let id = *next_id; *next_id += 1;
    let payload = json!({"jsonrpc":"2.0","id": id, "method": method, "params": []});
    single_ws_custom(payload, write, read, id).await
}

async fn single_ws_custom(payload: Value, write: &mut (impl SinkExt<tokio_tungstenite::tungstenite::Message> + Unpin), read: &mut (impl StreamExt<Item=Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>> + Unpin), id: u64) -> anyhow::Result<Duration> {
    let txt = serde_json::to_string(&payload)?;
    let start = Instant::now();
    if let Err(_e) = write.send(tokio_tungstenite::tungstenite::Message::Text(txt)).await { anyhow::bail!("ws send error"); }
    while let Some(msg) = read.next().await {
        let msg = msg?;
        if let tokio_tungstenite::tungstenite::Message::Text(txt) = msg {
            if let Ok(val) = serde_json::from_str::<Value>(&txt) {
                if val.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    return Ok(start.elapsed());
                }
            }
        }
    }
    anyhow::bail!("WebSocket closed before receiving id {id}")
}
