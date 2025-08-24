use crate::types::{NetworkId, Rpc};
use url::Url;

// Include the build-time generated chainlist data
include!(concat!(env!("OUT_DIR"), "/chainlist_data.rs"));

pub fn initialize_chain_data(chains_to_retain: Vec<NetworkId>) {
    /*
     * Calling `.lock()` on a mutex gives us a guard object that holds the lock
     * until it goes out of scope. Each lock is placed into own scope so that it
     * is released immediately after data is filtered.
     *
     * This avoids holding multiple locks at the same time, which reduces the risk
     * of deadlocks and improves concurrency.
     */

    {
        let mut chain_data = CHAIN_DATA.lock();
        chain_data.retain(|chain| chains_to_retain.contains(&chain.chain_id));
    }

    {
        let mut chain_ids = CHAIN_IDS.lock();
        chain_ids.retain(|(id, _)| chains_to_retain.contains(id));
    }

    {
        let mut extra_rpcs = EXTRA_RPCS_DATA.lock();
        extra_rpcs.retain(|(id,_)| chains_to_retain.contains(id));
    }
}

pub fn get_chain_ids() -> Vec<(NetworkId, String)> {
    CHAIN_IDS.lock().clone()
}

pub fn get_chain_info(chain_id: NetworkId) -> Option<ChainInfo> {
    CHAIN_DATA
        .lock()
        .iter()
        .find(|chain| chain.chain_id == chain_id)
        .cloned()
}

pub fn get_chains_by_tvl() -> Vec<ChainInfo> {
    let mut chains = CHAIN_DATA.lock().clone();
    chains.sort_by(|a, b| {
        b.tvl
            .partial_cmp(&a.tvl)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    chains
}

pub fn find_chains_by_name(name: &str) -> Vec<ChainInfo> {
    let search_term = name.to_lowercase();
    CHAIN_DATA
        .lock()
        .iter()
        .filter(|chain| chain.name.to_lowercase().contains(&search_term))
        .cloned()
        .collect()
}

pub fn get_extra_rpcs(chain_id: NetworkId) -> Vec<Rpc> {
    EXTRA_RPCS_DATA
        .lock()
        .iter()
        .find(|(id, _)| *id == chain_id)
        .map(|(_, rpcs)| {
            rpcs.iter()
                .filter_map(|rpc_url| {
                    Url::parse(rpc_url).ok().map(|url| Rpc {
                        url,
                        tracking: Some(crate::types::Tracking::None),
                        tracking_details: Some("None as default".to_string()),
                        is_open_source: Some(true),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
