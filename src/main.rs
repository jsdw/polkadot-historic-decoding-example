mod commands;
mod extrinsic_decoder;
mod runner;
mod binary_chopper;
mod extrinsic_type_info;
mod utils;

use clap::Parser;

#[derive(Parser)]
enum Commands {
    /// Decode blocks, printing the decoded output.
    DecodeBlocks(commands::decode_blocks::Opts),
    /// Fetch the metadata at a given block as JSON.
    FetchMetadata(commands::fetch_metadata::Opts),
    /// Find the block numbers where spec version changes happen.
    /// This is where the metadata/node API may have changed.
    FinsSpecChanges(commands::find_spec_changes::Opts),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cmd = Commands::parse();

    match cmd {
        Commands::DecodeBlocks(opts) => {
            commands::decode_blocks::run(opts).await?;
        },
        Commands::FetchMetadata(opts) => {
            commands::fetch_metadata::run(opts).await?;
        },
        Commands::FinsSpecChanges(opts) => {
            commands::find_spec_changes::run(opts).await?;
        }
    }

    Ok(())
}