use ez_web3_rpc::*;
use serde_json;

#[test]
fn test_proxy_settings_default() {
    let d = ProxySettings::default();
    assert_eq!(d.retry_count, 3);
    assert_eq!(d.retry_delay_ms, 1000);
    assert_eq!(d.rpc_call_timeout_ms, 5000);
}

#[test]
fn test_latency_record_serialization_roundtrip() {
    let record = LatencyRecord { latency_ms: 42, last_tested: std::time::SystemTime::now(), failure_count: 1 };
    let json = serde_json::to_string(&record).unwrap();
    let deser: LatencyRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.latency_ms, 42);
    assert_eq!(deser.failure_count, 1);
}

#[test]
fn test_handler_config_new_defaults() {
    // pick an existing chain id if possible else skip test early
    let maybe_id = chainlist::get_chain_ids().first().cloned();
    if let Some((id, name)) = maybe_id { 
        let cfg = HandlerConfig::new(id);
        let settings = cfg.settings.unwrap();
        assert_eq!(settings.network_name, name);
        assert_eq!(settings.log_level as u8, LogLevel::Error as u8);
        assert_eq!(settings.tracking as u8, Tracking::Limited as u8);
        assert!(settings.proxy_settings.is_some());
    }
}
