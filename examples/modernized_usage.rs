use ez_web3_rpc::{HandlerConfig, RpcHandler, Strategy, JsonRpcRequest};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Create a handler configuration for Ethereum mainnet
    let config = HandlerConfig::new(1); // Ethereum mainnet

    // Create the RPC handler with fastest strategy
    let handler = RpcHandler::new(config, Some(Strategy::Fastest)).await?;
    
    // Initialize the handler (this will test RPCs and find the fastest)
    handler.init().await?;

    println!("Handler initialized successfully!");
    
    // Get latencies
    let latencies = handler.get_latencies().await;
    println!("RPC Latencies: {:?}", latencies);
    
    // Get provider URL
    let provider_url = handler.get_provider_url().await?;
    println!("Using provider: {}", provider_url);

    // Create RpcCalls instance for consensus operations
    let calls = ez_web3_rpc::calls::RpcCalls::new(handler.clone());

    // Test basic RPC call
    let block_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "eth_blockNumber".to_string(),
        params: json!([]),
        id: Some(1),
    };

    match calls.try_rpc_call(&block_request).await {
        Ok(response) => {
            println!("Block number response: {:?}", response);
        }
        Err(e) => {
            println!("Error getting block number: {:?}", e);
        }
    }

    // Test consensus call
    match calls.consensus::<String>(&block_request, 0.66, None).await {
        Ok(block_number) => {
            println!("Consensus block number: {}", block_number);
        }
        Err(e) => {
            println!("Error getting consensus block number: {:?}", e);
        }
    }

    Ok(())
}
