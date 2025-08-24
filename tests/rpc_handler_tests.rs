use ez_web3_rpc::*;
use serde_json::json;
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

// Use a network id that won't exist in the generated chainlist data so tests stay hermetic.
const TEST_NETWORK_ID: u64 = 424242;

fn build_mock_jsonrpc_response(id: u64, result: serde_json::Value) -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn normalize(url: &str) -> &str { url.trim_end_matches('/') }

#[tokio::test]
async fn test_handler_initializes_and_selects_fastest_rpc() {
    // spin up two mock servers with slight latency differences
    let server_fast = MockServer::start().await;
    let server_slow = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(build_mock_jsonrpc_response(1, json!("0x1"))))
        .mount(&server_fast)
        .await;

    // slower by sleeping inside response
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_json(build_mock_jsonrpc_response(1, json!("0x1")))
            .set_delay(std::time::Duration::from_millis(50)))
        .mount(&server_slow)
        .await;

    let config = HandlerConfig {
        network_id: TEST_NETWORK_ID,
        settings: Some(HandlerSettings {
            log_level: LogLevel::Error,
            tracking: Tracking::Limited,
            network_rpcs: vec![
                Rpc { url: server_slow.uri().parse().unwrap(), tracking: None, tracking_details: None, is_open_source: Some(true) },
                Rpc { url: server_fast.uri().parse().unwrap(), tracking: None, tracking_details: None, is_open_source: Some(true) },
            ],
            network_name: "local_testnet".to_string(),
            rpc_probe_timeout_ms: 5000,
            proxy_settings: Some(ProxySettings::default()),
            // Ensure we wipe chain data so no external RPC URLs are added.
            wipe_chain_data: WipeChainData { clear_data: true, retain_these_chains: vec![TEST_NETWORK_ID] }
        })
    };

    let handler = RpcHandler::new(Some(config), TEST_NETWORK_ID).await.expect("handler init");
    // Insert synthetic latency records to avoid relying on probe success
    handler.get_latencies().insert(server_fast.uri(), LatencyRecord { latency_ms: 5, last_tested: std::time::SystemTime::now(), failure_count: 0 });
    handler.get_latencies().insert(server_slow.uri(), LatencyRecord { latency_ms: 55, last_tested: std::time::SystemTime::now(), failure_count: 0 });
    let fastest = handler.get_fastest_rpc(None).await.expect("fastest rpc");
    assert_eq!(normalize(&fastest), normalize(&server_fast.uri()));
}

#[tokio::test]
async fn test_try_proxy_request_success() {
    let server = MockServer::start().await;
    // Always succeed for this test
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(build_mock_jsonrpc_response(42, json!("0xabc"))))
        .mount(&server)
        .await;

    let config = HandlerConfig {
        network_id: TEST_NETWORK_ID,
        settings: Some(HandlerSettings {
            log_level: LogLevel::Error,
            tracking: Tracking::Limited,
            network_rpcs: vec![
                Rpc { url: server.uri().parse().unwrap(), tracking: None, tracking_details: None, is_open_source: Some(true) }
            ],
            network_name: "local".to_string(),
            rpc_probe_timeout_ms: 5000,
            proxy_settings: Some(ProxySettings { retry_count: 1, retry_delay_ms: 10, rpc_call_timeout_ms: 1000 }),
            wipe_chain_data: WipeChainData { clear_data: true, retain_these_chains: vec![TEST_NETWORK_ID] }
        })
    };

    let handler = RpcHandler::new(Some(config), TEST_NETWORK_ID).await.unwrap();
    handler.get_latencies().insert(server.uri(), LatencyRecord { latency_ms: 10, last_tested: std::time::SystemTime::now(), failure_count: 0 });

    let request = JsonRpcRequest { jsonrpc: "2.0".into(), method: "eth_chainId".into(), params: json!([]), id: Some(42) };

    let resp = handler.try_proxy_request(request).await.expect("proxy request success");
    assert!(resp.error.is_none());
    assert_eq!(resp.result.unwrap(), json!("0xabc"));
}

#[tokio::test]
async fn test_try_proxy_request_all_fail() {
    let server = MockServer::start().await;

    // All attempts fail
    for _ in 0..3 {
        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
    }

    let config = HandlerConfig {
        network_id: TEST_NETWORK_ID,
        settings: Some(HandlerSettings {
            log_level: LogLevel::Error,
            tracking: Tracking::Limited,
            network_rpcs: vec![
                Rpc { url: server.uri().parse().unwrap(), tracking: None, tracking_details: None, is_open_source: Some(true) }
            ],
            network_name: "local".to_string(),
            rpc_probe_timeout_ms: 5000,
            proxy_settings: Some(ProxySettings { retry_count: 3, retry_delay_ms: 5, rpc_call_timeout_ms: 1000 }),
            wipe_chain_data: WipeChainData { clear_data: true, retain_these_chains: vec![TEST_NETWORK_ID] }
        })
    };

    let handler = RpcHandler::new(Some(config), TEST_NETWORK_ID).await.unwrap();
    handler.get_latencies().insert(server.uri(), LatencyRecord { latency_ms: 10, last_tested: std::time::SystemTime::now(), failure_count: 0 });

    let request = JsonRpcRequest { jsonrpc: "2.0".into(), method: "eth_chainId".into(), params: json!([]), id: Some(2) };

    let err = handler.try_proxy_request(request).await.err().expect("should err");
    assert!(matches!(err, RpcHandlerError::AllEndpointsFailed | RpcHandlerError::JsonRpc(_)));
}

#[tokio::test]
async fn test_get_fastest_rpc_no_available() {
    let config = HandlerConfig {
        network_id: TEST_NETWORK_ID,
        settings: Some(HandlerSettings {
            log_level: LogLevel::Error,
            tracking: Tracking::Limited,
            network_rpcs: vec![],
            network_name: "none".to_string(),
            rpc_probe_timeout_ms: 100,
            proxy_settings: Some(ProxySettings { retry_count: 1, retry_delay_ms: 1, rpc_call_timeout_ms: 50 }),
            wipe_chain_data: WipeChainData { clear_data: true, retain_these_chains: vec![TEST_NETWORK_ID] }
        })
    };

    let handler = RpcHandler::new(Some(config), TEST_NETWORK_ID).await.unwrap();
    // Ensure no latencies inserted
    let err = handler.get_fastest_rpc(None).await.err().expect("expected error");
    assert!(matches!(err, RpcHandlerError::NoAvailableRpcs { .. }));
}
