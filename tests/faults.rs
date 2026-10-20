mod common;
use common::{Daemon, Mcp};
use serde_json::json;
use std::io::{Read, Write};
use std::sync::{Arc, Barrier};
use std::time::Duration;

#[test]
fn mcp_error_response_keeps_stdout_json_and_next_request_works() {
    let d = Daemon::new();
    let mut m = Mcp::new(&d.url);
    let response = m.request_raw(
        "tools/call",
        json!({"name":"submit_result","arguments":{"id":uuid::Uuid::new_v4().to_string(),"agent_id":"missing","lease_token":"stale","result":{}}}),
    );
    assert!(response.get("error").is_some() || response["result"]["isError"] == true);
    assert_eq!(
        m.tool("claim_task", json!({"agent_id":"healthy"}))["empty"],
        true
    );
}

#[test]
fn dropped_response_then_daemon_restart_replays_receipt_without_duplicate_ack() {
    let mut d = Daemon::new();
    let t = d.rpc("tasks/send", json!({"name":"response lost"}));
    let c = d.rpc("tasks/claim", json!({"id":t["id"],"agentId":"a"}));
    let params = json!({"id":t["id"],"agentId":"a","leaseToken":c["lease_token"],"result":{"receipt":true},"postAck":true,"ackName":"fault receipt"});
    let body =
        json!({"jsonrpc":"2.0","id":1,"method":"tasks/complete","params":params}).to_string();
    let address = d.url.trim_start_matches("http://");
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream,"POST / HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    let mut header_byte = [0u8; 1];
    stream.read_exact(&mut header_byte).unwrap();
    drop(stream); // Lose response body after the committed transition.
    d.restart();
    let replay = d.rpc("tasks/complete", params);
    assert_eq!(replay["ok"], true);
    assert_eq!(
        d.rpc("tasks/list", json!({"kind":"ack"}))
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn seeded_process_kills_recover_without_task_loss_or_stale_result_acceptance() {
    let mut d = Daemon::new();
    let batch = d.rpc("batches/create", json!({"name":"fault batch"}));
    let id = batch["id"].clone();
    d.rpc("batches/add_tasks",json!({"batch_id":id,"tasks":(0..20).map(|i|json!({"key":format!("{i}"),"name":"fault"})).collect::<Vec<_>>()}));
    let mut stale = Vec::new();
    let mut completed = 0;
    let mut seed = 0xC00Du64;
    for _ in 0..8 {
        let mut m = Mcp::new(&d.url);
        let c = m.tool(
            "claim_task",
            json!({"batch_id":id,"agent_id":"same","lease_seconds":1}),
        );
        let task = c["task"].clone();
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        if seed & 1 == 0 {
            stale.push(task);
            drop(m);
        } else {
            m.tool("submit_result",json!({"id":task["id"],"agent_id":"same","lease_token":task["lease_token"],"result":{}}));
            completed += 1;
        }
    }
    assert_eq!(stale.len(), 4);
    d.restart();
    std::thread::sleep(Duration::from_millis(1100));
    d.rpc("tasks/reclaim", json!({}));
    std::thread::sleep(Duration::from_millis(120));
    let mut m = Mcp::new(&d.url);
    loop {
        let c = m.tool("claim_task", json!({"batch_id":id,"agent_id":"same"}));
        if c["empty"] == true {
            break;
        }
        let t = &c["task"];
        m.tool(
            "submit_result",
            json!({"id":t["id"],"agent_id":"same","lease_token":t["lease_token"],"result":{}}),
        );
        completed += 1;
    }
    assert_eq!(completed, 20);
    for t in stale {
        assert_eq!(
            d.request(
                "tasks/complete",
                json!({"id":t["id"],"agentId":"same","leaseToken":t["lease_token"],"result":{}})
            )["error"]["code"],
            -32001
        );
    }
    assert_eq!(
        d.rpc("batches/status", json!({"batch_id":id}))["states"]["completed"],
        20
    );
}

#[test]
fn twenty_cancel_vs_async_submission_races_keep_one_terminal_state() {
    let d = Daemon::new();
    let mut accepted = 0;
    for i in 0..20 {
        let b = d.rpc(
            "batches/create",
            json!({"name":"cancel race","config":{"validator":{"delay_ms":250}}}),
        );
        let id = b["id"].clone();
        d.rpc(
            "batches/add_tasks",
            json!({"batch_id":id,"tasks":[{"key":"one","name":"race"}]}),
        );
        let c = d.rpc("scheduler/claim", json!({"batch_id":id,"agent_id":"a"}));
        let task = c["task"].clone();
        let barrier = Arc::new(Barrier::new(2));
        let ready = barrier.clone();
        let url = d.url.clone();
        let t = task.clone();
        let submit = std::thread::spawn(move || {
            let mut m = Mcp::new(&url);
            ready.wait();
            let response=m.request_raw("tools/call",json!({"name":"submit_result","arguments":{"id":t["id"],"agent_id":"a","lease_token":t["lease_token"],"result":{}}}));
            response.get("error").is_none() && response["result"]["isError"] != true
        });
        barrier.wait();
        if i % 2 == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
        d.rpc("batches/cancel", json!({"batch_id":id}));
        if submit.join().unwrap() {
            accepted += 1;
        }
        assert_eq!(
            d.rpc("batches/status", json!({"batch_id":id}))["states"]["cancelled"],
            1
        );
    }
    std::thread::sleep(Duration::from_millis(350));
    assert!(d
        .rpc("tasks/list", json!({"kind":"ack"}))
        .as_array()
        .unwrap()
        .is_empty());
    println!(
        "cancel races: accepted-before-cancel={accepted}, refused={}",
        20 - accepted
    );
}
