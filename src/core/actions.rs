use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub const MAX_ACTION_EVENT_BYTES: usize = 4 * 1024 * 1024;
pub const ACTION_LEASE_SECONDS: u64 = 360;
const ALLOWED_PLACEHOLDERS: &[&str] = &[
    "event_id",
    "task_id",
    "batch_id",
    "action_kind",
    "attempt_no",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRegistryFile {
    pub version: u32,
    pub allowed_root: PathBuf,
    pub profiles: BTreeMap<String, ActionProfile>,
    #[serde(default)]
    pub management_rpc_key_file: Option<PathBuf>,
    #[serde(default)]
    /// Opaque to Core's scheduler startup; parsed inside the Broker fault domain.
    pub external_runner: Option<Value>,
    /// Unsafe for production; only the existing isolated process canaries use it.
    #[serde(default)]
    pub isolated_in_process: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionProfile {
    pub version: u32,
    pub on_task_completed: Option<ActionCommand>,
    pub on_batch_completed: Option<ActionCommand>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionCommand {
    pub executable: PathBuf,
    pub working_directory: PathBuf,
    pub args: Vec<String>,
    #[serde(default)]
    pub include_result: bool,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
    pub max_attempts: u32,
    pub idempotent: bool,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ActionRegistry {
    allowed_root: PathBuf,
    profiles: BTreeMap<String, ActionProfile>,
    management_rpc_key_file: Option<PathBuf>,
    pub external_runner: Option<Value>,
    pub isolated_in_process: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionJob {
    pub id: String,
    pub event_id: String,
    pub lease_owner: String,
    pub profile_id: String,
    pub profile_version: u32,
    pub action_kind: String,
    pub payload: Value,
    pub lease_token: String,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub idempotent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Succeeded,
    Failed,
    Uncertain,
}

impl ActionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Uncertain => "uncertain",
        }
    }
}

#[derive(Debug)]
pub struct ActionExecution {
    pub state: ActionState,
    pub code: &'static str,
    pub exit_code: Option<i32>,
    pub output_bytes: usize,
}

impl ActionRegistry {
    pub fn load(path: &Path) -> Result<Self> {
        #[cfg(not(windows))]
        bail!("completion actions require Windows Job Objects");

        let safe_path = super::windows_security::safe_existing_path(path)?;
        let metadata = fs::symlink_metadata(&safe_path).context("inspect action registry")?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 256 * 1024 {
            bail!("action registry must be a regular file no larger than 256 KiB");
        }
        let file: ActionRegistryFile = serde_json::from_slice(&fs::read(safe_path)?)?;
        if file.version != 1 || file.profiles.is_empty() {
            bail!("unsupported or empty action registry");
        }
        let nested_management_path = file
            .external_runner
            .as_ref()
            .and_then(|runner| runner.get("daemon_rpc_key_file"))
            .and_then(Value::as_str)
            .map(PathBuf::from);
        if file.management_rpc_key_file.is_some()
            && nested_management_path.as_ref().is_some_and(|nested| {
                !super::action_runner::same_config_path(
                    file.management_rpc_key_file.as_ref().unwrap(),
                    nested,
                )
            })
        {
            bail!("conflicting management RPC key paths");
        }
        let management_rpc_key_file = file
            .management_rpc_key_file
            .clone()
            .or(nested_management_path);
        if file.external_runner.is_some() && management_rpc_key_file.is_none() {
            bail!("separate management RPC key path required");
        }
        if management_rpc_key_file
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            bail!("management RPC key path must be absolute");
        }
        if let (Some(management), Some(legacy)) = (
            management_rpc_key_file.as_ref(),
            file.external_runner
                .as_ref()
                .and_then(|runner| runner.get("key_file"))
                .and_then(Value::as_str)
                .map(PathBuf::from),
        ) {
            if super::action_runner::same_config_path(management, &legacy) {
                bail!("management RPC key path must not reuse the legacy Broker key path");
            }
        }
        if file.external_runner.is_some() && file.isolated_in_process {
            bail!("external and isolated in-process modes cannot be combined");
        }
        let root = super::windows_security::safe_existing_path(&file.allowed_root)?;
        if !root.is_dir() {
            bail!("action root must be a directory");
        }
        for (name, profile) in &file.profiles {
            if !valid_profile_id(name) || profile.version == 0 {
                bail!("invalid action profile identifier or version");
            }
            if profile.on_task_completed.is_none() && profile.on_batch_completed.is_none() {
                bail!("action profile must define at least one completion action");
            }
        }
        let mut profiles = file.profiles;
        for profile in profiles.values_mut() {
            for command in [
                &mut profile.on_task_completed,
                &mut profile.on_batch_completed,
            ]
            .into_iter()
            .flatten()
            {
                validate_command(command, &root)?;
                if file.external_runner.is_some() && command.sha256.is_none() {
                    bail!("external runner requires every executable SHA256 pin");
                }
            }
        }
        Ok(Self {
            allowed_root: root,
            profiles,
            management_rpc_key_file,
            external_runner: file.external_runner,
            isolated_in_process: file.isolated_in_process,
        })
    }

    pub fn runner_config(&self) -> Result<Option<super::action_runner::RunnerConfig>> {
        self.external_runner
            .as_ref()
            .map(|value| {
                serde_json::from_value(value.clone())
                    .context("invalid external_runner configuration")
            })
            .transpose()
    }

    pub fn management_key_path(&self) -> Result<Option<PathBuf>> {
        let Some(management_path) = self.management_rpc_key_file.clone() else {
            return Ok(None);
        };
        // Do this as a lexical config check before reading either file. The
        // legacy Broker-key path can be inaccessible to Core by design.
        if self
            .external_runner
            .as_ref()
            .and_then(|runner| runner.get("key_file"))
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .is_some_and(|legacy_path| {
                super::action_runner::same_config_path(&management_path, &legacy_path)
            })
        {
            bail!("management RPC key path must not reuse the legacy Broker key path");
        }
        Ok(Some(management_path))
    }

    pub fn profile(&self, id: &str) -> Option<&ActionProfile> {
        self.profiles.get(id)
    }

    pub fn execute(&self, job: &ActionJob) -> ActionExecution {
        let Some(profile) = self.profiles.get(&job.profile_id) else {
            return execution(ActionState::Failed, "profile_missing", None, 0);
        };
        if profile.version != job.profile_version {
            return execution(ActionState::Failed, "profile_version_changed", None, 0);
        }
        let command = match job.action_kind.as_str() {
            "on_task_completed" => profile.on_task_completed.as_ref(),
            "on_batch_completed" => profile.on_batch_completed.as_ref(),
            _ => None,
        };
        let Some(command) = command else {
            return execution(ActionState::Failed, "action_missing", None, 0);
        };
        if super::windows_security::safe_existing_path(&command.executable).is_err()
            || super::windows_security::safe_existing_path(&command.working_directory).is_err()
            || command.sha256.as_ref().is_some_and(|pin| {
                super::windows_security::file_sha256(&command.executable)
                    .map_or(true, |actual| !actual.eq_ignore_ascii_case(pin))
            })
        {
            return execution(ActionState::Failed, "executable_integrity_failed", None, 0);
        }
        if command.idempotent != job.idempotent || command.max_attempts != job.max_attempts {
            return execution(ActionState::Failed, "profile_changed", None, 0);
        }
        let event = match serde_json::to_vec(&job.payload) {
            Ok(event) if event.len() <= MAX_ACTION_EVENT_BYTES => event,
            _ => return execution(ActionState::Failed, "event_too_large", None, 0),
        };
        let args = match expand_args(command, job) {
            Ok(args) => args,
            Err(_) => return execution(ActionState::Failed, "invalid_profile_arguments", None, 0),
        };
        run_command(command, &args, &event)
    }

    pub fn allowed_root(&self) -> &Path {
        &self.allowed_root
    }
}

fn valid_profile_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
        && id.as_bytes()[0].is_ascii_lowercase()
}

fn validate_command(command: &mut ActionCommand, root: &Path) -> Result<()> {
    if !command.executable.is_absolute()
        || !command.working_directory.is_absolute()
        || command.args.len() > 32
        || command.timeout_ms == 0
        || command.timeout_ms > 300_000
        || command.max_output_bytes == 0
        || command.max_output_bytes > 64 * 1024
        || command.max_attempts == 0
        || command.max_attempts > 5
        || (!command.idempotent && command.max_attempts != 1)
    {
        bail!("invalid bounded action command configuration");
    }
    let executable_metadata = fs::symlink_metadata(&command.executable)?;
    if executable_metadata.file_type().is_symlink() || !executable_metadata.is_file() {
        bail!("action executable must be a regular non-symlink file");
    }
    if !command
        .executable
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com")
        })
    {
        bail!("completion action executable must be a native .exe or .com file");
    }
    let executable = super::windows_security::safe_existing_path(&command.executable)?;
    if !executable.starts_with(root) {
        bail!("action executable is outside the trusted root");
    }
    let directory_metadata = fs::symlink_metadata(&command.working_directory)?;
    if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
        bail!("action working directory must be a non-symlink directory");
    }
    let working_directory =
        super::windows_security::safe_existing_path(&command.working_directory)?;
    if !working_directory.starts_with(root) {
        bail!("action working directory is outside the trusted root");
    }
    if command
        .args
        .iter()
        .any(|arg| arg.len() > 1024 || arg.contains('\0') || !valid_template(arg))
    {
        bail!("action arguments contain an unsupported placeholder");
    }
    command.executable = executable;
    command.working_directory = working_directory;
    if let Some(pin) = &command.sha256 {
        if pin.len() != 64
            || !pin.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !super::windows_security::file_sha256(&command.executable)?.eq_ignore_ascii_case(pin)
        {
            bail!("executable SHA256 pin mismatch");
        }
    }
    Ok(())
}

fn valid_template(value: &str) -> bool {
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('}') else {
            return false;
        };
        if !ALLOWED_PLACEHOLDERS.contains(&&rest[..close]) {
            return false;
        }
        rest = &rest[close + 1..];
    }
    !rest.contains('}')
}

fn expand_args(command: &ActionCommand, job: &ActionJob) -> Result<Vec<String>> {
    let event = &job.payload;
    let task_id = event.get("task_id").and_then(Value::as_str).unwrap_or("");
    let batch_id = event
        .get("batch_id")
        .and_then(Value::as_str)
        .context("missing batch id")?;
    let attempt_no = event.get("attempt_no").and_then(Value::as_u64).unwrap_or(0);
    Ok(command
        .args
        .iter()
        .map(|arg| {
            arg.replace("{event_id}", &job.event_id)
                .replace("{task_id}", task_id)
                .replace("{batch_id}", batch_id)
                .replace("{action_kind}", &job.action_kind)
                .replace("{attempt_no}", &attempt_no.to_string())
        })
        .collect())
}

fn execution(
    state: ActionState,
    code: &'static str,
    exit_code: Option<i32>,
    output_bytes: usize,
) -> ActionExecution {
    ActionExecution {
        state,
        code,
        exit_code,
        output_bytes,
    }
}

fn run_command(command: &ActionCommand, args: &[String], event: &[u8]) -> ActionExecution {
    let mut process = Command::new(&command.executable);
    process
        .args(args)
        .current_dir(&command.working_directory)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No user code runs before Job Object containment is established.
        process.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
    let mut child = match process.spawn() {
        Ok(child) => child,
        Err(_) => return execution(ActionState::Failed, "spawn_failed", None, 0),
    };
    let job = match WindowsJob::assign(&child) {
        Ok(job) => job,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return if command.idempotent {
                execution(ActionState::Failed, "job_assignment_retryable", None, 0)
            } else {
                execution(ActionState::Uncertain, "job_assignment_failed", None, 0)
            };
        }
    };
    #[cfg(windows)]
    if resume_contained_process(child.id()).is_err() {
        terminate_child(&mut child, &job);
        let _ = child.wait();
        return execution(ActionState::Uncertain, "process_resume_failed", None, 0);
    }
    let overflow = Arc::new(AtomicBool::new(false));
    let output_bytes = Arc::new(AtomicUsize::new(0));
    let stdout_thread = drain_pipe(
        child.stdout.take().expect("piped stdout"),
        command.max_output_bytes,
        overflow.clone(),
        output_bytes.clone(),
    );
    let stderr_thread = drain_pipe(
        child.stderr.take().expect("piped stderr"),
        command.max_output_bytes,
        overflow.clone(),
        output_bytes.clone(),
    );
    let input = child.stdin.take().expect("piped stdin");
    let event = event.to_vec();
    let input_thread = thread::spawn(move || {
        let mut input = input;
        input.write_all(&event)
    });

    let deadline = Instant::now() + Duration::from_millis(command.timeout_ms);
    let (status, timed_out, too_much_output) = loop {
        let too_much_output = overflow.load(Ordering::Relaxed);
        if too_much_output {
            terminate_child(&mut child, &job);
            break (child.wait().ok(), false, true);
        }
        match child.try_wait() {
            Ok(Some(status)) => break (Some(status), false, false),
            Ok(None) => {}
            Err(_) => {
                terminate_child(&mut child, &job);
                break (child.wait().ok(), false, false);
            }
        }
        if Instant::now() >= deadline {
            terminate_child(&mut child, &job);
            break (child.wait().ok(), true, false);
        }
        thread::sleep(Duration::from_millis(10));
    };
    // Completion actions are short-lived jobs, not detached daemon processes.
    // Closing the Job Object then also contains descendants that kept a pipe open.
    job.terminate();
    let _ = input_thread.join();
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    let bytes = output_bytes.load(Ordering::Relaxed);
    if timed_out || too_much_output {
        let retryable = command.idempotent;
        return execution(
            if retryable {
                ActionState::Failed
            } else {
                ActionState::Uncertain
            },
            if timed_out {
                if retryable {
                    "timeout_retryable"
                } else {
                    "timeout"
                }
            } else if retryable {
                "output_limit_retryable"
            } else {
                "output_limit"
            },
            status.and_then(|value| value.code()),
            bytes,
        );
    }
    match status {
        Some(status) if status.success() => {
            execution(ActionState::Succeeded, "exit_zero", status.code(), bytes)
        }
        Some(status) if command.idempotent => execution(
            ActionState::Failed,
            "nonzero_exit_retryable",
            status.code(),
            bytes,
        ),
        Some(status) => execution(ActionState::Uncertain, "nonzero_exit", status.code(), bytes),
        None if command.idempotent => {
            execution(ActionState::Failed, "process_wait_retryable", None, bytes)
        }
        None => execution(ActionState::Uncertain, "process_wait_failed", None, bytes),
    }
}

#[cfg(windows)]
fn resume_contained_process(process_id: u32) -> Result<()> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            bail!("snapshot suspended action threads");
        }
        let result = (|| -> Result<()> {
            let mut entry: THREADENTRY32 = zeroed();
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut available = Thread32First(snapshot, &mut entry) != 0;
            let mut thread_id = None;
            while available {
                if entry.th32OwnerProcessID == process_id {
                    if thread_id.is_some() {
                        bail!("unexpected threads before action startup");
                    }
                    thread_id = Some(entry.th32ThreadID);
                }
                available = Thread32Next(snapshot, &mut entry) != 0;
            }
            let thread = OpenThread(
                THREAD_SUSPEND_RESUME,
                0,
                thread_id.context("suspended primary thread missing")?,
            );
            if thread.is_null() {
                bail!("open suspended action thread");
            }
            let previous = ResumeThread(thread);
            CloseHandle(thread);
            if previous != 1 {
                bail!("action primary thread had unexpected suspend count");
            }
            Ok(())
        })();
        CloseHandle(snapshot);
        result
    }
}

fn drain_pipe<R: Read + Send + 'static>(
    mut pipe: R,
    max_bytes: usize,
    overflow: Arc<AtomicBool>,
    output_bytes: Arc<AtomicUsize>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    let total = output_bytes.fetch_add(count, Ordering::Relaxed) + count;
                    if total > max_bytes {
                        overflow.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
    })
}

fn terminate_child(child: &mut std::process::Child, job: &WindowsJob) {
    job.terminate();
    let _ = child.kill();
}

#[cfg(windows)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl WindowsJob {
    fn assign(child: &std::process::Child) -> Result<Self> {
        use std::mem::{size_of, zeroed};
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_MEMORY,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        };
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                bail!("CreateJobObjectW failed");
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                | JOB_OBJECT_LIMIT_JOB_MEMORY;
            limits.BasicLimitInformation.ActiveProcessLimit = 8;
            limits.ProcessMemoryLimit = 256 * 1024 * 1024;
            limits.JobMemoryLimit = 256 * 1024 * 1024;
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle() as _) == 0
            {
                windows_sys::Win32::Foundation::CloseHandle(handle);
                bail!("failed to place action process in a kill-on-close Job Object");
            }
            Ok(Self(handle))
        }
    }

    fn terminate(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(not(windows))]
struct WindowsJob;

#[cfg(not(windows))]
impl WindowsJob {
    fn assign(_child: &std::process::Child) -> Result<Self> {
        Ok(Self)
    }

    fn terminate(&self) {}
}
