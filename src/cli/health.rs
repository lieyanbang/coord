use anyhow::{anyhow, Context, Result};
use chrono::{SecondsFormat, Utc};
use clap::Args;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::Client;

#[derive(Args)]
pub struct HealthArgs {
    /// Production SQLite database path. Opened read-only; it is never created.
    #[arg(long)]
    db: PathBuf,
    /// Status JSON path, overwritten on every probe.
    #[arg(long)]
    status: PathBuf,
    /// Timeout for each readiness check in milliseconds.
    #[arg(long, default_value_t = 2000)]
    timeout_ms: u64,
    /// Mark successful but slower probes DEGRADED above this latency.
    #[arg(long, default_value_t = 2000)]
    degraded_after_ms: u64,
}

#[derive(Debug, Serialize)]
struct Snapshot {
    status: &'static str,
    checked_at_utc: String,
    checked_at_unix_ms: i64,
    last_success_at_utc: Option<String>,
    consecutive_failures: u32,
    latency_ms: u64,
    tasks_list_latency_ms: u64,
    sqlite_latency_ms: u64,
    checks: Checks,
    failed_checks: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct Checks {
    tasks_list: &'static str,
    sqlite_read_only: &'static str,
}

#[derive(Deserialize)]
struct PreviousSnapshot {
    #[serde(default)]
    last_success_at_utc: Option<String>,
    #[serde(default)]
    consecutive_failures: u32,
}

pub fn run(url: &str, args: &HealthArgs) -> Result<()> {
    let timeout = Duration::from_millis(args.timeout_ms.max(1));
    let started = Instant::now();

    let tasks_started = Instant::now();
    let tasks_ok = check_tasks_list(url, timeout).is_ok();
    let tasks_list_latency_ms = elapsed_ms(tasks_started.elapsed());

    let sqlite_started = Instant::now();
    let sqlite_ok = check_database(&args.db, timeout).is_ok();
    let sqlite_latency_ms = elapsed_ms(sqlite_started.elapsed());

    let latency_ms = elapsed_ms(started.elapsed());
    let checked_at = Utc::now();
    let checked_at_utc = checked_at.to_rfc3339_opts(SecondsFormat::Millis, true);
    let prior = read_previous(&args.status);
    let hard_failure = !tasks_ok || !sqlite_ok;
    let slow = latency_ms > args.degraded_after_ms;
    let status = if hard_failure {
        "FAIL"
    } else if slow {
        "DEGRADED"
    } else {
        "READY"
    };

    let mut failed_checks = Vec::new();
    if !tasks_ok {
        failed_checks.push("tasks_list");
    }
    if !sqlite_ok {
        failed_checks.push("sqlite_read_only");
    }
    if slow {
        failed_checks.push("latency_threshold");
    }

    let consecutive_failures = if hard_failure {
        prior.as_ref().map_or(1, |snapshot| {
            snapshot.consecutive_failures.saturating_add(1)
        })
    } else {
        0
    };
    let last_success_at_utc = if status == "READY" {
        Some(checked_at_utc.clone())
    } else {
        prior.and_then(|snapshot| snapshot.last_success_at_utc)
    };

    let snapshot = Snapshot {
        status,
        checked_at_utc,
        checked_at_unix_ms: checked_at.timestamp_millis(),
        last_success_at_utc,
        consecutive_failures,
        latency_ms,
        tasks_list_latency_ms,
        sqlite_latency_ms,
        checks: Checks {
            tasks_list: if tasks_ok { "PASS" } else { "FAIL" },
            sqlite_read_only: if sqlite_ok { "PASS" } else { "FAIL" },
        },
        failed_checks,
    };
    write_snapshot(&args.status, &snapshot)?;
    println!("{}", serde_json::to_string(&snapshot)?);

    if hard_failure {
        Err(anyhow!("health probe failed"))
    } else {
        Ok(())
    }
}

fn check_tasks_list(url: &str, timeout: Duration) -> Result<()> {
    let client = Client::with_timeout(url.to_owned(), timeout)?;
    let result = client.call("tasks/list", json!({ "limit": 1, "skip_reclaim": true }))?;
    if !result.is_array() {
        return Err(anyhow!("tasks/list did not return an array"));
    }
    Ok(())
}

fn check_database(path: &Path, timeout: Duration) -> Result<()> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("open SQLite read-only")?;
    connection.busy_timeout(timeout)?;
    connection.pragma_update(None, "query_only", "ON")?;

    let one: i64 = connection.query_row("SELECT 1", [], |row| row.get(0))?;
    let has_tasks_table: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='tasks')",
        [],
        |row| row.get(0),
    )?;
    if one != 1 || !has_tasks_table {
        return Err(anyhow!("SQLite is unavailable or schema is incomplete"));
    }
    Ok(())
}

fn read_previous(path: &Path) -> Option<PreviousSnapshot> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_snapshot(path: &Path, snapshot: &Snapshot) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create health directory")?;
    }
    let mut temp = path.to_path_buf();
    let extension = format!("{}.tmp", std::process::id());
    temp.set_extension(extension);
    let mut bytes = serde_json::to_vec_pretty(snapshot)?;
    bytes.push(b'\n');
    fs::write(&temp, bytes).context("write temporary health status")?;
    if let Err(error) = fs::rename(&temp, path) {
        if error.kind() == std::io::ErrorKind::AlreadyExists && path.exists() {
            fs::remove_file(path).context("replace prior health status")?;
            fs::rename(&temp, path).context("publish health status")?;
        } else {
            return Err(error).context("publish health status");
        }
    }
    Ok(())
}

fn elapsed_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}
