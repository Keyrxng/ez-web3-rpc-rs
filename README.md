# ez-web3-rpc (Rust)

High‑performance, asynchronous JSON‑RPC handler for EVM chains with automatic endpoint discovery (from embedded chain data), latency probing, failover retries, and a pragmatic ergonomics-first API.

> This is an initial Rust rewrite of the original TypeScript package `@keyrxng/ez-web3-rpc`. The scope today is a lean core (latency probing + fastest endpoint selection + retries). Additional features from the TS version (multi-endpoint racing per call, consensus helpers, richer filtering, adaptive refresh) are on the roadmap.

## Why

If you routinely interact with public Web3 RPC endpoints you know the pain: inconsistent latency, silent stalls, and manual curation of provider lists. This crate encapsulates the boring bits so your application can talk to “one logical provider” that automatically prefers fast/healthy endpoints and retries transient failures.

## Benchmarks (early snapshot)

Single-run micro benchmark on network `100` (Gnosis) over 20 iterations (cold start + 19 warm).

High-level:

| Phase | Mean (ms) | Median (ms) | p95 (ms) | Success |
|-------|---------:|-----------:|--------:|--------:|
| Init probe (handler construction) | 3098.78 | 3096.48 | 3116.05 | 20/20 |
| eth_blockNumber | 30.81 | 30.97 | 32.41 | 20/20 |
| Heavy block fetch* | 24.46 | 25.10 | 29.17 | 20/20 |
| eth_gasPrice | 24.47 | 24.93 | 26.90 | 20/20 |

(*"Heavy" here represents a slightly larger payload request used in the harness — not necessarily worst‑case.)

- Cold vs warm difference was negligible for this network (cold `eth_blockNumber` 30.47ms vs warm mean 30.83ms — within noise).
- Per-iteration composite latency mean distribution (internal aggregation): mean 230.83ms, median 224.29ms across 20 iterations (includes init weighting in first cycle; dominated by the initial ~3.1s probe phase).

Interpretation:

- First call cost is dominated by the one-time parallel probing of candidate RPCs (~3.1s with the default 3s probe timeout window).
- After initialization, typical JSON-RPC read latency in this run was ~25–31ms on Gnosis public endpoints.
- Reducing `rpc_probe_timeout_ms` (default 3000) can shrink cold-start at the risk of discarding slower-yet-healthy endpoints.

Benchmark numbers are indicative only; real performance depends on network location, chosen endpoints, and concurrent system load.

## Key capabilities (current)

- Embedded chain & RPC metadata (generated at build time) — no runtime fetch needed.
- Fastest endpoint selection: probe all configured RPCs (plus embedded extras) and keep latency records.
- Simple proxy: send a JSON-RPC request through the currently fastest endpoint.
- Configurable retries & backoff (fixed delay) for transient failures.
- Structured errors via `RpcHandlerError` (timeout, network, exhaustion, etc.).
- Pluggable settings: log level, custom RPC URLs, tracking preference, probe timeout.
- Chain data pruning: optionally retain only the target chain to slim memory (default behavior).

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
ez_web3_rpc = "0.1"
```

Requires Rust 1.82+ (2024 edition) and Tokio (brought in automatically).

## Quick start

```rust
use ez_web3_rpc::{
    RpcHandler, HandlerConfig, JsonRpcRequest, Result
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Pick a network (example: Gnosis = 100). You can choose any embedded chain ID.
    let config = HandlerConfig::new(100);
    let handler = RpcHandler::new(Some(config), 100).await?;

    // Obtain the fastest probed RPC URL (optional — handler uses it internally too)
    let fastest = handler.get_fastest_rpc(None).await?;
    println!("Fastest RPC: {fastest}");

    // Build a JSON-RPC request
    let req = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        method: "eth_blockNumber".into(),
        params: json!([]),
        id: Some(1),
    };

    // Proxy through handler (with retries) — returns deserialized JSON response
    let resp = handler.try_proxy_request(req).await?;
    println!("Block: {:?}", resp.result);

    Ok(())
}
```

## Core types

| Type | Purpose |
|------|---------|
| `RpcHandler` | Main orchestrator: probes RPCs, tracks latency, proxies requests. |
| `HandlerConfig` / `HandlerSettings` | Build-time style config (network, timeouts, custom RPCs, logging). |
| `ProxySettings` | Retry policy (count, delay, per-call timeout placeholder). |
| `LatencyRecord` | Latency (ms), last test time, failure count. |
| `JsonRpcRequest` / `JsonRpcResponse<T>` | Lightweight JSON-RPC model structs. |
| `RpcHandlerError` | Error enum (network, timeout, no RPCs, all failed, JSON-RPC code). |

## Configuration overview

```rust
let mut config = HandlerConfig::new(100); // network id
// Access & modify nested settings if you need to customize:
let settings = config.settings.as_mut().unwrap();
// Add your own private / paid RPC endpoints (preferred if fast)
// settings.network_rpcs.push(Rpc { url: Url::parse("https://my-node.example")?, tracking: None, tracking_details: None, is_open_source: None });
// Adjust probe timeout
settings.rpc_probe_timeout_ms = 2_500;
// Change log level (Error | Warn | Info | Debug | Trace)
settings.log_level = ez_web3_rpc::LogLevel::Info;
// Proxy (retry) tuning
if let Some(proxy) = settings.proxy_settings.as_mut() { proxy.retry_count = 5; proxy.retry_delay_ms = 750; }
```

### Retry behavior

`try_proxy_request` will attempt the fastest known RPC up to `retry_count` times, sleeping `retry_delay_ms` between attempts. A future enhancement will broaden this to rotate or race multiple candidates per attempt.

### Chain data pruning

By default generated chain data is reduced to the single target chain (memory conscious). Set `wipe_chain_data.clear_data = false` if you later expose multi-chain features.

## Logging

Set `settings.log_level`. The crate uses `tracing` — install a subscriber (e.g. `tracing_subscriber::fmt::init()`) in your binary and filter with `RUST_LOG=ez_web3_rpc=info` etc.

## Examples

Run the included Gnosis example:

```bash
cargo run --example gnosis_latency
```

## Design notes

- Data-first: chain metadata is embedded at build-time (no network fetch). See `build.rs` for generation logic (not yet fully documented here).
- Non-blocking: uses `reqwest` + Tokio for async I/O and concurrent probe racing.
- Minimal surface: only the obvious ergonomic entrypoints are exposed in `lib.rs` re-exports.

## Contributing

Small, focused PRs welcome. Please keep changes scoped and include rationale.

Suggested flow:

1. Fork & branch
2. `cargo fmt && cargo clippy --all-targets --all-features -- -D warnings`
3. Add / update tests (where logical) & run `cargo test`
4. Document new public items (rustdoc)
5. Open PR with concise description

## License

MIT © keyrxng

---

If this crate saves you time, a star helps others discover it.
