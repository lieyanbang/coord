mod common;

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn project_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!("coord-init-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn init(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new(common::binary())
        .current_dir(cwd)
        .arg("init")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn default_init_writes_client_guide_and_compatible_mcp_config() {
    let root = project_dir();
    let output = init(&root, &[]);
    assert!(output.contains("2 written, 0 skipped"), "{output}");
    let mcp: Value = serde_json::from_slice(&fs::read(root.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(
        mcp,
        json!({"mcpServers":{"coord":{"command":"coord","args":["mcp"]}}})
    );
    let guide = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    for required in [
        "coord mcp",
        "tasks_claim",
        "claim_task",
        "coord_claim",
        "coord_resume",
        "lease_token",
        "report_failure",
        "idempotency",
    ] {
        assert!(
            guide.contains(required),
            "missing client guidance: {required}"
        );
    }
    for development_rule in ["Repository development", "TASK.md", "scripts/verify.ps1"] {
        assert!(
            !guide.contains(development_rule),
            "repository rule leaked into client guide: {development_rule}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_nested_directory_preserves_each_existing_file_by_default() {
    let root = project_dir();
    let target = root.join("nested project").join("client");
    let target_arg = target.to_str().unwrap();
    let output = init(&root, &[target_arg]);
    assert!(output.contains("2 written, 0 skipped"), "{output}");
    assert!(target.join("AGENTS.md").is_file());
    assert!(target.join(".mcp.json").is_file());
    assert!(!root.join("AGENTS.md").exists());
    fs::remove_dir_all(&target).unwrap();
    for existing in ["AGENTS.md", ".mcp.json"] {
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join(existing), "user-owned contents").unwrap();
        let output = init(&root, &[target_arg]);
        assert!(output.contains("1 written, 1 skipped"), "{output}");
        assert_eq!(
            fs::read_to_string(target.join(existing)).unwrap(),
            "user-owned contents"
        );
        assert!(target.join("AGENTS.md").is_file());
        assert!(target.join(".mcp.json").is_file());
        fs::remove_dir_all(&target).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn force_replaces_both_files_and_repeated_init_preserves_them() {
    let root = project_dir();
    for file in ["AGENTS.md", ".mcp.json"] {
        fs::write(root.join(file), "user-owned contents").unwrap();
    }
    let output = init(&root, &["--force"]);
    assert!(output.contains("2 written, 0 skipped"), "{output}");
    let guide = fs::read(root.join("AGENTS.md")).unwrap();
    let mcp = fs::read(root.join(".mcp.json")).unwrap();
    assert!(String::from_utf8_lossy(&guide).starts_with("# Using Coord"));
    assert!(serde_json::from_slice::<Value>(&mcp).is_ok());
    let output = init(&root, &[]);
    assert!(output.contains("0 written, 2 skipped"), "{output}");
    assert_eq!(fs::read(root.join("AGENTS.md")).unwrap(), guide);
    assert_eq!(fs::read(root.join(".mcp.json")).unwrap(), mcp);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn skip_options_exclude_files_even_with_force() {
    for (args, mcp, agents) in [
        (vec!["--no-mcp"], false, true),
        (vec!["--no-agents"], true, false),
        (vec!["--no-mcp", "--no-agents"], false, false),
    ] {
        let root = project_dir();
        let expected = format!(
            "{} written, 0 skipped",
            usize::from(mcp) + usize::from(agents)
        );
        let output = init(&root, &args);
        assert!(output.contains(&expected), "{output}");
        assert_eq!(root.join(".mcp.json").exists(), mcp);
        assert_eq!(root.join("AGENTS.md").exists(), agents);
        for file in ["AGENTS.md", ".mcp.json"] {
            fs::write(root.join(file), "user-owned contents").unwrap();
        }
        let mut forced = args;
        forced.push("--force");
        let output = init(&root, &forced);
        assert!(output.contains(&expected), "{output}");
        if !mcp {
            assert!(!output.contains("Claude Code reads .mcp.json"), "{output}");
            assert_eq!(
                fs::read_to_string(root.join(".mcp.json")).unwrap(),
                "user-owned contents"
            );
        }
        if !agents {
            assert!(!output.contains("Read AGENTS.md"), "{output}");
            assert_eq!(
                fs::read_to_string(root.join("AGENTS.md")).unwrap(),
                "user-owned contents"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
