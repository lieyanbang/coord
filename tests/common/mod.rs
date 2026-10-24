#![allow(dead_code)]
use serde_json::{json, Value};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

pub fn binary() -> std::path::PathBuf {
    std::env::var_os("COORD_TEST_BIN")
        .map(Into::into)
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_coord").into())
}

pub struct Daemon {
    pub root: std::path::PathBuf,
    pub url: String,
    pub child: Child,
    pub http: reqwest::blocking::Client,
    port: u16,
    actions_config: Option<std::path::PathBuf>,
    rpc_key: Option<String>,
}
impl Daemon {
    pub fn new() -> Self {
        Self::with_optional_actions(None)
    }
    pub fn with_actions_config(path: std::path::PathBuf) -> Self {
        Self::with_optional_actions(Some(path))
    }
    fn with_optional_actions(actions_config: Option<std::path::PathBuf>) -> Self {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let root = std::env::temp_dir().join(format!("coord-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let child = Self::spawn(port, &root, actions_config.as_deref());
        let rpc_key = actions_config
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|registry| {
                registry["external_runner"]["daemon_rpc_key_file"]
                    .as_str()
                    .map(str::to_string)
            })
            .map(|path| std::fs::read_to_string(path).unwrap().trim().to_string());
        let d = Self {
            root,
            url: format!("http://127.0.0.1:{port}"),
            child,
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
            port,
            actions_config,
            rpc_key,
        };
        d.ready();
        d
    }
    fn spawn(port: u16, root: &std::path::Path, actions_config: Option<&std::path::Path>) -> Child {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("daemon.log"))
            .unwrap();
        let mut command = Command::new(binary());
        command
            .args(["serve", "--addr", &format!("127.0.0.1:{port}"), "--db"])
            .arg(root.join("state.db"));
        if let Some(path) = actions_config {
            command.args(["--actions-config"]).arg(path);
        }
        command
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap()
    }
    fn ready(&self) {
        for _ in 0..100 {
            if self
                .http
                .get(format!("{}/.well-known/agent.json", self.url))
                .send()
                .is_ok()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("daemon readiness timeout");
    }
    pub fn restart(&mut self) {
        self.stop();
        self.start();
    }
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    pub fn start(&mut self) {
        self.child = Self::spawn(self.port, &self.root, self.actions_config.as_deref());
        self.ready();
    }
    pub fn request(&self, method: &str, params: Value) -> Value {
        let mut request = self.http.post(&self.url);
        if let Some(key) = &self.rpc_key {
            request = request.bearer_auth(key);
        }
        request
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .unwrap()
            .json()
            .unwrap()
    }
    pub fn rpc(&self, method: &str, params: Value) -> Value {
        let v = self.request(method, params);
        assert!(v.get("error").is_none(), "{v}");
        v["result"].clone()
    }
    pub fn cli(&self, args: &[&str]) -> Value {
        let mut command = Command::new(binary());
        if let Some(key) = &self.rpc_key {
            command.env("COORD_RPC_AUTH_KEY", key);
        }
        let out = command
            .args(["--url", &self.url])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Mcp {
    child: Child,
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
    next: u64,
}
impl Mcp {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }
    pub fn new(url: &str) -> Self {
        let mut child = Command::new(binary())
            .args(["mcp", "--url", url])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut m = Self {
            child,
            input,
            output,
            next: 1,
        };
        m.request("initialize",json!({"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"coord-tests","version":"1"}}));
        m.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        m
    }
    fn send(&mut self, v: Value) {
        use std::io::Write;
        writeln!(self.input, "{v}").unwrap();
        self.input.flush().unwrap();
    }
    pub fn request_raw(&mut self, method: &str, params: Value) -> Value {
        use std::io::BufRead;
        let id = self.next;
        self.next += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        loop {
            let mut line = String::new();
            assert!(self.output.read_line(&mut line).unwrap() > 0, "MCP EOF");
            let v: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("MCP stdout must be JSON only: {e}: {line}"));
            if v["id"] == id {
                return v;
            }
        }
    }
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let v = self.request_raw(method, params);
        assert!(v.get("error").is_none(), "{v}");
        v["result"].clone()
    }
    pub fn tool_raw(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
    }
    pub fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let r = self.tool_raw(name, arguments);
        assert_ne!(r["isError"], true, "{r}");
        serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}
impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
