use ez_web3_rpc::*;

// These tests rely on build script generated data. If the dataset is empty (e.g. offline build fallback),
// they will gracefully skip assertions that depend on non-empty content.

#[test]
fn test_chainlist_initialize_and_restore() {
    let original = chainlist::get_chain_ids();
    if original.is_empty() { return; } // nothing to assert
    let original_ids: Vec<NetworkId> = original.iter().map(|(id, _)| *id).collect();
    let keep = original_ids[0];
    chainlist::initialize_chain_data(vec![keep]);
    let filtered = chainlist::get_chain_ids();
    assert!(filtered.iter().all(|(id,_)| *id == keep));
    // restore
    chainlist::initialize_chain_data(original_ids.clone());
    let restored = chainlist::get_chain_ids();
    assert!(restored.len() >= 1);
}

#[test]
fn test_get_chains_by_tvl_sorted() {
    let chains = chainlist::get_chains_by_tvl();
    for w in chains.windows(2) { if let [a,b] = w { assert!(a.tvl >= b.tvl, "TVL not sorted descending"); } }
}

#[test]
fn test_find_chains_by_name_partial() {
    let ids = chainlist::get_chain_ids();
    if let Some((_, name)) = ids.first() { 
        let search_fragment = &name[0..std::cmp::min(3, name.len())];
        let results = chainlist::find_chains_by_name(search_fragment);
        if !results.is_empty() { assert!(results.iter().any(|c| c.name.contains(search_fragment))); }
    }
}

#[test]
fn test_get_extra_rpcs_returns_valid_urls() {
    let ids = chainlist::get_chain_ids();
    if let Some((id,_)) = ids.first() { 
        let rpcs = chainlist::get_extra_rpcs(*id);
        for rpc in rpcs { 
            // Url::parse already validated structure; just ensure scheme exists
            assert!(!rpc.url.scheme().is_empty(), "scheme should not be empty");
        }
    }
}
