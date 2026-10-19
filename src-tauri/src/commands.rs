//! IPC commands. Each one validates its input through `boxdeck-core` before anything reaches
//! the `vagrant` runner (plan §4.7).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use boxdeck_core::jobs::{JobEvent, JobInfo, JobRequest, LogLine, Origin, Scope};
use boxdeck_core::machines::Project;
use boxdeck_core::process;
use boxdeck_core::process::CommandSpec;
use boxdeck_core::pty::PtySession;
use boxdeck_core::vagrant::ProjectEnv;
use boxdeck_core::vagrant::actions::{self, Action, ActionOptions};
use boxdeck_core::vagrant::boxes::{self, BoxInfo, BoxOp, BoxUpdate};
use boxdeck_core::vagrant::catalog::{self, CatalogBox, PopularBox};
use boxdeck_core::vagrant::locate::Vagrant;
use boxdeck_core::vagrant::machine_readable;
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::state::{AppState, VagrantState};

pub type CmdResult<T> = Result<T, String>;

/// Low-rate notification that the machine list should be refetched (plan §3.2).
pub const MACHINES_CHANGED: &str = "machines-changed";
/// Same, for the box list.
pub const BOXES_CHANGED: &str = "boxes-changed";

const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct AppInfo {
    pub version: String,
    pub vagrant: Option<Vagrant>,
    pub vagrant_error: Option<String>,
    pub detecting: bool,
    pub index_path: Option<String>,
    /// This computer's box architecture (`arm64`, `amd64`), for matching boxes.
    pub host_architecture: Option<String>,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    let (vagrant, vagrant_error, detecting) = match state.vagrant() {
        Ok(v) => (Some(v), None, false),
        Err(e) => {
            let detecting = matches!(
                *state.vagrant.read().unwrap_or_else(|e| e.into_inner()),
                VagrantState::Detecting
            );
            (None, (!detecting).then_some(e), detecting)
        }
    };
    AppInfo {
        version: boxdeck_core::VERSION.to_owned(),
        vagrant,
        vagrant_error,
        detecting,
        index_path: state
            .index_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        host_architecture: catalog::host_architecture().map(str::to_owned),
    }
}

#[tauri::command]
pub async fn list_machines(state: State<'_, AppState>) -> CmdResult<Vec<Project>> {
    state.rebuild_projects().await
}

/// Runs `vagrant status` for one project, or all of them, and returns the updated list.
#[tauri::command]
pub async fn refresh_status(
    state: State<'_, AppState>,
    project_dir: Option<PathBuf>,
    auto: Option<bool>,
) -> CmdResult<Vec<Project>> {
    let envs = match project_dir {
        Some(dir) => vec![state.project_env(&dir)?],
        None => state
            .rebuild_projects()
            .await?
            .into_iter()
            .map(|p| p.env)
            .collect(),
    };
    refresh_envs(&state, envs, auto.unwrap_or(false)).await;
    state.rebuild_projects().await
}

/// Automatic refreshes wait this long after the index last changed, so they don't run
/// `vagrant status` in the middle of an action started from a terminal.
const INDEX_QUIET_MS: u64 = 30_000;

/// Runs `vagrant status` for each project that has no Boxdeck job queued or running (a status
/// call takes Vagrant's machine lock and makes a running action fail). `auto` refreshes also
/// skip everything while the index changed recently.
async fn refresh_envs(state: &AppState, envs: Vec<ProjectEnv>, auto: bool) {
    let Some(service) = state.status_service() else {
        return;
    };
    let since_change = boxdeck_core::jobs::now_ms()
        .saturating_sub(state.last_index_change.load(Ordering::Relaxed));
    if auto && since_change < INDEX_QUIET_MS {
        return;
    }
    let settings = state.settings();
    let mut set = tokio::task::JoinSet::new();
    for env in envs {
        let service = service.clone();
        let jobs = state.jobs.clone();
        let vars = settings.project_env(&env.dir);
        set.spawn(async move { service.refresh_if_idle(&jobs, &env, vars).await });
    }
    while set.join_next().await.is_some() {}
}

/// Refreshes every project, coalescing bursts into one extra run.
pub async fn sync_all(app: AppHandle, auto: bool) {
    static RUNNING: AtomicBool = AtomicBool::new(false);
    static PENDING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::AcqRel) {
        PENDING.store(true, Ordering::Release);
        return;
    }
    loop {
        PENDING.store(false, Ordering::Release);
        let state = app.state::<AppState>();
        if let Ok(projects) = state.rebuild_projects().await {
            machines_changed(&app);
            refresh_envs(&state, projects.into_iter().map(|p| p.env).collect(), auto).await;
            let _ = state.rebuild_projects().await;
            machines_changed(&app);
        }
        if !PENDING.load(Ordering::Acquire) {
            break;
        }
    }
    RUNNING.store(false, Ordering::Release);
}

#[tauri::command]
pub fn run_action(
    app: AppHandle,
    project_dir: PathBuf,
    machine: Option<String>,
    action: Action,
    options: Option<ActionOptions>,
) -> CmdResult<u64> {
    start_action(
        &app,
        &project_dir,
        machine.as_deref(),
        action,
        &options.unwrap_or_default(),
        Origin::Ui,
    )
}

/// Validates and queues a lifecycle action, then refreshes that project once it finishes.
/// Shared by the UI, the tray and (in M6) MCP.
pub fn start_action(
    app: &AppHandle,
    project_dir: &Path,
    machine: Option<&str>,
    action: Action,
    options: &ActionOptions,
    origin: Origin,
) -> CmdResult<u64> {
    let state = app.state::<AppState>();
    let env = state.project_env(project_dir)?;
    if let Some(name) = machine {
        state.check_machine(&env.dir, name)?;
    }
    let vagrant = state.vagrant()?;
    let mut options = options.clone();
    if action == Action::Up
        && let Some(project) = state.settings().project(&env.dir)
    {
        // Project defaults fill in whatever the caller left unset.
        options.provider = options.provider.or(project.up.provider.clone());
        options.provision = options.provision.or(project.up.provision);
    }
    let mut spec = actions::build(&vagrant.path, &env, action, machine, &options)
        .map_err(|e| e.to_string())?;
    with_settings_env(&state, &mut spec);
    let scope = match machine {
        Some(name) => Scope::Machine {
            project: env.dir.clone(),
            name: name.to_owned(),
        },
        None => Scope::Project {
            project: env.dir.clone(),
        },
    };
    let label = format!(
        "{} {}",
        action.command(),
        machine.unwrap_or("(all machines)")
    );
    let id = state.jobs.submit(JobRequest {
        label,
        origin,
        scope,
        spec,
        timeout: None,
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        state.jobs.wait(id).await;
        refresh_envs(&state, vec![env], false).await;
        let _ = state.rebuild_projects().await;
        machines_changed(&app);
    });
    Ok(id)
}

/// Tells the UI and the tray that the machine list changed.
pub fn machines_changed(app: &AppHandle) {
    let _ = app.emit(MACHINES_CHANGED, ());
    crate::tray::request_refresh(app);
}

/// Settings env (global, then per-project for the command's folder) goes first, so the
/// command's own variables, such as `VAGRANT_VAGRANTFILE`, win.
pub fn with_settings_env(state: &AppState, spec: &mut CommandSpec) {
    let own = std::mem::take(&mut spec.env);
    let settings = state.settings();
    spec.env = match &spec.cwd {
        Some(dir) => settings.project_env(dir),
        None => settings.command_env(),
    };
    spec.env.extend(own);
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, id: u64, force: bool) -> CmdResult<()> {
    state.jobs.cancel(id, force).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_jobs(state: State<'_, AppState>) -> Vec<JobInfo> {
    state.jobs.list()
}

#[tauri::command]
pub fn job_log(
    state: State<'_, AppState>,
    id: u64,
    tail: Option<usize>,
) -> CmdResult<Vec<LogLine>> {
    state
        .jobs
        .log(id, tail.unwrap_or(usize::MAX))
        .ok_or_else(|| format!("no job {id}"))
}

/// Streams job updates and output lines until the frontend drops the channel.
#[tauri::command]
pub fn subscribe_jobs(state: State<'_, AppState>, on_event: Channel<JobEvent>) {
    let mut rx = state.jobs.subscribe();
    tauri::async_runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if on_event.send(event).is_err() {
                        break;
                    }
                }
                // The UI catches up from `job_log` using sequence numbers.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// Opens `vagrant ssh MACHINE` in a PTY. Output arrives as raw bytes on `on_data`.
#[tauri::command]
pub fn ssh_open(
    state: State<'_, AppState>,
    project_dir: PathBuf,
    machine: String,
    cols: u16,
    rows: u16,
    on_data: Channel,
    on_exit: Channel<Option<u32>>,
) -> CmdResult<u32> {
    let env = state.project_env(&project_dir)?;
    state.check_machine(&env.dir, &machine)?;
    let vagrant = state.vagrant()?;
    let mut spec = actions::ssh(&vagrant.path, &env, &machine).map_err(|e| e.to_string())?;
    with_settings_env(&state, &mut spec);
    open_pty(&state, &spec, cols, rows, on_data, on_exit)
}

/// Re-runs a finished job in a PTY, for commands that need input (e.g. a sudo password).
#[tauri::command]
pub fn pty_rerun_job(
    state: State<'_, AppState>,
    job_id: u64,
    cols: u16,
    rows: u16,
    on_data: Channel,
    on_exit: Channel<Option<u32>>,
) -> CmdResult<u32> {
    let job = state
        .jobs
        .get(job_id)
        .ok_or_else(|| format!("no job {job_id}"))?;
    // The argv came from our own validated builder; only drop the non-interactive flag.
    let mut argv = job.argv.into_iter();
    let program = argv.next().ok_or("job has no command")?;
    let mut spec = CommandSpec::new(program).args(argv.filter(|a| a != "--no-tty"));
    match &job.scope {
        Scope::Machine { project, .. } | Scope::Project { project } => {
            let env = state.project_env(project)?;
            spec = spec.cwd(&env.dir);
            for (k, v) in env.vars() {
                spec = spec.env(k, v);
            }
        }
        // Box commands aren't tied to a project folder.
        Scope::Boxes => {}
        Scope::Unlocked => return Err("this job can't be re-run in a terminal".into()),
    }
    with_settings_env(&state, &mut spec);
    open_pty(&state, &spec, cols, rows, on_data, on_exit)
}

fn open_pty(
    state: &AppState,
    spec: &CommandSpec,
    cols: u16,
    rows: u16,
    on_data: Channel,
    on_exit: Channel<Option<u32>>,
) -> CmdResult<u32> {
    let session = PtySession::spawn(
        spec,
        cols,
        rows,
        move |bytes| {
            let _ = on_data.send(InvokeResponseBody::Raw(bytes.to_vec()));
        },
        move |code| {
            let _ = on_exit.send(code);
        },
    )
    .map_err(|e| e.to_string())?;
    let id = state.next_pty_id();
    state
        .ptys
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, session);
    Ok(id)
}

fn with_pty<T>(
    state: &AppState,
    id: u32,
    f: impl FnOnce(&PtySession) -> boxdeck_core::Result<T>,
) -> CmdResult<T> {
    let ptys = state.ptys.lock().unwrap_or_else(|e| e.into_inner());
    let session = ptys.get(&id).ok_or_else(|| format!("no terminal {id}"))?;
    f(session).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pty_write(state: State<'_, AppState>, id: u32, data: String) -> CmdResult<()> {
    with_pty(&state, id, |s| s.write(data.as_bytes()))
}

#[tauri::command]
pub fn pty_resize(state: State<'_, AppState>, id: u32, cols: u16, rows: u16) -> CmdResult<()> {
    with_pty(&state, id, |s| s.resize(cols, rows))
}

#[tauri::command]
pub fn pty_close(state: State<'_, AppState>, id: u32) {
    // Dropping the session kills the process.
    state
        .ptys
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
}

#[tauri::command]
pub fn open_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    project_dir: PathBuf,
) -> CmdResult<()> {
    let env = state.project_env(&project_dir)?;
    app.opener()
        .open_path(env.dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Runs a short read-only Vagrant query with the settings environment.
pub async fn query(state: &AppState, mut spec: CommandSpec) -> CmdResult<process::CommandOutput> {
    with_settings_env(state, &mut spec);
    process::run(&spec, QUERY_TIMEOUT)
        .await
        .map_err(|e| e.to_string())
}

/// Installed boxes with their size on disk and the machines using them.
#[tauri::command]
pub async fn list_boxes(state: State<'_, AppState>) -> CmdResult<Vec<BoxInfo>> {
    let vagrant = state.vagrant()?;
    let out = query(&state, boxes::list_command(&vagrant.path)).await?;
    if !out.success() {
        return Err(machine_readable::failure(&out.stdout, &out.stderr, out.exit_code).to_string());
    }
    let mut list = boxes::parse_list(&out.stdout);
    let indexed = state.read_index().await?;
    let home = state.vagrant_home.clone();
    tokio::task::spawn_blocking(move || {
        boxes::annotate(&mut list, &indexed, home.as_deref());
        list
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct BoxUpdates {
    pub updates: Vec<BoxUpdate>,
    /// Vagrant's own text, shown alongside because the parse is best effort (plan §4.5).
    pub raw: String,
}

#[tauri::command]
pub async fn check_box_updates(state: State<'_, AppState>) -> CmdResult<BoxUpdates> {
    let vagrant = state.vagrant()?;
    let out = query(&state, boxes::outdated_command(&vagrant.path)).await?;
    let (lines, plain) = machine_readable::split(&out.stdout);
    let mut raw: Vec<String> = lines
        .iter()
        .filter(|l| l.kind == "ui")
        .filter_map(|l| l.data.get(1).cloned())
        .collect();
    raw.extend(plain.into_iter().map(str::to_owned));
    if !out.stderr.trim().is_empty() {
        raw.push(out.stderr.trim().to_owned());
    }
    Ok(BoxUpdates {
        updates: boxes::parse_outdated(&out.stdout),
        raw: raw.join("\n"),
    })
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct CatalogSearch {
    pub boxes: Vec<CatalogBox>,
    /// Why the search failed (usually no internet); the UI then offers `popular`.
    pub error: Option<String>,
    pub popular: Vec<PopularBox>,
    /// This computer's box architecture, e.g. `arm64`.
    pub host_architecture: Option<String>,
}

/// Boxes to suggest before searching; no network needed.
#[tauri::command]
pub fn popular_boxes() -> Vec<PopularBox> {
    catalog::popular()
}

/// Searches the public box catalog through `vagrant cloud search` (anonymous).
#[tauri::command]
pub async fn search_boxes(
    state: State<'_, AppState>,
    query: String,
    provider: Option<String>,
) -> CmdResult<CatalogSearch> {
    let popular = catalog::popular();
    let host = catalog::host_architecture();
    let vagrant = state.vagrant()?;
    let spec = catalog::search_command(&vagrant.path, &query, provider.as_deref(), host, 20)
        .map_err(|e| e.to_string())?;
    let result = match query_with_timeout(&state, spec, SEARCH_TIMEOUT).await {
        Ok(out) if out.success() => catalog::parse_search(&out.stdout).map_err(|e| e.to_string()),
        Ok(out) => {
            Err(machine_readable::failure(&out.stdout, &out.stderr, out.exit_code).to_string())
        }
        Err(e) => Err(e),
    };
    Ok(match result {
        Ok(boxes) => CatalogSearch {
            boxes,
            error: None,
            popular,
            host_architecture: host.map(str::to_owned),
        },
        Err(e) => CatalogSearch {
            boxes: Vec::new(),
            error: Some(e),
            popular,
            host_architecture: host.map(str::to_owned),
        },
    })
}

const SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

async fn query_with_timeout(
    state: &AppState,
    mut spec: CommandSpec,
    timeout: std::time::Duration,
) -> CmdResult<process::CommandOutput> {
    with_settings_env(state, &mut spec);
    process::run(&spec, timeout)
        .await
        .map_err(|e| e.to_string())
}

/// What `box prune` would remove, as Vagrant's own text.
#[tauri::command]
pub async fn prune_dry_run(state: State<'_, AppState>, keep_used: bool) -> CmdResult<String> {
    let vagrant = state.vagrant()?;
    let out = query(
        &state,
        boxes::prune_dry_run_command(&vagrant.path, keep_used),
    )
    .await?;
    if !out.success() {
        return Err(machine_readable::failure(&out.stdout, &out.stderr, out.exit_code).to_string());
    }
    Ok(out.stdout.trim().to_owned())
}

#[tauri::command]
pub fn run_box_op(app: AppHandle, op: BoxOp) -> CmdResult<u64> {
    start_box_op(&app, &op, Origin::Ui)
}

/// Validates and queues a box mutation; the UI confirms destructive ones first.
pub fn start_box_op(app: &AppHandle, op: &BoxOp, origin: Origin) -> CmdResult<u64> {
    let state = app.state::<AppState>();
    let vagrant = state.vagrant()?;
    let mut spec = boxes::build_op(&vagrant.path, op).map_err(|e| e.to_string())?;
    with_settings_env(&state, &mut spec);
    let id = state.jobs.submit(JobRequest {
        label: op.label(),
        origin,
        scope: Scope::Boxes,
        spec,
        timeout: None,
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        app.state::<AppState>().jobs.wait(id).await;
        let _ = app.emit(BOXES_CHANGED, ());
    });
    Ok(id)
}

/// Locates Vagrant, then loads the machine list and live status.
pub async fn startup(app: AppHandle) {
    let state = app.state::<AppState>();
    let override_path = state.settings().vagrant_path;
    let result = Vagrant::detect(override_path.as_deref())
        .await
        .map_err(|e| e.to_string());
    state.set_vagrant(result);
    machines_changed(&app);
    if let Ok(log) = boxdeck_core::audit::AuditLog::default_log() {
        let _ = log.rotate_if_needed(state.settings().mcp.audit_max_bytes);
    }
    sync_all(app.clone(), true).await;
    tauri::async_runtime::spawn(refresh_on_interval(app));
}

/// Background status refresh at the user's chosen interval (Settings; off by default).
async fn refresh_on_interval(app: AppHandle) {
    let mut last = std::time::Instant::now();
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let Some(every) = app.state::<AppState>().settings().status.interval_secs else {
            last = std::time::Instant::now();
            continue;
        };
        if last.elapsed().as_secs() >= every {
            last = std::time::Instant::now();
            sync_all(app.clone(), true).await;
        }
    }
}

/// Low-rate notification that agent calls were logged (by the app or `boxdeck-mcp`).
pub const AUDIT_CHANGED: &str = "audit-changed";

pub fn watch_audit(app: &AppHandle) {
    let Ok(log) = boxdeck_core::audit::AuditLog::default_log() else {
        return;
    };
    let handle = app.clone();
    match boxdeck_core::watch::watch_file(log.path(), move || {
        let _ = handle.emit(AUDIT_CHANGED, ());
        crate::tray::request_refresh(&handle);
    }) {
        // Kept alive for the app's lifetime.
        Ok(w) => std::mem::forget(w),
        Err(e) => tracing::warn!("not watching the audit log: {e}"),
    }
}

/// The index changed, usually because a Vagrant action is running right now. Running
/// `vagrant status` here would take the machine's lock and break that action, so this only
/// re-reads the index, dropping cached live results it contradicts.
async fn on_index_change(app: AppHandle) {
    let state = app.state::<AppState>();
    state
        .last_index_change
        .store(boxdeck_core::jobs::now_ms(), Ordering::Relaxed);
    if let (Some(service), Ok(indexed)) = (state.status_service(), state.read_index().await) {
        for cached in service.all_cached().await {
            let Ok(live) = &cached.result else { continue };
            let stale = indexed.iter().any(|m| {
                m.env == cached.env && live.iter().any(|l| l.name == m.name && l.state != m.state)
            });
            if stale {
                service.forget(&cached.env).await;
            }
        }
    }
    let _ = state.rebuild_projects().await;
    machines_changed(&app);
}

pub fn watch_index(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(path) = state.index_path.clone() else {
        return;
    };
    let handle = app.clone();
    match boxdeck_core::watch::watch_file(&path, move || {
        tauri::async_runtime::spawn(on_index_change(handle.clone()));
    }) {
        Ok(w) => *state.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(w),
        Err(e) => tracing::warn!("not watching the machine index: {e}"),
    }
}
