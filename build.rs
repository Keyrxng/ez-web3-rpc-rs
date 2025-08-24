use std::env;
use std::fs;
use std::path::Path;

/**
 * This pulls all of the data used by ChainList prior to building the main crate
 * building out the runtime data structures.
 */

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("chainlist_data.rs");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let chain_data = runtime.block_on(async {
        generate_chainlist_data().await
    });


    match chain_data {
        Ok(data) => {
            fs::write(&dest_path, data).unwrap();
            println!("Generated chainlist data at: {}", dest_path.display());
        }
        Err(e) => {
            eprintln!("Failed to generate chainlist data: {}", e);
            let fallback = r#"
pub static CHAIN_DATA: std::sync::LazyLock<std::sync::Arc<parking_lot::Mutex<Vec<ChainInfo>>>> = std::sync::LazyLock::new(|| {
    std::sync::Arc::new(parking_lot::Mutex::new(vec![]))
});

pub const CHAIN_IDS: std::sync::LazyLock<std::sync::Arc<parking_lot::Mutex<Vec<(NetworkId, String)>>>> = std::sync::LazyLock::new(|| {
    std::sync::Arc::new(parking_lot::Mutex::new(vec![]))
});

pub const EXTRA_RPCS: std::sync::LazyLock<std::sync::Arc<parking_lot::Mutex<Vec<String>>>> = std::sync::LazyLock::new(|| {
    std::sync::Arc::new(parking_lot::Mutex::new(vec![]))
});
"#;

            fs::write(&dest_path, fallback).unwrap();
        }
    }



}

/**
 * Box: heap-allocated smart pointer that owns it's data, memory is auto-deallocateed,
 *      useful for when you don't know the size at compile time or want to transfer ownership
 * 
 * Dyn: `dynamic trait object` is some time that implements this trait but is unknown at compile time
 *      without `dyn` Rust would try use static dispatch (compile-time)
 * 
 * Send: marker trait indicating **cross-thread transfers** are safe and are moveable from one thread to
 *       another. Often the default unlike `Rc<T>`
 * 
 * Sync: marker trait indicating **Cross-thread sharing** is safe when `T` is `Sync`, `&T` can be shared between threads
 * 
 * ===
 * 
 * In context, the below method returns any type of error (network, parsing, file I/O) while ensuring they're safe to use in the async/multi-thread env.
 */

async fn generate_chainlist_data() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use serde::{Deserialize};

    // allows logging, deep copying, and parsing
    #[derive(Debug, Clone, Deserialize)]
    struct ChainResponse {
        #[serde(rename = "chainId")] // fix the casing for json writes
        chain_id: u64,
        name: String, // This struct owns this growable heap-allocated string
        rpc: Vec<String>, // dynamic array of strings
        #[serde(default)] // use None if empty
        status: Option<String> 
    }


    #[derive(Debug, Deserialize)]
    struct TvlResponse {
        name: String,
        tvl: f64
    }

    // http client
    let client = reqwest::Client::new();

    // fat json of chain data: explorerse, rpc providers, native token infos etc.
    let chains_response = client
        .get("https://chainid.network/chains.json") // build a GET req
        .send() // send the req
        .await? // await the resp, propagating any errors
        .json::<Vec<ChainResponse>>() // parse the json into a Vec<ChainResponse>
        .await?; // await unpacking, propagating any errors

        // validates chain credibility etc.
    let tvl_response = client
        .get("https://api.llama.fi/chains")
        .send()
        .await?
        .json::<Vec<TvlResponse>>()
        .await?;

    // mutable arrays for post-processed data
    let mut processed_chains = Vec::new();
    let mut chain_ids = Vec::new();
    let mut extra_rpcs = Vec::new();

    // for loops
    for chain in chains_response {
        // convert the Owner String into a reference, compare with str slice
        if (chain.status.as_deref() == Some("deprecated")) || chain.rpc.is_empty() {
            continue;
        }


        let mut rpcs: Vec<String> = chain.rpc
            .into_iter() // taking ownership via into_inter as we intend to mutate
            .filter(|rpc| !rpc.contains("${INFURA_API_KEY}"))
            .map(|rpc| remove_trailing_slash(&rpc)) // remove any with api placeholders
            .collect(); // return the array


        rpcs.sort();
        rpcs.dedup();


        if !rpcs.is_empty() {
            let chain_name = chain.name.to_lowercase().replace(" ", "_");

            let tvl = tvl_response
                .iter() // borrow each item with iter as we are readonly from here
                .find(|t| t.name.to_lowercase() == chain.name) // find first occurence
                .map(|t| t.tvl) //map into array of tvl value (f64)
                .unwrap_or(0.0); // if not found, use 0.0

            processed_chains.push((chain.chain_id, chain_name.clone(), tvl));
            chain_ids.push((chain.chain_id, chain_name));

            if !rpcs.is_empty() {
                extra_rpcs.push((chain.chain_id, rpcs));
            }
        }
    }

    processed_chains.sort_by(|a, b|b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut output = String::new();

    // start building our dynamic output file

    output.push_str("// Auto-generated chainlist data -- DO NOT EDIT\n\n");

    output.push_str("#[derive(Debug, Clone)]\n");
    output.push_str("pub struct ChainInfo {\n");
    output.push_str("   pub chain_id: NetworkId,\n");
    output.push_str("   pub name: String,\n");
    output.push_str("   pub tvl: f64,\n");
    output.push_str("}\n\n");


    /*
     * Arc: An atomic reference-counted pointer for sharing ownership across threads.
     *      Multiple thread can read OR mutate the same data safely, as long as it's protected by a Mutex
     * 
     * LazyLock: A thread-safe, lazily-initialized static value. It's created only when it's first accessed and 
     *           initializiation is guaranteed to happen only once, even in concurrent scenarios.
     * 
     * Mutex: A mutual exclusion primitive for protecting shared data. Only one thread can access the data at a time.
     *        It's well known that the `parking_lot` versions are faster and friendly to work with
     * 
     * Parking_Lot Mutex: Generally preferred for it's performance and features (fair locking and no poisoning), works the same
     *                    way as the std::sync::Mutex but is more efficient and less error-prone.
     * 
     * ===
     * 
     * In context, I'm wrapping a vector of ChainInfo structs in a Mutex, then in an Arc, and finally in a LazyLock which means:
     * 
     * - The data is initialized only when needed (LazyLock)
     * - It can be shared across threads (Arc)
     * - Only one thread can mutate it at a time (Mutex)
     */

    
    output.push_str("pub static CHAIN_DATA: std::sync::LazyLock<std::sync::Arc<parking_lot::Mutex<Vec<ChainInfo>>>> = std::sync::LazyLock::new(|| {\n");
    output.push_str("   std::sync::Arc::new(parking_lot::Mutex::new(vec![\n");
    for (chain_id, name, tvl) in &processed_chains {
        output.push_str(&format!(
            "       ChainInfo {{ chain_id: {}, name: \"{}\".to_string(), tvl: {:.1} }},\n",
            chain_id, name, tvl
        ));
    }
    output.push_str("   ]))\n");
    output.push_str("});\n\n");


    /*
     * Using the same pattern as above, but this time for a vector of tuples (NetworkId, String)
     */

    output.push_str("pub static CHAIN_IDS: std::sync::LazyLock<std::sync::Arc<parking_lot::Mutex<Vec<(NetworkId, String)>>>> = std::sync::LazyLock::new(|| {\n");
    output.push_str("   std::sync::Arc::new(parking_lot::Mutex::new(vec![\n");
    for (chain_id, name) in &chain_ids {
        output.push_str(&format!(
            "       ({} , \"{}\".to_string()),\n",
            chain_id, name
        ));
    }
    output.push_str("   ]))\n");
    output.push_str("});\n\n");

    output.push_str("pub static EXTRA_RPCS_DATA: std::sync::LazyLock<std::sync::Arc<parking_lot::Mutex<Vec<(NetworkId, Vec<String>)>>>> = std::sync::LazyLock::new(|| {\n");
    output.push_str("   std::sync::Arc::new(parking_lot::Mutex::new(vec![\n");
    for (chain_id, rpcs) in &extra_rpcs {
        output.push_str(&format!("      ({}, vec![", chain_id));
        for(i,rpc) in rpcs.iter().enumerate() {
            if i > 0 { output.push_str(", ");}
            output.push_str(&format!("\"{}\".to_string()", rpc));
        }
        output.push_str("]),\n");
    }

    output.push_str("   ]))\n");
    output.push_str("});\n\n");

    Ok(output)
}


fn remove_trailing_slash(rpc: &str) -> String {
    if rpc.ends_with("/") {
        rpc[..rpc.len()-1].to_string()
    }else{
        rpc.to_string()
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_trailing_slash_basic() {
        assert_eq!(remove_trailing_slash("http://foo.com/"), "http://foo.com");
        assert_eq!(remove_trailing_slash("http://foo.com"), "http://foo.com");
        assert_eq!(remove_trailing_slash("/"), "");
        assert_eq!(remove_trailing_slash("") , "");
    }

    #[tokio::test]
    async fn test_generate_chainlist_data_returns_string() {
        let result = generate_chainlist_data().await;
        assert!(result.is_ok());
        let data = result.unwrap();
        assert!(data.contains("CHAIN_DATA"));
        assert!(data.len() > 0);
    }
}