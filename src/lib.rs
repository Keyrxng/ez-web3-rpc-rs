pub mod calls;
pub mod chainlist;
pub mod config;
pub mod error;
pub mod handler;
pub mod jsonrpc;
pub mod performance;
pub mod provider;
pub mod rpc;
pub mod strategy;
pub mod types;

// Legacy module for backward compatibility
pub mod rpc_service;

pub use error::{RpcHandlerError, Result};
pub use handler::RpcHandler;
pub use jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcError};
pub use types::{
    NetworkId, NetworkName, Rpc, Tracking, LogLevel,
    LatencyRecord, HandlerConfig, ProxySettings, HandlerSettings, WipeChainData
};

// Re-export commonly used items
pub use calls::RpcCalls;
pub use config::{NormalizedConfig, resolve_config};
pub use strategy::Strategy;