mod common;
use common::{Daemon, Mcp};
use serde_json::json;
use std::collections::HashSet;

fn batch(d: &Daemon, n: usize) -> String {
    let b=d.rpc("batches/create",json!({"name":"generic batch","config":{"retry_backoff_ms":10,"max_backoff_ms":100,"max_attempts":2}}));
    let id = b["id"].as_str().unwrap().to_string();
    d.rpc("batches/add_tasks",json!({"batch_id":id,"tasks":(0..n).map(|i|json!({"key":format!("key-{i}"),"name":format!("task {i}"),"payload":{"number":i}})).collect::<Vec<_>>()}));
    id
}

#[test]
fn hundred_tasks_over_dynamic_independent_mcp_workers_and_recovery() {
    let mut d = Daemon::new();
    let id = batch(&d, 100);
    let mut leaving = Mcp::new(&d.url);
    let abandoned = leaving.tool(
        "claim_task",
        json!({"agent_id":"leaving","batch_id":id,"lease_seconds":1}),
    );
    assert_eq!(abandoned["empty"], false);
    drop(leaving);
    let mut workers = Vec::new();
    for w in 0..4 {
        let url = d.url.clone();
        let id = id.clone();
        workers.push(std::thread::spawn(move||{
        let mut m=Mcp::new(&url);let mut done=Vec::new();let agent=format!("dynamic-{w}");
        loop {let c=m.tool("claim_task",json!({"agent_id":agent,"batch_id":id,"lease_seconds":30}));if c["empty"]==true{break}let t=&c["task"];
            m.tool("submit_result",json!({"id":t["id"],"agent_id":agent,"lease_token":t["lease_token"],"result":t["payload"]}));done.push(t["id"].as_str().unwrap().to_string());}
        done
    }));
    }
    let mut ids = HashSet::new();
    for w in workers {
        for id in w.join().unwrap() {
            assert!(ids.insert(id), "double active claim");
        }
    }
    assert_eq!(ids.len(), 99);
    std::thread::sleep(std::time::Duration::from_millis(1200));
    d.restart();
    d.rpc("tasks/reclaim", json!({}));
    std::thread::sleep(std::time::Duration::from_millis(120));
    let mut joining = Mcp::new(&d.url);
    let c = joining.tool("claim_task", json!({"agent_id":"late join","batch_id":id}));
    assert_eq!(c["task"]["id"], abandoned["task"]["id"]);
    joining.tool("submit_result",json!({"id":c["task"]["id"],"agent_id":"late join","lease_token":c["task"]["lease_token"],"result":{}}));
    let p = joining.tool("get_progress", json!({"batch_id":id}));
    assert_eq!(p["total"], 100);
    assert_eq!(p["terminal"], 100);
    assert_eq!(p["states"]["completed"], 100);
}

#[test]
fn retry_budget_backoff_receipts_idempotent_keys_and_cancel() {
    let d = Daemon::new();
    let id = batch(&d, 1);
    let mut m = Mcp::new(&d.url);
    let input =
        json!({"batch_id":id,"tasks":[{"key":"key-0","name":"task 0","payload":{"number":0}}]});
    d.rpc("batches/add_tasks", input.clone());
    assert_eq!(d.rpc("batches/status", json!({"batch_id":id}))["total"], 1);
    assert_eq!(
        d.request(
            "batches/add_tasks",
            json!({"batch_id":id,"tasks":[{"key":"key-0","name":"different"}]})
        )["error"]["code"],
        -32001
    );
    assert_eq!(d.request("batches/add_tasks",json!({"batch_id":id,"tasks":[{"key":"must rollback","name":"new"},{"key":"key-0","name":"conflict"}]}))["error"]["code"],-32001);
    assert_eq!(
        d.rpc("batches/status", json!({"batch_id":id}))["total"],
        1,
        "conflict must roll back the earlier insert"
    );
    let mut last = serde_json::Value::Null;
    for attempt in 1..=2 {
        let c = m.tool("claim_task", json!({"agent_id":"a","batch_id":id}));
        assert_eq!(c["task"]["attempt_count"], attempt);
        let t = &c["task"];
        let failure = json!({"id":t["id"],"agent_id":"a","lease_token":t["lease_token"],"error":{"reason":"retry"}});
        m.tool("report_failure", failure.clone());
        m.tool("report_failure", failure);
        last = t.clone();
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    assert_eq!(
        d.rpc("batches/status", json!({"batch_id":id}))["states"]["failed"],
        1
    );
    assert_eq!(
        d.rpc("batches/retry_failed", json!({"batch_id":id}))["retried"],
        1
    );
    assert_eq!(
        d.rpc("batches/retry_failed", json!({"batch_id":id}))["retried"],
        0
    );
    let next = m.tool("claim_task", json!({"agent_id":"a","batch_id":id}));
    assert!(next["task"]["attempt_no"].as_u64().unwrap() > last["attempt_no"].as_u64().unwrap());
    d.rpc("batches/cancel", json!({"batch_id":id}));
    d.rpc("batches/cancel", json!({"batch_id":id}));
    assert_eq!(
        d.request(
            "scheduler/submit",
            json!({"id":next["task"]["id"],"agentId":"a","leaseToken":next["task"]["lease_token"]})
        )["error"]["code"],
        -32001
    );
    assert_eq!(
        d.request("batches/add_tasks", input)["error"]["code"],
        -32001
    );
    assert_eq!(
        d.rpc("batches/status", json!({"batch_id":id}))["states"]["cancelled"],
        1
    );
}
