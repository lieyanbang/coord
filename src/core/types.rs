//! Wire types shared between the HTTP layer and the store.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Pending,
    Claimed,
    Submitted,
    Completed,
    Failed,
    Cancelled,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Submitted => "submitted",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "pending" => Self::Pending,
            "claimed" => Self::Claimed,
            "submitted" => Self::Submitted,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => return None,
        })
    }
}

/// Free-form task category. Common values: `task` (default), `bug`,
/// `feature`, `decision`, `ack`, `knowledge`, `build`. The set is open
/// so callers can add new kinds without a server upgrade.
pub const DEFAULT_KIND: &str = "task";

/// Free-form priority. Common values: `low`, `normal` (default), `high`,
/// `urgent`. The TUI colors rows based on these strings; unknown values
/// render as default.
pub const DEFAULT_PRIORITY: &str = "normal";

/// Default lease length for a fresh claim, in seconds. Short on purpose:
/// the cost of "too short" is one extra `tasks/extend` RPC per minute
/// (cheap), the cost of "too long" is a stuck task blocking the
/// bulletin for the whole window. Callers can override per-claim and
/// operators can override the daemon default via `COORD_DEFAULT_LEASE`.
pub const DEFAULT_LEASE_SECONDS: u64 = 300;

/// Hard ceiling on a single lease. Prevents agents from accidentally
/// requesting hours-long leases that would defeat the auto-reclaim
/// loop. Callers that need this much wall-clock should re-extend.
pub const MAX_LEASE_SECONDS: u64 = 3_600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub priority: String,
    pub state: TaskState,
    pub claimed_by: Option<String>,
    pub payload: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// When the current claim was last (re-)granted. `None` for tasks
    /// that have never been claimed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<DateTime<Utc>>,
    /// Wall-clock deadline after which a claim is considered abandoned
    /// and the task is eligible for auto-reclaim back to `pending`.
    /// `None` for unclaimed tasks and for legacy rows from before
    /// the lease migration ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub lease_token: Option<String>,
    #[serde(default)]
    pub attempt_no: u64,
    #[serde(default)]
    pub attempt_count: u32,
    /// Compatibility count of how many claims have been issued for this task.
    /// It is retained for history and is no longer the business retry gate.
    #[serde(default)]
    pub failure_count: u32,
    /// Consecutive abandoned attempts since the last accepted progress update.
    #[serde(default)]
    pub interruption_streak: u32,
    /// Set after repeated interruptions so operators can see a stuck task.
    #[serde(default)]
    pub needs_attention: bool,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default)]
    pub available_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub batch_id: Option<Uuid>,
    #[serde(default)]
    pub task_key: Option<String>,
    #[serde(default)]
    pub error: Option<serde_json::Value>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub submission_id: Option<String>,
    #[serde(default)]
    pub validation_attempts: u32,
    #[serde(default)]
    pub validation_error_count: u32,
}

pub fn default_max_attempts() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BatchConfig {
    pub max_attempts: u32,
    pub retry_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub validator: Option<super::validator::ValidatorConfig>,
    pub require_progress: bool,
    pub agent_lease_budget_seconds: u64,
    pub action_profile: Option<String>,
    pub action_profile_version: Option<u32>,
    pub action_profile_actions: Vec<String>,
    pub action_policy: BTreeMap<String, ActionPolicy>,
}

#[derive(Debug)]
pub struct ProgressReport<'a> {
    pub id: Uuid,
    pub agent_id: &'a str,
    pub lease_token: &'a str,
    pub attempt_no: u64,
    pub sequence: u64,
    pub request_id: Uuid,
    pub checkpoint: &'a str,
}

#[derive(Debug)]
pub struct ProgressLeaseExtension<'a> {
    pub id: Uuid,
    pub agent_id: &'a str,
    pub lease_token: &'a str,
    pub attempt_no: u64,
    pub progress_sequence: u64,
    pub request_id: Uuid,
    pub lease_seconds: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct ActionPolicy {
    pub idempotent: bool,
    pub max_attempts: u32,
    pub include_result: bool,
}
impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            retry_backoff_ms: 100,
            max_backoff_ms: 30_000,
            validator: None,
            require_progress: false,
            agent_lease_budget_seconds: 3_600,
            action_profile: None,
            action_profile_version: None,
            action_profile_actions: Vec::new(),
            action_policy: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// First time this agent ID heartbeated against this database. Used
    /// to render uptime in the TUI (a monotonically growing counter).
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub current_task: Option<Uuid>,
}
