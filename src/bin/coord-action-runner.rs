use anyhow::{Context, Result};
use clap::Parser;
#[derive(Parser)]
struct Args {
    #[arg(long)]
    registry: Option<std::path::PathBuf>,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    check_identity: bool,
    /// Ed25519 private key readable only by the dedicated Runner service SID.
    #[arg(long)]
    private_key_file: Option<std::path::PathBuf>,
    /// Generate a fresh private/public key pair at the explicit protected paths.
    #[arg(long)]
    generate_keypair: bool,
    #[arg(long)]
    public_key_file: Option<std::path::PathBuf>,
    #[arg(long)]
    probe_acl: Option<std::path::PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    if args.generate_keypair {
        let private_key_file = args
            .private_key_file
            .as_deref()
            .context("--private-key-file is required for --generate-keypair")?;
        let public_key_file = args
            .public_key_file
            .as_deref()
            .context("--public-key-file is required for --generate-keypair")?;
        coord::core::action_runner::generate_keypair(private_key_file, public_key_file)?;
        eprintln!("Runner keypair generated; private and public key material were not printed.");
        return Ok(());
    }
    let registry_path = args.registry.as_deref().context("--registry is required")?;
    let registry = coord::core::actions::ActionRegistry::load(registry_path)?;
    if let Some(path) = args.probe_acl {
        let config = registry
            .runner_config()?
            .context("external runner configuration required")?;
        let identity = coord::core::windows_security::require_runner_identity(
            &config.runner_sid,
            &config.daemon_sid,
        )?;
        eprintln!("{}", serde_json::to_string(&identity)?);
        return coord::core::action_runner::probe_acl(&path);
    }
    coord::core::action_runner::run(
        registry,
        args.private_key_file.as_deref(),
        args.once,
        args.check_identity,
    )
}
