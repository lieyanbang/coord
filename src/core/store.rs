//! SQLite-backed state. All mutations go through atomic transactions so
//! that task claims are race-free across many connected agents.
//!
//! Optionally writes a human-readable markdown audit trail to a vault
//! directory on every state change. Drop the vault into Obsidian to get
//! a graph view of the project's nervous system for free.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

use super::types::{
    Agent, ProgressLeaseExtension, ProgressReport, Task, TaskState, DEFAULT_KIND,
    DEFAULT_LEASE_SECONDS, DEFAULT_PRIORITY, MAX_LEASE_SECONDS,
};
use crate::vault::Vault;

/// SQL-side filter applied to `list_tasks_filtered`. Each `Some` field
/// becomes a `WHERE` clause so the database returns only matching rows
/// rather than the most-recent N rows that we then re-filter in
/// memory. The latter silently drops matches when the bulletin is
/// busy — see the 0.4 SQL-pushdown work for the regression test.
#[derive(Default, Debug, Clone)]
pub struct TaskFilter {
    pub state: Option<String>,
    pub kind: Option<String>,
    pub priority: Option<String>,
}

/// Caller-supplied ack to write atomically alongside a `tasks/complete`.
/// `priority` defaults to `"high"` if unset (matches the example in
/// `AGENTS.md`). `payload` is merged with a daemon-injected
/// `fixed_bug_id` pointing at the source task so the markdown vault
/// renders the wikilink without the caller having to know the UUID.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AckSpec {
    pub name: String,
    pub priority: Option<String>,
    pub payload: Option<serde_json::Value>,
}

/// Result of `complete_task`. `Completed { ack }` means the source
/// task transitioned to `completed`; `ack` is `Some` iff the caller
/// requested one and it was written in the same transaction.
///
/// The completed variant is boxed (well, the `Task` payload is
/// optional and large) so the enum stays cheap to pass around even
/// when no ack was requested — clippy yells otherwise.
#[derive(Debug)]
pub enum CompleteOutcome {
    Completed { ack: Box<Option<Task>> },
    NotInClaimedState,
    Submitted { submission_id: String },
}

pub struct Store {
    conn: Mutex<Connection>,
    vault: Option<Arc<Vault>>,
    actions: Option<Arc<super::actions::ActionRegistry>>,
}

const MAX_AGENT_LEASE_SECONDS: u64 = 600;
const PROGRESS_FRESH_SECONDS: i64 = 600;

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_vault(path, None)
    }

    pub fn open_with_vault(path: &Path, vault_dir: Option<PathBuf>) -> Result<Self> {
        Self::open_with_vault_and_actions(path, vault_dir, None)
    }

    pub fn open_with_actions(
        path: &Path,
        actions: Arc<super::actions::ActionRegistry>,
    ) -> Result<Self> {
        Self::open_with_vault_and_actions(path, None, Some(actions))
    }

    pub fn open_with_vault_and_actions(
        path: &Path,
        vault_dir: Option<PathBuf>,
        actions: Option<Arc<super::actions::ActionRegistry>>,
    ) -> Result<Self> {
        let mut conn = Connection::open(path).context("open sqlite")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // WAL keeps short writes from blocking concurrent readers, which
        // matters when N agents poll for tasks in tight loops.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        // Forward-compatible additive migrations. SQLite errors with
        // "duplicate column name" if these have already run; we swallow
        // those specifically and let real errors surface.
        try_add_column(&tx, "tasks", "kind", "TEXT NOT NULL DEFAULT 'task'")?;
        try_add_column(&tx, "tasks", "priority", "TEXT NOT NULL DEFAULT 'normal'")?;
        // Lease columns are nullable: unclaimed tasks have no lease, and
        // legacy rows from before the migration keep NULLs (the
        // reclaim sweep treats NULL as "no lease, don't touch").
        try_add_column(&tx, "tasks", "claimed_at", "TEXT")?;
        try_add_column(&tx, "tasks", "lease_until", "TEXT")?;
        // `first_seen` defaults to epoch on existing rows; the next heartbeat
        // for that agent will not overwrite it (see UPSERT in `heartbeat`).
        try_add_column(
            &tx,
            "agents",
            "first_seen",
            "TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z'",
        )?;
        for (column, definition) in [
            ("lease_token", "TEXT"),
            ("attempt_no", "INTEGER NOT NULL DEFAULT 0"),
            ("attempt_count", "INTEGER NOT NULL DEFAULT 0"),
            ("max_attempts", "INTEGER NOT NULL DEFAULT 3"),
            ("available_at", "TEXT"),
            ("batch_id", "TEXT"),
            ("task_key", "TEXT"),
            ("error", "TEXT"),
            ("completed_at", "TEXT"),
            ("submission_id", "TEXT"),
            ("validation_attempts", "INTEGER NOT NULL DEFAULT 0"),
            ("validation_error_count", "INTEGER NOT NULL DEFAULT 0"),
            ("validation_token", "TEXT"),
            ("validation_until", "TEXT"),
            ("pending_ack", "TEXT"),
        ] {
            try_add_column(&tx, "tasks", column, definition)?;
        }
        tx.execute_batch("CREATE TABLE IF NOT EXISTS batches(id TEXT PRIMARY KEY,name TEXT NOT NULL,description TEXT NOT NULL,config TEXT NOT NULL,created_at TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'active');
            CREATE UNIQUE INDEX IF NOT EXISTS idx_task_key ON tasks(batch_id,task_key);
            CREATE INDEX IF NOT EXISTS idx_tasks_ready ON tasks(state,available_at,batch_id);")?;
        // Legacy claims lack a fence: invalidate them rather than allow a bypass.
        tx.execute("UPDATE tasks SET state='pending',claimed_by=NULL,claimed_at=NULL,lease_until=NULL WHERE state='claimed' AND lease_token IS NULL", [])?;
        tx.execute_batch("CREATE TABLE IF NOT EXISTS task_receipts(task_id TEXT NOT NULL,lease_token TEXT NOT NULL,operation TEXT NOT NULL,agent_id TEXT NOT NULL,request TEXT NOT NULL,response TEXT NOT NULL,PRIMARY KEY(task_id,lease_token,operation));")?;
        // Index just `lease_until` so the periodic reclaim sweep is a
        // cheap range scan even when the bulletin has thousands of
        // completed/cancelled rows that don't carry a lease.
        tx.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_tasks_kind_priority ON tasks(kind, priority);",
        )?;
        tx.execute(
            "CREATE INDEX IF NOT EXISTS idx_tasks_lease_until ON tasks(lease_until)",
            [],
        )?;
        tx.commit()?;
        validate_action_registry(&conn, actions.as_deref())?;

        let vault = vault_dir
            .map(|p| -> Result<Arc<Vault>> { Ok(Arc::new(Vault::open(p)?)) })
            .transpose()?;
        Ok(Self {
            conn: Mutex::new(conn),
            vault,
            actions,
        })
    }

    pub fn action_registry(&self) -> Option<Arc<super::actions::ActionRegistry>> {
        self.actions.clone()
    }

    pub fn create_task(&self, name: &str, payload: serde_json::Value) -> Result<Task> {
        self.create_task_full(name, DEFAULT_KIND, DEFAULT_PRIORITY, payload)
    }

    pub fn create_task_full(
        &self,
        name: &str,
        kind: &str,
        priority: &str,
        payload: serde_json::Value,
    ) -> Result<Task> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let payload_s = serde_json::to_string(&payload)?;
        // Announcement-kind tasks (ack / knowledge / decision) are
        // publications, not work to be claimed — they're "done" the
        // instant they're posted. Other kinds (bug / feature / task)
        // need someone to pick them up, so they start pending.
        let initial_state = default_state_for_kind(kind);
        {
            let conn = self.conn.lock();
            conn.execute(
                "INSERT INTO tasks (id, name, kind, priority, state, payload, created_at, updated_at, completed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, CASE WHEN ?5='completed' THEN ?7 ELSE NULL END)",
                params![
                    id.to_string(),
                    name,
                    kind,
                    priority,
                    initial_state.as_str(),
                    payload_s,
                    now.to_rfc3339(),
                ],
            )?;
        }
        let task = Task {
            id,
            name: name.into(),
            kind: kind.into(),
            priority: priority.into(),
            state: initial_state,
            claimed_by: None,
            payload,
            result: None,
            created_at: now,
            updated_at: now,
            claimed_at: None,
            lease_until: None,
            lease_token: None,
            attempt_no: 0,
            attempt_count: 0,
            max_attempts: 3,
            submission_id: None,
            validation_attempts: 0,
            validation_error_count: 0,
            available_at: None,
            batch_id: None,
            task_key: None,
            error: None,
            completed_at: if initial_state == TaskState::Completed {
                Some(now)
            } else {
                None
            },
        };
        self.vault_write(&task);
        Ok(task)
    }

    pub fn get_task(&self, id: Uuid) -> Result<Option<Task>> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT id, name, kind, priority, state, claimed_by, payload, result, \
                 created_at, updated_at, claimed_at, lease_until, lease_token, attempt_no, attempt_count, max_attempts, available_at, batch_id, task_key, error, completed_at, submission_id, validation_attempts, validation_error_count \
                 FROM tasks WHERE id = ?1",
                params![id.to_string()],
                row_to_task,
            )
            .optional()?;
        Ok(row)
    }

    pub fn list_tasks(&self, limit: usize) -> Result<Vec<Task>> {
        self.list_tasks_filtered(limit, &TaskFilter::default())
    }

    /// Filtered list. Filters are applied in SQL so callers receive
    /// matching rows up to `limit`, not just the N most-recent rows
    /// re-filtered in memory (which silently drops matches when the
    /// bulletin is busy).
    pub fn list_tasks_filtered(&self, limit: usize, filter: &TaskFilter) -> Result<Vec<Task>> {
        let conn = self.conn.lock();
        let mut sql = String::from(
            "SELECT id, name, kind, priority, state, claimed_by, payload, result, \
             created_at, updated_at, claimed_at, lease_until, lease_token, attempt_no, attempt_count, max_attempts, available_at, batch_id, task_key, error, completed_at, submission_id, validation_attempts, validation_error_count \
             FROM tasks",
        );
        let mut clauses: Vec<&'static str> = Vec::new();
        let mut bind: Vec<String> = Vec::new();
        if let Some(s) = &filter.state {
            clauses.push("state = ?");
            bind.push(s.clone());
        }
        if let Some(k) = &filter.kind {
            clauses.push("kind = ?");
            bind.push(k.clone());
        }
        if let Some(p) = &filter.priority {
            clauses.push("priority = ?");
            bind.push(p.clone());
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC LIMIT ?");
        let mut stmt = conn.prepare(&sql)?;
        let mut bind_refs: Vec<&dyn rusqlite::ToSql> =
            bind.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let limit_i = limit as i64;
        bind_refs.push(&limit_i);
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bind_refs), row_to_task)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Atomically transition a `pending` task to `claimed` for `agent_id`,
    /// granting a fresh lease of `lease_seconds` (clamped to
    /// `MAX_LEASE_SECONDS`). Returns the claimed task on success, or
    /// `None` if it was already claimed.
    pub fn claim_task(
        &self,
        id: Uuid,
        agent_id: &str,
        lease_seconds: Option<u64>,
    ) -> Result<Option<Task>> {
        let requested_lease = lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS);
        let task_opt = {
            let mut connection = self.conn.lock();
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let batch: Option<String> = tx
                .query_row(
                    "SELECT batch_id FROM tasks WHERE id=?1",
                    params![id.to_string()],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            let batch_cfg = batch
                .map(|value| Uuid::parse_str(&value).map_err(anyhow::Error::from))
                .transpose()?
                .map(|batch_id| batch_config(&tx, batch_id, false))
                .transpose()?;
            let lease = if batch_cfg
                .as_ref()
                .is_some_and(|config| config.require_progress)
            {
                requested_lease.clamp(1, MAX_AGENT_LEASE_SECONDS)
            } else {
                clamp_lease(requested_lease)
            };
            let now = Utc::now();
            let until = now + Duration::seconds(lease as i64);
            let token = Uuid::new_v4().to_string();
            let updated = tx.execute(
                "UPDATE tasks SET state='claimed',claimed_by=?1,claimed_at=?2,lease_until=?3,updated_at=?2,lease_token=?5,attempt_no=attempt_no+1,attempt_count=attempt_count+1
                 WHERE id=?4 AND state='pending' AND attempt_count<max_attempts AND (available_at IS NULL OR julianday(available_at)<=julianday('now'))",
                params![agent_id,now.to_rfc3339(),until.to_rfc3339(),id.to_string(),token])?;
            let task = if updated == 0 {
                None
            } else {
                Some(tx.query_row(
                "SELECT id,name,kind,priority,state,claimed_by,payload,result,created_at,updated_at,claimed_at,lease_until,lease_token,attempt_no,attempt_count,max_attempts,available_at,batch_id,task_key,error,completed_at,submission_id,validation_attempts,validation_error_count FROM tasks WHERE id=?1",
                params![id.to_string()], row_to_task)?)
            };
            tx.commit()?;
            task
        };
        if let Some(t) = &task_opt {
            self.vault_write(t);
            self.vault_agent_event(agent_id, &format!("claimed [[{}]]", task_slug(t)));
        }
        Ok(task_opt)
    }

    /// Push the lease forward on a currently-claimed task. Idempotent
    /// and race-safe: only the current claimer can extend, and only
    /// while the task is still `claimed`. Returns the new
    /// `lease_until` on success, or `None` if the agent no longer
    /// owns the claim (cancelled, completed, reclaimed, or another
    /// agent took over after a reclaim).
    pub fn extend_lease(
        &self,
        id: Uuid,
        agent_id: &str,
        lease_token: &str,
        lease_seconds: Option<u64>,
    ) -> Result<Option<DateTime<Utc>>> {
        let conn = self.conn.lock();
        let batch: Option<String> = conn
            .query_row(
                "SELECT batch_id FROM tasks WHERE id=?1",
                params![id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        if let Some(batch) = batch {
            let batch_id = Uuid::parse_str(&batch)?;
            if batch_config(&conn, batch_id, false)?.require_progress {
                return Ok(None);
            }
        }
        let now = Utc::now();
        let until = now
            + Duration::seconds(clamp_lease(lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS)) as i64);
        let value: Option<String> = conn.query_row(
            "UPDATE tasks SET lease_until=CASE WHEN julianday(lease_until)>julianday(?1) THEN lease_until ELSE ?1 END,updated_at=?2
             WHERE id=?3 AND state='claimed' AND claimed_by=?4 AND lease_token=?5 AND julianday(lease_until)>julianday('now') RETURNING lease_until",
            params![until.to_rfc3339(),now.to_rfc3339(),id.to_string(),agent_id,lease_token], |r| r.get(0)).optional()?;
        drop(conn);
        if let Some(t) = self.get_task(id)? {
            self.vault_write(&t);
        }
        value
            .map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(Into::into)
            })
            .transpose()
    }

    pub fn report_progress(&self, report: ProgressReport<'_>) -> Result<Option<DateTime<Utc>>> {
        let ProgressReport {
            id,
            agent_id,
            lease_token,
            attempt_no,
            sequence,
            request_id,
            checkpoint,
        } = report;
        if sequence == 0 || checkpoint.trim().is_empty() || checkpoint.len() > 1024 {
            return Ok(None);
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some((owner, prior_attempt, prior_sequence, prior_checkpoint, at)) = tx
            .query_row(
                "SELECT r.agent_id,p.attempt_no,r.sequence,r.checkpoint,r.progress_at FROM task_progress_receipts r JOIN task_progress p ON p.task_id=r.task_id AND p.lease_token=r.lease_token WHERE r.task_id=?1 AND r.lease_token=?2 AND r.request_id=?3",
                params![id.to_string(), lease_token, request_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?, row.get::<_, u64>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)),
            )
            .optional()?
        {
            if owner != agent_id || prior_attempt != attempt_no || prior_sequence != sequence || prior_checkpoint != checkpoint {
                return Ok(None);
            }
            return Ok(Some(DateTime::parse_from_rfc3339(&at)?.with_timezone(&Utc)));
        }
        let current: Option<i64> = tx
            .query_row(
                "SELECT attempt_no FROM tasks WHERE id=?1 AND state='claimed' AND claimed_by=?2 AND lease_token=?3 AND attempt_no=?4 AND julianday(lease_until)>julianday('now')",
                params![id.to_string(), agent_id, lease_token, attempt_no],
                |row| row.get(0),
            )
            .optional()?;
        if current.is_none() {
            return Ok(None);
        }
        let previous: Option<u64> = tx
            .query_row(
                "SELECT sequence FROM task_progress WHERE task_id=?1 AND lease_token=?2",
                params![id.to_string(), lease_token],
                |row| row.get(0),
            )
            .optional()?;
        if sequence != previous.unwrap_or(0).saturating_add(1) {
            return Ok(None);
        }
        let now = Utc::now();
        let at = now.to_rfc3339();
        tx.execute(
            "INSERT INTO task_progress(task_id,lease_token,agent_id,attempt_no,sequence,checkpoint,progress_at) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(task_id,lease_token) DO UPDATE SET sequence=excluded.sequence,checkpoint=excluded.checkpoint,progress_at=excluded.progress_at",
            params![id.to_string(), lease_token, agent_id, attempt_no, sequence, checkpoint, at],
        )?;
        tx.execute(
            "INSERT INTO task_progress_receipts(task_id,lease_token,request_id,agent_id,sequence,checkpoint,progress_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![id.to_string(), lease_token, request_id.to_string(), agent_id, sequence, checkpoint, at],
        )?;
        tx.commit()?;
        Ok(Some(now))
    }

    pub fn extend_lease_with_progress(
        &self,
        extension: ProgressLeaseExtension<'_>,
    ) -> Result<Option<DateTime<Utc>>> {
        let ProgressLeaseExtension {
            id,
            agent_id,
            lease_token,
            attempt_no,
            progress_sequence,
            request_id,
            lease_seconds,
        } = extension;
        if !(1..=MAX_AGENT_LEASE_SECONDS).contains(&lease_seconds) || progress_sequence == 0 {
            return Ok(None);
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if let Some((owner, prior_attempt, sequence, at)) = tx
            .query_row(
                "SELECT p.agent_id,p.attempt_no,r.progress_sequence,r.lease_until FROM task_progress_extend_receipts r JOIN task_progress p ON p.task_id=r.task_id AND p.lease_token=r.lease_token WHERE r.task_id=?1 AND r.lease_token=?2 AND r.request_id=?3",
                params![id.to_string(), lease_token, request_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?, row.get::<_, u64>(2)?, row.get::<_, String>(3)?)),
            )
            .optional()?
        {
            if owner != agent_id || prior_attempt != attempt_no || sequence != progress_sequence {
                return Ok(None);
            }
            return Ok(Some(DateTime::parse_from_rfc3339(&at)?.with_timezone(&Utc)));
        }
        let task = tx
            .query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1 AND state='claimed' AND claimed_by=?2 AND lease_token=?3 AND attempt_no=?4 AND julianday(lease_until)>julianday('now')"),
                params![id.to_string(), agent_id, lease_token, attempt_no],
                row_to_task,
            )
            .optional()?;
        let Some(task) = task else { return Ok(None) };
        let Some(batch_id) = task.batch_id else {
            return Ok(None);
        };
        let config = batch_config(&tx, batch_id, false)?;
        if !config.require_progress {
            return Ok(None);
        }
        let progress: Option<(u64, String, i64)> = tx
            .query_row(
                "SELECT sequence,progress_at,last_extended_sequence FROM task_progress WHERE task_id=?1 AND lease_token=?2 AND agent_id=?3 AND attempt_no=?4",
                params![id.to_string(), lease_token, agent_id, attempt_no],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((latest_sequence, progress_at, last_extended_sequence)) = progress else {
            return Ok(None);
        };
        let reported = DateTime::parse_from_rfc3339(&progress_at)?.with_timezone(&Utc);
        let now = Utc::now();
        if latest_sequence != progress_sequence
            || progress_sequence <= last_extended_sequence as u64
            || now.signed_duration_since(reported).num_seconds() > PROGRESS_FRESH_SECONDS
        {
            return Ok(None);
        }
        let claimed_at = task
            .claimed_at
            .context("claimed task is missing claimed_at")?;
        let budget =
            Duration::seconds(config.agent_lease_budget_seconds.min(i64::MAX as u64) as i64);
        let budget_until = claimed_at + budget;
        let requested_until = now + Duration::seconds(lease_seconds as i64);
        let capped_until = std::cmp::min(requested_until, budget_until);
        if capped_until <= now {
            return Ok(None);
        }
        let current_until = task
            .lease_until
            .context("claimed task is missing lease_until")?;
        let until = std::cmp::max(current_until, capped_until);
        let changed = tx.execute(
            "UPDATE tasks SET lease_until=?1,updated_at=?2 WHERE id=?3 AND state='claimed' AND claimed_by=?4 AND lease_token=?5 AND attempt_no=?6 AND julianday(lease_until)>julianday('now')",
            params![until.to_rfc3339(), now.to_rfc3339(), id.to_string(), agent_id, lease_token, attempt_no],
        )?;
        if changed == 0 {
            return Ok(None);
        }
        tx.execute(
            "UPDATE task_progress SET last_extended_sequence=?3 WHERE task_id=?1 AND lease_token=?2",
            params![id.to_string(), lease_token, progress_sequence],
        )?;
        tx.execute(
            "INSERT INTO task_progress_extend_receipts(task_id,lease_token,request_id,progress_sequence,lease_until) VALUES(?1,?2,?3,?4,?5)",
            params![id.to_string(), lease_token, request_id.to_string(), progress_sequence, until.to_rfc3339()],
        )?;
        tx.commit()?;
        Ok(Some(until))
    }

    pub fn agent_status(&self, agent_id: &str, batch_id: Uuid) -> Result<serde_json::Value> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT t.id,t.state,t.attempt_no,t.lease_until,p.sequence,p.checkpoint,p.progress_at FROM tasks t LEFT JOIN task_progress p ON p.task_id=t.id AND p.lease_token=t.lease_token WHERE t.batch_id=?1 AND t.state='claimed' AND t.claimed_by=?2 ORDER BY t.id",
        )?;
        let rows = stmt
            .query_map(params![batch_id.to_string(), agent_id], |row| {
                Ok(serde_json::json!({
                    "task_id": row.get::<_, String>(0)?,
                    "state": row.get::<_, String>(1)?,
                    "attempt_no": row.get::<_, u64>(2)?,
                    "lease_until": row.get::<_, Option<String>>(3)?,
                    "progress_sequence": row.get::<_, Option<u64>>(4)?,
                    "checkpoint": row.get::<_, Option<String>>(5)?,
                    "progress_at": row.get::<_, Option<String>>(6)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(
            serde_json::json!({"agent_id":agent_id,"batch_id":batch_id,"active_claims":rows.len(),"tasks":rows}),
        )
    }

    /// Sweep claimed tasks whose lease has expired back to `pending`,
    /// clearing `claimed_by` and lease fields so another agent can pick
    /// them up. Called periodically by the daemon and on every
    /// `tasks/list` to keep the bulletin self-healing. Returns the
    /// reclaimed tasks (in their post-reclaim `pending` shape) so
    /// callers can log / write vault notes.
    pub fn reclaim_expired_leases(&self) -> Result<Vec<Task>> {
        let now = Utc::now();
        let reclaimed = {
            let mut connection = self.conn.lock();
            let tx =
                connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let conn = &tx;
            // Snapshot the to-be-reclaimed rows first so we can write
            // vault notes that record who abandoned what. The actual
            // state transition is a single UPDATE for atomicity.
            let mut stmt = conn.prepare(
                "SELECT id, name, kind, priority, state, claimed_by, payload, result, \
                 created_at, updated_at, claimed_at, lease_until, lease_token, attempt_no, attempt_count, max_attempts, available_at, batch_id, task_key, error, completed_at, submission_id, validation_attempts, validation_error_count \
                 FROM tasks \
                 WHERE state = 'claimed' AND lease_until IS NOT NULL AND julianday(lease_until) <= julianday(?1)",
            )?;
            let snapshot: Vec<Task> = stmt
                .query_map(params![now.to_rfc3339()], row_to_task)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut out = Vec::with_capacity(snapshot.len());
            for before in snapshot {
                if retry_task(
                    &tx,
                    &before,
                    serde_json::json!({"kind":"lease_expired"}),
                    true,
                )? == 0
                {
                    continue;
                }
                let after = tx.query_row(
                    &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1"),
                    params![before.id.to_string()],
                    row_to_task,
                )?;
                out.push((Some(before), after));
            }
            drop(stmt);
            tx.commit()?;
            out
        };
        let mut returned = Vec::with_capacity(reclaimed.len());
        for (before, after) in reclaimed {
            if let Some(before) = before {
                if let Some(agent) = &before.claimed_by {
                    self.vault_agent_event(
                        agent,
                        &format!(
                            "abandoned [[{}]] (lease expired at {})",
                            task_slug(&before),
                            before
                                .lease_until
                                .map(|d| d.to_rfc3339())
                                .unwrap_or_else(|| "unknown".into()),
                        ),
                    );
                }
                tracing::info!(
                    task = %before.id,
                    agent = ?before.claimed_by,
                    "reclaimed expired lease"
                );
            }
            self.vault_write(&after);
            returned.push(after);
        }
        Ok(returned)
    }

    /// Mark a claimed task complete. Idempotent against races: only
    /// transitions when current state is `claimed`, so a `cancel` that
    /// sneaks in first is sticky. Clears the lease so the row no
    /// longer trips the reclaim sweep. Returns
    /// `CompleteOutcome::Completed { ack }` on success, where `ack` is
    /// `Some` if the caller asked for `post_ack` and an ack row was
    /// written in the same transaction (otherwise `None`).
    /// `CompleteOutcome::NotInClaimedState` on race.
    pub fn complete_task(
        &self,
        id: Uuid,
        agent_id: &str,
        lease_token: &str,
        result: serde_json::Value,
        ack: Option<AckSpec>,
    ) -> Result<CompleteOutcome> {
        let now = Utc::now();
        let result_s = serde_json::to_string(&result)?;
        let request = serde_json::to_string(&serde_json::json!({"result":result,"ack":ack}))?;

        // Whole flow runs under a single tx so callers can't observe a
        // half-completed state: either both the UPDATE and the ack
        // INSERT land, or neither does. This is the core integrity
        // claim of `post_ack` — agents reliably leave acks behind.
        let outcome = {
            let mut conn = self.conn.lock();
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if let Some((owner,prior,response)) = tx.query_row("SELECT agent_id,request,response FROM task_receipts WHERE task_id=?1 AND lease_token=?2 AND operation='complete'",params![id.to_string(),lease_token],|r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).optional()? {
                return if owner==agent_id && prior==request { { let value:serde_json::Value=serde_json::from_str(&response)?; if let Some(id)=value.get("submitted").and_then(|v|v.as_str()){Ok(CompleteOutcome::Submitted{submission_id:id.into()})}else{Ok(CompleteOutcome::Completed { ack: Box::new(serde_json::from_value(value)?) })} } } else { Ok(CompleteOutcome::NotInClaimedState) };
            }
            let batch: Option<String> = tx
                .query_row(
                    "SELECT batch_id FROM tasks WHERE id=?1",
                    params![id.to_string()],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten();
            let validate = if let Some(batch) = batch {
                batch_config(&tx, Uuid::parse_str(&batch)?, false)?
                    .validator
                    .is_some()
            } else {
                false
            };
            if validate {
                let submission_id = Uuid::new_v4().to_string();
                let changed=tx.execute("UPDATE tasks SET state='submitted',result=?1,submission_id=?6,lease_until=NULL,lease_token=NULL,available_at=NULL,pending_ack=?7,validation_token=NULL,validation_until=NULL,validation_attempts=0,validation_error_count=0,updated_at=?2 WHERE id=?3 AND state='claimed' AND claimed_by=?4 AND lease_token=?5 AND julianday(lease_until)>julianday('now')",params![result_s,now.to_rfc3339(),id.to_string(),agent_id,lease_token,submission_id,serde_json::to_string(&ack)?])?;
                if changed == 0 {
                    return Ok(CompleteOutcome::NotInClaimedState);
                }
                tx.execute(
                    "INSERT INTO task_receipts VALUES(?1,?2,'complete',?3,?4,?5)",
                    params![
                        id.to_string(),
                        lease_token,
                        agent_id,
                        request,
                        serde_json::json!({"submitted":submission_id}).to_string()
                    ],
                )?;
                tx.commit()?;
                drop(conn);
                if let Some(t) = self.get_task(id)? {
                    self.vault_write(&t);
                }
                return Ok(CompleteOutcome::Submitted { submission_id });
            }
            let updated = tx.execute(
                "UPDATE tasks SET state = 'completed', result = ?1, lease_until = NULL,
                                  updated_at = ?2, completed_at=?2
                 WHERE id = ?3 AND state = 'claimed' AND claimed_by=?4 AND lease_token=?5 AND julianday(lease_until)>julianday('now')",
                params![result_s, now.to_rfc3339(), id.to_string(), agent_id, lease_token],
            )?;
            if updated == 0 {
                tx.rollback()?;
                return Ok(CompleteOutcome::NotInClaimedState);
            }

            let ack_task = write_ack(&tx, id, ack, now)?;
            let completed_task = tx.query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1"),
                params![id.to_string()],
                row_to_task,
            )?;
            enqueue_task_completion_action(&tx, &completed_task, now)?;
            enqueue_batch_completion_action(&tx, completed_task.batch_id, now)?;
            tx.execute(
                "INSERT INTO task_receipts VALUES(?1,?2,'complete',?3,?4,?5)",
                params![
                    id.to_string(),
                    lease_token,
                    agent_id,
                    request,
                    serde_json::to_string(&ack_task)?
                ],
            )?;
            tx.commit()?;
            ack_task
        };

        if let Some(t) = self.get_task(id)? {
            self.vault_write(&t);
            if let Some(agent) = &t.claimed_by {
                self.vault_agent_event(agent, &format!("completed [[{}]]", task_slug(&t)));
            }
        }
        if let Some(ack) = &outcome {
            // Vault writes happen outside the tx — they're best-effort
            // and must not fail a coordination call (see vault_write).
            // The wikilink resolver in `collect_relations` reads the
            // ack's `fixed_bug_id` and pulls in the source slug.
            self.vault_write(ack);
            if let Some(agent) = self
                .get_task(id)?
                .and_then(|t| t.claimed_by)
                .as_deref()
                .map(str::to_string)
            {
                self.vault_agent_event(&agent, &format!("posted ack [[{}]]", task_slug(ack)));
            }
        }
        Ok(CompleteOutcome::Completed {
            ack: Box::new(outcome),
        })
    }

    pub fn cancel_task(&self, id: Uuid) -> Result<()> {
        let now = Utc::now();
        {
            let conn = self.conn.lock();
            conn.execute(
                "UPDATE tasks SET state = 'cancelled', lease_until = NULL, lease_token=NULL, updated_at = ?1, completed_at=?1
                 WHERE id = ?2 AND state IN ('pending','claimed','submitted')",
                params![now.to_rfc3339(), id.to_string()],
            )?;
        }
        if let Some(t) = self.get_task(id)? {
            self.vault_write(&t);
        }
        Ok(())
    }

    pub fn heartbeat(&self, agent_id: &str, name: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        {
            let conn = self.conn.lock();
            // first_seen is set on initial insert and *never* updated,
            // so the TUI can render a stable uptime per agent. last_seen
            // and name update on every beat as before.
            conn.execute(
                "INSERT INTO agents (id, name, last_seen, first_seen) VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(id) DO UPDATE SET last_seen = excluded.last_seen, name = excluded.name",
                params![agent_id, name, now],
            )?;
            // Backfill: if this agent existed before the migration ran (so
            // first_seen got set to epoch), promote it to last_seen. After
            // this one-shot upgrade it stays put.
            conn.execute(
                "UPDATE agents SET first_seen = last_seen
                 WHERE id = ?1 AND first_seen = '1970-01-01T00:00:00Z'",
                params![agent_id],
            )?;
        }
        // We deliberately do not write a vault note for every heartbeat — that
        // would explode the agent log with noise. Heartbeats only refresh
        // the last_seen field, which the next claim/complete event will pick
        // up when it rewrites the agent note.
        if let Some(v) = &self.vault {
            let _ = v.touch_agent(agent_id, name);
        }
        Ok(())
    }

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT a.id, a.name, a.last_seen, a.first_seen,
                    (SELECT t.id FROM tasks t WHERE t.claimed_by = a.id AND t.state = 'claimed' LIMIT 1)
             FROM agents a ORDER BY a.last_seen DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                let last_seen: String = row.get(2)?;
                let last_seen = chrono::DateTime::parse_from_rfc3339(&last_seen)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?
                    .with_timezone(&Utc);
                let first_seen: String = row.get(3)?;
                let first_seen = chrono::DateTime::parse_from_rfc3339(&first_seen)
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?
                    .with_timezone(&Utc);
                let current: Option<String> = row.get(4)?;
                let current_task = current.as_deref().and_then(|s| Uuid::parse_str(s).ok());
                Ok(Agent {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    first_seen,
                    last_seen,
                    current_task,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn vault_write(&self, task: &Task) {
        if let Some(v) = &self.vault {
            // Resolve any UUID-typed payload fields to their target
            // task's slug so the vault can render real wikilinks (and
            // Obsidian's graph view shows actual edges).
            let relations = self.collect_relations(task);
            // Vault writes are best-effort: a failed disk write must not
            // fail a coordination call. Log and move on.
            if let Err(e) = v.write_task(task, &relations) {
                tracing::warn!(error = %e, task = %task.id, "vault: failed to write task note");
            }
        }
    }

    /// Scan a task's payload for fields that name another task by UUID.
    /// Resolve each one to (label, target_slug) so the vault can render
    /// `## Related` wikilinks. Unknown / unresolvable UUIDs are skipped
    /// silently — vault rendering must never block coordination.
    fn collect_relations(&self, task: &Task) -> Vec<(String, String)> {
        const RELATION_KEYS: &[&str] = &[
            "fixed_bug_id",
            "related_ack",
            "related_bug",
            "bug_id",
            "ack_id",
            "blocked_by",
            "follows_up",
        ];
        let mut out = Vec::new();
        for key in RELATION_KEYS {
            if let Some(uuid_str) = task.payload.get(key).and_then(|v| v.as_str()) {
                if let Ok(uuid) = Uuid::parse_str(uuid_str) {
                    if let Ok(Some(target)) = self.get_task(uuid) {
                        out.push(((*key).to_string(), task_slug(&target)));
                    }
                }
            }
        }
        out
    }

    fn vault_agent_event(&self, agent_id: &str, event: &str) {
        if let Some(v) = &self.vault {
            if let Err(e) = v.append_agent_event(agent_id, event) {
                tracing::warn!(error = %e, agent = agent_id, "vault: failed to append agent event");
            }
        }
    }
}

pub const TASK_COLUMNS: &str = "id,name,kind,priority,state,claimed_by,payload,result,created_at,updated_at,claimed_at,lease_until,lease_token,attempt_no,attempt_count,max_attempts,available_at,batch_id,task_key,error,completed_at,submission_id,validation_attempts,validation_error_count";

fn validate_action_registry(
    conn: &Connection,
    registry: Option<&super::actions::ActionRegistry>,
) -> Result<()> {
    let mut stmt = conn.prepare("SELECT config FROM batches WHERE status='active'")?;
    let configs = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for raw in configs {
        let config: super::types::BatchConfig = serde_json::from_str(&raw)?;
        if let Some(profile_id) = config.action_profile {
            let profile_version = config
                .action_profile_version
                .context("active batch is missing its trusted action profile version")?;
            let registry = registry.context("active action profile requires --actions-config")?;
            let profile = registry
                .profile(&profile_id)
                .filter(|profile| profile.version == profile_version)
                .context("active batch action profile is not present at its pinned version")?;
            for action_kind in config.action_profile_actions {
                let configured = match action_kind.as_str() {
                    "on_task_completed" => profile.on_task_completed.is_some(),
                    "on_batch_completed" => profile.on_batch_completed.is_some(),
                    _ => false,
                };
                if !configured {
                    anyhow::bail!("active batch action is missing from its pinned profile");
                }
            }
        }
    }

    let mut stmt = conn.prepare(
        "SELECT DISTINCT profile_id,profile_version FROM completion_actions WHERE state IN ('pending','running')",
    )?;
    let pending = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (profile_id, version) in pending {
        let profile = registry
            .and_then(|registry| registry.profile(&profile_id))
            .filter(|profile| profile.version == version)
            .context("pending completion actions require their pinned profile version")?;
        if profile.on_task_completed.is_none() && profile.on_batch_completed.is_none() {
            anyhow::bail!("pending completion action profile has no actions");
        }
    }
    Ok(())
}

fn enqueue_task_completion_action(tx: &Connection, task: &Task, now: DateTime<Utc>) -> Result<()> {
    let Some(batch_id) = task.batch_id else {
        return Ok(());
    };
    let config = batch_config(tx, batch_id, false)?;
    enqueue_configured_action(
        tx,
        &config,
        CompletionActionEvent {
            action_kind: "on_task_completed",
            batch_id,
            task_id: Some(task.id),
            attempt_no: task.attempt_no,
            result: task.result.clone(),
            batch_counts: None,
        },
        now,
    )
}

fn enqueue_batch_completion_action(
    tx: &Connection,
    batch_id: Option<Uuid>,
    now: DateTime<Utc>,
) -> Result<()> {
    let Some(batch_id) = batch_id else {
        return Ok(());
    };
    let (status, total, completed): (String, u64, u64) = tx.query_row(
        "SELECT b.status,count(t.id),coalesce(sum(t.state='completed'),0)
         FROM batches b LEFT JOIN tasks t ON t.batch_id=b.id WHERE b.id=?1 GROUP BY b.id",
        params![batch_id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if status != "active" || total == 0 || total != completed {
        return Ok(());
    }
    let config = batch_config(tx, batch_id, false)?;
    enqueue_configured_action(
        tx,
        &config,
        CompletionActionEvent {
            action_kind: "on_batch_completed",
            batch_id,
            task_id: None,
            attempt_no: 0,
            result: None,
            batch_counts: Some((total, completed)),
        },
        now,
    )
}

struct CompletionActionEvent {
    action_kind: &'static str,
    batch_id: Uuid,
    task_id: Option<Uuid>,
    attempt_no: u64,
    result: Option<serde_json::Value>,
    batch_counts: Option<(u64, u64)>,
}

fn enqueue_configured_action(
    tx: &Connection,
    config: &super::types::BatchConfig,
    event: CompletionActionEvent,
    now: DateTime<Utc>,
) -> Result<()> {
    let CompletionActionEvent {
        action_kind,
        batch_id,
        task_id,
        attempt_no,
        result,
        batch_counts,
    } = event;
    let (Some(profile_id), Some(profile_version), Some(policy)) = (
        config.action_profile.as_deref(),
        config.action_profile_version,
        config.action_policy.get(action_kind),
    ) else {
        return Ok(());
    };
    let event_id = Uuid::new_v4().to_string();
    let mut payload = serde_json::json!({
        "schema_version": 1,
        "event_id": event_id,
        "batch_id": batch_id,
        "task_id": task_id,
        "action_kind": action_kind,
        "attempt_no": attempt_no,
    });
    if policy.include_result {
        if let Some(result) = result {
            payload["result"] = redact_action_secrets(result);
        }
    }
    if let Some((total, completed)) = batch_counts {
        payload["total_tasks"] = total.into();
        payload["completed_tasks"] = completed.into();
    }
    let event_key = format!(
        "{}:{}:{}:{}",
        if task_id.is_some() { "task" } else { "batch" },
        task_id.unwrap_or(batch_id),
        action_kind,
        profile_version
    );
    tx.execute(
        "INSERT INTO completion_actions(
            id,event_id,event_key,batch_id,task_id,profile_id,profile_version,
            action_kind,payload,state,idempotent,max_attempts,created_at,updated_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'pending',?10,?11,?12,?12)
         ON CONFLICT(event_key) DO NOTHING",
        params![
            Uuid::new_v4().to_string(),
            event_id,
            event_key,
            batch_id.to_string(),
            task_id.map(|id| id.to_string()),
            profile_id,
            profile_version,
            action_kind,
            serde_json::to_string(&payload)?,
            policy.idempotent,
            policy.max_attempts,
            now.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn redact_action_secrets(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .into_iter()
                .filter(|(key, _)| {
                    let normalized = key
                        .chars()
                        .filter(|character| character.is_ascii_alphanumeric())
                        .flat_map(char::to_lowercase)
                        .collect::<String>();
                    !(normalized.ends_with("token")
                        || normalized.ends_with("secret")
                        || normalized.ends_with("password")
                        || normalized.ends_with("credential")
                        || matches!(normalized.as_str(), "authorization" | "capability"))
                })
                .map(|(key, value)| (key, redact_action_secrets(value)))
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(redact_action_secrets).collect())
        }
        other => other,
    }
}

#[derive(Debug)]
pub struct SchedulerError {
    pub code: i32,
    pub message: String,
}
impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for SchedulerError {}
fn scheduler_error(code: i32, message: &str) -> anyhow::Error {
    SchedulerError {
        code,
        message: message.into(),
    }
    .into()
}

impl Store {
    pub fn create_batch(
        &self,
        name: &str,
        description: &str,
        mut config: super::types::BatchConfig,
    ) -> Result<serde_json::Value> {
        if config.max_attempts == 0
            || config.max_attempts > 100
            || config.max_backoff_ms > 3_600_000
            || config.retry_backoff_ms > config.max_backoff_ms
            || config.agent_lease_budget_seconds == 0
            || config.agent_lease_budget_seconds > 86_400
        {
            return Err(scheduler_error(-32602, "invalid retry configuration"));
        }
        if let Some(profile_id) = config.action_profile.clone() {
            let profile = self
                .actions
                .as_ref()
                .and_then(|registry| registry.profile(&profile_id))
                .ok_or_else(|| {
                    scheduler_error(-32602, "trusted action profile is not configured")
                })?;
            config.action_profile_version = Some(profile.version);
            config.action_profile_actions.clear();
            config.action_policy.clear();
            for (action_kind, command) in [
                ("on_task_completed", profile.on_task_completed.as_ref()),
                ("on_batch_completed", profile.on_batch_completed.as_ref()),
            ] {
                if let Some(command) = command {
                    config.action_profile_actions.push(action_kind.to_string());
                    config.action_policy.insert(
                        action_kind.to_string(),
                        super::types::ActionPolicy {
                            idempotent: command.idempotent,
                            max_attempts: command.max_attempts,
                            include_result: command.include_result,
                        },
                    );
                }
            }
        } else {
            config.action_profile_version = None;
            config.action_profile_actions.clear();
            config.action_policy.clear();
        }
        if let Some(v) = &config.validator {
            if v.timeout_ms == 0
                || v.timeout_ms > 10_000
                || v.delay_ms > 10_000
                || v.max_error_attempts == 0
                || v.max_error_attempts > 10
                || v.required.len() > 128
            {
                return Err(scheduler_error(
                    -32602,
                    "invalid bounded validator configuration",
                ));
            }
        }
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        self.conn.lock().execute(
            "INSERT INTO batches(id,name,description,config,created_at) VALUES(?1,?2,?3,?4,?5)",
            params![
                id.to_string(),
                name,
                description,
                serde_json::to_string(&config)?,
                now
            ],
        )?;
        Ok(
            serde_json::json!({"id":id,"name":name,"description":description,"config":config,"created_at":now,"status":"active"}),
        )
    }

    pub fn add_tasks(
        &self,
        batch_id: Uuid,
        tasks: Vec<super::types::TaskInput>,
    ) -> Result<Vec<Task>> {
        if tasks.len() > 1000 || tasks.iter().any(|t| t.key.is_empty() || t.key.len() > 256) {
            return Err(scheduler_error(
                -32602,
                "1..256 byte keys and at most 1000 tasks per request required",
            ));
        }
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let config = batch_config(&tx, batch_id, true)?;
        let now = Utc::now().to_rfc3339();
        let mut out = Vec::new();
        for input in tasks {
            let previous = tx
                .query_row(
                    &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE batch_id=?1 AND task_key=?2"),
                    params![batch_id.to_string(), input.key],
                    row_to_task,
                )
                .optional()?;
            if let Some(t) = previous {
                if t.name != input.name || t.payload != input.payload {
                    return Err(scheduler_error(
                        -32001,
                        "task key already exists with different input",
                    ));
                }
                out.push(t);
                continue;
            }
            let batch_event_exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM completion_actions
                 WHERE batch_id=?1 AND task_id IS NULL AND action_kind='on_batch_completed')",
                params![batch_id.to_string()],
                |row| row.get(0),
            )?;
            if batch_event_exists {
                return Err(scheduler_error(
                    -32001,
                    "cannot add new tasks after the batch completion action was emitted",
                ));
            }
            let id = Uuid::new_v4();
            tx.execute("INSERT INTO tasks(id,name,state,payload,created_at,updated_at,batch_id,task_key,max_attempts) VALUES(?1,?2,'pending',?3,?4,?4,?5,?6,?7)",params![id.to_string(),input.name,serde_json::to_string(&input.payload)?,now,batch_id.to_string(),input.key,config.max_attempts])?;
            out.push(tx.query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1"),
                params![id.to_string()],
                row_to_task,
            )?);
        }
        tx.commit()?;
        drop(conn);
        for t in &out {
            self.vault_write(t);
        }
        Ok(out)
    }

    pub fn claim_next(
        &self,
        agent_id: &str,
        batch_id: Option<Uuid>,
        lease_seconds: Option<u64>,
    ) -> Result<Option<Task>> {
        self.reclaim_expired_leases()?;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let config = batch_id.map(|id| batch_config(&tx, id, true)).transpose()?;
        let now = Utc::now();
        let requested_lease = lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS);
        let lease = if config
            .as_ref()
            .is_some_and(|config| config.require_progress)
        {
            requested_lease.clamp(1, MAX_AGENT_LEASE_SECONDS)
        } else {
            clamp_lease(requested_lease)
        };
        let until = now + Duration::seconds(lease as i64);
        let t=tx.query_row(&format!("UPDATE tasks SET state='claimed',claimed_by=?1,claimed_at=?2,updated_at=?2,lease_until=?3,lease_token=?4,attempt_no=attempt_no+1,attempt_count=attempt_count+1
            WHERE id=(SELECT id FROM tasks WHERE state='pending' AND attempt_count<max_attempts AND (?5 IS NULL OR batch_id=?5) AND (batch_id IS NULL OR EXISTS(SELECT 1 FROM batches b WHERE b.id=tasks.batch_id AND b.status='active')) AND (available_at IS NULL OR julianday(available_at)<=julianday('now')) ORDER BY CASE priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 ELSE 2 END,created_at,id LIMIT 1) RETURNING {TASK_COLUMNS}"),params![agent_id,now.to_rfc3339(),until.to_rfc3339(),Uuid::new_v4().to_string(),batch_id.map(|id|id.to_string())],row_to_task).optional()?;
        tx.commit()?;
        drop(conn);
        if let Some(t) = &t {
            self.vault_write(t);
        }
        Ok(t)
    }

    pub fn report_failure(
        &self,
        id: Uuid,
        agent_id: &str,
        token: &str,
        error: serde_json::Value,
    ) -> Result<bool> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let request = serde_json::to_string(&error)?;
        if let Some((owner,prior))=tx.query_row("SELECT agent_id,request FROM task_receipts WHERE task_id=?1 AND lease_token=?2 AND operation='failure'",params![id.to_string(),token],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional()? {return Ok(owner==agent_id && prior==request);}
        let task=tx.query_row(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1 AND state='claimed' AND claimed_by=?2 AND lease_token=?3 AND julianday(lease_until)>julianday('now')"),params![id.to_string(),agent_id,token],row_to_task).optional()?;
        let Some(task) = task else { return Ok(false) };
        if retry_task(&tx, &task, error, false)? == 0 {
            return Ok(false);
        };
        tx.execute(
            "INSERT INTO task_receipts VALUES(?1,?2,'failure',?3,?4,'true')",
            params![id.to_string(), token, agent_id, request],
        )?;
        tx.commit()?;
        drop(conn);
        if let Some(t) = self.get_task(id)? {
            self.vault_write(&t);
        }
        Ok(true)
    }

    pub fn get_batch_status(&self, id: Uuid) -> Result<serde_json::Value> {
        // Read batch metadata and counts in the same read transaction.
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        batch_config(&tx, id, false)?;
        let cancelled: bool = tx.query_row(
            "SELECT status='cancelled' FROM batches WHERE id=?1",
            params![id.to_string()],
            |r| r.get(0),
        )?;
        let mut stmt =
            tx.prepare("SELECT state,count(*) FROM tasks WHERE batch_id=?1 GROUP BY state")?;
        let counts = stmt
            .query_map(params![id.to_string()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        let mut states = serde_json::json!({"pending":0,"claimed":0,"submitted":0,"completed":0,"failed":0,"cancelled":0});
        let mut total = 0;
        for (state, count) in counts {
            if TaskState::parse(&state).is_none() {
                return Err(scheduler_error(
                    -32000,
                    "corrupt task state in batch progress",
                ));
            }
            states[&state] = count.into();
            total += count;
        }
        let active = states["pending"].as_u64().unwrap()
            + states["claimed"].as_u64().unwrap()
            + states["submitted"].as_u64().unwrap();
        tx.commit()?;
        Ok(
            serde_json::json!({"id":id,"status":if cancelled{"cancelled"}else if total>0 && active==0{"completed"}else{"active"},"total":total,"terminal":total-active,"active":active,"states":states}),
        )
    }

    pub fn retry_failed_tasks(&self, id: Uuid) -> Result<usize> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        batch_config(&tx, id, true)?;
        let changed = {
            let mut stmt=tx.prepare(&format!("UPDATE tasks SET state='pending',attempt_count=0,validation_attempts=0,validation_error_count=0,validation_token=NULL,validation_until=NULL,pending_ack=NULL,submission_id=NULL,available_at=NULL,error=NULL,result=NULL,completed_at=NULL,lease_token=NULL,updated_at=?2 WHERE batch_id=?1 AND state='failed' RETURNING {TASK_COLUMNS}"))?;
            let rows = stmt
                .query_map(
                    params![id.to_string(), Utc::now().to_rfc3339()],
                    row_to_task,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        tx.commit()?;
        drop(conn);
        for task in &changed {
            self.vault_write(task);
        }
        Ok(changed.len())
    }

    pub fn cancel_batch(&self, id: Uuid) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        batch_config(&tx, id, false)?;
        tx.execute(
            "UPDATE batches SET status='cancelled' WHERE id=?1",
            params![id.to_string()],
        )?;
        let changed = {
            let mut stmt=tx.prepare(&format!("UPDATE tasks SET state='cancelled',lease_token=NULL,lease_until=NULL,completed_at=?2,updated_at=?2 WHERE batch_id=?1 AND state IN ('pending','claimed','submitted') RETURNING {TASK_COLUMNS}"))?;
            let rows = stmt
                .query_map(
                    params![id.to_string(), Utc::now().to_rfc3339()],
                    row_to_task,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        tx.commit()?;
        drop(conn);
        for task in &changed {
            self.vault_write(task);
        }
        Ok(())
    }
}

impl Store {
    pub fn begin_action(&self, lease_owner: &str) -> Result<Option<super::actions::ActionJob>> {
        if self.actions.is_none() || lease_owner.is_empty() || lease_owner.len() > 128 {
            return Ok(None);
        }
        let now = Utc::now();
        let token = Uuid::new_v4().to_string();
        let until = now + Duration::seconds(super::actions::ACTION_LEASE_SECONDS as i64);
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE completion_actions SET
                state=CASE WHEN idempotent=1 AND attempt_count<max_attempts THEN 'pending'
                           WHEN idempotent=1 THEN 'failed' ELSE 'uncertain' END,
                available_at=NULL,
                lease_owner=NULL,lease_token=NULL,lease_until=NULL,
                last_error='action_lease_expired',updated_at=?1
             WHERE state='running' AND lease_until IS NOT NULL
               AND julianday(lease_until)<=julianday('now')",
            params![now.to_rfc3339()],
        )?;
        let running: u32 = tx.query_row(
            "SELECT count(*) FROM completion_actions WHERE state='running'",
            [],
            |row| row.get(0),
        )?;
        if running >= 2 {
            tx.commit()?;
            return Ok(None);
        }
        let candidate = tx
            .query_row(
                "SELECT id,event_id,profile_id,profile_version,action_kind,payload,
                        idempotent,max_attempts
                 FROM completion_actions
                 WHERE state='pending'
                   AND (available_at IS NULL OR julianday(available_at)<=julianday('now'))
                   AND (action_kind!='on_batch_completed' OR NOT EXISTS(
                     SELECT 1 FROM completion_actions task_action
                     WHERE task_action.batch_id=completion_actions.batch_id
                       AND task_action.action_kind='on_task_completed' AND task_action.state!='succeeded'))
                 ORDER BY created_at,id LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, bool>(6)?,
                        row.get::<_, u32>(7)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            id,
            event_id,
            profile_id,
            profile_version,
            action_kind,
            payload,
            idempotent,
            max_attempts,
        )) = candidate
        else {
            tx.commit()?;
            return Ok(None);
        };
        let changed = tx.execute(
            "UPDATE completion_actions SET state='running',attempt_count=attempt_count+1,
                lease_owner=?1,lease_token=?2,lease_until=?3,updated_at=?4
             WHERE id=?5 AND state='pending'",
            params![lease_owner, token, until.to_rfc3339(), now.to_rfc3339(), id],
        )?;
        if changed != 1 {
            tx.commit()?;
            return Ok(None);
        }
        let attempt_count = tx.query_row(
            "SELECT attempt_count FROM completion_actions WHERE id=?1",
            params![id],
            |row| row.get(0),
        )?;
        tx.commit()?;
        Ok(Some(super::actions::ActionJob {
            id,
            event_id,
            lease_owner: lease_owner.to_string(),
            profile_id,
            profile_version,
            action_kind,
            payload: serde_json::from_str(&payload)?,
            lease_token: token,
            attempt_count,
            max_attempts,
            idempotent,
        }))
    }

    /// Dedicated broker lookup. Client-supplied payload/profile data is never used.
    pub fn runner_action(
        &self,
        id: &str,
        token: &str,
        owner: &str,
        authorize: bool,
    ) -> Result<Option<super::actions::ActionJob>> {
        let conn = self.conn.lock();
        let sql = "SELECT event_id,profile_id,profile_version,action_kind,payload,attempt_count,max_attempts,idempotent
            FROM completion_actions WHERE id=?1 AND lease_token=?2 AND lease_owner=?3 AND state='running'
              AND julianday(lease_until)>julianday('now',?4)";
        let row = conn
            .query_row(
                sql,
                params![
                    id,
                    token,
                    owner,
                    if authorize {
                        "+305 seconds"
                    } else {
                        "+0 seconds"
                    }
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, u32>(5)?,
                        row.get::<_, u32>(6)?,
                        row.get::<_, bool>(7)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            event_id,
            profile_id,
            profile_version,
            action_kind,
            payload,
            attempt_count,
            max_attempts,
            idempotent,
        )) = row
        else {
            return Ok(None);
        };
        let mut payload: serde_json::Value = serde_json::from_str(&payload)?;
        if authorize {
            let batch = payload["batch_id"]
                .as_str()
                .context("action batch missing")?;
            let mut proofs = Vec::new();
            let task_ids: Vec<String> = if let Some(id) = payload["task_id"].as_str() {
                vec![id.to_string()]
            } else {
                let mut statement =
                    conn.prepare("SELECT id FROM tasks WHERE batch_id=?1 ORDER BY task_key,id")?;
                let rows = statement
                    .query_map([batch], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<String>>>()?;
                rows
            };
            if task_ids.is_empty() || task_ids.len() > 10_000 {
                anyhow::bail!("invalid action task set");
            }
            for task_id in task_ids {
                let task = conn.query_row(
                    &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1"),
                    [&task_id],
                    row_to_task,
                )?;
                if task.state != TaskState::Completed
                    || task.batch_id.map(|id| id.to_string()).as_deref() != Some(batch)
                {
                    anyhow::bail!("action task no longer completed in batch");
                }
                let result = task
                    .result
                    .clone()
                    .context("completed task has no result")?;
                let owner = task
                    .claimed_by
                    .as_deref()
                    .context("completed task has no owner")?;
                let mut receipts=conn.prepare("SELECT lease_token,request,response FROM task_receipts WHERE task_id=?1 AND agent_id=?2 AND operation='complete'")?;
                let receipt_rows = receipts
                    .query_map(params![task_id, owner], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let accepted = receipt_rows.iter().any(|(token, request, response)| {
                    let request = serde_json::from_str::<serde_json::Value>(request).ok();
                    let response = serde_json::from_str::<serde_json::Value>(response).ok();
                    let current = if let Some(current) = task.lease_token.as_deref() {
                        token == current
                    } else {
                        task.submission_id.as_deref().is_some_and(|submission| {
                            response
                                .as_ref()
                                .and_then(|value| value["submitted"].as_str())
                                == Some(submission)
                        })
                    };
                    current && request.as_ref().map(|value| &value["result"]) == Some(&result)
                });
                if !accepted {
                    anyhow::bail!("completed task lacks matching current submit receipt");
                }
                if payload["task_id"].is_string()
                    && (payload["attempt_no"].as_u64() != Some(task.attempt_no)
                        || payload.get("result").is_some_and(|event_result| {
                            *event_result != redact_action_secrets(result.clone())
                        }))
                {
                    anyhow::bail!("completion event does not match current attempt/result");
                }
                proofs.push(serde_json::json!({"task_id":task.id,"batch_id":task.batch_id,"agent_id":owner,
                    "attempt_no":task.attempt_no,"input":redact_action_secrets(serde_json::json!({"group_id":task.payload.get("group_id"),"source_image_ids":task.payload.get("source_image_ids")})),"receipt_confirmed":true,
                    "result_sha256":super::windows_security::sha256(&serde_json::to_vec(&redact_action_secrets(result))?)?}));
            }
            payload["authorization"] =
                serde_json::json!({"schema_version":1,"event_id":event_id,"tasks":proofs});
        }
        Ok(Some(super::actions::ActionJob {
            id: id.into(),
            event_id,
            lease_owner: owner.into(),
            profile_id,
            profile_version,
            action_kind,
            payload,
            lease_token: token.into(),
            attempt_count,
            max_attempts,
            idempotent,
        }))
    }

    pub fn finish_action(
        &self,
        job: &super::actions::ActionJob,
        outcome: &super::actions::ActionExecution,
    ) -> Result<bool> {
        let now = Utc::now();
        let retry = outcome.state == super::actions::ActionState::Failed
            && job.idempotent
            && job.attempt_count < job.max_attempts
            && matches!(
                outcome.code,
                "spawn_failed"
                    | "job_assignment_retryable"
                    | "timeout_retryable"
                    | "output_limit_retryable"
                    | "nonzero_exit_retryable"
                    | "process_wait_retryable"
            );
        let state = if retry {
            "pending"
        } else {
            outcome.state.as_str()
        };
        let delay_ms = 100u64
            .saturating_mul(1u64 << job.attempt_count.saturating_sub(1).min(8))
            .min(30_000);
        let available_at =
            retry.then(|| (now + Duration::milliseconds(delay_ms as i64)).to_rfc3339());
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE completion_actions SET state=?1,available_at=?2,last_error=?3,
                exit_code=?4,output_bytes=?5,lease_owner=NULL,lease_token=NULL,lease_until=NULL,
                updated_at=?6
             WHERE id=?7 AND event_id=?8 AND state='running' AND lease_owner=?9
               AND lease_token=?10 AND julianday(lease_until)>julianday('now')",
            params![
                state,
                available_at,
                if outcome.state == super::actions::ActionState::Succeeded {
                    None
                } else {
                    Some(outcome.code)
                },
                outcome.exit_code,
                outcome.output_bytes as u64,
                now.to_rfc3339(),
                job.id,
                job.event_id,
                job.lease_owner,
                job.lease_token,
            ],
        )?;
        tx.commit()?;
        Ok(changed == 1)
    }

    pub fn action_status(&self, batch_id: Uuid) -> Result<serde_json::Value> {
        let conn = self.conn.lock();
        batch_config(&conn, batch_id, false)?;
        let mut stmt = conn.prepare(
            "SELECT id,event_id,task_id,profile_id,profile_version,action_kind,state,
                    attempt_count,max_attempts,available_at,last_error,exit_code,output_bytes
             FROM completion_actions WHERE batch_id=?1 ORDER BY created_at,id LIMIT 201",
        )?;
        let rows = stmt
            .query_map(params![batch_id.to_string()], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "event_id": row.get::<_, String>(1)?,
                    "task_id": row.get::<_, Option<String>>(2)?,
                    "profile_id": row.get::<_, String>(3)?,
                    "profile_version": row.get::<_, u32>(4)?,
                    "action_kind": row.get::<_, String>(5)?,
                    "state": row.get::<_, String>(6)?,
                    "attempt_count": row.get::<_, u32>(7)?,
                    "max_attempts": row.get::<_, u32>(8)?,
                    "available_at": row.get::<_, Option<String>>(9)?,
                    "last_error": row.get::<_, Option<String>>(10)?,
                    "exit_code": row.get::<_, Option<i32>>(11)?,
                    "output_bytes": row.get::<_, Option<u64>>(12)?,
                }))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = rows.len() > 200;
        Ok(serde_json::json!({
            "batch_id": batch_id,
            "actions": rows.into_iter().take(200).collect::<Vec<_>>(),
            "truncated": truncated,
        }))
    }

    pub fn begin_validation(&self) -> Result<Option<super::validator::ValidationJob>> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let candidate=tx.query_row(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE state='submitted' AND (available_at IS NULL OR julianday(available_at)<=julianday('now')) AND (validation_token IS NULL OR julianday(validation_until)<=julianday('now')) ORDER BY updated_at,id LIMIT 1"),[],row_to_task).optional()?;
        let Some(mut task) = candidate else {
            return Ok(None);
        };
        let config = batch_config(
            &tx,
            task.batch_id.context("submitted task requires batch")?,
            true,
        )?
        .validator
        .context("submitted task requires validator")?;
        let recovering: bool = tx.query_row(
            "SELECT validation_token IS NOT NULL FROM tasks WHERE id=?1",
            params![task.id.to_string()],
            |r| r.get(0),
        )?;
        if recovering {
            task.validation_error_count = task
                .validation_error_count
                .saturating_add(1)
                .min(config.max_error_attempts);
        }
        let token = Uuid::new_v4().to_string();
        let until = Utc::now() + Duration::milliseconds(config.timeout_ms as i64 + 5000);
        tx.execute("UPDATE tasks SET validation_token=?2,validation_until=?3,validation_attempts=validation_attempts+1,validation_error_count=?4 WHERE id=?1 AND state='submitted'",params![task.id.to_string(),token,until.to_rfc3339(),task.validation_error_count])?;
        task.validation_attempts += 1;
        tx.commit()?;
        Ok(Some(super::validator::ValidationJob {
            task,
            token,
            config,
        }))
    }

    pub fn finish_validation(
        &self,
        job: &super::validator::ValidationJob,
        outcome: super::validator::Validation,
    ) -> Result<bool> {
        use super::validator::Validation;
        let mut conn = self.conn.lock();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current=tx.query_row(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1 AND state='submitted' AND submission_id=?2 AND validation_token=?3 AND julianday(validation_until)>julianday('now')"),params![job.task.id.to_string(),job.task.submission_id,job.token],row_to_task).optional()?;
        let Some(task) = current else {
            return Ok(false);
        };
        let now = Utc::now();
        let mut ack = None;
        let (state, error, ready, errors) = match outcome {
            Validation::Pass => ("completed", None, None, task.validation_error_count),
            Validation::Fail(reason) => {
                let c = batch_config(&tx, task.batch_id.unwrap(), false)?;
                let delay = c
                    .retry_backoff_ms
                    .saturating_mul(1u64 << task.attempt_count.saturating_sub(1).min(20))
                    .min(c.max_backoff_ms);
                (
                    if task.attempt_count >= task.max_attempts {
                        "failed"
                    } else {
                        "pending"
                    },
                    Some(serde_json::json!({"kind":"validation_fail","message":reason})),
                    Some(now + Duration::milliseconds(delay as i64)),
                    task.validation_error_count,
                )
            }
            Validation::Error(reason) => {
                let errors = (task.validation_error_count + 1).min(job.config.max_error_attempts);
                (
                    if errors >= job.config.max_error_attempts {
                        "failed"
                    } else {
                        "submitted"
                    },
                    Some(serde_json::json!({"kind":"validation_error","message":reason})),
                    Some(now + Duration::milliseconds(100)),
                    errors,
                )
            }
        };
        let terminal = state == "completed" || state == "failed";
        let changed=tx.execute("UPDATE tasks SET state=?4,error=?5,available_at=?6,validation_error_count=?7,validation_token=NULL,validation_until=NULL,completed_at=?8,updated_at=?9 WHERE id=?1 AND state='submitted' AND submission_id=?2 AND validation_token=?3 AND julianday(validation_until)>julianday('now')",params![task.id.to_string(),task.submission_id,job.token,state,error.map(|v|v.to_string()),ready.map(|t|t.to_rfc3339()),errors,if terminal{Some(now.to_rfc3339())}else{None},now.to_rfc3339()])?;
        if changed == 0 {
            return Ok(false);
        }
        if state == "completed" {
            let spec: String = tx.query_row(
                "SELECT pending_ack FROM tasks WHERE id=?1",
                params![task.id.to_string()],
                |r| r.get(0),
            )?;
            ack = write_ack(&tx, task.id, serde_json::from_str(&spec)?, now)?;
            let completed_task = tx.query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id=?1"),
                params![task.id.to_string()],
                row_to_task,
            )?;
            enqueue_task_completion_action(&tx, &completed_task, now)?;
            enqueue_batch_completion_action(&tx, completed_task.batch_id, now)?;
        }
        tx.commit()?;
        drop(conn);
        if let Some(t) = self.get_task(task.id)? {
            self.vault_write(&t);
        }
        if let Some(t) = ack {
            self.vault_write(&t);
        }
        Ok(true)
    }
}

fn batch_config(
    conn: &Connection,
    id: Uuid,
    require_active: bool,
) -> Result<super::types::BatchConfig> {
    let row = conn
        .query_row(
            "SELECT config,status FROM batches WHERE id=?1",
            params![id.to_string()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| scheduler_error(-32004, "batch not found"))?;
    if require_active && row.1 == "cancelled" {
        return Err(scheduler_error(-32001, "batch cancelled"));
    }
    Ok(serde_json::from_str(&row.0)?)
}

fn retry_task(
    conn: &Connection,
    task: &Task,
    error: serde_json::Value,
    expired: bool,
) -> Result<usize> {
    let config = task
        .batch_id
        .map(|id| batch_config(conn, id, false))
        .transpose()?
        .unwrap_or_default();
    let delay = config
        .retry_backoff_ms
        .saturating_mul(1u64 << task.attempt_count.saturating_sub(1).min(20))
        .min(config.max_backoff_ms);
    let now = Utc::now();
    let ready = now + Duration::milliseconds(delay as i64);
    let failed = task.attempt_count >= task.max_attempts;
    let changed=conn.execute("UPDATE tasks SET state=?2,error=?3,available_at=?4,lease_token=NULL,claimed_by=NULL,claimed_at=NULL,lease_until=NULL,result=NULL,completed_at=?5,updated_at=?6 WHERE id=?1 AND state='claimed' AND lease_token=?7 AND (CASE WHEN ?8 THEN julianday(lease_until)<=julianday('now') ELSE julianday(lease_until)>julianday('now') END)",params![task.id.to_string(),if failed{"failed"}else{"pending"},serde_json::to_string(&error)?,ready.to_rfc3339(),if failed{Some(now.to_rfc3339())}else{None},now.to_rfc3339(),task.lease_token,expired])?;
    Ok(changed)
}

/// Map a task `kind` to its initial state on creation. Announcement
/// kinds are completed-on-create — they're publications, not work to
/// be picked up. Work kinds (bug, feature, task) start pending so an
/// agent can claim them.
fn default_state_for_kind(kind: &str) -> TaskState {
    match kind {
        "ack" | "knowledge" | "decision" => TaskState::Completed,
        _ => TaskState::Pending,
    }
}

/// Clamp a requested lease length to a sane range. Zero (and anything
/// suspiciously small) bumps up to a few seconds — otherwise an agent
/// could claim a task and have it auto-reclaimed before its next
/// instruction. The high end is capped to keep the bulletin healable
/// even when an agent forgets to extend.
fn clamp_lease(secs: u64) -> u64 {
    secs.clamp(1, MAX_LEASE_SECONDS)
}

/// Human/wikilink-friendly slug for a task — used as the markdown
/// filename and the wikilink target in agent notes.
pub(crate) fn task_slug(task: &Task) -> String {
    let stamp = task.created_at.format("%Y-%m-%d-%H%M");
    let name = slugify(&task.name);
    format!("{stamp}-{}-{}", &name[..name.len().min(48)], task.id)
}

pub(crate) fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("untitled");
    }
    out
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let id: String = row.get(0)?;
    let id = Uuid::parse_str(&id).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let kind: String = row.get(2)?;
    let priority: String = row.get(3)?;
    let state: String = row.get(4)?;
    let state = TaskState::parse(&state).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown task state: {state}"),
            )),
        )
    })?;
    let payload: String = row.get(6)?;
    let payload: serde_json::Value = serde_json::from_str(&payload).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let result: Option<String> = row.get(7)?;
    let result = result
        .map(|s| {
            serde_json::from_str(&s).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })
        .transpose()?;
    let created_at: String = row.get(8)?;
    let updated_at: String = row.get(9)?;
    let claimed_at: Option<String> = row.get(10)?;
    let lease_until: Option<String> = row.get(11)?;
    let parse_ts = |s: &str| -> rusqlite::Result<chrono::DateTime<Utc>> {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
    };
    let parse_opt_ts = |s: Option<String>| -> rusqlite::Result<Option<DateTime<Utc>>> {
        match s {
            Some(s) => parse_ts(&s).map(Some),
            None => Ok(None),
        }
    };
    Ok(Task {
        id,
        name: row.get(1)?,
        kind,
        priority,
        state,
        claimed_by: row.get(5)?,
        payload,
        result,
        created_at: parse_ts(&created_at)?,
        updated_at: parse_ts(&updated_at)?,
        claimed_at: parse_opt_ts(claimed_at)?,
        lease_until: parse_opt_ts(lease_until)?,
        lease_token: row.get(12)?,
        attempt_no: row.get(13)?,
        attempt_count: row.get(14)?,
        max_attempts: row.get(15)?,
        available_at: parse_opt_ts(row.get(16)?)?,
        batch_id: row
            .get::<_, Option<String>>(17)?
            .map(|s| {
                Uuid::parse_str(&s).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        17,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })
            .transpose()?,
        task_key: row.get(18)?,
        error: row
            .get::<_, Option<String>>(19)?
            .map(|s| {
                serde_json::from_str(&s).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        19,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })
            .transpose()?,
        completed_at: parse_opt_ts(row.get(20)?)?,
        submission_id: row.get(21)?,
        validation_attempts: row.get(22)?,
        validation_error_count: row.get(23)?,
    })
}

fn try_add_column(conn: &Connection, table: &str, column: &str, def: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if columns.iter().any(|c| c == column) {
        return Ok(());
    }
    let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {def}");
    conn.execute(&sql, [])?;
    Ok(())
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tasks (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    state       TEXT NOT NULL,
    claimed_by  TEXT,
    payload     TEXT NOT NULL,
    result      TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'task',
    priority    TEXT NOT NULL DEFAULT 'normal',
    claimed_at  TEXT,
    lease_until TEXT
);
CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state);
CREATE INDEX IF NOT EXISTS idx_tasks_claimed_by ON tasks(claimed_by);

CREATE TABLE IF NOT EXISTS agents (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    last_seen  TEXT NOT NULL,
    first_seen TEXT NOT NULL DEFAULT '1970-01-01T00:00:00Z'
);
CREATE TABLE IF NOT EXISTS task_progress (
    task_id TEXT NOT NULL,
    lease_token TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    attempt_no INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    checkpoint TEXT NOT NULL,
    progress_at TEXT NOT NULL,
    last_extended_sequence INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(task_id, lease_token)
);
CREATE TABLE IF NOT EXISTS task_progress_receipts (
    task_id TEXT NOT NULL,
    lease_token TEXT NOT NULL,
    request_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    checkpoint TEXT NOT NULL,
    progress_at TEXT NOT NULL,
    PRIMARY KEY(task_id, lease_token, request_id),
    UNIQUE(task_id, lease_token, sequence)
);
CREATE TABLE IF NOT EXISTS task_progress_extend_receipts (
    task_id TEXT NOT NULL,
    lease_token TEXT NOT NULL,
    request_id TEXT NOT NULL,
    progress_sequence INTEGER NOT NULL,
    lease_until TEXT NOT NULL,
    PRIMARY KEY(task_id, lease_token, request_id),
    UNIQUE(task_id, lease_token, progress_sequence)
);
CREATE INDEX IF NOT EXISTS idx_task_progress_at ON task_progress(progress_at);
CREATE TABLE IF NOT EXISTS completion_actions (
    id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    event_key TEXT NOT NULL UNIQUE,
    batch_id TEXT NOT NULL,
    task_id TEXT,
    profile_id TEXT NOT NULL,
    profile_version INTEGER NOT NULL,
    action_kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    idempotent INTEGER NOT NULL,
    max_attempts INTEGER NOT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    available_at TEXT,
    lease_owner TEXT,
    lease_token TEXT,
    lease_until TEXT,
    last_error TEXT,
    exit_code INTEGER,
    output_bytes INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_completion_actions_ready ON completion_actions(state, available_at, created_at);
CREATE INDEX IF NOT EXISTS idx_completion_actions_batch ON completion_actions(batch_id, created_at);
"#;

fn write_ack(
    tx: &Connection,
    id: Uuid,
    ack: Option<AckSpec>,
    now: DateTime<Utc>,
) -> Result<Option<Task>> {
    let ack_task = if let Some(spec) = ack {
        // Daemon stitches the wikilink (`fixed_bug_id` → source
        // task) into the ack's payload so the vault graph
        // renders the chain. Caller-supplied payload wins on
        // key collision so an agent that already knows the
        // wikilink it wants can override.
        let mut payload = match spec.payload {
            Some(serde_json::Value::Object(m)) => m,
            Some(other) => {
                // Non-object payload (e.g. caller passed a
                // string or array): preserve it under `value`
                // and inject the wikilink alongside.
                let mut m = serde_json::Map::new();
                m.insert("value".into(), other);
                m
            }
            None => serde_json::Map::new(),
        };
        payload
            .entry("fixed_bug_id".to_string())
            .or_insert_with(|| serde_json::Value::String(id.to_string()));
        let payload_v = serde_json::Value::Object(payload);
        let payload_s = serde_json::to_string(&payload_v)?;
        let ack_id = Uuid::new_v4();
        let priority = spec.priority.as_deref().unwrap_or("high");
        tx.execute(
                    "INSERT INTO tasks
                       (id, name, kind, priority, state, payload, created_at, updated_at, completed_at)
                     VALUES (?1, ?2, 'ack', ?3, 'completed', ?4, ?5, ?5, ?5)",
                    params![
                        ack_id.to_string(),
                        spec.name,
                        priority,
                        payload_s,
                        now.to_rfc3339(),
                    ],
                )?;
        Some(Task {
            id: ack_id,
            name: spec.name,
            kind: "ack".into(),
            priority: priority.into(),
            state: TaskState::Completed,
            claimed_by: None,
            payload: payload_v,
            result: None,
            created_at: now,
            updated_at: now,
            claimed_at: None,
            lease_until: None,
            lease_token: None,
            attempt_no: 0,
            attempt_count: 0,
            max_attempts: 3,
            submission_id: None,
            validation_attempts: 0,
            validation_error_count: 0,
            available_at: None,
            batch_id: None,
            task_key: None,
            error: None,
            completed_at: Some(now),
        })
    } else {
        None
    };
    Ok(ack_task)
}
