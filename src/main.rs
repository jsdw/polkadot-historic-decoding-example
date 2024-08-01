mod decoder;
mod runner;
mod extrinsic_type_info;

use decoder::{decode_extrinsic, Extrinsic, ExtrinsicCallData};
use extrinsic_type_info::extend_with_call_info;
use frame_metadata::RuntimeMetadata;
use scale_info_legacy::{ChainTypeRegistry, TypeRegistrySet};
use std::path::PathBuf;
use clap::Parser;
use subxt::{backend::{
    legacy::{ rpc_methods::{Bytes, NumberOrHex}, LegacyRpcMethods }, rpc::{rpc_params, RpcClient}
}, utils::H256};
use subxt::{Config, SubstrateConfig};
use subxt::ext::codec::Decode;
use std::iter::repeat;
use anyhow::{anyhow, Context};
use runner::{Runner, RoundRobin};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::io::Write;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Opts {
    /// Historic type definitions.
    #[arg(short, long)]
    types: PathBuf,

    /// URL of the node to connect to.
    #[arg(short, long)]
    url: Option<String>,

    /// How many connections to establish to each url.
    #[arg(short, long)]
    connections_per_url: Option<usize>,

    /// Only log errors; don't log extrinsics that decode successfully.
    #[arg(short, long)]
    errors_only: bool,

    /// Keep outputting blocks once we hit an error.
    #[arg(short, long)]
    continue_on_error: bool,

    /// Block number to start from.
    #[arg(short, long)]
    block: Option<u64>
}

const RPC_NODE_URLS: [&str; 7] = [
    // "wss://polkadot-rpc.publicnode.com", // bad; can't fetch runtime version.
    "wss://polkadot-public-rpc.blockops.network/ws",
    "wss://polkadot-rpc.dwellir.com",
    "wss://polkadot.api.onfinality.io/public-ws",
    "wss://polkadot.public.curie.radiumblock.co/ws",
    "wss://rockx-dot.w3node.com/polka-public-dot/ws",
    "wss://rpc.ibp.network/polkadot",
    "wss://rpc.dotters.network/polkadot",
    // "wss://dot-rpc.stakeworld.io", // seemed unreliable.
];

struct RunnerState {
    rpc_client: RpcClient,
    rpcs: LegacyRpcMethods<SubstrateConfig>,
    current_spec_version: u32,
    current_metadata: Option<RuntimeMetadata>,
    current_types_for_spec: Option<TypeRegistrySet<'static>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    let errors_only = opts.errors_only;
    let continue_on_error = opts.continue_on_error;
    let connections_per_url = opts.connections_per_url.unwrap_or(1);
    let start_block_num = opts.block.unwrap_or_default();
    let historic_types_str = std::fs::read_to_string(&opts.types)
        .with_context(|| "Could not load historic types")?;

    // Use our default or provided URLs. duplicate each one once for each
    // `connections_per_url`. We'll parallelise over each URL in the resulting vec.
    let urls = opts.url
        .as_ref()
        .map(|url| {
            url.split(',')
                .flat_map(|url| repeat(url.to_owned()).take(connections_per_url))
                .collect::<Vec<String>>()
        })
        .unwrap_or_else(|| {
            RPC_NODE_URLS
                .into_iter()
                .flat_map(|url| repeat(url.to_owned()).take(connections_per_url))
                .collect()
        });

    let urls_len = urls.len();

    // Our base type mappings that we'll use to decode pre-V14 blocks.
    let historic_types: ChainTypeRegistry = serde_yaml::from_str(&historic_types_str)
        .with_context(|| "Can't parse historic types from JSON")?;
    let historic_types = Arc::new(historic_types);

    // Create a runner to download and decode blocks in parallel.
    let runner = Runner::new(
        // Initial state; each task fetches the next URl to connect to.
        RoundRobin::new(urls),
        // Turn each URL into some state that we'll reuse to fetch a bunch of blocks. This reruns on error.
        |_n, urls| {
            let url = urls.get().to_owned();
            async move {
                let rpc_client = RpcClient::from_insecure_url(url).await?;

                let state = RunnerState {
                    rpc_client: rpc_client.clone(),
                    rpcs: LegacyRpcMethods::<SubstrateConfig>::new(rpc_client),
                    current_spec_version: u32::MAX,
                    current_metadata: None,
                    current_types_for_spec: None
                };

                Ok(Arc::new(Mutex::new(state)))
            }
        },
        // Fetch a block and decode it. This runs in parallel for number of initial state items.
        move |block_number, state| {
            let historic_types = historic_types.clone();
            let state = state.clone();
            async move {
                let mut state = state.lock().await;

                let block_hash = state.rpcs.chain_get_block_hash(Some(NumberOrHex::Number(block_number as u64)))
                    .await
                    .with_context(|| "Could not fetch block hash")?
                    .ok_or_else(|| anyhow!("Couldn't find block {block_number}"))?;

                let runtime_version = state.rpcs.state_get_runtime_version(Some(block_hash))
                    .await
                    .with_context(|| format!("Could not fetch runtime version for block {block_number} with hash {block_hash}"))?;

                if runtime_version.spec_version != state.current_spec_version || state.current_metadata.is_none() || state.current_types_for_spec.is_none() {
                    // Fetch new metadata for this spec version.
                    let metadata = state_get_metadata(&state.rpc_client, Some(block_hash))
                        .await
                        .with_context(|| "Could not fetch metadata")?;

                    // Prepare new historic type info for this new spec/metadata. Extend the type info
                    // with Call types so that things like utility.batch "Just Work" based on metadata.
                    let mut historic_types_for_spec = historic_types.for_spec_version(runtime_version.spec_version as u64).to_owned();
                    extend_with_call_info(&mut historic_types_for_spec, &metadata)?;

                    // Print out all of the call types for any metadata we are given, for debugging etc:
                    // extrinsic_type_info::print_call_types(&historic_types_for_spec);

                    state.current_types_for_spec = Some(historic_types_for_spec);
                    state.current_metadata = Some(metadata);
                    state.current_spec_version = runtime_version.spec_version;
                }

                let current_metadata = state.current_metadata.as_ref().unwrap();
                let current_types_for_spec = state.current_types_for_spec.as_ref().unwrap();

                let block_body = state.rpcs.chain_get_block(Some(block_hash))
                    .await
                    .with_context(|| "Could not fetch block body")?
                    .expect("block should exist");

                let extrinsics = block_body.block.extrinsics.into_iter().map(|ext| {
                    let ext_bytes = ext.0;
                    decode_extrinsic(&ext_bytes, current_metadata, current_types_for_spec)
                        .with_context(|| format!("Failed to decode extrinsic in block {block_number}"))
                }).collect();

                Ok(Output {
                    block_number,
                    block_hash,
                    spec_version: runtime_version.spec_version,
                    extrinsics
                })
            }
        },
        // Log the output. This runs sequentially, in order of task numbers.
        {
            let mut last_spec_version = None;

            move |output: Output| {
                let mut stdout = std::io::stdout().lock();

                let block_number = output.block_number;
                let block_hash = output.block_hash;
                let spec_version = output.spec_version;
                let extrinsics = output.extrinsics;
                let is_error = extrinsics.iter().any(|e| e.is_err());
                let should_print_header = !errors_only || (errors_only && is_error);
                let should_print_success = !errors_only;

                if should_print_header {
                    writeln!(stdout, "==============================================")?;
                    writeln!(stdout, "Block {block_number} ({})", subxt::utils::to_hex(block_hash))?;
                }

                if last_spec_version.is_none() || Some(spec_version) != last_spec_version {
                    writeln!(stdout, "Spec version changed to {spec_version}")?;
                    last_spec_version = Some(spec_version)
                }

                for (ext_idx, ext) in extrinsics.into_iter().enumerate() {
                    match ext {
                        Ok(Extrinsic::V4Unsigned { call_data }) => {
                            if should_print_success {
                                writeln!(stdout, "  {}.{}:", call_data.pallet_name, call_data.call_name)?;
                                print_call_data(&mut stdout, &call_data);
                            }
                        },
                        Ok(Extrinsic::V4Signed { address, signature, signed_exts, call_data }) => {
                            if should_print_success {
                                writeln!(stdout, "  {}.{}:", call_data.pallet_name, call_data.call_name)?;
                                writeln!(stdout, "    Address: {address}")?;
                                writeln!(stdout, "    Signature: {signature}")?;
                                print_signed_exts(&mut stdout, &signed_exts);
                                print_call_data(&mut stdout, &call_data);
                            }
                        },
                        Err(e) => {
                            writeln!(stdout, "Error decoding extrinsic {ext_idx}: {e:?}")?;
                        }
                    }
                }

                if !continue_on_error && is_error {
                    Err(anyhow!("Stopping: error decoding extrinsic"))
                } else {
                    Ok(())
                }
            }
        }
    );

    if let Err(e) = runner.run(urls_len * connections_per_url, start_block_num as usize).await {
        eprintln!("{e}");
    }
    Ok(())
}

async fn state_get_metadata(client: &RpcClient, at: Option<<SubstrateConfig as Config>::Hash>) -> anyhow::Result<frame_metadata::RuntimeMetadata> {
    let bytes: Bytes = client
        .request("state_getMetadata", rpc_params![at])
        .await?;
    let metadata = frame_metadata::RuntimeMetadataPrefixed::decode(&mut &bytes[..])?;
    Ok(metadata.1)
}

fn print_call_data<W: std::io::Write>(mut w: W, call_data: &ExtrinsicCallData) {
    writeln!(w, "    Call data:").unwrap();
    for arg in &call_data.args {
        writeln!(w, "      {}: {}", arg.0, arg.1).unwrap();
    }
}

fn print_signed_exts<W: std::io::Write>(mut w: W, signed_exts: &[(String, scale_value::Value)]) {
    writeln!(w, "    Signed exts:").unwrap();
    for ext in signed_exts {
        writeln!(w, "      {}: {}", ext.0, ext.1).unwrap();
    }
}

struct Output {
    spec_version: u32,
    block_number: usize,
    block_hash: H256,
    extrinsics: Vec<Result<Extrinsic, anyhow::Error>>
}