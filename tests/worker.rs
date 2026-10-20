mod common;
use common::Daemon;
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn python() -> String {
    std::env::var("COORD_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into())
}
fn enqueue_attempts(d: &Daemon, payload: Value, attempts: u32) -> String {
    let b = d.rpc(
        "batches/create",
        json!({"name":"worker","config":{"max_attempts":attempts}}),
    );
    let id = b["id"].as_str().unwrap().to_string();
    d.rpc(
        "batches/add_tasks",
        json!({"batch_id":id,"tasks":[{"key":"one","name":"adapter","payload":payload}]}),
    );
    id
}
fn enqueue(d: &Daemon, payload: Value) -> String {
    enqueue_attempts(d, payload, 1)
}
fn worker(d: &Daemon, id: &str, external: bool, timeout: &str) -> std::process::Child {
    let mut c = Command::new(python());
    c.arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/worker.py"))
        .args([
            "--url",
            &d.url,
            "--batch",
            id,
            "--agent",
            "reference",
            "--lease",
            "2",
            "--timeout",
            timeout,
        ]);
    if external {
        c.arg("--workdir")
            .arg(&d.root)
            .arg("--adapter")
            .arg(python())
            .arg(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/adapter.py"),
            );
    }
    c.stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}
fn wait(mut c: std::process::Child) {
    let start = Instant::now();
    loop {
        if let Some(s) = c.try_wait().unwrap() {
            assert!(s.success());
            return;
        }
        if start.elapsed() > Duration::from_secs(15) {
            let _ = c.kill();
            panic!("worker did not exit")
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn real_worker_identity_external_timeout_output_limit_and_kill_recovery() {
    let d = Daemon::new();
    let id = enqueue(&d, json!({"generic":[1,2,3]}));
    wait(worker(&d, &id, false, "5"));
    assert_eq!(
        d.rpc("batches/status", json!({"batch_id":id}))["states"]["completed"],
        1
    );
    let id = enqueue(&d, json!({"sleep":3.2,"result":{"ok":true}}));
    wait(worker(&d, &id, true, "5"));
    assert_eq!(
        d.rpc("batches/status", json!({"batch_id":id}))["states"]["completed"],
        1,
        "adapter must renew its two-second lease"
    );
    for (payload, timeout, expected) in [
        (json!({"sleep":3}), "0.2", "timed out"),
        (json!({"oversize":true}), "5", "exceeded"),
    ] {
        let id = enqueue(&d, payload);
        wait(worker(&d, &id, true, timeout));
        assert_eq!(
            d.rpc("batches/status", json!({"batch_id":id}))["states"]["failed"],
            1
        );
        let tasks = d.rpc("tasks/list", json!({"state":"failed","limit":100}));
        let task = tasks
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["batch_id"] == id)
            .unwrap();
        assert!(
            task["error"]["message"]
                .as_str()
                .unwrap()
                .contains(expected),
            "{task}"
        );
    }
    let pid_file = d.root.join("adapter.pid");
    let id = enqueue_attempts(&d, json!({"sleep":60,"pid_file":pid_file}), 2);
    let mut c = worker(&d, &id, true, "120");
    let started = Instant::now();
    while !pid_file.exists() {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(30));
    }
    let adapter_pid = std::fs::read_to_string(&pid_file).unwrap();
    c.kill().unwrap();
    c.wait().unwrap();
    if cfg!(windows) {
        std::thread::sleep(Duration::from_millis(200));
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {adapter_pid}"), "/FO", "CSV", "/NH"])
            .output()
            .unwrap();
        let alive = String::from_utf8_lossy(&output.stdout).contains(&format!("\"{adapter_pid}\""));
        if alive {
            let _ = Command::new("taskkill")
                .args(["/PID", &adapter_pid, "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        assert!(
            !alive,
            "worker crash must close the Job Object and kill its adapter tree"
        );
    }

    std::thread::sleep(Duration::from_millis(2200));
    d.rpc("tasks/reclaim", json!({}));
    assert_eq!(
        d.rpc("batches/status", json!({"batch_id":id}))["states"]["pending"],
        1
    );
    std::thread::sleep(Duration::from_millis(120));
    wait(worker(&d, &id, false, "5"));
    assert_eq!(
        d.rpc("batches/status", json!({"batch_id":id}))["states"]["completed"],
        1
    );
}
