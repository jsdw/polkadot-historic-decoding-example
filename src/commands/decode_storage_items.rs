use std::path::PathBuf;
use clap::Parser;

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Opts {
    /// Historic type definitions.
    #[arg(short, long)]
    types: PathBuf,

    /// Spec version updates.
    #[arg(short, long)]
    spec_versions: PathBuf,

    /// URL of the node to connect to. 
    /// Defaults to using Polkadot RPC URLs if not given.
    #[arg(short, long)]
    url: Option<String>,

    /// How many connections to establish to each url.
    #[arg(long)]
    connections: Option<usize>,
}

pub async fn run(_opts: Opts) -> anyhow::Result<()> {

    /* 
    Use runner to run a bunch of tasks which are each going to select a spec version and then
    randomly pick a block within that spec version. Each task will:
    
    1. Fetch metadata at that block (we can cache this per spec version if we like).
    2. Use StorageEntriesList to fetch list of all available storage entries.
    3. For each storage entry, use StorageTypeInfo to get info for this entry.
    4. Construct the storage prefix (twox64(pallet) + twox64(entry_name) iirc)
    5. If plain entry, fetch value as is and use storage_decoder to decode it.
       If map, iterate from prefix and decode both keys and values via storage_decoder.
    6. Report any decode errors back, else print the output?
    */

    Ok(())
}
