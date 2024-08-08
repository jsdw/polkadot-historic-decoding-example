mod decoder;
mod runner;
mod extrinsic_type_info;

use decoder::{decode_extrinsic, Extrinsic, ExtrinsicCallData};
use extrinsic_type_info::extend_with_call_info;
use frame_metadata::RuntimeMetadata;
use scale_info_legacy::{ChainTypeRegistry, TypeRegistrySet};
use scale_value::{Composite, Value, ValueDef};
use std::path::PathBuf;
use clap::Parser;
use subxt::{backend::{
    legacy::{ rpc_methods::{Bytes, NumberOrHex}, LegacyRpcMethods }, rpc::{rpc_params, RpcClient}
}, utils::H256};
use subxt::{Config, SubstrateConfig};
use subxt::ext::codec::Decode;
use anyhow::{anyhow, Context};
use runner::{Runner, RoundRobin};
use std::sync::Arc;
use tokio::sync::Mutex;
use std::io::Write as _;

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
    connections: Option<usize>,

    /// Only log errors; don't log extrinsics that decode successfully.
    #[arg(short, long)]
    errors_only: bool,

    /// Keep outputting blocks once we hit an error.
    #[arg(short, long)]
    continue_on_error: bool,

    /// Block number to start from.
    #[arg(short, long)]
    block: Option<u64>,

    /// Fetch the metadata at the block given and exit.
    #[arg(short, long)]
    metadata: bool,
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

    let start_block_num = opts.block.unwrap_or_default();
    let metadata = opts.metadata;
    let errors_only = opts.errors_only;
    let continue_on_error = opts.continue_on_error;
    let connections = opts.connections.unwrap_or(RPC_NODE_URLS.len());
    let historic_types_str = std::fs::read_to_string(&opts.types)
        .with_context(|| "Could not load historic types")?;

    // Use our default or built-in URLs if not provided.
    let urls = RoundRobin::new(opts.url
        .as_ref()
        .map(|urls| {
            urls.split(',')
                .map(|url| url.to_owned())
                .collect::<Vec<String>>()
        })
        .unwrap_or_else(|| {
            RPC_NODE_URLS
                .iter()
                .map(|url| url.to_string())
                .collect()
        }));

    // Just fetch metadata and exit if --metadata flag is given
    if metadata {
        let block_number = start_block_num;
        let url = urls.get();
        let rpc_client = RpcClient::from_insecure_url(url).await?;
        let rpcs = LegacyRpcMethods::<SubstrateConfig>::new(rpc_client.clone());
        let block_hash = rpcs.chain_get_block_hash(Some(NumberOrHex::Number(block_number as u64)))
            .await
            .with_context(|| "Could not fetch block hash")?
            .ok_or_else(|| anyhow!("Couldn't find block {block_number}"))?;
        let metadata = state_get_metadata(&rpc_client, Some(block_hash))
            .await
            .with_context(|| "Could not fetch metadata")?;

        serde_json::to_writer_pretty(std::io::stdout(), &metadata)?;
        return Ok(())
    }

    // Our base type mappings that we'll use to decode pre-V14 blocks.
    let historic_types: ChainTypeRegistry = serde_yaml::from_str(&historic_types_str)
        .with_context(|| "Can't parse historic types from JSON")?;
    let historic_types = Arc::new(historic_types);

    // Create a runner to download and decode blocks in parallel.
    let runner = Runner::new(
        // Initial state; each task fetches the next URl to connect to.
        urls,
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

                // Check the last block to see if a runtime update happened. Runtime updates
                // take effect the block after they are applied.
                let runtime_update_block = block_number.saturating_sub(1);
                let runtime_update_block_hash = chain_get_block_hash(&state.rpcs, runtime_update_block).await?;
                let runtime_version = state.rpcs.state_get_runtime_version(Some(runtime_update_block_hash))
                    .await
                    .with_context(|| format!("Could not fetch runtime version for block {runtime_update_block} with hash {runtime_update_block_hash}"))?;

                let this_spec_version = runtime_version.spec_version;
                if this_spec_version != state.current_spec_version || state.current_metadata.is_none() || state.current_types_for_spec.is_none() {
                    // Fetch new metadata for this spec version.
                    let metadata = state_get_metadata(&state.rpc_client, Some(runtime_update_block_hash)).await?;

                    // Prepare new historic type info for this new spec/metadata. Extend the type info
                    // with Call types from the metadataa so that things like utility.batch "Just Work".
                    let mut historic_types_for_spec = historic_types.for_spec_version(this_spec_version as u64).to_owned();
                    extend_with_call_info(&mut historic_types_for_spec, &metadata)?;

                    // Print out all of the call types for any metadata we are given, for debugging etc:
                    // extrinsic_type_info::print_call_types(&historic_types_for_spec);

                    state.current_types_for_spec = Some(historic_types_for_spec);
                    state.current_metadata = Some(metadata);
                    state.current_spec_version = this_spec_version;
                }

                let current_metadata = state.current_metadata.as_ref().unwrap();
                let current_types_for_spec = state.current_types_for_spec.as_ref().unwrap();

                let block_hash = chain_get_block_hash(&state.rpcs, block_number).await?;
                let block_body = state.rpcs.chain_get_block(Some(block_hash))
                    .await
                    .with_context(|| "Could not fetch block body")?
                    .expect("block should exist");

                let extrinsics = block_body.block.extrinsics.into_iter().map(|ext| {
                    let ext_bytes = &ext.0;
                    let decoded = decode_extrinsic(ext_bytes, current_metadata, current_types_for_spec);
                    (ext, decoded)
                }).collect();

                Ok(Output {
                    block_number,
                    block_hash,
                    spec_version: this_spec_version,
                    extrinsics
                })
            }
        },
        // Log the output. This runs sequentially, in order of task numbers.
        move |output: Output| {
            let mut stdout = std::io::stdout().lock();

            let block_number = output.block_number;
            let block_hash = output.block_hash;
            let spec_version = output.spec_version;
            let extrinsics = output.extrinsics;
            let is_error = extrinsics.iter().any(|(_,e)| e.is_err());
            let should_print_header = !errors_only || (errors_only && is_error);
            let should_print_success = !errors_only;

            if should_print_header {
                writeln!(stdout, "==============================================")?;
                writeln!(stdout, "Block {block_number} ({})", subxt::utils::to_hex(block_hash))?;
                writeln!(stdout, "Spec version {spec_version}")?;
            }

            for (ext_idx, (_ext_bytes, ext_decoded)) in extrinsics.into_iter().enumerate() {
                match ext_decoded {
                    Ok(Extrinsic::V4Unsigned { call_data }) => {
                        if should_print_success {
                            writeln!(stdout, "  {}.{}:", call_data.pallet_name, call_data.call_name)?;
                            print_call_data(&mut stdout, &call_data)?;
                        }
                    },
                    Ok(Extrinsic::V4Signed { address, signature, signed_exts, call_data }) => {
                        if should_print_success {
                            writeln!(stdout, "  {}.{}:", call_data.pallet_name, call_data.call_name)?;
                            writeln!(stdout, "    Address: {address}")?;
                            writeln!(stdout, "    Signature: {signature}")?;
                            print_signed_exts(&mut stdout, &signed_exts)?;
                            print_call_data(&mut stdout, &call_data)?;
                        }
                    },
                    Err(e) => {
                        // let bytes_hex = serde_json::to_string(&ext_bytes).unwrap();
                        writeln!(stdout, "Error decoding extrinsic {ext_idx}: {e:?}")?;
                        break;
                    }
                }
            }

            if !continue_on_error && is_error {
                Err(anyhow!("Stopping: error decoding extrinsic"))
            } else {
                Ok(())
            }
        }
    );

    if let Err(e) = runner.run(connections, start_block_num).await {
        eprintln!("{e}");
    }
    Ok(())
}

async fn chain_get_block_hash(rpcs: &LegacyRpcMethods<SubstrateConfig>, block_number: u64) -> anyhow::Result<<SubstrateConfig as Config>::Hash> {
    let block_hash = rpcs.chain_get_block_hash(Some(NumberOrHex::Number(block_number)))
        .await
        .with_context(|| "Could not fetch block hash")?
        .ok_or_else(|| anyhow!("Couldn't find block {block_number}"))?;
    Ok(block_hash)
}

async fn state_get_metadata(client: &RpcClient, at: Option<<SubstrateConfig as Config>::Hash>) -> anyhow::Result<frame_metadata::RuntimeMetadata> {
    let bytes: Bytes = client
        .request("state_getMetadata", rpc_params![at])
        .await
        .with_context(|| "Could not fetch metadata")?;
    let metadata = frame_metadata::RuntimeMetadataPrefixed::decode(&mut &bytes[..])
        .with_context(|| "Could not decode metadata")?;
    Ok(metadata.1)
}

fn print_call_data<W: std::io::Write>(mut w: W, call_data: &ExtrinsicCallData) -> anyhow::Result<()> {
    writeln!(w, "    Call data:")?;
    for arg in &call_data.args {
        write!(w, "      {}: ", arg.0)?;
        write_value(&mut w, &arg.1)?;
        writeln!(w)?;
    }
    Ok(())
}

fn print_signed_exts<W: std::io::Write>(mut w: W, signed_exts: &[(String, scale_value::Value<String>)]) -> anyhow::Result<()> {
    writeln!(w, "    Signed exts:")?;
    for ext in signed_exts {
        write!(w, "      {}: ", ext.0)?;
        write_value(&mut w, &ext.1)?;
        writeln!(w)?;
    }
    Ok(())
}

fn write_value<W: std::io::Write, T: std::fmt::Display>(w: W, value: &Value<T>) -> core::fmt::Result {
    // Our stdout lock is io::Write but we need fmt::Write below.
    // Ideally we'd about this, but io::Write is std-only among
    // other things, so scale-value uses fmt::Write.
    struct ToFmtWrite<W>(W);
    impl <W: std::io::Write> std::fmt::Write for ToFmtWrite<W> {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write(s.as_bytes()).map(|_| ()).map_err(|_| std::fmt::Error)
        }
    }

    write_value_fmt(ToFmtWrite(w), value, "      ")
}

fn write_value_fmt<W: std::fmt::Write, T: std::fmt::Display>(w: W, value: &Value<T>, leading_indent: impl Into<String>) -> core::fmt::Result {
    scale_value::stringify::to_writer_custom()
        .pretty()
        .leading_indent(leading_indent.into())
        .format_context(|type_id, w: &mut W| write!(w, "{type_id}"))
        .add_custom_formatter(|v, w: &mut W| scale_value::stringify::custom_formatters::format_hex(v,w))
        .add_custom_formatter(|v, w: &mut W| {
            // don't space unnamed composites over multiple lines if lots of primitive values.
            if let ValueDef::Composite(Composite::Unnamed(vals)) = &v.value {
                let are_primitive = vals.iter().all(|val| matches!(val.value, ValueDef::Primitive(_)));
                if are_primitive {
                    return Some(write!(w, "{v}"))
                }
            }
            None
        })
        .write(&value, w)
}

struct Output {
    spec_version: u32,
    block_number: u64,
    block_hash: H256,
    extrinsics: Vec<(Bytes, Result<Extrinsic, anyhow::Error>)>
}