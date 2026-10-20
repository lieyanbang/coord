//! Built-in validation only. No untrusted command/plugin execution.
use super::types::Task;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ValidatorConfig {
    pub required: Vec<String>,
    pub equals: Option<Value>,
    pub delay_ms: u64,
    pub timeout_ms: u64,
    pub max_error_attempts: u32,
}
impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            required: vec![],
            equals: None,
            delay_ms: 0,
            timeout_ms: 1000,
            max_error_attempts: 3,
        }
    }
}

#[derive(Debug)]
pub enum Validation {
    Pass,
    Fail(String),
    Error(String),
}
pub trait Validator: Send + Sync {
    fn validate(&self, task: &Task, result: &Value, config: &ValidatorConfig) -> Validation;
}
pub struct JsonValidator;
impl Validator for JsonValidator {
    fn validate(&self, _task: &Task, result: &Value, config: &ValidatorConfig) -> Validation {
        for key in &config.required {
            if result.get(key).is_none() {
                return Validation::Fail(format!("required key missing: {key}"));
            }
        }
        if config
            .equals
            .as_ref()
            .is_some_and(|expected| expected != result)
        {
            return Validation::Fail("result differs from expected JSON".into());
        }
        Validation::Pass
    }
}

pub struct ValidationJob {
    pub task: Task,
    pub token: String,
    pub config: ValidatorConfig,
}
pub async fn validate(job: &ValidationJob) -> Validation {
    if job.task.validation_error_count >= job.config.max_error_attempts {
        return Validation::Error("validator interruption budget exhausted".into());
    }
    use std::time::Duration;
    let run = async {
        tokio::time::sleep(Duration::from_millis(job.config.delay_ms)).await;
        JsonValidator.validate(
            &job.task,
            job.task.result.as_ref().unwrap_or(&Value::Null),
            &job.config,
        )
    };
    match tokio::time::timeout(Duration::from_millis(job.config.timeout_ms), run).await {
        Ok(v) => v,
        Err(_) => Validation::Error("validator timeout".into()),
    }
}
