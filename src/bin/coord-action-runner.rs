use anyhow::Result;
use clap::Parser;
#[derive(Parser)]
struct Args {
    #[arg(long)]
    registry: std::path::PathBuf,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    check_identity: bool,
    #[arg(long)]
    probe_acl: Option<std::path::PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let registry = coord::core::actions::ActionRegistry::load(&args.registry)?;
    if let Some(path) = args.probe_acl {
        let config = registry
            .external_runner
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("external runner required"))?;
        let identity = coord::core::windows_security::require_runner_identity(
            &config.runner_sid,
            &config.daemon_sid,
        )?;
        eprintln!("{}", serde_json::to_string(&identity)?);
        return coord::core::action_runner::probe_acl(&path);
    }
    coord::core::action_runner::run(registry, args.once, args.check_identity)
}
