//! Version-pinned, bounded JSONL sidecars with isolated provider homes.
//!
//! On Unix, Carl owns a POSIX process group and terminates ordinary descendants in
//! that group. A hostile descendant can escape by calling `setsid` or moving to a
//! different process group: this is not a cgroup or equivalent process-tree
//! containment. Authentication sidecars are trusted, version-pinned provider
//! executables. Later delegate execution needs stronger OS containment for detached
//! descendants.
//!
//! Unix provider-home creation walks from an already-open Carl data-root directory
//! using `openat`/`mkdirat` and `O_NOFOLLOW`. Windows rejects reparse points and
//! verifies inherited DACLs after each creation step, but its path walk assumes the
//! Carl-owned data root remains trusted during creation.

mod jsonl;

use std::collections::{HashMap, hash_map::Entry};
use std::env;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap};
use semver::{Version, VersionReq};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex as AsyncMutex, Semaphore, mpsc, oneshot};
use tokio::task::{AbortHandle, JoinHandle};

use self::jsonl::{encode_line, read_bounded_line};

const STATE_RUNNING: u8 = 0;
const STATE_CANCELLING: u8 = 1;
const STATE_STOPPED: u8 = 2;
const VERSION_OUTPUT_LIMIT: usize = 4 * 1_024;
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const REDACTED_STDERR: &str = "<redacted sidecar stderr>";
const MAX_PENDING_REQUESTS: usize = 128;
const SUPERVISOR_CHANNEL_CAPACITY: usize = 256;
const WRITER_CHANNEL_CAPACITY: usize = 128;
const NOTIFICATION_CHANNEL_CAPACITY: usize = 64;

#[cfg(all(unix, not(target_os = "macos")))]
const ALLOWED_ENVIRONMENT: &[&str] = &["LANG", "LC_ALL", "LC_CTYPE", "PATH", "TERM", "TZ"];

#[cfg(target_os = "macos")]
const ALLOWED_ENVIRONMENT: &[&str] = &[
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "PATH",
    "TERM",
    "TZ",
    // macOS inserts this locale/encoding variable during process startup even
    // after env_clear, so treat that non-credential key as an explicit allowlist entry.
    "__CF_USER_TEXT_ENCODING",
];

#[cfg(windows)]
const ALLOWED_ENVIRONMENT: &[&str] = &[
    "COMSPEC",
    "PATHEXT",
    "PATH",
    "SYSTEMDRIVE",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
    "WINDIR",
];

/// The closed parser selected for a provider's documented version output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionOutputFormat {
    /// Accept exactly `<prefix> <semver>`, apart from surrounding ASCII whitespace.
    ExactPrefix(&'static str),
    /// Accept output containing exactly one whitespace-delimited semantic-version token.
    SingleSemverToken,
}

/// A provider process and its version compatibility contract.
pub struct SidecarCommand {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub version_arguments: Vec<OsString>,
    pub version_output: VersionOutputFormat,
    pub home_variable: &'static str,
    pub isolated_home: PathBuf,
    pub supported_versions: VersionReq,
}

impl SidecarCommand {
    /// Resolve and validate the executable, invoke the configured version command, and
    /// apply the pinned compatibility requirement.
    pub async fn detect_version(
        &self,
        carl_data_root: impl AsRef<Path>,
        workspace: impl AsRef<Path>,
    ) -> Result<Version, SidecarError> {
        validate_home_variable(self.home_variable)?;
        prepare_provider_home(
            carl_data_root.as_ref(),
            workspace.as_ref(),
            &self.isolated_home,
        )?;
        let executable = VerifiedExecutable::resolve(&self.executable)?;
        detect_version(self, &executable, &self.isolated_home).await
    }
}

/// Resource limits and shutdown deadlines for a sidecar.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SidecarLimits {
    pub max_stdout_line_bytes: usize,
    pub max_stderr_bytes: usize,
    pub graceful_shutdown_timeout: Duration,
    pub forced_shutdown_timeout: Duration,
    pub process_poll_interval: Duration,
}

impl Default for SidecarLimits {
    fn default() -> Self {
        Self {
            max_stdout_line_bytes: 256 * 1_024,
            max_stderr_bytes: 16 * 1_024,
            graceful_shutdown_timeout: Duration::from_secs(2),
            forced_shutdown_timeout: Duration::from_secs(2),
            process_poll_interval: Duration::from_millis(20),
        }
    }
}

impl SidecarLimits {
    fn validate(self) -> Result<Self, SidecarError> {
        if self.max_stdout_line_bytes == 0
            || self.max_stderr_bytes == 0
            || self.graceful_shutdown_timeout.is_zero()
            || self.forced_shutdown_timeout.is_zero()
            || self.process_poll_interval.is_zero()
        {
            return Err(SidecarError::from_code(
                SidecarErrorCode::InvalidConfiguration,
            ));
        }
        Ok(self)
    }
}

/// Stable, non-sensitive sidecar failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidecarErrorCode {
    ExecutableMissing,
    ExecutableUnavailable,
    UnsafeExecutable,
    UnsupportedVersion,
    InvalidProviderHome,
    InvalidConfiguration,
    SpawnFailed,
    ProtocolViolation,
    DuplicateRequestId,
    SidecarExited,
    Cancelled,
    TimedOut,
}

impl SidecarErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecutableMissing => "executable_missing",
            Self::ExecutableUnavailable => "executable_unavailable",
            Self::UnsafeExecutable => "unsafe_executable",
            Self::UnsupportedVersion => "unsupported_version",
            Self::InvalidProviderHome => "invalid_provider_home",
            Self::InvalidConfiguration => "invalid_configuration",
            Self::SpawnFailed => "spawn_failed",
            Self::ProtocolViolation => "protocol_violation",
            Self::DuplicateRequestId => "duplicate_request_id",
            Self::SidecarExited => "sidecar_exited",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        }
    }
}

impl fmt::Display for SidecarErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("sidecar operation failed: {code}")]
pub struct SidecarError {
    code: SidecarErrorCode,
}

impl SidecarError {
    #[must_use]
    pub const fn from_code(code: SidecarErrorCode) -> Self {
        Self { code }
    }

    #[must_use]
    pub const fn code(self) -> SidecarErrorCode {
        self.code
    }
}

/// Environment names that may be copied from Carl to provider sidecars.
#[must_use]
pub const fn allowed_environment_variables() -> &'static [&'static str] {
    ALLOWED_ENVIRONMENT
}

struct VerifiedExecutable {
    canonical_path: PathBuf,
}

impl VerifiedExecutable {
    fn resolve(candidate: &Path) -> Result<Self, SidecarError> {
        let discovered = discover_executable(candidate)?;
        let canonical_path = fs::canonicalize(discovered).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SidecarError::from_code(SidecarErrorCode::ExecutableMissing)
            } else {
                SidecarError::from_code(SidecarErrorCode::ExecutableUnavailable)
            }
        })?;
        let metadata = fs::symlink_metadata(&canonical_path)
            .map_err(|_| SidecarError::from_code(SidecarErrorCode::ExecutableUnavailable))?;
        if !metadata.file_type().is_file() || is_link_or_reparse(&metadata) {
            return Err(SidecarError::from_code(
                SidecarErrorCode::ExecutableUnavailable,
            ));
        }
        verify_executable_metadata(&canonical_path, &metadata)?;
        Ok(Self { canonical_path })
    }
}

fn discover_executable(candidate: &Path) -> Result<PathBuf, SidecarError> {
    if candidate.is_absolute() || candidate.components().count() != 1 {
        return Ok(candidate.to_path_buf());
    }

    let path = env::var_os("PATH")
        .ok_or_else(|| SidecarError::from_code(SidecarErrorCode::ExecutableMissing))?;
    for directory in env::split_paths(&path) {
        #[cfg(windows)]
        {
            let extensions =
                env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
            let has_extension = candidate.extension().is_some();
            if has_extension {
                let path = directory.join(candidate);
                if path.is_file() {
                    return Ok(path);
                }
            } else {
                for extension in extensions.to_string_lossy().split(';') {
                    let path = directory.join(format!(
                        "{}{}",
                        candidate.as_os_str().to_string_lossy(),
                        extension
                    ));
                    if path.is_file() {
                        return Ok(path);
                    }
                }
            }
        }
        #[cfg(unix)]
        {
            let path = directory.join(candidate);
            if path.is_file() {
                return Ok(path);
            }
        }
    }

    Err(SidecarError::from_code(SidecarErrorCode::ExecutableMissing))
}

#[cfg(unix)]
fn verify_executable_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), SidecarError> {
    use std::os::unix::fs::MetadataExt;

    // SAFETY: geteuid has no preconditions.
    let effective_user = unsafe { libc::geteuid() };
    if metadata.uid() != effective_user && metadata.uid() != 0 {
        return Err(SidecarError::from_code(SidecarErrorCode::UnsafeExecutable));
    }
    if metadata.mode() & 0o111 == 0 {
        return Err(SidecarError::from_code(
            SidecarErrorCode::ExecutableUnavailable,
        ));
    }
    if metadata.mode() & 0o022 != 0 {
        return Err(SidecarError::from_code(SidecarErrorCode::UnsafeExecutable));
    }
    if metadata.mode() & 0o6000 != 0 {
        return Err(SidecarError::from_code(SidecarErrorCode::UnsafeExecutable));
    }
    for parent in path.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(parent)
            .map_err(|_| SidecarError::from_code(SidecarErrorCode::UnsafeExecutable))?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || (metadata.uid() != effective_user && metadata.uid() != 0)
            || metadata.mode() & 0o022 != 0
        {
            return Err(SidecarError::from_code(SidecarErrorCode::UnsafeExecutable));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn verify_executable_metadata(path: &Path, _metadata: &fs::Metadata) -> Result<(), SidecarError> {
    for component in path.ancestors() {
        let metadata = fs::symlink_metadata(component)
            .map_err(|_| SidecarError::from_code(SidecarErrorCode::UnsafeExecutable))?;
        if is_link_or_reparse(&metadata) {
            return Err(SidecarError::from_code(SidecarErrorCode::UnsafeExecutable));
        }
        windows_security::verify_no_broad_write(component)
            .map_err(|()| SidecarError::from_code(SidecarErrorCode::UnsafeExecutable))?;
    }
    Ok(())
}

async fn detect_version(
    specification: &SidecarCommand,
    executable: &VerifiedExecutable,
    provider_home: &Path,
) -> Result<Version, SidecarError> {
    let mut command = Command::new(&executable.canonical_path);
    command
        .args(&specification.version_arguments)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    copy_allowed_environment(&mut command)?;
    command
        .env(specification.home_variable, provider_home)
        .current_dir(provider_home);
    set_owner_only_child_umask(&mut command);

    let mut process = SidecarProcessGuard::new(
        spawn_grouped(command)
            .map_err(|_| SidecarError::from_code(SidecarErrorCode::SpawnFailed))?,
    );
    let mut stdout = process
        .child
        .stdout()
        .take()
        .ok_or_else(|| SidecarError::from_code(SidecarErrorCode::SpawnFailed))?;
    let result = async {
        let output = tokio::time::timeout(VERSION_TIMEOUT, read_version_output(&mut stdout))
            .await
            .map_err(|_| SidecarError::from_code(SidecarErrorCode::TimedOut))??;

        let status = poll_guard_until(
            &mut process,
            Instant::now() + VERSION_TIMEOUT,
            Duration::from_millis(10),
        )
        .await?;
        let Some(status) = status else {
            return Err(SidecarError::from_code(SidecarErrorCode::TimedOut));
        };
        if !status.success() {
            return Err(SidecarError::from_code(SidecarErrorCode::ProtocolViolation));
        }

        let output = std::str::from_utf8(&output)
            .map_err(|_| SidecarError::from_code(SidecarErrorCode::ProtocolViolation))?;
        let version = parse_version_output(output, specification.version_output)?;
        if !specification.supported_versions.matches(&version) {
            return Err(SidecarError::from_code(
                SidecarErrorCode::UnsupportedVersion,
            ));
        }
        Ok(version)
    }
    .await;

    // Version commands may spawn helpers. Always kill their process container and
    // bounded-reap the leader, including after read, size, parse, or status failures.
    process.start_kill();
    match poll_guard_until(
        &mut process,
        Instant::now() + VERSION_TIMEOUT,
        Duration::from_millis(10),
    )
    .await
    {
        Ok(Some(_)) => result,
        Ok(None) => Err(SidecarError::from_code(SidecarErrorCode::TimedOut)),
        Err(error) => Err(error),
    }
}

async fn read_version_output(stdout: &mut ChildStdout) -> Result<Vec<u8>, SidecarError> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 1_024];
    loop {
        let read = stdout
            .read(&mut buffer)
            .await
            .map_err(|_| SidecarError::from_code(SidecarErrorCode::ProtocolViolation))?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > VERSION_OUTPUT_LIMIT {
            return Err(SidecarError::from_code(SidecarErrorCode::ProtocolViolation));
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

fn parse_version_output(
    output: &str,
    format: VersionOutputFormat,
) -> Result<Version, SidecarError> {
    let parse_error = || SidecarError::from_code(SidecarErrorCode::ProtocolViolation);
    match format {
        VersionOutputFormat::ExactPrefix(prefix) => {
            let mut tokens = output.split_ascii_whitespace();
            if tokens.next() != Some(prefix) {
                return Err(parse_error());
            }
            let version = tokens.next().ok_or_else(parse_error)?;
            if tokens.next().is_some() {
                return Err(parse_error());
            }
            Version::parse(version).map_err(|_| parse_error())
        }
        VersionOutputFormat::SingleSemverToken => {
            let mut versions = output.split_ascii_whitespace().filter_map(|token| {
                let token = token.trim_matches(|character: char| {
                    matches!(character, '(' | ')' | '[' | ']' | ',' | ';')
                });
                Version::parse(token).ok()
            });
            let version = versions.next().ok_or_else(parse_error)?;
            if versions.next().is_some() {
                return Err(parse_error());
            }
            Ok(version)
        }
    }
}

/// A running JSONL sidecar. Its process wrapper and pipes remain private.
pub struct JsonlSidecar {
    process: Arc<Mutex<SidecarProcessGuard>>,
    supervisor: mpsc::Sender<SupervisorEvent>,
    writer: mpsc::Sender<Vec<u8>>,
    notifications: AsyncMutex<mpsc::Receiver<serde_json::Value>>,
    request_slots: Arc<Semaphore>,
    pipe_task_aborts: Vec<AbortHandle>,
    supervisor_task: Mutex<Option<JoinHandle<()>>>,
    state: Arc<AtomicU8>,
    stderr: Arc<Mutex<StderrCapture>>,
    limits: SidecarLimits,
    process_id: Option<u32>,
    executable_path: PathBuf,
}

impl fmt::Debug for JsonlSidecar {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JsonlSidecar")
            .field("state", &self.state.load(Ordering::Acquire))
            .field("process_id", &self.process_id)
            .field("executable_path", &"<foreground-only>")
            .finish_non_exhaustive()
    }
}

impl JsonlSidecar {
    /// Validate/create the provider home, verify the executable and version, and spawn
    /// one isolated process-group/job-owned sidecar.
    pub async fn spawn(
        specification: SidecarCommand,
        carl_data_root: impl AsRef<Path>,
        workspace: impl AsRef<Path>,
        limits: SidecarLimits,
    ) -> Result<Self, SidecarError> {
        let limits = limits.validate()?;
        validate_home_variable(specification.home_variable)?;
        prepare_provider_home(
            carl_data_root.as_ref(),
            workspace.as_ref(),
            &specification.isolated_home,
        )?;
        let executable = VerifiedExecutable::resolve(&specification.executable)?;
        detect_version(&specification, &executable, &specification.isolated_home).await?;

        let mut command = Command::new(&executable.canonical_path);
        command
            .args(&specification.arguments)
            .env_clear()
            .env(specification.home_variable, &specification.isolated_home)
            .current_dir(&specification.isolated_home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        copy_allowed_environment(&mut command)?;
        set_owner_only_child_umask(&mut command);

        let mut child = spawn_grouped(command)
            .map_err(|_| SidecarError::from_code(SidecarErrorCode::SpawnFailed))?;
        let process_id = child.id();
        let stdin = child
            .stdin()
            .take()
            .ok_or_else(|| SidecarError::from_code(SidecarErrorCode::SpawnFailed))?;
        let stdout = child
            .stdout()
            .take()
            .ok_or_else(|| SidecarError::from_code(SidecarErrorCode::SpawnFailed))?;
        let stderr_pipe = child
            .stderr()
            .take()
            .ok_or_else(|| SidecarError::from_code(SidecarErrorCode::SpawnFailed))?;

        let process = Arc::new(Mutex::new(SidecarProcessGuard::new(child)));
        let state = Arc::new(AtomicU8::new(STATE_RUNNING));
        let stderr = Arc::new(Mutex::new(StderrCapture::default()));
        let (supervisor_tx, supervisor_rx) = mpsc::channel(SUPERVISOR_CHANNEL_CAPACITY);
        let (writer_tx, writer_rx) = mpsc::channel(WRITER_CHANNEL_CAPACITY);
        let (notification_tx, notification_rx) = mpsc::channel(NOTIFICATION_CHANNEL_CAPACITY);

        let writer_task = tokio::spawn(writer_worker(stdin, writer_rx, supervisor_tx.clone()));
        let stdout_task = tokio::spawn(stdout_worker(
            stdout,
            limits.max_stdout_line_bytes,
            supervisor_tx.clone(),
        ));
        let stderr_task = tokio::spawn(stderr_worker(
            stderr_pipe,
            Arc::clone(&stderr),
            limits.max_stderr_bytes,
        ));
        let pipe_task_aborts = vec![
            writer_task.abort_handle(),
            stdout_task.abort_handle(),
            stderr_task.abort_handle(),
        ];
        let supervisor_task = tokio::spawn(supervisor_worker(SupervisorContext {
            process: Arc::clone(&process),
            events: supervisor_rx,
            notifications: notification_tx,
            state: Arc::clone(&state),
            writer_task,
            stdout_task,
            stderr_task,
            limits,
        }));

        Ok(Self {
            process,
            supervisor: supervisor_tx,
            writer: writer_tx,
            notifications: AsyncMutex::new(notification_rx),
            request_slots: Arc::new(Semaphore::new(MAX_PENDING_REQUESTS)),
            pipe_task_aborts,
            supervisor_task: Mutex::new(Some(supervisor_task)),
            state,
            stderr,
            limits,
            process_id,
            executable_path: executable.canonical_path,
        })
    }

    /// Send one bounded JSON request and await the response with the matching JSON ID.
    pub async fn request(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, SidecarError> {
        let _slot = Arc::clone(&self.request_slots)
            .acquire_owned()
            .await
            .map_err(|_| stopped_error(&self.state))?;
        if self.state.load(Ordering::Acquire) != STATE_RUNNING {
            return Err(stopped_error(&self.state));
        }
        let key = correlation_key(&request)?;
        let line = encode_line(&request, self.limits.max_stdout_line_bytes)
            .map_err(|()| SidecarError::from_code(SidecarErrorCode::ProtocolViolation))?;
        let (sender, receiver) = oneshot::channel();
        let (acknowledge, acknowledged) = oneshot::channel();
        self.supervisor
            .send(SupervisorEvent::Register {
                key: key.clone(),
                response: sender,
                acknowledge,
            })
            .await
            .map_err(|_| stopped_error(&self.state))?;
        acknowledged
            .await
            .map_err(|_| stopped_error(&self.state))??;
        let mut registration = PendingRegistration::new(self.supervisor.clone(), key);
        self.writer
            .send(line)
            .await
            .map_err(|_| stopped_error(&self.state))?;
        let response = receiver
            .await
            .unwrap_or_else(|_| Err(stopped_error(&self.state)));
        registration.disarm();
        response
    }

    /// Receive the next bounded JSONL notification (an object with a method and no ID).
    pub async fn next_notification(&self) -> Result<serde_json::Value, SidecarError> {
        let mut notifications = self.notifications.lock().await;
        notifications
            .recv()
            .await
            .ok_or_else(|| stopped_error(&self.state))
    }

    /// Close stdin, request graceful group termination, then force group/job
    /// termination and reap the leader within bounded deadlines.
    pub async fn cancel(&self) -> Result<(), SidecarError> {
        let result = if self.state.load(Ordering::Acquire) == STATE_STOPPED {
            Ok(())
        } else {
            let (complete, completion) = oneshot::channel();
            if self
                .supervisor
                .send(SupervisorEvent::Cancel { complete })
                .await
                .is_err()
            {
                Err(stopped_error(&self.state))
            } else {
                completion
                    .await
                    .unwrap_or_else(|_| Err(stopped_error(&self.state)))
            }
        };
        self.wait_for_supervisor().await;
        result
    }

    /// The canonical executable actually used for version probing and sidecar spawn.
    ///
    /// This path is intended for foreground doctor/configuration UI. A matching
    /// version is compatibility evidence, not publisher attestation.
    #[must_use]
    pub fn executable_path(&self) -> &Path {
        &self.executable_path
    }

    #[must_use]
    pub const fn process_id(&self) -> Option<u32> {
        self.process_id
    }

    /// Return a bounded diagnostic marker without returning provider stderr content.
    #[must_use]
    pub fn stderr_snapshot(&self) -> String {
        let capture = lock(&self.stderr);
        if !capture.saw_output {
            return String::new();
        }
        REDACTED_STDERR
            .get(..self.limits.max_stderr_bytes.min(REDACTED_STDERR.len()))
            .unwrap_or_default()
            .to_owned()
    }

    async fn wait_for_supervisor(&self) {
        let task = lock(&self.supervisor_task).take();
        if let Some(task) = task {
            let _ = task.await;
        }
    }
}

impl Drop for JsonlSidecar {
    fn drop(&mut self) {
        self.state.store(STATE_CANCELLING, Ordering::Release);
        lock(&self.process).start_kill();
        for task in &self.pipe_task_aborts {
            task.abort();
        }
        if let Some(task) = lock(&self.supervisor_task).as_ref() {
            task.abort();
        }
        self.state.store(STATE_STOPPED, Ordering::Release);
    }
}

type PendingRequests = HashMap<String, oneshot::Sender<Result<serde_json::Value, SidecarError>>>;

struct PendingRegistration {
    supervisor: mpsc::Sender<SupervisorEvent>,
    key: Option<String>,
}

impl PendingRegistration {
    fn new(supervisor: mpsc::Sender<SupervisorEvent>, key: String) -> Self {
        Self {
            supervisor,
            key: Some(key),
        }
    }

    fn disarm(&mut self) {
        self.key = None;
    }
}

impl Drop for PendingRegistration {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            let _ = self.supervisor.try_send(SupervisorEvent::Abandon { key });
        }
    }
}

enum SupervisorEvent {
    Register {
        key: String,
        response: oneshot::Sender<Result<serde_json::Value, SidecarError>>,
        acknowledge: oneshot::Sender<Result<(), SidecarError>>,
    },
    Abandon {
        key: String,
    },
    Incoming(serde_json::Value),
    Failure(SidecarErrorCode),
    Cancel {
        complete: oneshot::Sender<Result<(), SidecarError>>,
    },
}

async fn writer_worker(
    mut stdin: ChildStdin,
    mut lines: mpsc::Receiver<Vec<u8>>,
    supervisor: mpsc::Sender<SupervisorEvent>,
) {
    while let Some(line) = lines.recv().await {
        if stdin.write_all(&line).await.is_err() {
            let _ = supervisor
                .send(SupervisorEvent::Failure(SidecarErrorCode::SidecarExited))
                .await;
            return;
        }
    }
}

async fn stdout_worker(
    stdout: ChildStdout,
    maximum_line_bytes: usize,
    supervisor: mpsc::Sender<SupervisorEvent>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let line = match read_bounded_line(&mut reader, maximum_line_bytes).await {
            Ok(Some(line)) => line,
            Ok(None) => {
                let _ = supervisor
                    .send(SupervisorEvent::Failure(SidecarErrorCode::SidecarExited))
                    .await;
                return;
            }
            Err(_) => {
                let _ = supervisor
                    .send(SupervisorEvent::Failure(
                        SidecarErrorCode::ProtocolViolation,
                    ))
                    .await;
                return;
            }
        };
        let response: serde_json::Value = match serde_json::from_slice(&line) {
            Ok(response) => response,
            Err(_) => {
                let _ = supervisor
                    .send(SupervisorEvent::Failure(
                        SidecarErrorCode::ProtocolViolation,
                    ))
                    .await;
                return;
            }
        };
        if supervisor
            .send(SupervisorEvent::Incoming(response))
            .await
            .is_err()
        {
            return;
        }
    }
}

#[derive(Default)]
struct StderrCapture {
    saw_output: bool,
    observed_bytes: usize,
}

async fn stderr_worker(
    mut stderr: ChildStderr,
    capture: Arc<Mutex<StderrCapture>>,
    maximum_bytes: usize,
) {
    let mut buffer = [0_u8; 4 * 1_024];
    loop {
        let read = match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => read,
        };
        let mut capture = lock(&capture);
        capture.saw_output = true;
        capture.observed_bytes = capture
            .observed_bytes
            .saturating_add(read)
            .min(maximum_bytes);
    }
}

struct SupervisorContext {
    process: Arc<Mutex<SidecarProcessGuard>>,
    events: mpsc::Receiver<SupervisorEvent>,
    notifications: mpsc::Sender<serde_json::Value>,
    state: Arc<AtomicU8>,
    writer_task: JoinHandle<()>,
    stdout_task: JoinHandle<()>,
    stderr_task: JoinHandle<()>,
    limits: SidecarLimits,
}

async fn supervisor_worker(context: SupervisorContext) {
    let SupervisorContext {
        process,
        mut events,
        notifications,
        state,
        mut writer_task,
        mut stdout_task,
        mut stderr_task,
        limits,
    } = context;
    let mut pending =
        HashMap::<String, oneshot::Sender<Result<serde_json::Value, SidecarError>>>::new();
    loop {
        let event = tokio::time::timeout(limits.process_poll_interval, events.recv()).await;
        match event {
            Ok(Some(SupervisorEvent::Register {
                key,
                response,
                acknowledge,
            })) => match pending.entry(key) {
                Entry::Vacant(entry) if state.load(Ordering::Acquire) == STATE_RUNNING => {
                    entry.insert(response);
                    let _ = acknowledge.send(Ok(()));
                }
                Entry::Vacant(_) => {
                    let _ = acknowledge.send(Err(stopped_error(&state)));
                }
                Entry::Occupied(_) => {
                    let _ = acknowledge.send(Err(SidecarError::from_code(
                        SidecarErrorCode::DuplicateRequestId,
                    )));
                }
            },
            Ok(Some(SupervisorEvent::Abandon { key })) => {
                pending.remove(&key);
            }
            Ok(Some(SupervisorEvent::Incoming(message))) => {
                if let Ok(key) = correlation_key(&message) {
                    let Some(response) = pending.remove(&key) else {
                        shutdown_supervisor(
                            &process,
                            &mut pending,
                            &state,
                            [&mut writer_task, &mut stdout_task, &mut stderr_task],
                            limits,
                            SidecarErrorCode::ProtocolViolation,
                        )
                        .await;
                        return;
                    };
                    let _ = response.send(Ok(message));
                } else if is_notification(&message) {
                    if notifications.try_send(message).is_err() {
                        shutdown_supervisor(
                            &process,
                            &mut pending,
                            &state,
                            [&mut writer_task, &mut stdout_task, &mut stderr_task],
                            limits,
                            SidecarErrorCode::ProtocolViolation,
                        )
                        .await;
                        return;
                    }
                } else {
                    shutdown_supervisor(
                        &process,
                        &mut pending,
                        &state,
                        [&mut writer_task, &mut stdout_task, &mut stderr_task],
                        limits,
                        SidecarErrorCode::ProtocolViolation,
                    )
                    .await;
                    return;
                }
            }
            Ok(Some(SupervisorEvent::Failure(code))) => {
                shutdown_supervisor(
                    &process,
                    &mut pending,
                    &state,
                    [&mut writer_task, &mut stdout_task, &mut stderr_task],
                    limits,
                    code,
                )
                .await;
                return;
            }
            Ok(Some(SupervisorEvent::Cancel { complete })) => {
                state.store(STATE_CANCELLING, Ordering::Release);
                writer_task.abort();
                let _ = (&mut writer_task).await;
                let result = terminate_process(&process, limits).await;
                stdout_task.abort();
                stderr_task.abort();
                let _ = (&mut stdout_task).await;
                let _ = (&mut stderr_task).await;
                fail_pending(&mut pending, SidecarErrorCode::Cancelled);
                state.store(STATE_STOPPED, Ordering::Release);
                let _ = complete.send(result);
                return;
            }
            Ok(None) => {
                shutdown_supervisor(
                    &process,
                    &mut pending,
                    &state,
                    [&mut writer_task, &mut stdout_task, &mut stderr_task],
                    limits,
                    SidecarErrorCode::SidecarExited,
                )
                .await;
                return;
            }
            Err(_) => {}
        }

        pending.retain(|_, response| !response.is_closed());
        let exit_result = {
            let mut process = lock(&process);
            match process.try_wait() {
                Ok(Some(_)) => {
                    process.start_kill();
                    Some(SidecarErrorCode::SidecarExited)
                }
                Ok(None) => None,
                Err(()) => {
                    process.start_kill();
                    Some(SidecarErrorCode::SidecarExited)
                }
            }
        };
        if let Some(code) = exit_result {
            shutdown_supervisor(
                &process,
                &mut pending,
                &state,
                [&mut writer_task, &mut stdout_task, &mut stderr_task],
                limits,
                code,
            )
            .await;
            return;
        }
    }
}

fn correlation_key(value: &serde_json::Value) -> Result<String, SidecarError> {
    let object = value
        .as_object()
        .ok_or_else(|| SidecarError::from_code(SidecarErrorCode::ProtocolViolation))?;
    let id = object
        .get("id")
        .ok_or_else(|| SidecarError::from_code(SidecarErrorCode::ProtocolViolation))?;
    if !id.is_string() && id.as_i64().is_none() {
        return Err(SidecarError::from_code(SidecarErrorCode::ProtocolViolation));
    }
    serde_json::to_string(id)
        .map_err(|_| SidecarError::from_code(SidecarErrorCode::ProtocolViolation))
}

fn is_notification(value: &serde_json::Value) -> bool {
    value.as_object().is_some_and(|object| {
        !object.contains_key("id")
            && object
                .get("method")
                .is_some_and(serde_json::Value::is_string)
    })
}

async fn shutdown_supervisor(
    process: &Arc<Mutex<SidecarProcessGuard>>,
    pending: &mut PendingRequests,
    state: &Arc<AtomicU8>,
    tasks: [&mut JoinHandle<()>; 3],
    limits: SidecarLimits,
    code: SidecarErrorCode,
) {
    let [writer_task, stdout_task, stderr_task] = tasks;
    state.store(STATE_STOPPED, Ordering::Release);
    writer_task.abort();
    lock(process).start_kill();
    stdout_task.abort();
    stderr_task.abort();
    let _ = writer_task.await;
    let _ = stdout_task.await;
    let _ = stderr_task.await;
    fail_pending(pending, code);
    let _ = force_kill_and_reap(process, limits).await;
}

fn fail_pending(pending: &mut PendingRequests, code: SidecarErrorCode) {
    let requests = std::mem::take(pending);
    for sender in requests.into_values() {
        let _ = sender.send(Err(SidecarError::from_code(code)));
    }
}

fn stopped_error(state: &AtomicU8) -> SidecarError {
    let code = if state.load(Ordering::Acquire) == STATE_CANCELLING {
        SidecarErrorCode::Cancelled
    } else {
        SidecarErrorCode::SidecarExited
    };
    SidecarError::from_code(code)
}

struct SidecarProcessGuard {
    child: Box<dyn ChildWrapper>,
}

impl SidecarProcessGuard {
    fn new(child: Box<dyn ChildWrapper>) -> Self {
        Self { child }
    }

    fn start_kill(&mut self) {
        let _ = self.child.start_kill();
    }

    fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>, ()> {
        self.child.try_wait().map_err(|_| ())
    }

    #[cfg(unix)]
    fn signal(&self, signal: i32) -> Result<(), ()> {
        self.child.signal(signal).map_err(|_| ())
    }
}

impl Drop for SidecarProcessGuard {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

// Compile-time ownership invariant: the private guard is constructed from, and
// permanently owns, the group-aware trait object rather than a bare Tokio child.
const _: fn(Box<dyn ChildWrapper>) -> SidecarProcessGuard = SidecarProcessGuard::new;

async fn poll_guard_until(
    process: &mut SidecarProcessGuard,
    deadline: Instant,
    poll_interval: Duration,
) -> Result<Option<std::process::ExitStatus>, SidecarError> {
    loop {
        match process.try_wait() {
            Ok(Some(status)) => {
                process.start_kill();
                return Ok(Some(status));
            }
            Ok(None) => {}
            Err(()) => {
                process.start_kill();
                return Err(SidecarError::from_code(SidecarErrorCode::SpawnFailed));
            }
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(poll_interval).await;
    }
}

async fn poll_shared_guard_until(
    process: &Arc<Mutex<SidecarProcessGuard>>,
    deadline: Instant,
    poll_interval: Duration,
) -> Result<bool, SidecarError> {
    loop {
        let status = {
            let mut process = lock(process);
            match process.try_wait() {
                Ok(Some(_)) => {
                    process.start_kill();
                    Some(Ok(true))
                }
                Ok(None) => None,
                Err(()) => {
                    process.start_kill();
                    Some(Err(SidecarError::from_code(SidecarErrorCode::SpawnFailed)))
                }
            }
        };
        if let Some(status) = status {
            return status;
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(poll_interval).await;
    }
}

async fn terminate_process(
    process: &Arc<Mutex<SidecarProcessGuard>>,
    limits: SidecarLimits,
) -> Result<(), SidecarError> {
    #[cfg(unix)]
    {
        let process = lock(process);
        let _ = process.signal(libc::SIGTERM);
    }

    let graceful_exit = poll_shared_guard_until(
        process,
        Instant::now() + limits.graceful_shutdown_timeout,
        limits.process_poll_interval,
    )
    .await?;
    if !graceful_exit {
        return force_kill_and_reap(process, limits).await;
    }
    lock(process).start_kill();
    Ok(())
}

async fn force_kill_and_reap(
    process: &Arc<Mutex<SidecarProcessGuard>>,
    limits: SidecarLimits,
) -> Result<(), SidecarError> {
    lock(process).start_kill();
    let reaped = poll_shared_guard_until(
        process,
        Instant::now() + limits.forced_shutdown_timeout,
        limits.process_poll_interval,
    )
    .await?;
    lock(process).start_kill();
    if reaped {
        Ok(())
    } else {
        Err(SidecarError::from_code(SidecarErrorCode::TimedOut))
    }
}

fn spawn_grouped(command: Command) -> std::io::Result<Box<dyn ChildWrapper>> {
    let mut command = CommandWrap::from(command);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    command.spawn()
}

fn copy_allowed_environment(command: &mut Command) -> Result<(), SidecarError> {
    for name in ALLOWED_ENVIRONMENT {
        if *name == "PATH" {
            continue;
        }
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("PATH", minimal_system_path()?);
    Ok(())
}

#[cfg(unix)]
fn minimal_system_path() -> Result<OsString, SidecarError> {
    Ok(OsString::from("/usr/bin:/bin:/usr/sbin:/sbin"))
}

#[cfg(windows)]
fn minimal_system_path() -> Result<OsString, SidecarError> {
    use std::os::windows::ffi::OsStringExt;

    use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;

    // Windows' documented maximum extended path length is 32,767 UTF-16 code units.
    let mut buffer = vec![0_u16; 32_768];
    // SAFETY: `buffer` is writable for its full declared length.
    let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    let length = usize::try_from(length)
        .map_err(|_| SidecarError::from_code(SidecarErrorCode::SpawnFailed))?;
    if length == 0 || length >= buffer.len() {
        return Err(SidecarError::from_code(SidecarErrorCode::SpawnFailed));
    }
    buffer.truncate(length);
    let windows = PathBuf::from(OsString::from_wide(&buffer));
    env::join_paths([windows.join("System32"), windows])
        .map_err(|_| SidecarError::from_code(SidecarErrorCode::SpawnFailed))
}

fn validate_home_variable(variable: &str) -> Result<(), SidecarError> {
    let valid = variable.ends_with("_HOME")
        && !ALLOWED_ENVIRONMENT.contains(&variable)
        && variable.bytes().enumerate().all(|(index, byte)| {
            byte == b'_' || byte.is_ascii_uppercase() || (index > 0 && byte.is_ascii_digit())
        });
    if valid {
        Ok(())
    } else {
        Err(SidecarError::from_code(
            SidecarErrorCode::InvalidConfiguration,
        ))
    }
}

#[cfg(unix)]
fn set_owner_only_child_umask(command: &mut Command) {
    // SAFETY: `umask` is async-signal-safe and this closure performs no allocation.
    unsafe {
        command.pre_exec(|| {
            libc::umask(0o077);
            Ok(())
        });
    }
}

#[cfg(windows)]
fn set_owner_only_child_umask(_command: &mut Command) {}

fn prepare_provider_home(
    data_root: &Path,
    workspace: &Path,
    provider_home: &Path,
) -> Result<(), SidecarError> {
    if !data_root.is_absolute() || !workspace.is_absolute() || !provider_home.is_absolute() {
        return Err(SidecarError::from_code(
            SidecarErrorCode::InvalidProviderHome,
        ));
    }
    let relative = provider_home
        .strip_prefix(data_root)
        .map_err(|_| SidecarError::from_code(SidecarErrorCode::InvalidProviderHome))?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(SidecarError::from_code(
            SidecarErrorCode::InvalidProviderHome,
        ));
    }

    let root_metadata = fs::symlink_metadata(data_root)
        .map_err(|_| SidecarError::from_code(SidecarErrorCode::InvalidProviderHome))?;
    if !root_metadata.is_dir() || is_link_or_reparse(&root_metadata) {
        return Err(SidecarError::from_code(
            SidecarErrorCode::InvalidProviderHome,
        ));
    }
    let workspace_metadata = fs::symlink_metadata(workspace)
        .map_err(|_| SidecarError::from_code(SidecarErrorCode::InvalidProviderHome))?;
    if !workspace_metadata.is_dir() || is_link_or_reparse(&workspace_metadata) {
        return Err(SidecarError::from_code(
            SidecarErrorCode::InvalidProviderHome,
        ));
    }

    let canonical_root = fs::canonicalize(data_root)
        .map_err(|_| SidecarError::from_code(SidecarErrorCode::InvalidProviderHome))?;
    let canonical_workspace = fs::canonicalize(workspace)
        .map_err(|_| SidecarError::from_code(SidecarErrorCode::InvalidProviderHome))?;
    let intended_home = canonical_root.join(relative);
    if intended_home.starts_with(&canonical_workspace) || provider_home.starts_with(workspace) {
        return Err(SidecarError::from_code(
            SidecarErrorCode::InvalidProviderHome,
        ));
    }

    create_provider_home(data_root, relative)
}

#[cfg(unix)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.file_type().is_symlink()
}

#[cfg(unix)]
fn create_provider_home(data_root: &Path, relative: &Path) -> Result<(), SidecarError> {
    use std::ffi::CString;
    use std::fs::{File, OpenOptions};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    fn invalid_home() -> SidecarError {
        SidecarError::from_code(SidecarErrorCode::InvalidProviderHome)
    }

    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(data_root)
        .map_err(|_| invalid_home())?;
    verify_owned_directory(&directory)?;

    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(invalid_home());
        };
        let component = CString::new(component.as_bytes()).map_err(|_| invalid_home())?;
        // SAFETY: both the live directory fd and NUL-terminated component pointer are valid.
        let created = unsafe { libc::mkdirat(directory.as_raw_fd(), component.as_ptr(), 0o700) };
        if created != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(invalid_home());
        }
        // SAFETY: openat receives a live directory fd and a valid component-only C string.
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                component.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(invalid_home());
        }
        // SAFETY: `descriptor` was just returned by openat and ownership transfers to File.
        let next = unsafe { File::from_raw_fd(descriptor) };
        verify_owned_directory(&next)?;
        // SAFETY: `next` owns a live directory descriptor.
        if unsafe { libc::fchmod(next.as_raw_fd(), 0o700) } != 0 {
            return Err(invalid_home());
        }
        directory = next;
    }
    Ok(())
}

#[cfg(unix)]
fn verify_owned_directory(directory: &std::fs::File) -> Result<(), SidecarError> {
    use std::mem::MaybeUninit;
    use std::os::fd::AsRawFd;

    let mut status = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the descriptor is live and `status` points to writable stat storage.
    if unsafe { libc::fstat(directory.as_raw_fd(), status.as_mut_ptr()) } != 0 {
        return Err(SidecarError::from_code(
            SidecarErrorCode::InvalidProviderHome,
        ));
    }
    // SAFETY: fstat succeeded and initialized the structure.
    let status = unsafe { status.assume_init() };
    // SAFETY: geteuid has no preconditions.
    let effective_user = unsafe { libc::geteuid() };
    if status.st_uid != effective_user
        || status.st_mode & libc::S_IFMT != libc::S_IFDIR
        || status.st_mode & 0o022 != 0
    {
        return Err(SidecarError::from_code(
            SidecarErrorCode::InvalidProviderHome,
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn create_provider_home(data_root: &Path, relative: &Path) -> Result<(), SidecarError> {
    let mut path = data_root.to_path_buf();
    windows_security::verify_private_directory(&path)
        .map_err(|()| SidecarError::from_code(SidecarErrorCode::InvalidProviderHome))?;
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(SidecarError::from_code(
                SidecarErrorCode::InvalidProviderHome,
            ));
        };
        path.push(component);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                return Err(SidecarError::from_code(
                    SidecarErrorCode::InvalidProviderHome,
                ));
            }
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| SidecarError::from_code(SidecarErrorCode::InvalidProviderHome))?;
        if !metadata.is_dir() || is_link_or_reparse(&metadata) {
            return Err(SidecarError::from_code(
                SidecarErrorCode::InvalidProviderHome,
            ));
        }
        windows_security::verify_private_directory(&path)
            .map_err(|()| SidecarError::from_code(SidecarErrorCode::InvalidProviderHome))?;
    }
    Ok(())
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(windows)]
mod windows_security {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        ACE_HEADER, ACE_INHERITED_OBJECT_TYPE_PRESENT, ACE_OBJECT_TYPE_PRESENT, ACL,
        DACL_SECURITY_INFORMATION, GetAce, GetLengthSid, IsValidSid, IsWellKnownSid,
        PSECURITY_DESCRIPTOR, PSID, WinAnonymousSid, WinAuthenticatedUserSid, WinBuiltinGuestsSid,
        WinBuiltinUsersSid, WinWorldSid,
    };
    use windows_sys::Win32::System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
        ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_ALLOWED_COMPOUND_ACE_TYPE,
        ACCESS_ALLOWED_OBJECT_ACE_TYPE,
    };

    const BROAD_WRITE_MASK: u32 =
        0x1000_0000 | 0x4000_0000 | 0x0001_0000 | 0x0004_0000 | 0x0008_0000 | 0x116;

    pub(super) fn verify_no_broad_write(path: &Path) -> Result<(), ()> {
        verify_dacl(path, false)
    }

    pub(super) fn verify_private_directory(path: &Path) -> Result<(), ()> {
        verify_dacl(path, true)
    }

    fn verify_dacl(path: &Path, reject_any_broad_access: bool) -> Result<(), ()> {
        let mut path: Vec<u16> = path.as_os_str().encode_wide().collect();
        path.push(0);
        let mut dacl = ptr::null_mut::<ACL>();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: all output pointers are valid and `path` is NUL-terminated for this call.
        let result = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if result != 0 || dacl.is_null() || descriptor.is_null() {
            return Err(());
        }

        // SAFETY: the security API returned live descriptor/DACL pointers.
        let is_private = unsafe {
            let result = dacl_has_no_broad_allow(dacl, reject_any_broad_access);
            let _ = LocalFree(descriptor);
            result
        };
        is_private.then_some(()).ok_or(())
    }

    unsafe fn dacl_has_no_broad_allow(dacl: *mut ACL, reject_any: bool) -> bool {
        // SAFETY: caller validated the DACL pointer returned by Windows.
        let ace_count = unsafe { (*dacl).AceCount };
        for index in 0..u32::from(ace_count) {
            let mut ace = ptr::null_mut();
            // SAFETY: DACL is live and `ace` is a valid output pointer.
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 || ace.is_null() {
                return false;
            }
            // SAFETY: GetAce returned a pointer to at least an ACE header.
            let header = unsafe { &*(ace.cast::<ACE_HEADER>()) };
            let ace_size = usize::from(header.AceSize);
            let (mask, sid, remaining) = match u32::from(header.AceType) {
                ACCESS_ALLOWED_ACE_TYPE | ACCESS_ALLOWED_CALLBACK_ACE_TYPE => {
                    let sid_offset = 8;
                    if ace_size < sid_offset + 8 {
                        return false;
                    }
                    // SAFETY: the checked ACE size contains header, mask, and SID prefix.
                    let mask = unsafe { *ace.cast::<u8>().add(4).cast::<u32>() };
                    // SAFETY: same size check; standard allow ACE SID starts after the mask.
                    let sid = unsafe { ace.cast::<u8>().add(8) };
                    (mask, sid, ace_size - sid_offset)
                }
                ACCESS_ALLOWED_OBJECT_ACE_TYPE | ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE => {
                    if ace_size < 20 {
                        return false;
                    }
                    // SAFETY: object allow ACE has mask and flags at fixed aligned offsets.
                    let mask = unsafe { *ace.cast::<u8>().add(4).cast::<u32>() };
                    // SAFETY: same checked fixed layout.
                    let flags = unsafe { *ace.cast::<u8>().add(8).cast::<u32>() };
                    let mut sid_offset = 12;
                    if flags & ACE_OBJECT_TYPE_PRESENT != 0 {
                        sid_offset += 16;
                    }
                    if flags & ACE_INHERITED_OBJECT_TYPE_PRESENT != 0 {
                        sid_offset += 16;
                    }
                    if sid_offset + 8 > ace_size {
                        return false;
                    }
                    // SAFETY: variable offset was bounded by the ACE's declared size.
                    let sid = unsafe { ace.cast::<u8>().add(sid_offset) };
                    (mask, sid, ace_size - sid_offset)
                }
                ACCESS_ALLOWED_COMPOUND_ACE_TYPE => return false,
                _ => continue,
            };
            // SAFETY: the SID pointer comes from a validated allow ACE layout.
            if unsafe { is_broad_sid(sid.cast_mut().cast(), remaining) }
                && (reject_any || mask & BROAD_WRITE_MASK != 0)
            {
                return false;
            }
        }
        true
    }

    unsafe fn is_broad_sid(sid: PSID, remaining: usize) -> bool {
        // SAFETY: caller bounded the SID header within the ACE.
        if unsafe { IsValidSid(sid) } == 0 {
            return true;
        }
        // SAFETY: IsValidSid succeeded.
        let length = usize::try_from(unsafe { GetLengthSid(sid) }).unwrap_or(usize::MAX);
        if length > remaining {
            return true;
        }

        [
            WinWorldSid,
            WinAuthenticatedUserSid,
            WinBuiltinUsersSid,
            WinBuiltinGuestsSid,
            WinAnonymousSid,
        ]
        .into_iter()
        // SAFETY: `sid` passed IsValidSid and its full length fits this ACE.
        .any(|kind| unsafe { IsWellKnownSid(sid, kind) != 0 })
    }
}
