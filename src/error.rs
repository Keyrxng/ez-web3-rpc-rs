#[derive(Debug, thiserror::Error)]
pub enum RpcHandlerError {
    #[error("No available RPCs for network {network_id}")]
    NoAvailableRpcs { network_id: crate::NetworkId },

    #[error("JSON-RPC error from {0}")]
    JsonRpc(String),

    #[error("Request timed out after {duration_ms}ms")]
    Timeout { duration_ms: u64 },

    #[error("All endpoints failed")]
    AllEndpointsFailed,

    #[error("Consensus failure: {most_common}")]
    ConsensusFailure { most_common: String },

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Timeout error: {0}")]
    TimeoutError(#[from] tokio::time::error::Elapsed),

    #[error("Chain info not found for network {network_id}")]
    ChainInfoNotFound { network_id: crate::NetworkId },
}

pub type Result<T> = std::result::Result<T, RpcHandlerError>;