use crate::{NetworkId, Result};

#[derive(Debug, Clone)]
pub struct JsonRpcProvider {
    pub url: String,
    pub chain_id: NetworkId,
}

pub fn create_provider(url: String, chain_id: NetworkId) -> Result<JsonRpcProvider> {
    Ok(JsonRpcProvider { url, chain_id })
}
