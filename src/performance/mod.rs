pub mod measure;
pub mod pick_fastest;

pub use measure::{measure_rpcs, LatencyMap, RpcCheckResult};
pub use pick_fastest::pick_fastest;
