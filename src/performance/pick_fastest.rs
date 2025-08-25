use crate::performance::LatencyMap;

pub fn pick_fastest(latencies: &LatencyMap) -> Option<String> {
    latencies
        .iter()
        .min_by_key(|(_, latency)| *latency)
        .map(|(url, _)| url.clone())
}
