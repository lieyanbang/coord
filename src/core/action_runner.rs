//! Opt-in queue-only broker. It never spawns action processes or exposes SQLite.
use super::actions::{ActionExecution, ActionJob, ActionRegistry, ActionState};
use super::store::Store;
use anyhow::{bail, Context, Result};
use axum::{
    body::{to_bytes, Body},
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::post,
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use ring::{
    digest::{digest, SHA256},
    rand::{SecureRandom, SystemRandom},
    signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519},
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

const REQUEST_VERSION: &str = "1";
const CLOCK_SKEW_MS: i64 = 30_000;
const MAX_SIGNED_BODY_BYTES: usize = 4096;
const NONCE_BYTES: usize = 16;

fn nonce_expiry_unix_ms(timestamp_ms: i64) -> i64 {
    // A request signed at the oldest still-accepted timestamp remains valid
    // through `timestamp + CLOCK_SKEW_MS`. Keep its nonce until the next
    // millisecond so the cleanup query cannot erase it while that request is
    // still admissible at the inclusive clock-skew boundary.
    timestamp_ms.saturating_add(CLOCK_SKEW_MS).saturating_add(1)
}

#[derive(Clone)]
pub struct RuntimeHealth(Arc<parking_lot::Mutex<RuntimeHealthState>>);

#[derive(Clone)]
struct RuntimeHealthState {
    broker_status: &'static str,
    broker_reason: &'static str,
    runner_security_reason: Option<&'static str>,
    runner_last_seen_unix_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthComponent {
    pub status: &'static str,
    pub reason: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeHealthSnapshot {
    pub checked_at_unix_ms: i64,
    pub status: &'static str,
    pub core: HealthComponent,
    pub database: HealthComponent,
    pub broker: HealthComponent,
    pub runner: HealthComponent,
    pub runner_last_seen_unix_ms: Option<i64>,
}

impl RuntimeHealth {
    pub fn new() -> Self {
        Self(Arc::new(parking_lot::Mutex::new(RuntimeHealthState {
            broker_status: "UNAVAILABLE",
            broker_reason: "not_configured",
            runner_security_reason: None,
            runner_last_seen_unix_ms: None,
        })))
    }

    pub fn broker_ready(&self) {
        let mut state = self.0.lock();
        state.broker_status = "READY";
        state.broker_reason = "";
    }

    pub fn broker_starting(&self) {
        let mut state = self.0.lock();
        state.broker_status = "UNAVAILABLE";
        state.broker_reason = "initializing";
        state.runner_security_reason = None;
        state.runner_last_seen_unix_ms = None;
    }

    pub fn broker_unavailable(&self, reason: &'static str) {
        let mut state = self.0.lock();
        state.broker_status = "UNAVAILABLE";
        state.broker_reason = reason;
        state.runner_security_reason = None;
        state.runner_last_seen_unix_ms = None;
    }

    pub fn broker_security_error(&self, reason: &'static str) {
        let mut state = self.0.lock();
        state.broker_status = "SECURITY_ERROR";
        state.broker_reason = reason;
        state.runner_security_reason = None;
        state.runner_last_seen_unix_ms = None;
    }

    fn runner_seen(&self) {
        let mut state = self.0.lock();
        state.runner_security_reason = None;
        state.runner_last_seen_unix_ms = Some(Utc::now().timestamp_millis());
    }

    pub fn runner_security_error(&self, reason: &'static str) {
        let mut state = self.0.lock();
        state.runner_security_reason = Some(reason);
        state.runner_last_seen_unix_ms = None;
    }

    pub fn snapshot(&self) -> RuntimeHealthSnapshot {
        const RUNNER_STALE_AFTER_MS: i64 = 30_000;
        let now = Utc::now().timestamp_millis();
        let state = self.0.lock().clone();
        let runner_ready = state.broker_status == "READY"
            && state.runner_security_reason.is_none()
            && state
                .runner_last_seen_unix_ms
                .is_some_and(|last| now.saturating_sub(last) <= RUNNER_STALE_AFTER_MS);
        let runner_status = if state.runner_security_reason.is_some() {
            "SECURITY_ERROR"
        } else if state.broker_status != "READY" {
            "UNAVAILABLE"
        } else if runner_ready {
            "READY"
        } else {
            "UNAVAILABLE"
        };
        let status = if state.broker_status == "READY" && runner_ready {
            "READY"
        } else {
            "DEGRADED"
        };
        RuntimeHealthSnapshot {
            checked_at_unix_ms: now,
            status,
            core: HealthComponent {
                status: "READY",
                reason: None,
            },
            database: HealthComponent {
                status: "READY",
                reason: None,
            },
            broker: HealthComponent {
                status: state.broker_status,
                reason: (!state.broker_reason.is_empty()).then_some(state.broker_reason),
            },
            runner: HealthComponent {
                status: runner_status,
                reason: if let Some(reason) = state.runner_security_reason {
                    Some(reason)
                } else if state.broker_status != "READY" {
                    Some("broker_unavailable")
                } else if runner_ready {
                    None
                } else {
                    Some("heartbeat_stale_or_missing")
                },
            },
            runner_last_seen_unix_ms: state.runner_last_seen_unix_ms,
        }
    }
}

impl Default for RuntimeHealth {
    fn default() -> Self {
        Self::new()
    }
}

/// Compare configured credential paths without touching the files. This is
/// intentionally lexical so a denied legacy Broker-key path never needs to be
/// inspected in order to reject accidental credential-path reuse.
pub(crate) fn same_config_path(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        let value = path.to_string_lossy().replace('/', "\\");
        #[cfg(windows)]
        {
            value.trim_end_matches('\\').to_ascii_lowercase()
        }
        #[cfg(not(windows))]
        {
            value
        }
    };
    normalize(left) == normalize(right)
}

fn valid_sid_text(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').collect();
    parts.len() >= 4
        && parts.len() <= 18
        && parts[0] == "S"
        && parts[1] == "1"
        && parts[2]
            .parse::<u64>()
            .is_ok_and(|authority| authority <= 0x0000_FFFF_FFFF_FFFF)
        && parts[3..].iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part
                    .parse::<u32>()
                    .is_ok_and(|number| number.to_string() == *part)
        })
}

fn valid_runner_sid(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').collect();
    valid_sid_text(value) && parts.len() == 8 && parts[..4] == ["S", "1", "5", "21"]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerConfig {
    pub broker_addr: SocketAddr,
    pub runner_sid: String,
    pub daemon_sid: String,
    /// Deprecated registry field retained only for backward-compatible
    /// deserialization. Runner authentication never reads this path.
    #[serde(default)]
    #[serde(rename = "daemon_rpc_key_file")]
    pub legacy_management_key_path: Option<PathBuf>,
    pub runner_key_id: String,
    pub runner_public_key_file: PathBuf,
    /// Accepted only while reading old registries; never opened by Core or Runner.
    #[serde(default, rename = "key_file")]
    pub legacy_key_file: Option<PathBuf>,
}
impl RunnerConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.broker_addr.ip().is_loopback()
            || self.broker_addr.port() == 0
            || self.runner_sid == self.daemon_sid
            || !valid_runner_sid(&self.runner_sid)
            || !valid_sid_text(&self.daemon_sid)
            || self.runner_key_id.is_empty()
            || self.runner_key_id.len() > 128
            || !self.runner_key_id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-')
            })
        {
            bail!("runner requires loopback broker, distinct SIDs, and a valid key ID");
        }
        if !self.runner_public_key_file.is_absolute()
            || self
                .legacy_key_file
                .as_ref()
                .is_some_and(|legacy| same_config_path(legacy, &self.runner_public_key_file))
        {
            bail!("distinct absolute Runner public-key path required");
        }
        Ok(())
    }
    fn public_key(&self) -> Result<Vec<u8>> {
        let path = super::windows_security::safe_existing_path(&self.runner_public_key_file)?;
        if std::fs::metadata(&path)?.len() > 256 {
            bail!("Runner public key file too large");
        }
        let encoded = std::fs::read_to_string(path)?.trim().to_string();
        let key = URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .context("Runner public key encoding invalid")?;
        if key.len() != 32 || URL_SAFE_NO_PAD.encode(&key) != encoded {
            bail!("Runner public key must be canonical base64url Ed25519 bytes");
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
}

#[derive(Clone)]
struct RunnerAuth {
    store: Arc<Store>,
    config: RunnerConfig,
    public_key: Arc<Vec<u8>>,
    health: RuntimeHealth,
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

fn single_header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a str, StatusCode> {
    let mut values = headers.get_all(name).iter();
    let value = values.next().ok_or(StatusCode::UNAUTHORIZED)?;
    if values.next().is_some() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    value.to_str().map_err(|_| StatusCode::UNAUTHORIZED)
}

fn body_digest(body: &[u8]) -> String {
    digest(&SHA256, body)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn canonical_request(
    key_id: &str,
    runner_sid: &str,
    method: &str,
    path: &str,
    timestamp: &str,
    nonce: &str,
    body: &[u8],
) -> String {
    format!(
        "coord-runner-request-v{REQUEST_VERSION}\n{key_id}\n{runner_sid}\n{method}\n{path}\n{timestamp}\n{nonce}\n{}",
        body_digest(body)
    )
}

async fn authenticate_runner(
    State(auth): State<RunnerAuth>,
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    if request.method() != axum::http::Method::POST || request.uri().query().is_some() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let path = request.uri().path().to_owned();
    if !matches!(path.as_str(), "/claim" | "/authorize" | "/finish") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let headers = request.headers();
    let version = single_header(headers, "x-coord-protocol")?.to_owned();
    let key_id = single_header(headers, "x-coord-key-id")?.to_owned();
    let runner_sid = single_header(headers, "x-coord-runner-sid")?.to_owned();
    let timestamp = single_header(headers, "x-coord-timestamp")?.to_owned();
    let nonce = single_header(headers, "x-coord-nonce")?.to_owned();
    let signature = single_header(headers, "x-coord-signature")?.to_owned();
    if version != REQUEST_VERSION
        || key_id != auth.config.runner_key_id
        || runner_sid != auth.config.runner_sid
        || !timestamp.is_ascii()
        || !nonce.is_ascii()
        || !signature.is_ascii()
        || !key_id.is_ascii()
        || !runner_sid.is_ascii()
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let claimed_runner = version == REQUEST_VERSION
        && key_id == auth.config.runner_key_id
        && runner_sid == auth.config.runner_sid;
    let timestamp_ms = timestamp
        .parse::<i64>()
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    if timestamp_ms.to_string() != timestamp
        || timestamp_ms.abs_diff(Utc::now().timestamp_millis()) > CLOCK_SKEW_MS as u64
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let nonce_bytes = URL_SAFE_NO_PAD
        .decode(nonce.as_bytes())
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    if nonce_bytes.len() != NONCE_BYTES || URL_SAFE_NO_PAD.encode(&nonce_bytes) != nonce {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let signature_bytes = match URL_SAFE_NO_PAD.decode(signature.as_bytes()) {
        Ok(bytes) => bytes,
        Err(_) => {
            if claimed_runner {
                auth.health.runner_security_error("signed_request_rejected");
            }
            return Err(StatusCode::UNAUTHORIZED);
        }
    };
    if signature_bytes.len() != 64 || URL_SAFE_NO_PAD.encode(&signature_bytes) != signature {
        if claimed_runner {
            auth.health.runner_security_error("signed_request_rejected");
        }
        return Err(StatusCode::UNAUTHORIZED);
    }
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, MAX_SIGNED_BODY_BYTES)
        .await
        .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
    let canonical = canonical_request(
        &key_id,
        &runner_sid,
        parts.method.as_str(),
        &path,
        &timestamp,
        &nonce,
        &body,
    );
    if UnparsedPublicKey::new(&ED25519, auth.public_key.as_slice())
        .verify(canonical.as_bytes(), &signature_bytes)
        .is_err()
    {
        if claimed_runner {
            auth.health.runner_security_error("signed_request_rejected");
        }
        return Err(StatusCode::UNAUTHORIZED);
    }
    let expires_at_ms = nonce_expiry_unix_ms(timestamp_ms);
    let accepted = auth
        .store
        .consume_runner_nonce(
            &key_id,
            &nonce,
            Utc::now().timestamp_millis(),
            expires_at_ms,
        )
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if !accepted {
        return Err(StatusCode::UNAUTHORIZED);
    }
    auth.health.runner_seen();
    Ok(next.run(Request::from_parts(parts, Body::from(body))).await)
}
type ApiError = StatusCode;
async fn claim(
    State(state): State<Broker>,
) -> std::result::Result<Json<Option<ActionJob>>, ApiError> {
    state
        .store
        .begin_action(&state.config.owner())
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
async fn authorize(
    State(state): State<Broker>,
    Json(fence): Json<Fence>,
) -> std::result::Result<Json<Option<ActionJob>>, ApiError> {
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
    Json(report): Json<Finish>,
) -> std::result::Result<Json<bool>, ApiError> {
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

pub fn broker_router(
    store: Arc<Store>,
    config: RunnerConfig,
    health: RuntimeHealth,
) -> Result<Router> {
    config.validate()?;
    let public_key = Arc::new(config.public_key()?);
    let auth = RunnerAuth {
        store: store.clone(),
        config: config.clone(),
        public_key,
        health: health.clone(),
    };
    Ok(Router::new()
        .route("/claim", post(claim))
        .route("/authorize", post(authorize))
        .route("/finish", post(finish))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(Broker { store, config })
        .layer(middleware::from_fn_with_state(auth, authenticate_runner)))
}

pub fn management_key_from_path(path: &Path) -> Result<String> {
    let path = super::windows_security::safe_existing_path(path)?;
    if std::fs::metadata(&path)?.len() > 128 {
        bail!("management key file too large");
    }
    let key = std::fs::read_to_string(path)?.trim().to_string();
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("32-byte RPC management key required");
    }
    Ok(key)
}

fn bearer_authenticated(headers: &HeaderMap, key: &str) -> bool {
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
        if request.method() != axum::http::Method::GET
            && !bearer_authenticated(request.headers(), &key)
        {
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
pub async fn serve_broker(
    store: Arc<Store>,
    config: RunnerConfig,
    health: RuntimeHealth,
) -> Result<()> {
    if super::windows_security::process_identity()?.sid != config.daemon_sid {
        bail!("broker daemon SID differs from administrator configuration");
    }
    let address = config.broker_addr;
    let router = broker_router(store, config, health)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    // Binding proves the broker listener exists. Runner readiness is reported
    // only after a successfully authenticated signed request.
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
        if http
            .post(&url)
            .bearer_auth("non-management-runner-credential")
            .json(&body)
            .send()?
            .status()
            != 401
        {
            bail!("non-management credential accessed management RPC");
        }
        eprintln!("management RPC rejected missing and Runner credentials: 401/401");
    }
    eprintln!(
        "{}",
        serde_json::json!({"acl_probe":"PASS","reads":probe.read_files.len(),"denied_reads":probe.denied_read_files.len(),"denied_writes":probe.denied_write_files.len(),"writable_directories":probe.write_directories.len(),"denied_directories":probe.denied_write_directories.len()})
    );
    Ok(())
}

struct RunnerSigner {
    config: RunnerConfig,
    key_pair: Ed25519KeyPair,
}

impl RunnerSigner {
    fn load(config: RunnerConfig, private_key_file: &Path) -> Result<Self> {
        config.validate()?;
        if same_config_path(private_key_file, &config.runner_public_key_file)
            || config
                .legacy_key_file
                .as_ref()
                .is_some_and(|legacy| same_config_path(private_key_file, legacy))
        {
            bail!("Runner private-key path overlaps a public or legacy Broker key path");
        }
        let path = super::windows_security::safe_existing_path(private_key_file)?;
        if std::fs::metadata(&path)?.len() > 1024 {
            bail!("Runner private key file too large");
        }
        let encoded = std::fs::read_to_string(path)?.trim().to_string();
        let private_key = URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .context("Runner private key encoding invalid")?;
        if URL_SAFE_NO_PAD.encode(&private_key) != encoded {
            bail!("Runner private key encoding is not canonical");
        }
        let key_pair = Ed25519KeyPair::from_pkcs8(&private_key)
            .map_err(|_| anyhow::anyhow!("Runner private key is not valid Ed25519 PKCS#8"))?;
        if key_pair.public_key().as_ref() != config.public_key()?.as_slice() {
            bail!("Runner private key does not match configured key ID");
        }
        Ok(Self { config, key_pair })
    }

    fn headers(&self, path: &str, body: &[u8]) -> Result<HeaderMap> {
        let mut nonce_bytes = [0u8; NONCE_BYTES];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| anyhow::anyhow!("cryptographic nonce generation failed"))?;
        let nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
        let timestamp = Utc::now().timestamp_millis().to_string();
        let canonical = canonical_request(
            &self.config.runner_key_id,
            &self.config.runner_sid,
            "POST",
            path,
            &timestamp,
            &nonce,
            body,
        );
        let signature = URL_SAFE_NO_PAD.encode(self.key_pair.sign(canonical.as_bytes()).as_ref());
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("x-coord-protocol", REQUEST_VERSION),
            ("x-coord-key-id", self.config.runner_key_id.as_str()),
            ("x-coord-runner-sid", self.config.runner_sid.as_str()),
            ("x-coord-timestamp", timestamp.as_str()),
            ("x-coord-nonce", nonce.as_str()),
            ("x-coord-signature", signature.as_str()),
        ] {
            headers.insert(name, HeaderValue::from_str(value)?);
        }
        Ok(headers)
    }

    fn post_json<T: DeserializeOwned, B: Serialize>(
        &self,
        http: &reqwest::blocking::Client,
        base: &str,
        path: &str,
        payload: &B,
    ) -> Result<T> {
        let body = serde_json::to_vec(payload)?;
        let headers = self.headers(path, &body)?;
        Ok(http
            .post(format!("{base}{path}"))
            .headers(headers)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()?
            .error_for_status()?
            .json()?)
    }
}

/// Create a fresh Ed25519 keypair without printing either key. Callers must
/// protect the private-key parent directory's ACL before invoking this helper.
pub fn generate_keypair(private_key_file: &Path, public_key_file: &Path) -> Result<()> {
    if private_key_file == public_key_file
        || !private_key_file.is_absolute()
        || !public_key_file.is_absolute()
    {
        bail!("distinct absolute Runner key paths required");
    }
    let private_parent = private_key_file
        .parent()
        .context("Runner private-key parent directory required")?;
    let public_parent = public_key_file
        .parent()
        .context("Runner public-key parent directory required")?;
    if !super::windows_security::safe_existing_path(private_parent)?.is_dir()
        || !super::windows_security::safe_existing_path(public_parent)?.is_dir()
    {
        bail!("Runner key parent directories must already exist and be trusted");
    }
    if private_key_file.exists() || public_key_file.exists() {
        bail!("refusing to overwrite an existing Runner key file");
    }
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .map_err(|_| anyhow::anyhow!("Ed25519 key generation failed"))?;
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|_| anyhow::anyhow!("generated Ed25519 key could not be loaded"))?;
    let private_encoded = URL_SAFE_NO_PAD.encode(pkcs8.as_ref());
    let public_encoded = URL_SAFE_NO_PAD.encode(key_pair.public_key().as_ref());
    let write_new = |path: &Path, bytes: &[u8]| -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("create Runner key file {}", path.display()))?;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    };
    write_new(private_key_file, private_encoded.as_bytes())?;
    if let Err(error) = write_new(public_key_file, public_encoded.as_bytes()) {
        // Remove only the private file created by this call if public output
        // could not be committed. Never touch a path that pre-existed.
        let _ = std::fs::remove_file(private_key_file);
        return Err(error);
    }
    Ok(())
}

pub fn run(
    registry: ActionRegistry,
    private_key_file: Option<&Path>,
    once: bool,
    check_identity: bool,
) -> Result<()> {
    let config = registry
        .runner_config()?
        .context("external_runner registry configuration required")?;
    let identity =
        super::windows_security::require_runner_identity(&config.runner_sid, &config.daemon_sid)?;
    // The service stderr log is part of the deployment identity evidence. It
    // contains only the account SID and privilege boolean, never credentials.
    eprintln!("{}", serde_json::to_string(&identity)?);
    if check_identity {
        if let Some(private_key_file) = private_key_file {
            // A service identity probe that is given the configured private
            // path also proves it can load the key matching the Core's public
            // key. The key material is never printed.
            RunnerSigner::load(config, private_key_file)?;
        }
        return Ok(());
    }
    let private_key_file = private_key_file.context("--private-key-file is required")?;
    let signer = RunnerSigner::load(config.clone(), private_key_file)?;
    let base = format!("http://{}", config.broker_addr);
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    loop {
        let job: Option<ActionJob> =
            signer.post_json(&http, &base, "/claim", &serde_json::json!({}))?;
        if let Some(job) = job {
            let fence = Fence {
                id: job.id,
                lease_token: job.lease_token,
            };
            // Ask immediately before spawn; the broker reconstructs the trusted
            // event and checks current receipt/attempt/fence with >=305s left.
            let authorized: Option<ActionJob> =
                signer.post_json(&http, &base, "/authorize", &fence)?;
            if let Some(authorized) = authorized {
                let outcome = registry.execute(&authorized);
                let report = Finish {
                    fence,
                    state: outcome.state,
                    code: outcome.code.into(),
                    exit_code: outcome.exit_code,
                    output_bytes: outcome.output_bytes,
                };
                let accepted: bool = signer.post_json(&http, &base, "/finish", &report)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_expiry_outlives_the_inclusive_clock_skew_window() {
        let now_ms = Utc::now().timestamp_millis();
        let oldest_accepted_timestamp = now_ms - CLOCK_SKEW_MS;
        assert!(nonce_expiry_unix_ms(oldest_accepted_timestamp) > now_ms);
        assert_eq!(nonce_expiry_unix_ms(now_ms), now_ms + CLOCK_SKEW_MS + 1);
    }

    fn fixture_root() -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("coord-runner-signer-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("private")).unwrap();
        std::fs::create_dir_all(root.join("public")).unwrap();
        root
    }

    fn config(public_key_file: PathBuf) -> RunnerConfig {
        RunnerConfig {
            broker_addr: "127.0.0.1:17779".parse().unwrap(),
            runner_sid: "S-1-5-21-111-222-333-9999".into(),
            daemon_sid: "S-1-5-19".into(),
            legacy_management_key_path: None,
            runner_key_id: "unit-test-runner".into(),
            runner_public_key_file: public_key_file,
            legacy_key_file: None,
        }
    }

    #[test]
    fn runner_signer_fails_closed_on_missing_mismatched_and_overlapping_private_keys() {
        let root = fixture_root();
        let private = root.join("private/runner.pk8");
        let public = root.join("public/runner.pub");
        generate_keypair(&private, &public).unwrap();
        let valid = config(public.clone());
        assert!(RunnerSigner::load(valid.clone(), &private).is_ok());

        let missing = root.join("private/missing.pk8");
        assert!(RunnerSigner::load(valid.clone(), &missing).is_err());

        let other_private = root.join("private/other.pk8");
        let other_public = root.join("public/other.pub");
        generate_keypair(&other_private, &other_public).unwrap();
        assert!(RunnerSigner::load(valid.clone(), &other_private).is_err());

        let mut legacy_alias = valid;
        legacy_alias.legacy_key_file = Some(other_private.clone());
        assert!(RunnerSigner::load(legacy_alias, &other_private).is_err());

        let _ = std::fs::remove_dir_all(root);
    }
}
