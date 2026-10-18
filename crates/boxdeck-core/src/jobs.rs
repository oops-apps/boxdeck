//! Job registry (plan §4.2): every Vagrant action runs as a job with queueing, per-machine and
//! project-wide locks, a global cap, streamed output with a ring buffer, timeouts and cancel.
//! The same registry serves UI, tray and MCP jobs in one process.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::time::Instant;

use crate::error::{Error, Result};
use crate::process::{self, CommandSpec};

pub type JobId = u64;

/// Who started a job; shown as a badge in Activity and recorded for agents.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "kind", content = "client", rename_all = "snake_case")]
#[ts(export)]
pub enum Origin {
    Ui,
    Tray,
    /// An MCP client, identified by its token name (plan §4.8).
    Mcp(String),
}

/// What a job locks while it runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "scope", rename_all = "snake_case")]
#[ts(export)]
pub enum Scope {
    /// One machine; waits for other jobs on it and for project-wide jobs.
    Machine { project: PathBuf, name: String },
    /// Every machine in a project (actions without a machine name).
    Project { project: PathBuf },
    /// Box mutations; one at a time.
    Boxes,
    /// No lock (e.g. `global-status --prune`).
    Unlocked,
}

impl Scope {
    fn conflicts_with(&self, other: &Scope) -> bool {
        use Scope::*;
        match (self, other) {
            (
                Machine {
                    project: a,
                    name: x,
                },
                Machine {
                    project: b,
                    name: y,
                },
            ) => a == b && x == y,
            (Machine { project: a, .. }, Project { project: b })
            | (Project { project: a }, Machine { project: b, .. })
            | (Project { project: a }, Project { project: b }) => a == b,
            (Boxes, Boxes) => true,
            _ => false,
        }
    }
}

/// A job to run.
#[derive(Debug, Clone)]
pub struct JobRequest {
    /// Short label, e.g. `up web`.
    pub label: String,
    pub origin: Origin,
    pub scope: Scope,
    pub spec: CommandSpec,
    /// Killed (tree included) after this long.
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobState {
    Queued,
    Running,
    /// An interrupt was sent; waiting for Vagrant to clean up.
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

impl JobState {
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }

    fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}

/// Snapshot of a job, as shown in the UI and returned to agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct JobInfo {
    #[ts(type = "number")]
    pub id: JobId,
    pub label: String,
    pub origin: Origin,
    pub scope: Scope,
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub state: JobState,
    pub exit_code: Option<i32>,
    /// Why the job couldn't start, when it couldn't.
    pub error: Option<String>,
    /// Milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub queued_at: u64,
    #[ts(type = "number | null")]
    pub started_at: Option<u64>,
    #[ts(type = "number | null")]
    pub finished_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct LogLine {
    /// Per-job sequence number, so a reconnecting viewer can resume without duplicates.
    #[ts(type = "number")]
    pub seq: u64,
    pub stream: Stream,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum JobEvent {
    Updated {
        job: JobInfo,
    },
    Output {
        #[ts(type = "number")]
        id: JobId,
        line: LogLine,
    },
}

#[derive(Debug, Clone)]
pub struct JobsConfig {
    pub global_cap: usize,
    /// Output lines kept per job.
    pub ring_lines: usize,
    /// Time Vagrant gets to clean up after an interrupt before the tree is killed.
    pub grace: Duration,
    /// Finished jobs kept in memory.
    pub keep_finished: usize,
    /// Runtime that runs jobs. Needed when jobs are submitted from threads without one, such
    /// as the GUI main thread where Tauri runs synchronous commands and tray handlers.
    pub runtime: Option<tokio::runtime::Handle>,
}

impl Default for JobsConfig {
    fn default() -> Self {
        Self {
            global_cap: 3,
            ring_lines: 10_000,
            grace: Duration::from_secs(30),
            keep_finished: 200,
            runtime: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancelMode {
    None,
    Graceful,
    Force,
}

struct Entry {
    info: JobInfo,
    log: VecDeque<LogLine>,
    next_seq: u64,
    request: Option<JobRequest>,
    cancel: Option<watch::Sender<CancelMode>>,
    done: watch::Sender<bool>,
}

struct State {
    next_id: JobId,
    jobs: BTreeMap<JobId, Entry>,
    queue: VecDeque<JobId>,
    /// Projects a status query is reading right now; jobs there wait (see [`ProjectHold`]).
    holds: BTreeMap<PathBuf, usize>,
}

struct Inner {
    state: Mutex<State>,
    events: broadcast::Sender<JobEvent>,
    config: JobsConfig,
    /// Separate from `config` so Settings can change it while jobs run.
    global_cap: AtomicUsize,
    runtime: Option<tokio::runtime::Handle>,
}

/// The job registry. Cheap to clone; all clones share state.
#[derive(Clone)]
pub struct Jobs {
    inner: Arc<Inner>,
}

impl Jobs {
    pub fn new(config: JobsConfig) -> Self {
        let (events, _) = broadcast::channel(4096);
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    next_id: 1,
                    jobs: BTreeMap::new(),
                    queue: VecDeque::new(),
                    holds: BTreeMap::new(),
                }),
                events,
                global_cap: AtomicUsize::new(config.global_cap.max(1)),
                runtime: config
                    .runtime
                    .clone()
                    .or_else(|| tokio::runtime::Handle::try_current().ok()),
                config,
            }),
        }
    }

    /// Queues a job and starts it as soon as its locks and the global cap allow. Callable from
    /// any thread; jobs run on the configured runtime (or the caller's, if it has one).
    pub fn submit(&self, request: JobRequest) -> JobId {
        let id = {
            let mut state = self.inner.lock();
            let id = state.next_id;
            state.next_id += 1;
            let info = JobInfo {
                id,
                label: request.label.clone(),
                origin: request.origin.clone(),
                scope: request.scope.clone(),
                argv: std::iter::once(request.spec.program.as_os_str())
                    .chain(request.spec.args.iter().map(|a| a.as_os_str()))
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect(),
                cwd: request.spec.cwd.clone(),
                state: JobState::Queued,
                exit_code: None,
                error: None,
                queued_at: now_ms(),
                started_at: None,
                finished_at: None,
            };
            self.inner.emit_updated(&info);
            state.jobs.insert(
                id,
                Entry {
                    info,
                    log: VecDeque::new(),
                    next_seq: 0,
                    request: Some(request),
                    cancel: None,
                    done: watch::channel(false).0,
                },
            );
            state.queue.push_back(id);
            id
        };
        Inner::schedule(&self.inner);
        id
    }

    /// Cancels a job. Queued jobs are dropped; running ones get an interrupt so Vagrant can clean
    /// up, then a tree kill after the grace period. `force` kills the tree right away.
    pub fn cancel(&self, id: JobId, force: bool) -> Result<()> {
        let mut state = self.inner.lock();
        let entry = state
            .jobs
            .get_mut(&id)
            .ok_or_else(|| Error::invalid("job", format!("no job {id}")))?;
        match entry.info.state {
            JobState::Queued => {
                entry.request = None;
                let info = finish_entry(entry, JobState::Cancelled, None, None);
                state.queue.retain(|q| *q != id);
                self.inner.emit_updated(&info);
                Ok(())
            }
            JobState::Running | JobState::Cancelling => {
                let mode = if force {
                    CancelMode::Force
                } else {
                    CancelMode::Graceful
                };
                if let Some(tx) = &entry.cancel {
                    tx.send_replace(mode);
                }
                if entry.info.state == JobState::Running {
                    entry.info.state = JobState::Cancelling;
                    self.inner.emit_updated(&entry.info);
                }
                Ok(())
            }
            _ => Err(Error::invalid("job", format!("job {id} already finished"))),
        }
    }

    /// Changes the global cap; queued jobs start if it went up.
    pub fn set_global_cap(&self, cap: usize) {
        self.inner.global_cap.store(cap.max(1), Ordering::Relaxed);
        Inner::schedule(&self.inner);
    }

    /// Reserves a project for a read-only Vagrant query such as `vagrant status`.
    ///
    /// Vagrant briefly locks each machine even to read its state, and an action that finds its
    /// machine locked fails with `MachineLocked`. So queries and jobs on one project must never
    /// overlap: this returns `None` while a job there is queued or running, and while the hold
    /// lives, new jobs on the project wait.
    pub fn try_hold_project(&self, dir: &Path) -> Option<ProjectHold> {
        let mut state = self.inner.lock();
        let busy = state.jobs.values().any(|e| {
            !e.info.state.is_finished()
                && match &e.info.scope {
                    Scope::Machine { project, .. } | Scope::Project { project } => project == dir,
                    _ => false,
                }
        });
        if busy {
            return None;
        }
        *state.holds.entry(dir.to_owned()).or_default() += 1;
        Some(ProjectHold {
            inner: self.inner.clone(),
            dir: dir.to_owned(),
        })
    }

    /// Whether a job is queued or running in this project.
    pub fn is_busy(&self, dir: &Path) -> bool {
        self.try_hold_project(dir).is_none()
    }

    pub fn get(&self, id: JobId) -> Option<JobInfo> {
        self.inner.lock().jobs.get(&id).map(|e| e.info.clone())
    }

    /// All known jobs, newest first.
    pub fn list(&self) -> Vec<JobInfo> {
        self.inner
            .lock()
            .jobs
            .values()
            .rev()
            .map(|e| e.info.clone())
            .collect()
    }

    /// The last `tail` buffered output lines of a job.
    pub fn log(&self, id: JobId, tail: usize) -> Option<Vec<LogLine>> {
        let state = self.inner.lock();
        let log = &state.jobs.get(&id)?.log;
        Some(
            log.iter()
                .skip(log.len().saturating_sub(tail))
                .cloned()
                .collect(),
        )
    }

    /// Live job updates and output lines. Slow receivers may miss lines; use [`Self::log`] to
    /// catch up (sequence numbers make that safe).
    pub fn subscribe(&self) -> broadcast::Receiver<JobEvent> {
        self.inner.events.subscribe()
    }

    /// Waits until the job finishes; `None` for an unknown id.
    pub async fn wait(&self, id: JobId) -> Option<JobInfo> {
        let mut done = self.inner.lock().jobs.get(&id)?.done.subscribe();
        // An Err only means the entry was pruned, which happens after it finished.
        let _ = done.wait_for(|d| *d).await;
        self.get(id)
    }
}

/// Releases its project when dropped; queued jobs there can start then.
pub struct ProjectHold {
    inner: Arc<Inner>,
    dir: PathBuf,
}

impl Drop for ProjectHold {
    fn drop(&mut self) {
        {
            let mut state = self.inner.lock();
            if let Some(n) = state.holds.get_mut(&self.dir) {
                *n -= 1;
                if *n == 0 {
                    state.holds.remove(&self.dir);
                }
            }
        }
        Inner::schedule(&self.inner);
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic while holding the lock leaves plain data behind; keep serving it.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn emit_updated(&self, info: &JobInfo) {
        let _ = self.events.send(JobEvent::Updated { job: info.clone() });
    }

    /// Starts every queued job whose locks are free, in queue order. A queued job also blocks
    /// later queued jobs that conflict with it, so jobs on one machine run in submission order.
    fn schedule(inner: &Arc<Inner>) {
        let mut state = inner.lock();
        let mut active: Vec<Scope> = state
            .jobs
            .values()
            .filter(|e| e.info.state.is_active())
            .map(|e| e.info.scope.clone())
            .collect();
        // Held projects block jobs without counting toward the global cap.
        let mut blocked: Vec<Scope> = state
            .holds
            .keys()
            .map(|p| Scope::Project { project: p.clone() })
            .collect();
        let mut to_start = Vec::new();
        for id in state.queue.iter().copied() {
            let scope = &state.jobs[&id].info.scope;
            let busy = active
                .iter()
                .chain(&blocked)
                .any(|s| s.conflicts_with(scope));
            if busy || active.len() >= inner.global_cap.load(Ordering::Relaxed) {
                blocked.push(scope.clone());
            } else {
                active.push(scope.clone());
                to_start.push(id);
            }
        }
        state.queue.retain(|id| !to_start.contains(id));
        let runtime = tokio::runtime::Handle::try_current()
            .ok()
            .or_else(|| inner.runtime.clone());
        for id in to_start {
            let entry = state.jobs.get_mut(&id).expect("queued job exists");
            let request = entry.request.take().expect("queued job has a request");
            let Some(runtime) = &runtime else {
                // Fail the job rather than panic (a panic here aborts the whole app).
                let info = finish_entry(
                    entry,
                    JobState::Failed,
                    None,
                    Some("internal error: no async runtime to run jobs on".into()),
                );
                inner.emit_updated(&info);
                continue;
            };
            let (cancel_tx, cancel_rx) = watch::channel(CancelMode::None);
            entry.cancel = Some(cancel_tx);
            entry.info.state = JobState::Running;
            entry.info.started_at = Some(now_ms());
            inner.emit_updated(&entry.info);
            runtime.spawn(run(inner.clone(), id, request, cancel_rx));
        }
    }

    fn push_line(&self, id: JobId, stream: Stream, text: String) {
        let mut state = self.lock();
        let Some(entry) = state.jobs.get_mut(&id) else {
            return;
        };
        let line = LogLine {
            seq: entry.next_seq,
            stream,
            text,
        };
        entry.next_seq += 1;
        if entry.log.len() >= self.config.ring_lines {
            entry.log.pop_front();
        }
        entry.log.push_back(line.clone());
        let _ = self.events.send(JobEvent::Output { id, line });
    }

    fn finish(inner: &Arc<Inner>, id: JobId, outcome: Outcome) {
        {
            let mut state = inner.lock();
            if let Some(entry) = state.jobs.get_mut(&id) {
                let (job_state, code, error) = match outcome {
                    Outcome::Exited(Some(0)) => (JobState::Succeeded, Some(0), None),
                    Outcome::Exited(code) => (JobState::Failed, code, None),
                    Outcome::Cancelled(code) => (JobState::Cancelled, code, None),
                    Outcome::TimedOut => (JobState::TimedOut, None, None),
                    Outcome::SpawnFailed(e) => (JobState::Failed, None, Some(e)),
                };
                let info = finish_entry(entry, job_state, code, error);
                inner.emit_updated(&info);
            }
            prune(&mut state, inner.config.keep_finished);
        }
        Inner::schedule(inner);
    }
}

fn finish_entry(
    entry: &mut Entry,
    state: JobState,
    exit_code: Option<i32>,
    error: Option<String>,
) -> JobInfo {
    entry.info.state = state;
    entry.info.exit_code = exit_code;
    entry.info.error = error;
    entry.info.finished_at = Some(now_ms());
    entry.cancel = None;
    entry.done.send_replace(true);
    entry.info.clone()
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn prune(state: &mut State, keep: usize) {
    let finished: Vec<JobId> = state
        .jobs
        .iter()
        .filter(|(_, e)| e.info.state.is_finished())
        .map(|(id, _)| *id)
        .collect();
    for id in finished.iter().take(finished.len().saturating_sub(keep)) {
        state.jobs.remove(id);
    }
}

enum Outcome {
    Exited(Option<i32>),
    Cancelled(Option<i32>),
    TimedOut,
    SpawnFailed(String),
}

/// How long to keep reading output after the process exits. A grandchild that inherited the
/// pipes could otherwise keep the job open forever.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

async fn run(
    inner: Arc<Inner>,
    id: JobId,
    request: JobRequest,
    cancel: watch::Receiver<CancelMode>,
) {
    let outcome = execute(&inner, id, &request, cancel).await;
    Inner::finish(&inner, id, outcome);
}

async fn execute(
    inner: &Arc<Inner>,
    id: JobId,
    request: &JobRequest,
    mut cancel: watch::Receiver<CancelMode>,
) -> Outcome {
    let mut child = match process::wrap(&request.spec).spawn() {
        Ok(c) => c,
        Err(e) => return Outcome::SpawnFailed(e.to_string()),
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout().take() {
        readers.push(tokio::spawn(read_lines(out, Stream::Stdout, tx.clone())));
    }
    if let Some(err) = child.stderr().take() {
        readers.push(tokio::spawn(read_lines(err, Stream::Stderr, tx.clone())));
    }
    drop(tx);
    let forward_inner = inner.clone();
    let forwarder = tokio::spawn(async move {
        while let Some((stream, text)) = rx.recv().await {
            forward_inner.push_line(id, stream, text);
        }
    });

    let deadline = request.timeout.map(|t| Instant::now() + t);
    let mut kill_at: Option<Instant> = None;
    let mut cancelled = false;
    let mut timed_out = false;

    enum Event {
        Exited(std::io::Result<std::process::ExitStatus>),
        Cancel(CancelMode),
        GraceOver,
        TimedOut,
    }

    let status = loop {
        let event = {
            let wait = child.wait();
            tokio::select! {
                r = wait => Event::Exited(r),
                Ok(()) = cancel.changed() => Event::Cancel(*cancel.borrow_and_update()),
                _ = sleep_until(kill_at), if kill_at.is_some() => Event::GraceOver,
                _ = sleep_until(deadline), if deadline.is_some() && !timed_out => Event::TimedOut,
            }
        };
        match event {
            Event::Exited(r) => break r,
            Event::Cancel(CancelMode::None) => {}
            Event::Cancel(CancelMode::Graceful) => {
                cancelled = true;
                if interrupt(child.as_ref()) {
                    kill_at.get_or_insert(Instant::now() + inner.config.grace);
                } else {
                    let _ = child.start_kill();
                }
            }
            Event::Cancel(CancelMode::Force) | Event::GraceOver => {
                cancelled = true;
                kill_at = None;
                let _ = child.start_kill();
            }
            Event::TimedOut => {
                timed_out = true;
                let _ = child.start_kill();
            }
        }
    };

    for reader in readers {
        let abort = reader.abort_handle();
        if tokio::time::timeout(DRAIN_TIMEOUT, reader).await.is_err() {
            abort.abort();
        }
    }
    let _ = forwarder.await;

    let code = status.ok().and_then(|s| s.code());
    if timed_out {
        Outcome::TimedOut
    } else if cancelled {
        Outcome::Cancelled(code)
    } else {
        Outcome::Exited(code)
    }
}

/// Sends SIGINT to the job's process group. Returns false where a graceful interrupt isn't
/// available: on Windows it would need console FFI (`unsafe`), so cancel kills the tree.
fn interrupt(child: &dyn process_wrap::tokio::ChildWrapper) -> bool {
    #[cfg(unix)]
    {
        const SIGINT: i32 = 2;
        child.signal(SIGINT).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = child;
        false
    }
}

async fn sleep_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

async fn read_lines(
    reader: impl AsyncRead + Unpin,
    stream: Stream,
    tx: mpsc::UnboundedSender<(Stream, String)>,
) {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                while matches!(buf.last(), Some(b'\n' | b'\r')) {
                    buf.pop();
                }
                if tx
                    .send((stream, String::from_utf8_lossy(&buf).into_owned()))
                    .is_err()
                {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, n: &str) -> Scope {
        Scope::Machine {
            project: p.into(),
            name: n.into(),
        }
    }

    #[test]
    fn scope_conflicts() {
        let proj = Scope::Project {
            project: "/p".into(),
        };
        assert!(m("/p", "web").conflicts_with(&m("/p", "web")));
        assert!(!m("/p", "web").conflicts_with(&m("/p", "db")));
        assert!(!m("/p", "web").conflicts_with(&m("/q", "web")));
        assert!(proj.conflicts_with(&m("/p", "db")));
        assert!(m("/p", "db").conflicts_with(&proj));
        assert!(proj.conflicts_with(&proj));
        assert!(Scope::Boxes.conflicts_with(&Scope::Boxes));
        assert!(!Scope::Boxes.conflicts_with(&proj));
        assert!(!Scope::Unlocked.conflicts_with(&Scope::Unlocked));
    }

    #[test]
    fn finished_states() {
        assert!(JobState::Cancelled.is_finished());
        assert!(!JobState::Cancelling.is_finished());
        assert!(JobState::Cancelling.is_active());
    }
}
