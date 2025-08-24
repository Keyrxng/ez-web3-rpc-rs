use serde_json::json;
use ez_web3_rpc::{
    chainlist, 
    handler::{ RpcHandler}, 
    HandlerConfig, 
    JsonRpcRequest, 
    Result
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("Starting Web3 EZ RPC handler");

    println!("Chainlist Data:");
    let chains = chainlist::get_chains_by_tvl();
    println!("  - loaded {} chains from build-time data", chains.len());

    if chains.len() > 0 {
        println!("  - Top 3 chains by TVL:");
        for (i, chain) in chains.iter().take(3).enumerate() {
            println!("  {}. {} (ID: {}, TVL: ${:.0})",
            i + 1, chain.name, chain.chain_id, chain.tvl);
        }
    }

    let config = HandlerConfig::new(100);
    let _retain = config.clone().settings.unwrap().wipe_chain_data.retain_these_chains.clone();
 
    println!("Creating RPC Handler");

    let handler = RpcHandler::new(Some(config), 100).await?;

    println!("RPC Handler initialized successfully!");

    match handler.get_fastest_rpc(None).await {
        Ok(fastest_rpc) => {
            println!("Fastest RPC: {}", fastest_rpc);
            println!("All latencies: {:?}", handler.get_latencies())
        },
        Err(e) => {
            println!("Could not determine fastest RPC: {}", e);
        }
    }

    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "eth_blockNumber".to_string(),
        params: json!([]),
    id: Some(1),
    };

    let response = handler.try_proxy_request(request).await?;

    println!("Response: {:?}", response);
    println!("Response data: {:?}", response.result);


    let rpcs = chainlist::get_extra_rpcs(100);
    let chains = chainlist::get_chain_ids();

    println!("RPCS: {:?}", rpcs);

    println!("CHAIN IDS: {:?}", chains);

    let latenciess = handler.get_latencies();

    println!("All Latencies: {:?}", latenciess);

    Ok(())
}
