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
    Ok(())
}
