mod decoder;
mod extrinsic_type_info;

use decoder::decode_extrinsic;
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

    /// Block number to decode.
    #[arg(short, long)]
    block_number: Option<u32>
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    let historic_types_str = std::fs::read_to_string(&opts.types)
        .with_context(|| "Could not load historic types")?;
    let rpc_client = RpcClient::from_insecure_url(&opts.url).await?;
    let rpcs = LegacyRpcMethods::<SubstrateConfig>::new(rpc_client.clone());
    let historic_types: ChainTypeRegistry = serde_yaml::from_str(&historic_types_str)
        .with_context(|| "Can't parse historic types from JSON")?;

    let mut current_metadata = None;
    let mut current_spec_version = 0;

    for block_number in 0.. {
        let block_hash = rpcs.chain_get_block_hash(Some(NumberOrHex::Number(block_number)))
            .await
            .with_context(|| "Could not fetch block hash")?
            .ok_or_else(|| anyhow!("Couldn't find block {block_number}"))?;

        let runtime_version = rpcs.state_get_runtime_version(Some(block_hash))
            .await
            .with_context(|| "Could not fetch runtime version")?;

        let historic_types_for_spec = historic_types.for_spec_version(runtime_version.spec_version as u64);

        if runtime_version.spec_version != current_spec_version || current_metadata.is_none() {
            let metadata = state_get_metadata(&rpc_client, Some(block_hash))
                .await
                .with_context(|| "Could not fetch metadata")?;
            current_metadata = Some(metadata);
            current_spec_version = runtime_version.spec_version;
        }

        let current_metadata = current_metadata.as_ref().unwrap();

        let block_body = rpcs.chain_get_block(Some(block_hash))
            .await
            .with_context(|| "Could not fetch block body")?
            .expect("block should exist");

        println!("Extrinsics in block {block_number} ({})", subxt::utils::to_hex(block_hash));
        for ext in block_body.block.extrinsics {
            let ext_bytes = ext.0;
            let ext_value = decode_extrinsic(&ext_bytes, current_metadata, &historic_types_for_spec)
                .with_context(|| format!("Failed to decode extrinsic {}", subxt::utils::to_hex(ext_bytes)))?;

            println!("{ext_value:#?}")
        }
    }

    Ok(())
}

pub async fn state_get_metadata(client: &RpcClient, at: Option<<SubstrateConfig as Config>::Hash>) -> anyhow::Result<frame_metadata::RuntimeMetadata> {
    let bytes: Bytes = client
        .request("state_getMetadata", rpc_params![at])
        .await?;
    let metadata = frame_metadata::RuntimeMetadataPrefixed::decode(&mut &bytes[..])?;
    Ok(metadata.1)
}

pub enum Info<A, B> {
    None,
    Historic(A),
    Modern(B)
}