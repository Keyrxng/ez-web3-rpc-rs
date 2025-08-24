use ez_web3_rpc::{HandlerConfig, JsonRpcRequest, RpcHandler};
use std::time::Instant;
use std::{fs, path::Path};
use serde::{Serialize};
use chrono::Utc;

#[derive(Serialize, Clone)]
struct Sample { ms: f64, ok: bool }

fn now_ms(start: Instant) -> f64 { let d = start.elapsed(); d.as_secs_f64() * 1000.0 }

fn stats(samples: &Vec<Sample>) -> serde_json::Value {
    let count = samples.len();
    let success = samples.iter().filter(|s| s.ok).count();
    let failures = count - success;
    let ok_vals: Vec<f64> = samples.iter().filter(|s| s.ok).map(|s| s.ms).collect();
    if ok_vals.is_empty() {
        return serde_json::json!({"mean":null,"median":null,"p95":null,"count":count,"success":success,"failures":failures,"success_ratio":0});
    }
    let mean = ok_vals.iter().sum::<f64>() / ok_vals.len() as f64;
    let mut sorted = ok_vals.clone(); sorted.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let median = if sorted.len() % 2 == 1 { sorted[(sorted.len()-1)/2] } else { (sorted[sorted.len()/2 -1] + sorted[sorted.len()/2]) / 2.0 };
    let p95_idx = ((sorted.len() as f64) * 0.95).floor() as usize;
    let p95 = sorted[std::cmp::min(p95_idx, sorted.len()-1)];
    serde_json::json!({"mean": mean, "median": median, "p95": p95, "count": count, "success": success, "failures": failures, "success_ratio": (success as f64)/ (count as f64)})
}

fn stats_opt_vec(vals: &Vec<Option<f64>>) -> serde_json::Value {
    let filtered: Vec<f64> = vals.iter().filter_map(|v| *v).collect();
    let count = vals.len();
    let present = filtered.len();
    if filtered.is_empty() {
        return serde_json::json!({"mean": null, "median": null, "count": count, "present": present});
    }
    let mean = filtered.iter().sum::<f64>() / (filtered.len() as f64);
    let mut s = filtered.clone(); s.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let median = if s.len() % 2 == 1 { s[(s.len()-1)/2] } else { (s[s.len()/2 -1] + s[s.len()/2]) / 2.0 };
    serde_json::json!({"mean": mean, "median": median, "count": count, "present": present})
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Config from env or defaults
    let network_id: u64 = std::env::var("NETWORK_ID").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let iterations: usize = std::env::var("ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);

    println!("[bench] network={} iterations={}", network_id, iterations);

    let cfg = HandlerConfig::new(network_id);

    let mut init_samples: Vec<Sample> = Vec::new();
    let mut block_samples: Vec<Sample> = Vec::new();
    let mut heavy_samples: Vec<Sample> = Vec::new();
    let mut gas_samples: Vec<Sample> = Vec::new();
    let mut per_iter_latency_means: Vec<Option<f64>> = Vec::new();

    for i in 0..iterations {
        // time creation/init
        let t0 = Instant::now();
        let handler_res = RpcHandler::new(Some(cfg.clone()), network_id).await;
        let ms = now_ms(t0);
        match handler_res {
            Ok(handler) => {
                init_samples.push(Sample{ ms, ok: true });
                // record initial probe duration (ms) if available
                // compute mean latency across all returned latencies in handler
                let lat_map = handler.get_latencies();
                let mut lat_vals: Vec<f64> = lat_map.iter().map(|kv| kv.value().latency_ms as f64).collect();
                if lat_vals.is_empty() {
                    per_iter_latency_means.push(None);
                } else {
                    let sum: f64 = lat_vals.iter().sum();
                    let mean = sum / (lat_vals.len() as f64);
                    per_iter_latency_means.push(Some(mean));
                }

                // call eth_blockNumber via proxy (actual block number RPC)
                let t1 = Instant::now();
                let block_req = JsonRpcRequest { id: Some(1), jsonrpc: "2.0".into(), method: "eth_blockNumber".into(), params: serde_json::Value::Array(vec![]) };
                let block_res = handler.try_proxy_request(block_req).await;
                let ms_block = now_ms(t1);
                block_samples.push(Sample{ ms: ms_block, ok: block_res.is_ok() });

                // use handler.try_proxy_request to send eth_getBlockByNumber (heavy)
                let t2 = Instant::now();
                let heavy_req = JsonRpcRequest { id: Some(1), jsonrpc: "2.0".into(), method: "eth_getBlockByNumber".into(), params: serde_json::json!( ["latest", true]) };
                let heavy_res = handler.try_proxy_request(heavy_req).await;
                let ms_heavy = now_ms(t2);
                heavy_samples.push(Sample{ ms: ms_heavy, ok: heavy_res.is_ok() });

                // gas price
                let t3 = Instant::now();
                let gas_req = JsonRpcRequest { id: Some(1), jsonrpc: "2.0".into(), method: "eth_gasPrice".into(), params: serde_json::Value::Array(vec![]) };
                let gas_res = handler.try_proxy_request(gas_req).await;
                let ms_gas = now_ms(t3);
                gas_samples.push(Sample{ ms: ms_gas, ok: gas_res.is_ok() });
            }
            Err(e) => {
                init_samples.push(Sample{ ms, ok: false });
                // push failures for others as NaN with ok=false
                block_samples.push(Sample{ ms: 0.0, ok: false });
                heavy_samples.push(Sample{ ms: 0.0, ok: false });
                gas_samples.push(Sample{ ms: 0.0, ok: false });
                eprintln!("handler init failed: {:?}", e);
            }
        }
    }

    // Cold vs warm split (iteration 0 cold)
    let slice_warm = |v: &Vec<Sample>| -> Vec<Sample> { if v.len() > 1 { v[1..].to_vec() } else { Vec::new() } };
    let cold_init = if init_samples.len() > 0 { vec![init_samples[0].clone()] } else { Vec::new() };
    let cold_block = if block_samples.len() > 0 { vec![block_samples[0].clone()] } else { Vec::new() };
    let cold_heavy = if heavy_samples.len() > 0 { vec![heavy_samples[0].clone()] } else { Vec::new() };
    let cold_gas = if gas_samples.len() > 0 { vec![gas_samples[0].clone()] } else { Vec::new() };
    let cold = serde_json::json!({
        "init": stats(&cold_init),
        "block": stats(&cold_block),
        "heavy": stats(&cold_heavy),
        "gas": stats(&cold_gas)
    });
    let warm = serde_json::json!({
        "init": stats(&slice_warm(&init_samples)),
        "block": stats(&slice_warm(&block_samples)),
        "heavy": stats(&slice_warm(&heavy_samples)),
        "gas": stats(&slice_warm(&gas_samples))
    });

    let per_iter_latency_aggregates = stats_opt_vec(&per_iter_latency_means);

    let out = serde_json::json!({
        "network_id": network_id,
        "iterations": iterations,
        "init": stats(&init_samples),
        "block": stats(&block_samples),
        "heavy": stats(&heavy_samples),
        "gas": stats(&gas_samples),
        "cold": cold,
        "warm": warm,
        "per_iteration_latency_means": { "per_iter": per_iter_latency_means, "aggregates": per_iter_latency_aggregates }
    });

    println!("JSON_RESULT {}", serde_json::to_string(&out)?);

    // write markdown
    let results_dir = Path::new("benchmarks/results");
    fs::create_dir_all(results_dir)?;
    let ts = Utc::now().to_rfc3339();
    let fname = format!("bench-{}-{}iter-{}.md", network_id, iterations, ts.replace(':', "-"));
    let fpath = results_dir.join(fname);
    let mut md = String::new();
    md.push_str(&format!("# RPC Benchmark\n\nNetwork {} iterations {}\n\n", network_id, iterations));
    md.push_str("| Metric | Mean (ms) | Median (ms) | p95 (ms) | Success | Count |\n| --- | ---: | ---: | ---: | ---: | ---: |\n");
    let add_row = |name: &str, v: &serde_json::Value, md: &mut String| {
        let mean = v.get("mean").and_then(|x| x.as_f64()).map(|n| format!("{:.2}", n)).unwrap_or("NaN".into());
        let median = v.get("median").and_then(|x| x.as_f64()).map(|n| format!("{:.2}", n)).unwrap_or("NaN".into());
        let p95 = v.get("p95").and_then(|x| x.as_f64()).map(|n| format!("{:.2}", n)).unwrap_or("NaN".into());
        let success = v.get("success").and_then(|x| x.as_u64()).unwrap_or(0);
        let count = v.get("count").and_then(|x| x.as_u64()).unwrap_or(0);
        md.push_str(&format!("| {} | {} | {} | {} | {} | {} |\n", name, mean, median, p95, success, count));
    };
    add_row("Init", &out["init"], &mut md);
    add_row("BlockNumber", &out["block"], &mut md);
    add_row("HeavyBlock", &out["heavy"], &mut md);
    add_row("GasPrice", &out["gas"], &mut md);

    // Cold vs Warm
    md.push_str("\n## Cold vs Warm (iteration 0 = cold)\n\n");
    md.push_str("| Phase | Variant | Mean (ms) | Median (ms) | p95 (ms) | Success | Count |\n| ----- | ------- | ----: | ------: | -----: | ----: | ----: |\n");
    let row_cw = |phase: &str, variant: &str, v: &serde_json::Value, md: &mut String| {
        let mean = v.get("mean").and_then(|x| x.as_f64()).map(|n| format!("{:.2}", n)).unwrap_or("NaN".into());
        let median = v.get("median").and_then(|x| x.as_f64()).map(|n| format!("{:.2}", n)).unwrap_or("NaN".into());
        let p95 = v.get("p95").and_then(|x| x.as_f64()).map(|n| format!("{:.2}", n)).unwrap_or("NaN".into());
        let success = v.get("success").and_then(|x| x.as_u64()).unwrap_or(0);
        let count = v.get("count").and_then(|x| x.as_u64()).unwrap_or(0);
        md.push_str(&format!("| {} | {} | {} | {} | {} | {} | {} |\n", phase, variant, mean, median, p95, success, count));
    };
    row_cw("Cold","Init", &out["cold"]["init"], &mut md);
    row_cw("Cold","Block", &out["cold"]["block"], &mut md);
    row_cw("Cold","Heavy", &out["cold"]["heavy"], &mut md);
    row_cw("Cold","Gas", &out["cold"]["gas"], &mut md);
    row_cw("Warm","Init", &out["warm"]["init"], &mut md);
    row_cw("Warm","Block", &out["warm"]["block"], &mut md);
    row_cw("Warm","Heavy", &out["warm"]["heavy"], &mut md);
    row_cw("Warm","Gas", &out["warm"]["gas"], &mut md);

    // Initial probe durations
    md.push_str("\n## Initial probe durations (ms)\n\n");
    md.push_str("Per-iteration probe durations (ms):\n\n");
    md.push_str(&format!("`{}`\n\n", out["initial_probe_durations_ms"].to_string()));

    // Per-iteration latency means
    md.push_str("## Per-iteration handler latency means (ms)\n\n");
    md.push_str(&format!("Per-iteration means: `{}`\n\n", out["per_iteration_latency_means"]["per_iter"].to_string()));
    md.push_str("Aggregates for per-iteration latency means:\n\n");
    md.push_str(&format!("`{}`\n\n", out["per_iteration_latency_means"]["aggregates"].to_string()));
    fs::write(&fpath, md)?;
    println!("WROTE_MARKDOWN {}", fpath.display());

    Ok(())
}
