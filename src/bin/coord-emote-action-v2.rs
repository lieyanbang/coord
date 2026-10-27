use anyhow::{bail, Result};
use clap::Parser;
use std::io::Read;
#[derive(Parser)]
struct Args {
    #[arg(long)]
    root: std::path::PathBuf,
    #[arg(long)]
    supplements: std::path::PathBuf,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((coord::core::actions::MAX_ACTION_EVENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > coord::core::actions::MAX_ACTION_EVENT_BYTES {
        bail!("action event exceeds 4MiB");
    }
    coord::core::emote_v2::execute(
        &args.root,
        &args.supplements,
        &serde_json::from_slice(&bytes)?,
    )
}
