use crate::{chainlist, NetworkId, Rpc, Tracking};

pub fn select_base_rpc_set(network_id: NetworkId, tracking: Tracking, injected_rpcs: Vec<Rpc>) -> Vec<Rpc> {
    let mut rpcs = injected_rpcs;
    
    // Add RPCs from chainlist based on tracking preference
    let chainlist_rpcs = chainlist::get_extra_rpcs(network_id);
    
    for rpc in chainlist_rpcs {
        // Filter based on tracking preference
        let should_include = match tracking {
            Tracking::Yes => true,
            Tracking::Limited => {
                rpc.tracking.as_ref().map_or(true, |t| matches!(t, Tracking::Limited | Tracking::None))
            }
            Tracking::None => {
                rpc.tracking.as_ref().map_or(false, |t| matches!(t, Tracking::None))
            }
        };
        
        if should_include {
            rpcs.push(rpc);
        }
    }
    
    rpcs
}
