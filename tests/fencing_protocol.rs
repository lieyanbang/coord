mod common;
use common::Daemon;
use serde_json::json;

#[test]
fn real_process_fencing_survives_restart_and_rejects_legacy_writes() {
    let mut d = Daemon::new();
    let task = d.cli(&["send", "protocol fence"]);
    let id = task["id"].as_str().unwrap();
    let first = d.cli(&["claim", id, "--as", "same", "--lease", "1"]);
    let old = first["lease_token"].as_str().unwrap();
    assert_eq!(
        d.request("tasks/complete", json!({"id":id}))["error"]["code"],
        -32602
    );
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(
        d.request(
            "tasks/extend",
            json!({"id":id,"agentId":"same","leaseToken":old})
        )["error"]["code"],
        -32001
    );
    d.restart();
    d.cli(&["reclaim"]);
    let second = d.cli(&["claim", id, "--as", "same"]);
    let token = second["lease_token"].as_str().unwrap();
    assert_ne!(old, token);
    assert_eq!(
        d.request(
            "tasks/complete",
            json!({"id":id,"agentId":"same","leaseToken":old,"result":1})
        )["error"]["code"],
        -32001
    );
    let result = d.cli(&[
        "complete",
        id,
        "--as",
        "same",
        "--token",
        token,
        "--result",
        "1",
        "--ack",
        "fenced done",
    ]);
    assert_eq!(result["ok"], true);
    let replay = d.cli(&[
        "complete",
        id,
        "--as",
        "same",
        "--token",
        token,
        "--result",
        "1",
        "--ack",
        "fenced done",
    ]);
    assert_eq!(result, replay);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_coord"))
        .args(["--url", &d.url, "complete", id, "--result", "1"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}
