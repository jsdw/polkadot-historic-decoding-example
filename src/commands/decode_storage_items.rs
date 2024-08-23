use std::path::PathBuf;
use clap::Parser;
use crate::{decoding::storage_entries_list::StorageEntry, utils::{self, runner::{RoundRobin, Runner}}};
use super::find_spec_changes::SpecVersionUpdate;
use super::fetch_metadata::state_get_metadata;
use anyhow::Context;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use scale_info_legacy::{ChainTypeRegistry, TypeRegistrySet};
use subxt::{backend::{
    legacy::{ rpc_methods::{Bytes, NumberOrHex}, LegacyRpcMethods }, rpc::RpcClient
}, ext::subxt_core::metadata, utils::H256, PolkadotConfig};

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Opts {
    /// Historic type definitions.
    #[arg(short, long)]
    types: PathBuf,

    /// Spec version updates.
    #[arg(short, long)]
    spec_versions: Option<PathBuf>,

    /// URL of the node to connect to. 
    /// Defaults to using Polkadot RPC URLs if not given.
    #[arg(short, long)]
    url: Option<String>,

    /// How many storage decode tasks/connections to run in parallel.
    #[arg(long)]
    connections: Option<usize>,
}

pub async fn run(opts: Opts) -> anyhow::Result<()> {

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
    let connections = opts.connections.unwrap_or(1);
    let urls = Arc::new(RoundRobin::new(utils::url_or_polkadot_rpc_nodes(opts.url.as_deref())));

    let historic_types: ChainTypeRegistry = {
        let historic_types_str = std::fs::read_to_string(&opts.types)
            .with_context(|| "Could not load historic types")?;
        serde_yaml::from_str(&historic_types_str)
            .with_context(|| "Can't parse historic types from JSON")?
    };
    let spec_versions = opts.spec_versions.as_ref().map(|path| {
        let spec_versions_str = std::fs::read_to_string(path)
            .with_context(|| "Could not load spec versions")?;
        serde_json::from_str::<Vec<SpecVersionUpdate>>(&spec_versions_str)
            .with_context(|| "Could not parse spec version JSON")
    }).transpose()?;

    let latest_block_number = {
        let url = urls.get();
        let rpc_client = RpcClient::from_insecure_url(url).await?;
        LegacyRpcMethods::<PolkadotConfig>::new(rpc_client)
            .chain_get_header(None)
            .await?
            .expect("latest block will be returned when no hash given")
            .number
    };  

    loop {
        // In the outer loop we select a random block.
        let spec_versions = spec_versions.as_ref().map(|s| s.as_slice());
        let block_number = pick_random_block(spec_versions, latest_block_number);

        loop {
            // In the inner loop we connect to a client and try to download entries.
            // If we hit a recoverable error, restart this loop to try again.
            let url = urls.get();
            let rpc_client = match RpcClient::from_insecure_url(url).await {
                Ok(client) => client,
                Err(e) => {
                    eprintln!("Couldn't instantiate RPC client: {e}");
                    continue
                }
            };
            let rpcs = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client.clone());

            let block_hash = match rpcs.chain_get_block_hash(Some(block_number.into())).await {
                Ok(Some(hash)) => hash,
                Ok(None) => break,
                Err(e) => {
                    eprintln!("Couldn't get block hash for {block_number}; will try again: {e}");
                    continue
                }
            };
            let metadata = match state_get_metadata(&rpc_client, Some(block_hash)).await {
                Ok(metadata) => metadata,
                Err(e) => {
                    eprintln!("Couldn't get metadata at {block_number}; will try again: {e}");
                    continue
                }
            };
            let storage_entries = match crate::decoding::storage_entries_list::get_storage_entries(&metadata) {
                Ok(entries) => entries,
                Err(e) => {
                    // The only error here is that the metadata can't be decoded, so break to give up on this block entirely.
                    eprintln!("Couldn't get storage entries at {block_number}: {e}");
                    break
                }
            };

            // try to decode storage entries in parallel.
            let runner = Runner::new(
                (block_hash, storage_entries, urls.clone()),
                // Connect to an RPC client to start decoding storage entries
                |task_idx, (block_hash, storage_entries, urls)| {
                    let url = urls.get().clone();
                    async move {
                        let rpc_client = RpcClient::from_insecure_url(url).await?;
                        let rpcs = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client.clone());
                        Ok(Some(()))
                    }
                },
                // Based on task number, decode an entry from the list, returning None when number exceeds list length.
                |task_num, state| {
                    async move {
                        Ok(Some(()))
                    }
                },
                // Output details.
                |output| {
                    Ok(())
                }
            );

            runner.run(10, 0).await;
        }

        // let runner = Runner::new(
        //     (
        //         urls.clone(),
        //         spec_versions.clone(),
        //         latest_block_number,
        //     ),
        //     // Get the details needed to decode storage entries in some block.
        //     |_task_idx, (urls, specs, latest_block_number)| {
        //         let url = urls.get().to_owned();
        //         let spec_versions = specs.clone();
        //         let latest_block_number = *latest_block_number;
        //         async move {
        //             let spec_versions = spec_versions.as_ref().map(|s| s.as_slice());
        //             let rpc_client = RpcClient::from_insecure_url(url).await?;
        //             let rpcs = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client.clone());
        //             let block_number = pick_random_block(spec_versions, latest_block_number);
        //             let Some(block_hash) = rpcs.chain_get_block_hash(Some(block_number.into()))
        //                 .await
        //                 .with_context(|| format!("Couldn't get block hash for {block_number}"))? else { return Ok(None) };
        //             let metadata = state_get_metadata(&rpc_client, Some(block_hash))
        //                 .await
        //                 .with_context(|| "Couldn't fetch metadata")?;
        //             let storage_entries = crate::decoding::storage_entries_list::get_storage_entries(&metadata)
        //                 .with_context(|| "Couldn't fetch list of storage entries from metadata")?;

        //             Ok(Some(Arc::new(RunnerState {
        //                 rpcs,
        //                 block_number,
        //                 storage_entries: Mutex::new(storage_entries)
        //             })))
        //         }
        //     },
        //     // Work through the storage entries, decoding them.
        //     |_task_num, state| {
        //         let state = state.clone();
        //         async move {
        //             /*
        //             TODO:

        //             - Fetch metadata for this block.
        //             - Get first entry in `storage_entries`.
        //             - Use `decode_storage_item::is_iterable` to see whether to fetch single entry or iterate it.
        //             - fetch or iterate.
        //             - Pop entry out of storage entries when all done; this means 

        //             */

        //             // Fetch metadata for this spec version


        //             // Pick random block in this spec version
        //             // Iterate storage entries for this block and try to decode them.
    
        //             Ok(Some(()))
        //         }
        //     },
        //     |output| {
        //         Ok(())
        //     }
        // );
    
        // runner.run(10, 0).await;
    }

    Ok(())
}

fn pick_random_block(spec_versions: Option<&[SpecVersionUpdate]>, latest_block: u32) -> u32 {
    match spec_versions {
        None => {
            // Just pick a random block from the whole range.
            rand::random::<u32>() % latest_block
        },
        Some(spec_versions) => {
            // Randomly select a range, remembering that we don't have a "first" or "last" spec version.
            let spec_version_idx = rand::random::<usize>() % (spec_versions.len() + 1);
            let (start_block, end_block) = if spec_version_idx == 0 {
                (0, spec_versions[0].block)
            } else if spec_version_idx == spec_versions.len() {
                (spec_versions.last().unwrap().block + 1, latest_block)
            } else {
                (spec_versions[spec_version_idx - 1].block + 1, spec_versions[spec_version_idx].block)
            };

            // Randomly select a block in this range.
            let range = end_block - start_block + 1;
            (rand::random::<u32>() % range) + start_block
        }
    }
}

struct RunnerState {
    rpcs: LegacyRpcMethods<PolkadotConfig>,
    block_number: u32,
    storage_entries: Mutex<VecDeque<StorageEntry<'static>>>
}