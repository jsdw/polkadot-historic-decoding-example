use clap::Parser;
use subxt::backend::{
    legacy::{ rpc_methods::{Bytes, NumberOrHex}, LegacyRpcMethods }, rpc::{rpc_params, RpcClient}
};
use subxt::{Config, PolkadotConfig};
use subxt::ext::codec::Decode;
use anyhow::{anyhow, Context};
use crate::utils::runner::RoundRobin;
use crate::utils;

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Opts {
    /// URL of the node to connect to. 
    /// Defaults to using Polkadot RPC URLs if not given.
    #[arg(short, long)]
    url: Option<String>,

    /// Block number to fetch metadata from.
    #[arg(short, long)]
    block: u64,
}

pub async fn run(opts: Opts) -> anyhow::Result<()> {
    let start_block_num = opts.block;

    // Use our the given URl, or polkadot RPC node urls if not given.
    let urls = RoundRobin::new(utils::url_or_polkadot_rpc_nodes(opts.url.as_deref()));

    let block_number = start_block_num;
    let url = urls.get();
    let rpc_client = RpcClient::from_insecure_url(url).await?;
    let rpcs = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client.clone());
    let block_hash = rpcs.chain_get_block_hash(Some(NumberOrHex::Number(block_number as u64)))
        .await
        .with_context(|| "Could not fetch block hash")?
        .ok_or_else(|| anyhow!("Couldn't find block {block_number}"))?;
    let metadata = state_get_metadata(&rpc_client, Some(block_hash))
        .await
        .with_context(|| "Could not fetch metadata")?;

    serde_json::to_writer_pretty(std::io::stdout(), &metadata)?;
    Ok(())
}

pub(super) async fn state_get_metadata(client: &RpcClient, at: Option<<PolkadotConfig as Config>::Hash>) -> anyhow::Result<frame_metadata::RuntimeMetadata> {
    let bytes: Bytes = client
        .request("state_getMetadata", rpc_params![at])
        .await
        .with_context(|| "Could not fetch metadata")?;
    let metadata = frame_metadata::RuntimeMetadataPrefixed::decode(&mut &bytes[..])
        .with_context(|| "Could not decode metadata")?;
    Ok(metadata.1)
}
