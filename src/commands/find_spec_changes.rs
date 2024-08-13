use clap::Parser;
use crate::utils;
use crate::binary_chopper::{BinaryChopper,Next};
use anyhow::{anyhow,Context};
use subxt::PolkadotConfig;
use subxt::backend::{
    legacy::{ rpc_methods::NumberOrHex, LegacyRpcMethods }, rpc::RpcClient
};

#[derive(Parser)]
#[command(version, about, long_about = None)]
pub struct Opts {
    /// URL of the node to connect to. 
    /// Defaults to using Polkadot RPC URLs if not given.
    #[arg(short, long)]
    url: Option<String>,
}

pub async fn run(opts: Opts) -> anyhow::Result<()> {
    let url = utils::url_or_polkadot_rpc_nodes(opts.url.as_deref()).remove(0);
    let rpc_client = RpcClient::from_insecure_url(&url).await?;
    let rpcs = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client.clone());

    let latest_block_number = rpcs
        .chain_get_header(None)
        .await?
        .expect("latest block will be returned when no hash given")
        .number;

    let mut low_version = get_spec_version(&rpcs, 0).await?;
    let high_version = get_spec_version(&rpcs, latest_block_number).await?;
    
    let mut start = 0;
    let end = latest_block_number;
    let mut changes = vec![];

    while start != end {
        let mut chopper = BinaryChopper::new(
            (start, low_version), 
            (end, high_version)
        );

        while let Next::NeedsState(n) = chopper.next_value() {
            let spec_version = get_spec_version(&rpcs, n).await?;
            chopper.set_state_for_next_value(spec_version);
        }

        let ((_block_num1, spec_version1), (block_num2, spec_version2)) = chopper
            .next_value()
            .unwrap_finished();

        eprintln!("Found spec version change at block {block_num2} (from spec version {spec_version1} to {spec_version2})");
        start = block_num2;
        low_version = spec_version2;
        changes.push((block_num2, spec_version2));
    }

    println!("{changes:?}");
    Ok(())
}

async fn get_spec_version(rpcs: &LegacyRpcMethods<PolkadotConfig>, block_number: u32) -> anyhow::Result<u32> {
    let block_hash = rpcs.chain_get_block_hash(Some(NumberOrHex::Number(block_number as u64)))
        .await
        .with_context(|| "Could not fetch block hash")?
        .ok_or_else(|| anyhow!("Couldn't find block {block_number}"))?;
    let version = rpcs.state_get_runtime_version(Some(block_hash))
        .await
        .with_context(|| "Could not fetch runtime version")?;
    Ok(version.spec_version)
}
