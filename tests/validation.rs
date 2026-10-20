mod common;
use common::{Daemon, Mcp};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn batch(d: &Daemon, validator: Value, n: usize) -> String {
    let b=d.rpc("batches/create",json!({"name":"validation","config":{"max_attempts":2,"retry_backoff_ms":10,"max_backoff_ms":100,"validator":validator}}));
    let id = b["id"].as_str().unwrap().to_string();
    d.rpc("batches/add_tasks",json!({"batch_id":id,"tasks":(0..n).map(|i|json!({"key":format!("{i}"),"name":"validate","payload":{"ok":true}})).collect::<Vec<_>>()}));
    id
}
fn terminal(d: &Daemon, id: &str) -> Value {
    let start = Instant::now();
    loop {
        let p = d.rpc("batches/status", json!({"batch_id":id}));
        if p["active"] == 0 {
            return p;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "{p}");
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn submit(m: &mut Mcp, t: &Value, result: Value) -> Value {
    m.tool("submit_result",json!({"id":t["id"],"agent_id":"a","lease_token":t["lease_token"],"result":result,"post_ack":true,"ack_name":"validated"}))
}

#[test]
fn async_pass_fail_retry_does_not_block_next_claim_and_ack_waits_for_pass() {
    let d = Daemon::new();
    let id = batch(&d, json!({"required":["ok"],"delay_ms":400}), 2);
    let mut m = Mcp::new(&d.url);
    let one = m.tool("claim_task", json!({"agent_id":"a","batch_id":id}));
    let started = Instant::now();
    let response = submit(&mut m, &one["task"], json!({"ok":true}));
    assert_eq!(response["state"], "submitted");
    assert!(started.elapsed() < Duration::from_millis(300));
    let two = m.tool("claim_task", json!({"agent_id":"a","batch_id":id}));
    assert_eq!(two["empty"], false);
    assert_eq!(
        m.tool("get_task", json!({"id":one["task"]["id"]}))["state"],
        "submitted"
    );
    assert!(d
        .rpc("tasks/list", json!({"kind":"ack"}))
        .as_array()
        .unwrap()
        .is_empty());
    submit(&mut m, &two["task"], json!({}));
    let start = Instant::now();
    let retried = loop {
        let c = m.tool("claim_task", json!({"agent_id":"a","batch_id":id}));
        if c["empty"] == false {
            break c;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(30));
    };
    assert_eq!(retried["task"]["attempt_count"], 2);
    submit(&mut m, &retried["task"], json!({"ok":true}));
    assert_eq!(terminal(&d, &id)["states"]["completed"], 2);
    assert_eq!(
        d.rpc("tasks/list", json!({"kind":"ack"}))
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        submit(&mut m, &one["task"], json!({"ok":true})),
        response,
        "accepted submission receipt remains stable"
    );
}

#[test]
fn validator_error_budget_is_separate_from_execution_budget() {
    let d = Daemon::new();
    let id = batch(
        &d,
        json!({"delay_ms":100,"timeout_ms":5,"max_error_attempts":2}),
        1,
    );
    let mut m = Mcp::new(&d.url);
    let c = m.tool("claim_task", json!({"agent_id":"a","batch_id":id}));
    submit(&mut m, &c["task"], json!({}));
    assert_eq!(terminal(&d, &id)["states"]["failed"], 1);
    let t = m.tool("get_task", json!({"id":c["task"]["id"]}));
    assert_eq!(t["attempt_count"], 1);
    assert_eq!(t["validation_error_count"], 2);
    assert_eq!(t["error"]["kind"], "validation_error");
}

#[test]
fn validation_pending_recovers_after_restart_and_cancel_verdict_is_stale() {
    let mut d = Daemon::new();
    let id = batch(&d, json!({"delay_ms":300,"timeout_ms":1000}), 1);
    let mut m = Mcp::new(&d.url);
    let c = m.tool("claim_task", json!({"agent_id":"a","batch_id":id}));
    submit(&mut m, &c["task"], json!({}));
    std::thread::sleep(Duration::from_millis(80));
    d.restart();
    assert_eq!(terminal(&d, &id)["states"]["completed"], 1);
    let cancel = batch(&d, json!({"delay_ms":500}), 1);
    let c = m.tool("claim_task", json!({"agent_id":"a","batch_id":cancel}));
    submit(&mut m, &c["task"], json!({}));
    std::thread::sleep(Duration::from_millis(80));
    d.rpc("batches/cancel", json!({"batch_id":cancel}));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(terminal(&d, &cancel)["states"]["cancelled"], 1);
    assert_eq!(
        d.rpc("tasks/list", json!({"kind":"ack"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn validator_fail_exhausts_execution_budget_and_old_job_token_cannot_commit() {
    let d = Daemon::new();
    let id = batch(&d, json!({"required":["ok"]}), 1);
    let mut m = Mcp::new(&d.url);
    for _ in 0..2 {
        let start = Instant::now();
        let c = loop {
            let c = m.tool("claim_task", json!({"agent_id":"a","batch_id":id}));
            if c["empty"] == false {
                break c;
            }
            assert!(start.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(30));
        };
        submit(&mut m, &c["task"], json!({}));
    }
    assert_eq!(terminal(&d, &id)["states"]["failed"], 1);
    use coord::core::{
        store::Store,
        types::{BatchConfig, TaskInput},
        validator::{Validation, ValidatorConfig},
    };
    let path = d.root.join("isolated-job.db");
    let store = Store::open(&path).unwrap();
    let batch = store
        .create_batch(
            "job",
            "",
            BatchConfig {
                validator: Some(ValidatorConfig::default()),
                ..Default::default()
            },
        )
        .unwrap();
    let id = uuid::Uuid::parse_str(batch["id"].as_str().unwrap()).unwrap();
    let t = store
        .add_tasks(
            id,
            vec![TaskInput {
                key: "one".into(),
                name: "job".into(),
                payload: json!({}),
            }],
        )
        .unwrap()
        .remove(0);
    let c = store.claim_task(t.id, "a", None).unwrap().unwrap();
    store
        .complete_task(
            t.id,
            "a",
            c.lease_token.as_deref().unwrap(),
            json!({}),
            None,
        )
        .unwrap();
    let old = store.begin_validation().unwrap().unwrap();
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE tasks SET validation_until='2000-01-01T00:00:00Z'",
            [],
        )
        .unwrap();
    let current = store.begin_validation().unwrap().unwrap();
    assert_ne!(old.token, current.token);
    assert!(!store.finish_validation(&old, Validation::Pass).unwrap());
    assert!(store.finish_validation(&current, Validation::Pass).unwrap());
    assert_eq!(
        store
            .get_task(t.id)
            .unwrap()
            .unwrap()
            .validation_error_count,
        1
    );
}
