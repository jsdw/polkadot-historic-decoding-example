mod decoder;
mod extrinsic_type_info;

use decoder::{decode_extrinsic, Extrinsic, ExtrinsicCallData};
use extrinsic_type_info::extend_with_call_info;
use scale_info_legacy::ChainTypeRegistry;
use std::path::PathBuf;
use clap::Parser;
use subxt::backend::{
    rpc::{RpcClient,rpc_params},
    legacy::{ LegacyRpcMethods, rpc_methods::{NumberOrHex,Bytes} }
};
use subxt::{Config, SubstrateConfig};
use subxt::ext::codec::Decode;
use anyhow::{anyhow,Context};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Opts {
    /// Historic type definitions.
    #[arg(short, long)]
    types: PathBuf,

    /// URL of the node to connect to.
    #[arg(short, long)]
    url: String,

    /// Block number to start from.
    #[arg(short, long)]
    block: Option<u64>
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    let start_block_num = opts.block.unwrap_or_default();
    let historic_types_str = std::fs::read_to_string(&opts.types)
        .with_context(|| "Could not load historic types")?;
    let rpc_client = RpcClient::from_insecure_url(&opts.url).await?;
    let rpcs = LegacyRpcMethods::<SubstrateConfig>::new(rpc_client.clone());
    let historic_types: ChainTypeRegistry = serde_yaml::from_str(&historic_types_str)
        .with_context(|| "Can't parse historic types from JSON")?;

    let mut current_metadata = None;
    let mut current_spec_version = u32::MAX;
    let mut current_types_for_spec = None;

    for block_number in start_block_num.. {
        let block_hash = rpcs.chain_get_block_hash(Some(NumberOrHex::Number(block_number)))
            .await
            .with_context(|| "Could not fetch block hash")?
            .ok_or_else(|| anyhow!("Couldn't find block {block_number}"))?;

        let runtime_version = rpcs.state_get_runtime_version(Some(block_hash))
            .await
            .with_context(|| "Could not fetch runtime version")?;

        if runtime_version.spec_version != current_spec_version || current_metadata.is_none() || current_types_for_spec.is_none() {
            println!("Spec version change to {}", runtime_version.spec_version);

            // Fetch new metadata for this spec version.
            let metadata = state_get_metadata(&rpc_client, Some(block_hash))
                .await
                .with_context(|| "Could not fetch metadata")?;

            // Prepare new historic type info for this new spec/metadata. Extend the type info
            // with Call types so that things like utility.batch "Just Work" based on metadata.
            let mut historic_types_for_spec = historic_types.for_spec_version(runtime_version.spec_version as u64);
            extend_with_call_info(&mut historic_types_for_spec, &metadata)?;

            // Print out all of the call types for any metadata we are given, for debugging etc:
            extrinsic_type_info::print_call_types(&historic_types_for_spec);

            current_types_for_spec = Some(historic_types_for_spec);
            current_metadata = Some(metadata);
            current_spec_version = runtime_version.spec_version;
        }

        let current_metadata = current_metadata.as_ref().unwrap();
        let current_types_for_spec = current_types_for_spec.as_ref().unwrap();

        let block_body = rpcs.chain_get_block(Some(block_hash))
            .await
            .with_context(|| "Could not fetch block body")?
            .expect("block should exist");

        println!("==============================================");
        println!("Extrinsics in block {block_number} ({})", subxt::utils::to_hex(block_hash));
        for ext in block_body.block.extrinsics {
            let ext_bytes = ext.0;
            let ext_value = decode_extrinsic(&ext_bytes, current_metadata, current_types_for_spec)
                .with_context(|| format!("Failed to decode extrinsic {}", subxt::utils::to_hex(ext_bytes)))?;

            match ext_value {
                Extrinsic::V4Unsigned { call_data } => {
                    println!("  {}.{}:", call_data.pallet_name, call_data.call_name);
                    print_call_data(&call_data);
                },
                Extrinsic::V4Signed { address, signature, signed_exts, call_data } => {
                    println!("  {}.{}:", call_data.pallet_name, call_data.call_name);
                    println!("    Address: {address}");
                    println!("    Signature: {signature}");
                    print_signed_exts(&signed_exts);
                    print_call_data(&call_data);
                }
            }
        }
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

fn print_call_data(call_data: &ExtrinsicCallData) {
    println!("    Call data:");
    for arg in &call_data.args {
        println!("      {}: {}", arg.0, arg.1);
    }
}

fn print_signed_exts(signed_exts: &[(String, scale_value::Value)]) {
    println!("    Signed exts:");
    for ext in signed_exts {
        println!("      {}: {}", ext.0, ext.1);
    }
}