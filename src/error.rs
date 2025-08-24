use thiserror::Error;

#[derive(Error, Debug)]
pub enum RpcHandlerError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("JSON-RPC error: {0}")]
    JsonRpc(#[from] serde_json::Error),

    #[error("No available RPCs for network {network_id}")]
    NoAvailableRpcs { network_id: u64 },

    #[error("Timeout after {duration_ms}ms")]
    Timeout { duration_ms: u64 },

    #[error("All endpoints failed to process your request")]
    AllEndpointsFailed,
}


pub type Result<T> = std::result::Result<T, RpcHandlerError>;