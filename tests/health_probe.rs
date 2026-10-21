#[path = "common/mod.rs"]
mod common;

use serde_json::Value;
use std::process::{Command, Output};

fn probe(daemon: &common::Daemon, db: &std::path::Path, status: &std::path::Path) -> Output {
    Command::new(common::binary())
        .args(["--url", &daemon.url, "health", "--db"])
        .arg(db)
        .arg("--status")
        .arg(status)
        .output()
        .unwrap()
}

fn snapshot(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("health output must be JSON: {error}"))
}

#[test]
fn health_probe_is_read_only_tracks_success_and_does_not_persist_task_data() {
    let daemon = common::Daemon::new();
    let task = daemon.rpc(
        "tasks/send",
        serde_json::json!({"name":"health-probe-secret","payload":{"marker":"private"}}),
    );
    let claim = daemon.rpc(
        "tasks/claim",
        serde_json::json!({"id":task["id"],"agentId":"health-test","leaseSeconds":300}),
    );
    let token = claim["lease_token"].as_str().unwrap().to_owned();
    let status_path = daemon.root.join("health").join("status.json");
    let db_path = daemon.root.join("state.db");

    let before = daemon
        .rpc("tasks/list", serde_json::json!({"limit":50}))
        .as_array()
        .unwrap()
        .len();
    let healthy = probe(&daemon, &db_path, &status_path);
    assert!(
        healthy.status.success(),
        "{}",
        String::from_utf8_lossy(&healthy.stderr)
    );
    let healthy_json = snapshot(&healthy);
    assert_eq!(healthy_json["status"], "READY");
    assert_eq!(healthy_json["checks"]["tasks_list"], "PASS");
    assert_eq!(healthy_json["checks"]["sqlite_read_only"], "PASS");
    assert!(healthy_json["last_success_at_utc"].is_string());
    assert_eq!(healthy_json["consecutive_failures"], 0);
    assert!(!String::from_utf8_lossy(&healthy.stdout).contains(&token));
    assert!(!std::fs::read_to_string(&status_path)
        .unwrap()
        .contains(&token));
    assert!(!std::fs::read_to_string(&status_path)
        .unwrap()
        .contains("health-probe-secret"));
    let after = daemon
        .rpc("tasks/list", serde_json::json!({"limit":50}))
        .as_array()
        .unwrap()
        .len();
    assert_eq!(before, after, "health must not mutate the production queue");

    let missing_db = daemon.root.join("missing.db");
    for expected_failures in 1..=2 {
        let failed = probe(&daemon, &missing_db, &status_path);
        assert!(!failed.status.success());
        let failed_json = snapshot(&failed);
        assert_eq!(failed_json["status"], "FAIL");
        assert_eq!(failed_json["consecutive_failures"], expected_failures);
        assert_eq!(
            failed_json["last_success_at_utc"],
            healthy_json["last_success_at_utc"]
        );
        assert!(!failed_json["failed_checks"].to_string().contains(&token));
        assert!(
            !missing_db.exists(),
            "read-only probe must not create a database"
        );
    }

    let recovered = probe(&daemon, &db_path, &status_path);
    assert!(recovered.status.success());
    let recovered_json = snapshot(&recovered);
    assert_eq!(recovered_json["status"], "READY");
    assert_eq!(recovered_json["consecutive_failures"], 0);
    assert!(recovered_json["last_success_at_utc"].is_string());
}

#[test]
fn health_probe_does_not_run_the_tasks_list_lease_reclaimer() {
    let daemon = common::Daemon::new();
    let task = daemon.rpc(
        "tasks/send",
        serde_json::json!({"name":"expired-lease-read-only-probe"}),
    );
    daemon.rpc(
        "tasks/claim",
        serde_json::json!({"id":task["id"],"agentId":"health-test","leaseSeconds":1}),
    );
    std::thread::sleep(std::time::Duration::from_millis(1200));

    let status_path = daemon.root.join("health").join("status.json");
    let db_path = daemon.root.join("state.db");
    let healthy = probe(&daemon, &db_path, &status_path);
    assert!(
        healthy.status.success(),
        "{}",
        String::from_utf8_lossy(&healthy.stderr)
    );
    assert_eq!(snapshot(&healthy)["status"], "READY");
    let still_claimed = daemon.rpc("tasks/get", serde_json::json!({"id":task["id"]}));
    assert_eq!(still_claimed["state"], "claimed");

    let normal_list = daemon.rpc("tasks/list", serde_json::json!({"limit":10}));
    let task_after_reclaim = normal_list
        .as_array()
        .unwrap()
        .iter()
        .find(|candidate| candidate["id"] == task["id"])
        .unwrap();
    assert_eq!(task_after_reclaim["state"], "pending");
}
