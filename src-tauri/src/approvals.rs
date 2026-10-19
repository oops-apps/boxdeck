//! Approval prompts for agent calls (plan §4.8). `boxdeck-mcp` files requests in the data dir
//! (`boxdeck_core::bridge`); the app watches that folder and shows a small always-on-top window
//! per request, not a native dialog, so it can close itself when the request times out.

use std::collections::HashSet;
use std::fs::File;
use std::sync::Mutex;

use boxdeck_core::bridge::{self, ApprovalRequest, Bridge};
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PendingApproval {
    pub id: String,
    pub client: String,
    pub tool: String,
    pub project: Option<String>,
    pub machine: Option<String>,
    /// Pretty-printed JSON of the exact arguments.
    pub arguments: String,
    /// Shell access: "Allow for this session" is offered.
    pub grantable: bool,
    /// Host folders the VM can write through synced folders.
    pub host_paths: Vec<String>,
    /// Milliseconds since the epoch when this denies itself.
    #[ts(type = "number")]
    pub expires_at: u64,
}

/// Requests with an open window, and the lock that tells `boxdeck-mcp` the app is running.
struct Shown {
    ids: HashSet<String>,
    _presence: Option<File>,
}

static SHOWN: Mutex<Option<Shown>> = Mutex::new(None);

fn with_shown<T>(f: impl FnOnce(&mut Shown) -> T) -> T {
    let mut guard = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(|| Shown {
        ids: HashSet::new(),
        _presence: None,
    }))
}

fn label(id: &str) -> String {
    format!("approval-{id}")
}

fn view(id: &str, req: ApprovalRequest) -> PendingApproval {
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
    PendingApproval {
        id: id.to_owned(),
        client: req.client,
        tool: req.tool,
        project: req.project.as_deref().map(path),
        machine: req.machine,
        arguments: serde_json::to_string_pretty(&req.arguments).unwrap_or_default(),
        grantable: req.grantable,
        host_paths: req.host_paths.iter().map(|p| path(p)).collect(),
        expires_at: req.created_at + bridge::TIMEOUT.as_millis() as u64,
    }
}

/// Clears leftovers, tells `boxdeck-mcp` the app is here, and starts watching for requests.
pub fn start(app: &AppHandle) {
    let Ok(bridge) = Bridge::default_bridge() else {
        return;
    };
    if let Err(e) = bridge.reset() {
        tracing::warn!("could not clear old approval requests: {e}");
    }
    match bridge.hold_presence() {
        Ok(file) => with_shown(|s| s._presence = Some(file)),
        Err(e) => tracing::warn!("agents can't ask for approval: {e}"),
    }
    let dir = match bridge.approvals_dir() {
        Ok(d) => d,
        Err(e) => return tracing::warn!("agents can't ask for approval: {e}"),
    };
    let handle = app.clone();
    match boxdeck_core::watch::watch_dir(&dir, move || sync(&handle)) {
        // Kept alive for the app's lifetime.
        Ok(w) => std::mem::forget(w),
        Err(e) => tracing::warn!("not watching approval requests: {e}"),
    }
}

/// Opens a window for each new request and closes those `boxdeck-mcp` gave up on.
fn sync(app: &AppHandle) {
    let Ok(bridge) = Bridge::default_bridge() else {
        return;
    };
    let pending = bridge.pending();
    let live: HashSet<String> = pending.iter().map(|(id, _)| id.clone()).collect();
    let gone: Vec<String> = with_shown(|s| s.ids.difference(&live).cloned().collect());
    for id in gone {
        with_shown(|s| s.ids.remove(&id));
        if let Some(w) = app.get_webview_window(&label(&id)) {
            let _ = w.close();
        }
    }
    for (id, req) in pending {
        if with_shown(|s| s.ids.insert(id.clone())) {
            open(app, &id, &req);
        }
    }
}

fn open(app: &AppHandle, id: &str, req: &ApprovalRequest) {
    let shell = req.grantable;
    let window = WebviewWindowBuilder::new(
        app,
        label(id),
        WebviewUrl::App(format!("index.html?approval={id}").into()),
    )
    .title("Boxdeck: approve an agent action")
    .inner_size(480.0, if shell { 480.0 } else { 420.0 })
    .resizable(false)
    .always_on_top(true)
    .focused(true)
    .center()
    .build();
    if let Err(e) = &window {
        tracing::warn!("could not open the approval window: {e}");
    }
    let main_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if !main_visible {
        let _ = app
            .notification()
            .builder()
            .title("Boxdeck: approval needed")
            .body(format!("{} wants to run {}", req.client, req.tool))
            .show();
    }
    // `boxdeck-mcp` stops waiting on its own; this just closes the window then.
    let handle = app.clone();
    let id = id.to_owned();
    let left = (req.created_at + bridge::TIMEOUT.as_millis() as u64)
        .saturating_sub(boxdeck_core::jobs::now_ms());
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(left)).await;
        if let Some(w) = handle.get_webview_window(&label(&id)) {
            let _ = w.close();
        }
    });
}

/// The window closed: unless it was answered, that's a no.
pub fn dismissed(app: &AppHandle, id: &str) {
    if with_shown(|s| s.ids.contains(id)) {
        answer_approval(app.clone(), id.to_owned(), false, None);
    }
}

#[tauri::command]
pub fn get_approval(id: String) -> Option<PendingApproval> {
    let bridge = Bridge::default_bridge().ok()?;
    bridge
        .pending()
        .into_iter()
        .find(|(i, _)| *i == id)
        .map(|(i, req)| view(&i, req))
}

/// `session`: also allow shell access to this machine until the app restarts.
#[tauri::command]
pub fn answer_approval(app: AppHandle, id: String, allow: bool, session: Option<bool>) {
    let Ok(bridge) = Bridge::default_bridge() else {
        return;
    };
    let request = bridge.pending().into_iter().find(|(i, _)| *i == id);
    if allow
        && session == Some(true)
        && let Some((_, req)) = &request
        && req.grantable
        && let (Some(project), Some(machine)) = (&req.project, &req.machine)
        && let Err(e) = bridge.grant(project, machine)
    {
        tracing::warn!("could not save the session grant: {e}");
    }
    if let Err(e) = bridge.answer(&id, allow) {
        tracing::warn!("could not answer approval {id}: {e}");
    }
    with_shown(|s| s.ids.remove(&id));
    if let Some(w) = app.get_webview_window(&label(&id)) {
        let _ = w.close();
    }
}
