//! Opt-in queue-only broker. It never spawns action processes or exposes SQLite.
use super::actions::{ActionExecution, ActionJob, ActionRegistry, ActionState};
use super::store::Store;
use anyhow::{bail, Context, Result};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerConfig {
    pub broker_addr: SocketAddr,
    pub key_file: PathBuf,
    pub runner_sid: String,
    pub daemon_sid: String,
    pub daemon_rpc_key_file: PathBuf,
}
impl RunnerConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.broker_addr.ip().is_loopback()
            || self.broker_addr.port() == 0
            || self.runner_sid == self.daemon_sid
            || !self.runner_sid.starts_with("S-1-5-21-")
            || !self.daemon_sid.starts_with("S-1-")
        {
            bail!("runner requires loopback broker and distinct dedicated account SID");
        }
        self.key()?;
        if !self.daemon_rpc_key_file.is_absolute() || self.daemon_rpc_key_file == self.key_file {
            bail!("separate protected daemon RPC key path required");
        }
        Ok(())
    }
    fn key(&self) -> Result<String> {
        let path = super::windows_security::safe_existing_path(&self.key_file)?;
        if std::fs::metadata(&path)?.len() > 128 {
            bail!("broker key file too large");
        }
        let key = std::fs::read_to_string(path)?.trim().to_string();
        if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("broker key must contain 32 random bytes encoded as hex");
        }
        Ok(key)
    }
    fn owner(&self) -> String {
        format!("runner:{}", self.runner_sid)
    }
}

#[derive(Clone)]
struct Broker {
    store: Arc<Store>,
    config: RunnerConfig,
    key: Arc<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Fence {
    id: String,
    lease_token: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Finish {
    fence: Fence,
    state: ActionState,
    code: String,
    exit_code: Option<i32>,
    output_bytes: usize,
}

fn authenticated(headers: &HeaderMap, key: &str) -> bool {
    let Some(value) = headers
        .get("authorization")
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
    else {
        return false;
    };
    value.len() == key.len()
        && value
            .bytes()
            .zip(key.bytes())
            .fold(0, |different, (left, right)| different | (left ^ right))
            == 0
}
type ApiError = StatusCode;
async fn claim(
    State(state): State<Broker>,
    headers: HeaderMap,
) -> std::result::Result<Json<Option<ActionJob>>, ApiError> {
    if !authenticated(&headers, &state.key) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    state
        .store
        .begin_action(&state.config.owner())
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
async fn authorize(
    State(state): State<Broker>,
    headers: HeaderMap,
    Json(fence): Json<Fence>,
) -> std::result::Result<Json<Option<ActionJob>>, ApiError> {
    if !authenticated(&headers, &state.key) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    state
        .store
        .runner_action(&fence.id, &fence.lease_token, &state.config.owner(), true)
        .map(Json)
        .map_err(|_| StatusCode::CONFLICT)
}
const CODES: &[&str] = &[
    "exit_zero",
    "spawn_failed",
    "job_assignment_retryable",
    "job_assignment_failed",
    "timeout_retryable",
    "timeout",
    "output_limit_retryable",
    "output_limit",
    "nonzero_exit_retryable",
    "nonzero_exit",
    "process_wait_retryable",
    "process_wait_failed",
    "executable_integrity_failed",
    "profile_missing",
    "profile_version_changed",
    "action_missing",
    "profile_changed",
    "event_too_large",
    "invalid_profile_arguments",
    "process_resume_failed",
];
async fn finish(
    State(state): State<Broker>,
    headers: HeaderMap,
    Json(report): Json<Finish>,
) -> std::result::Result<Json<bool>, ApiError> {
    if !authenticated(&headers, &state.key) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let code = *CODES
        .iter()
        .find(|code| **code == report.code)
        .ok_or(StatusCode::BAD_REQUEST)?;
    if report.output_bytes > 128 * 1024 * 1024
        || (report.state == ActionState::Succeeded
            && (code != "exit_zero" || report.exit_code != Some(0)))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let job = state
        .store
        .runner_action(
            &report.fence.id,
            &report.fence.lease_token,
            &state.config.owner(),
            false,
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::CONFLICT)?;
    state
        .store
        .finish_action(
            &job,
            &ActionExecution {
                state: report.state,
                code,
                exit_code: report.exit_code,
                output_bytes: report.output_bytes,
            },
        )
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub fn broker_router(store: Arc<Store>, config: RunnerConfig) -> Result<Router> {
    config.validate()?;
    let key = Arc::new(config.key()?);
    Ok(Router::new()
        .route("/claim", post(claim))
        .route("/authorize", post(authorize))
        .route("/finish", post(finish))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(Broker { store, config, key }))
}

pub fn management_key(config: &RunnerConfig) -> Result<String> {
    let path = super::windows_security::safe_existing_path(&config.daemon_rpc_key_file)?;
    if std::fs::metadata(&path)?.len() > 128 {
        bail!("management key file too large");
    }
    let key = std::fs::read_to_string(path)?.trim().to_string();
    if key.len() != 64
        || !key.bytes().all(|byte| byte.is_ascii_hexdigit())
        || key.eq_ignore_ascii_case(&config.key()?)
    {
        bail!("independent 32-byte management key required");
    }
    Ok(key)
}

pub fn protect_management_rpc(router: Router, key: String) -> Router {
    use axum::{
        extract::Request,
        middleware::{self, Next},
        response::Response,
    };
    async fn guard(
        State(key): State<Arc<String>>,
        request: Request,
        next: Next,
    ) -> std::result::Result<Response, StatusCode> {
        if request.method() != axum::http::Method::GET && !authenticated(request.headers(), &key) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        Ok(next.run(request).await)
    }
    router.layer(middleware::from_fn_with_state(Arc::new(key), guard))
}

pub fn client_management_key(url: &str) -> Result<Option<String>> {
    let Ok(key) = std::env::var("COORD_RPC_AUTH_KEY") else {
        return Ok(None);
    };
    let parsed = reqwest::Url::parse(url)?;
    if parsed.scheme() != "http"
        || !parsed
            .host_str()
            .is_some_and(|host| host == "127.0.0.1" || host == "[::1]" || host == "localhost")
    {
        bail!("COORD_RPC_AUTH_KEY is restricted to loopback HTTP");
    }
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid management key format");
    }
    Ok(Some(key))
}
pub async fn serve_broker(store: Arc<Store>, config: RunnerConfig) -> Result<()> {
    if super::windows_security::process_identity()?.sid != config.daemon_sid {
        bail!("broker daemon SID differs from administrator configuration");
    }
    let address = config.broker_addr;
    let router = broker_router(store, config)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AclProbe {
    read_files: Vec<PathBuf>,
    denied_read_files: Vec<PathBuf>,
    denied_write_files: Vec<PathBuf>,
    write_directories: Vec<PathBuf>,
    denied_write_directories: Vec<PathBuf>,
    denied_rpc_url: Option<String>,
    broker_key_file: Option<PathBuf>,
}
/// Operator-only rehearsal mode; it probes exact disposable fixtures and never
/// changes a service/account/ACL. The caller must create the protected fixtures.
pub fn probe_acl(path: &std::path::Path) -> Result<()> {
    let path = super::windows_security::safe_existing_path(path)?;
    if std::fs::metadata(&path)?.len() > 64 * 1024 {
        bail!("ACL probe configuration too large");
    }
    let probe: AclProbe = serde_json::from_slice(&std::fs::read(path)?)?;
    for path in &probe.read_files {
        let _ = std::fs::read(path).context("required read denied")?;
    }
    for path in &probe.denied_read_files {
        if std::fs::read(path).is_ok() {
            bail!("runner unexpectedly read a protected file");
        }
    }
    for path in &probe.denied_write_files {
        if std::fs::OpenOptions::new().write(true).open(path).is_ok() {
            bail!("runner unexpectedly opened a protected file for writing");
        }
    }
    for (paths, allowed) in [
        (&probe.write_directories, true),
        (&probe.denied_write_directories, false),
    ] {
        for directory in paths {
            let marker = directory.join(format!(".coord-acl-probe-{}", uuid::Uuid::new_v4()));
            let result = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&marker);
            let succeeded = result.is_ok();
            drop(result);
            if succeeded {
                std::fs::remove_file(marker)?;
            }
            if succeeded != allowed {
                bail!("runner directory write permission differs from policy");
            }
        }
    }
    if let Some(url) = probe.denied_rpc_url {
        let parsed = reqwest::Url::parse(&url)?;
        if parsed.scheme() != "http"
            || !parsed
                .host_str()
                .is_some_and(|host| host == "127.0.0.1" || host == "[::1]" || host == "localhost")
        {
            bail!("ACL RPC probe must target loopback HTTP");
        }
        let http = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()?;
        let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tasks/list","params":{"skip_reclaim":true,"limit":1}});
        if http.post(&url).json(&body).send()?.status() != 401 {
            bail!("runner accessed management RPC without a key");
        }
        let key =
            std::fs::read_to_string(probe.broker_key_file.context("RPC probe broker key path")?)?;
        if http
            .post(&url)
            .bearer_auth(key.trim())
            .json(&body)
            .send()?
            .status()
            != 401
        {
            bail!("runner broker key accessed management RPC");
        }
        eprintln!("management RPC rejected missing and runner broker credentials: 401/401");
    }
    eprintln!(
        "{}",
        serde_json::json!({"acl_probe":"PASS","reads":probe.read_files.len(),"denied_reads":probe.denied_read_files.len(),"denied_writes":probe.denied_write_files.len(),"writable_directories":probe.write_directories.len(),"denied_directories":probe.denied_write_directories.len()})
    );
    Ok(())
}

pub fn run(registry: ActionRegistry, once: bool, check_identity: bool) -> Result<()> {
    let config = registry
        .external_runner
        .as_ref()
        .context("external_runner registry configuration required")?;
    let identity =
        super::windows_security::require_runner_identity(&config.runner_sid, &config.daemon_sid)?;
    if check_identity {
        eprintln!("{}", serde_json::to_string(&identity)?);
        return Ok(());
    }
    let key = config.key()?;
    let base = format!("http://{}", config.broker_addr);
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    loop {
        let job: Option<ActionJob> = http
            .post(format!("{base}/claim"))
            .bearer_auth(&key)
            .send()?
            .error_for_status()?
            .json()?;
        if let Some(job) = job {
            let fence = Fence {
                id: job.id,
                lease_token: job.lease_token,
            };
            // Ask immediately before spawn; the broker reconstructs the trusted
            // event and checks current receipt/attempt/fence with >=305s left.
            let authorized: Option<ActionJob> = http
                .post(format!("{base}/authorize"))
                .bearer_auth(&key)
                .json(&fence)
                .send()?
                .error_for_status()?
                .json()?;
            if let Some(authorized) = authorized {
                let outcome = registry.execute(&authorized);
                let report = Finish {
                    fence,
                    state: outcome.state,
                    code: outcome.code.into(),
                    exit_code: outcome.exit_code,
                    output_bytes: outcome.output_bytes,
                };
                let accepted: bool = http
                    .post(format!("{base}/finish"))
                    .bearer_auth(&key)
                    .json(&report)
                    .send()?
                    .error_for_status()?
                    .json()?;
                if !accepted {
                    bail!("action result lost its fence; reconcile outbox before retrying");
                }
                eprintln!(
                    "action {}: {} ({})",
                    authorized.event_id,
                    outcome.state.as_str(),
                    outcome.code
                );
            }
        }
        if once {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}
