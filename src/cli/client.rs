//! Blocking JSON-RPC 2.0 client for the `coord` daemon. Used by every
//! client subcommand. The async surface is intentionally not exposed
//! here — the CLI runs on a `spawn_blocking` worker.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub struct Client {
    pub url: String,
    http: reqwest::blocking::Client,
}

impl Client {
    pub fn new(url: String) -> Self {
        Self {
            url,
            http: reqwest::blocking::Client::new(),
        }
    }

    pub fn with_timeout(url: String, timeout: Duration) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .context("build HTTP client")?;
        Ok(Self { url, http })
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        let body = Req {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        };
        let mut request = self.http.post(&self.url);
        if let Some(key) = coord::core::action_runner::client_management_key(&self.url)? {
            request = request.bearer_auth(key);
        }
        let resp: Resp = request
            .json(&body)
            .send()
            .with_context(|| format!("POST {}", self.url))?
            .error_for_status()
            .context("management RPC HTTP status")?
            .json()
            .context("decode response")?;
        if resp.jsonrpc.as_deref() != Some("2.0") || resp.id.as_ref() != Some(&Value::from(1)) {
            return Err(anyhow!("invalid JSON-RPC response envelope"));
        }
        if let Some(err) = resp.error {
            return Err(anyhow!("coord error {}: {}", err.code, err.message));
        }
        resp.result.ok_or_else(|| anyhow!("empty response"))
    }
}

#[derive(Serialize)]
struct Req<'a> {
    jsonrpc: &'a str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Deserialize)]
struct Resp {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<RpcErr>,
}

#[derive(Deserialize)]
struct RpcErr {
    code: i32,
    message: String,
}
