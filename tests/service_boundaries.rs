use serde_json::json;
use std::process::{Command, Stdio};

#[test]
fn browser_and_oversize_requests_cannot_write_tasks() {
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    let root = std::env::temp_dir().join(format!("coord-boundary-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_coord"))
        .args(["serve", "--addr", &format!("127.0.0.1:{port}"), "--db"])
        .arg(root.join("db"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let result = std::panic::catch_unwind(|| {
        let http = reqwest::blocking::Client::new();
        let url = format!("http://127.0.0.1:{port}");
        for _ in 0..100 {
            if http
                .get(format!("{url}/.well-known/agent.json"))
                .send()
                .is_ok()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        let body =
            json!({"jsonrpc":"2.0","id":1,"method":"tasks/send","params":{"name":"blocked"}});
        let v: serde_json::Value = http
            .post(&url)
            .header("Origin", "https://attacker.example")
            .json(&body)
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert!(v.get("error").is_some());
        assert_eq!(
            http.post(&url)
                .header("content-type", "application/json")
                .body(" ".repeat(1024 * 1024 + 1))
                .send()
                .unwrap()
                .status(),
            413
        );
        let v: serde_json::Value = http
            .post(&url)
            .json(&json!({"jsonrpc":"2.0","id":2,"method":"tasks/list","params":{}}))
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert!(v["result"].as_array().unwrap().is_empty());
    });
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
