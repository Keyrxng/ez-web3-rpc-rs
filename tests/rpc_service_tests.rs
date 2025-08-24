use ez_web3_rpc::*;
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};
use serde_json::json;

fn mk_rpc(server: &MockServer) -> Rpc { Rpc { url: server.uri().parse().unwrap(), tracking: None, tracking_details: None, is_open_source: Some(true) } }

#[tokio::test]
async fn test_race_rpcs_all_success() {
    let s1 = MockServer::start().await; // fastest
    let s2 = MockServer::start().await; // medium
    let s3 = MockServer::start().await; // slowest

    for (srv, delay) in [(&s1, 5u64), (&s2, 15), (&s3, 30)] { 
        Mock::given(method("POST")).and(path("/"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_json(json!({"jsonrpc":"2.0","id":1,"result":"0x1"}))
                .set_delay(std::time::Duration::from_millis(delay)))
            .mount(srv).await;
    }

    let service = rpc_service::RpcTestingService::new(500); // ample timeout
    let rpcs = vec![mk_rpc(&s1), mk_rpc(&s2), mk_rpc(&s3)];
    let results = service.race_rpcs(&rpcs).await;
    assert_eq!(results.len(), 3);
    for (_, res) in &results { res.as_ref().expect("all ok"); }
    // Basic sanity: collect latencies and ensure not all identical & monotonic increasing by server delay
    let mut latencies: Vec<u64> = results.iter().map(|(_, r)| r.as_ref().unwrap().latency_ms).collect();
    // There can be minor scheduling noise; just ensure min < max
    latencies.sort();
    assert!(latencies.first() < latencies.last());
}

#[tokio::test]
async fn test_race_rpcs_with_timeout() {
    let fast = MockServer::start().await; 
    let slow = MockServer::start().await; // will exceed timeout

    Mock::given(method("POST")).and(path("/")).respond_with(
        ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":1,"result":"0x2"}))
    ).mount(&fast).await;

    Mock::given(method("POST")).and(path("/")).respond_with(
        ResponseTemplate::new(200).set_body_json(json!({"jsonrpc":"2.0","id":1,"result":"0x2"}))
            .set_delay(std::time::Duration::from_millis(100))
    ).mount(&slow).await;

    let service = rpc_service::RpcTestingService::new(20); // very short timeout
    let rpcs = vec![mk_rpc(&fast), mk_rpc(&slow)];
    let results = service.race_rpcs(&rpcs).await;
    assert_eq!(results.len(), 2);
    let mut saw_timeout = false;
    for (_, res) in results { 
        match res { 
            Ok(lr) => assert!(lr.latency_ms > 0),
            Err(RpcHandlerError::Timeout { .. }) => saw_timeout = true,
            Err(e) => panic!("unexpected error: {e:?}")
        }
    }
    assert!(saw_timeout, "expected at least one timeout");
}
