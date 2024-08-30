use std::{path::PathBuf, sync::atomic::{AtomicBool, Ordering}};
use clap::Parser;
use frame_metadata::RuntimeMetadata;
use crate::{decoding::storage_decoder::StorageKey, utils::{self, runner::{RoundRobin, Runner}}};
use crate::decoding::{ storage_decoder, storage_entries_list::StorageEntry };
use crate::decoding::storage_type_info::StorageHasher;
use super::find_spec_changes::SpecVersionUpdate;
use super::fetch_metadata::state_get_metadata;
use anyhow::{anyhow, bail, Context};
use std::sync::Arc;
use std::collections::VecDeque;
use scale_info_legacy::ChainTypeRegistry;
use subxt::{backend::{
    legacy::{ LegacyBackend, LegacyRpcMethods }, rpc::RpcClient, Backend
}, utils::H256, PolkadotConfig};
use std::io::Write as _;
use crate::utils::{ToFmtWrite, IndentedWriter, write_compact_value};

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

    /// Only log errors; don't log extrinsics that decode successfully.
    #[arg(short, long)]
    errors_only: bool,

    /// Keep outputting blocks once we hit an error.
    #[arg(long)]
    continue_on_error: bool,

    /// The seed to start from. Blocks are picked in a deterministic way,
    /// and so we can provide this to continue from where we left off.
    #[arg(short,long)]
    starting_number: Option<usize>
}

pub async fn run(opts: Opts) -> anyhow::Result<()> {
    let connections = opts.connections.unwrap_or(1);
    let starting_number = opts.starting_number.unwrap_or(0);
    let urls = Arc::new(RoundRobin::new(utils::url_or_polkadot_rpc_nodes(opts.url.as_deref())));
    let errors_only = opts.errors_only;
    let continue_on_error = opts.continue_on_error;

    let historic_types: Arc<ChainTypeRegistry> = Arc::new({
        let historic_types_str = std::fs::read_to_string(&opts.types)
            .with_context(|| "Could not load historic types")?;
        serde_yaml::from_str(&historic_types_str)
            .with_context(|| "Can't parse historic types from JSON")?
    });
    let spec_versions = opts.spec_versions.as_ref().map(|path| {
        let spec_versions_str = std::fs::read_to_string(path)
            .with_context(|| "Could not load spec versions")?;
        serde_json::from_str::<Vec<SpecVersionUpdate>>(&spec_versions_str)
            .with_context(|| "Could not parse spec version JSON")
    }).transpose()?;

    let mut number = starting_number;
    'outer: loop {
        // In the outer loop we select a block.
        let spec_versions = spec_versions.as_ref().map(|s| s.as_slice());
        let block_number = pick_pseudorandom_block(spec_versions, number);

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
                Ok(metadata) => Arc::new(metadata),
                Err(e) => {
                    eprintln!("Couldn't get metadata at {block_number}; will try again: {e}");
                    continue
                }
            };
            let runtime_version = match rpcs.state_get_runtime_version(Some(block_hash)).await {
                Ok(runtime_version) => runtime_version,
                Err(e) => {
                    eprintln!("Couldn't get runtime version at {block_number}; will try again: {e}");
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
            
            // Print header for block.
            {
                let mut stdout = std::io::stdout().lock();
                writeln!(stdout, "==============================================")?;
                writeln!(stdout, "Number {number}")?;
                writeln!(stdout, "Storage for block {block_number} ({})", subxt::utils::to_hex(block_hash))?;
                writeln!(stdout, "Spec version {}", runtime_version.spec_version)?;
            }

            let stop = Arc::new(AtomicBool::new(false));
            let stop2 = stop.clone();

            // try to decode storage entries in parallel.
            let runner = Runner::new(
                (
                    block_hash, 
                    storage_entries, 
                    urls.clone(), 
                    historic_types.clone(), 
                    metadata, 
                    runtime_version.spec_version
                ),
                // Connect to an RPC client to start decoding storage entries
                |_task_idx, (block_hash, storage_entries, urls, historic_types, metadata, spec_version)| {
                    let url = urls.get().clone();
                    let storage_entries = storage_entries.clone();
                    let block_hash = *block_hash;
                    let historic_types = historic_types.clone();
                    let metadata = metadata.clone();
                    let spec_version = *spec_version;

                    async move {
                        let rpc_client = RpcClient::from_insecure_url(url).await?;
                        let backend = LegacyBackend::builder()
                            .storage_page_size(64)
                            .build(rpc_client);

                        Ok(Some(Arc::new(RunnerState {
                            backend,
                            block_hash,
                            storage_entries,
                            historic_types,
                            metadata,
                            spec_version
                        })))
                    }
                },
                // Based on task number, decode an entry from the list, returning None when number exceeds list length.
                |task_num, state| {
                    let state = state.clone();

                    async move {
                        let Some(storage_entry) = state.storage_entries.get(task_num as usize) else { return Ok(None) };
                        let metadata = &state.metadata;
                        let historic_types = &state.historic_types.for_spec_version(state.spec_version as u64);
                        let pallet = &storage_entry.pallet;
                        let entry = &storage_entry.entry;
                        let at = state.block_hash;
                        let key = {
                            let mut hash = Vec::with_capacity(32);
                            hash.extend(&sp_crypto_hashing::twox_128(pallet.as_bytes()));
                            hash.extend(&sp_crypto_hashing::twox_128(entry.as_bytes()));
                            hash
                        };

                        // Iterate or fetch single value depending on entry.
                        let is_iterable = crate::decoding::storage_type_info::is_iterable(pallet, entry, &state.metadata)?;
                        let mut values = if is_iterable {
                            state.backend
                                .storage_fetch_descendant_values(key, at)
                                .await
                                .with_context(|| format!("Failed to get a stream of storage items for {pallet}.{entry}"))
                        } else {
                            state.backend.storage_fetch_values(vec![key], at)
                                .await
                                .with_context(|| format!("Failed to fetch value at {pallet}.{entry}"))
                        }?;
    
                        let mut keyvals = vec![];

                        // Decode each value we get back.
                        while let Some(value) = values.next().await {
                            let value = value
                                .with_context(|| format!("Failed to get storage item in stream for {pallet}.{entry}"))?;

                            let key = storage_decoder::decode_storage_keys(pallet, entry, &value.key, metadata, historic_types)
                                .with_context(|| format!("Failed to decode storage key in {pallet}.{entry}"));
                            let value = storage_decoder::decode_storage_value(pallet, entry, &value.value, metadata, historic_types)
                                .with_context(|| format!("Failed to decode storage value in {pallet}.{entry}"));

                            keyvals.push(DecodedStorageKeyVal {
                                key,
                                value
                            })
                        }

                        Ok(Some(DecodedStorageEntry {
                            pallet: pallet.to_string(),
                            entry: entry.to_string(),
                            keyvals
                        }))
                    }
                },
                // Output details.
                move |output| {
                    let mut stdout = std::io::stdout().lock();

                    let is_error = output.keyvals.iter().any(|kv| kv.key.is_err() || kv.value.is_err());
                    let should_print_header = !errors_only || (errors_only && is_error);
                    let should_print_success = !errors_only;

                    if should_print_header {
                        writeln!(stdout, "{}.{}", output.pallet, output.entry)?;
                    }
                    for DecodedStorageKeyVal { key, value } in output.keyvals {
                        if key.is_ok() && value.is_ok() && !should_print_success {
                            continue
                        }

                        write!(stdout, "  ")?;
                        match key {
                            Ok(key) => {
                                write_storage_keys(IndentedWriter::<2, _>(&mut stdout), &key)?;
                            },
                            Err(e) => {
                                write!(IndentedWriter::<2, _>(&mut stdout), "Key Error: {e:?}")?;
                            }
                        }
                        write!(stdout, "\n    - ")?;
                        match value {
                            Ok(value) => {
                                write_compact_value(IndentedWriter::<6, _>(&mut stdout), &value)?;
                            },
                            Err(e) => {
                                write!(IndentedWriter::<6, _>(&mut stdout), "Value Error: {e:?}")?;

                            }
                        }
                        writeln!(stdout)?;
                    }

                    if !continue_on_error && is_error {
                        stop2.store(true, Ordering::Relaxed);
                        Err(anyhow!("Stopping: error decoding storage entries."))
                    } else {
                        Ok(())
                    }
                }
            );

            // Decode storage entries in the block.
            let _ = runner.run(connections, 0).await;
            // Stop if the runner tells us to. Quite a hacky way to communicate it.
            if stop.load(Ordering::Relaxed) == true {
                break 'outer;
            }
            // Don't retry this block; move on to next.
            break;
        }

        number += 1;
    }

    Ok(())
}

fn write_storage_keys<W: std::io::Write>(writer: W, keys: &[StorageKey]) -> anyhow::Result<()> {
    let mut writer = ToFmtWrite(writer);

    // Plain entries have no keys:
    if keys.is_empty() {
        write!(&mut writer, "plain")?;
        return Ok(())
    }

    // blake2: AccountId(0x2331) + ident: Foo(123) + blake2:0x23edbfe
    for (idx, key) in keys.into_iter().enumerate() {
        if idx != 0 {
            write!(&mut writer, " + ")?;
        }

        match (key.hasher, &key.value) {
            (StorageHasher::Blake2_128, None) => {
                write!(&mut writer, "blake2_128: ")?;
                write!(&mut writer, "{}", hex::encode(&key.hash))?;
            },
            (StorageHasher::Blake2_256, None) => {
                write!(&mut writer, "blake2_256: ")?;
                write!(&mut writer, "{}", hex::encode(&key.hash))?;
            },
            (StorageHasher::Blake2_128Concat, Some(value)) => {
                write!(&mut writer, "blake2_128_concat: ")?;
                write_compact_value(&mut writer, &value)?;
            },
            (StorageHasher::Twox128, None) => {
                write!(&mut writer, "twox_128: ")?;
                write!(&mut writer, "{}", hex::encode(&key.hash))?;
            },
            (StorageHasher::Twox256, None) => {
                write!(&mut writer, "twox_256: ")?;
                write!(&mut writer, "{}", hex::encode(&key.hash))?;
            },
            (StorageHasher::Twox64Concat, Some(value)) => {
                write!(&mut writer, "twox64_concat: ")?;
                write_compact_value(&mut writer, &value)?;
            },
            (StorageHasher::Identity, Some(value)) => {
                write!(&mut writer, "ident: ")?;
                write_compact_value(&mut writer, &value)?;
            },
            _ => {
                bail!("Invalid storage hasher/value pair")
            }
        }
    }

    Ok(())
}

/// Given the same spec versions and the same number, this should output the same value,
/// but the output block number can be pseudorandom in nature. The output number should be
/// between the first and last spec versions provided (so blocks newer than the last runtime
/// upgrade aren't tested).
fn pick_pseudorandom_block(spec_versions: Option<&[SpecVersionUpdate]>, number: usize) -> u32 {
    let Some(spec_versions) = spec_versions else {
        return number as u32;
    };

    // Given spec versions, we deterministically work from first blocks seen (ie blocks before
    // update is enacted, which is a good edge to test) and then blocks after and so on. 
    // 0 0 0 1 1 1 2 2 2 3 3
    // 0 4   1 5   2 6   3 7
    let spec_version_idx = number % spec_versions.len();
    let spec_version_block_idx = number / spec_versions.len();

    let block_number = spec_versions[spec_version_idx].block + spec_version_block_idx as u32;
    block_number
}

struct RunnerState {
    backend: LegacyBackend<PolkadotConfig>,
    block_hash: H256,
    storage_entries: VecDeque<StorageEntry<'static>>,
    historic_types: Arc<ChainTypeRegistry>,
    metadata: Arc<RuntimeMetadata>,
    spec_version: u32,
}

struct DecodedStorageEntry {
    pallet: String,
    entry: String,
    keyvals: Vec<DecodedStorageKeyVal>
}

struct DecodedStorageKeyVal {
    key: anyhow::Result<Vec<StorageKey>>,
    value: anyhow::Result<scale_value::Value<String>>
}