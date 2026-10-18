//! Approval prompts for destructive agent calls (plan §4.8): a small always-on-top window per
//! request, not a native dialog, so it can close itself when the 2-minute timeout denies.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use boxdeck_mcp::ApprovalRequest;
use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::oneshot;

const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PendingApproval {
    #[ts(type = "number")]
    pub id: u64,
    pub client: String,
    pub tool: String,
    pub project: Option<String>,
    pub machine: Option<String>,
    /// Pretty-printed JSON of the exact arguments.
    pub arguments: String,
    /// Milliseconds since the epoch when this denies itself.
    #[ts(type = "number")]
    pub expires_at: u64,
}

struct Waiting {
    view: PendingApproval,
    answer: oneshot::Sender<bool>,
}

static NEXT: AtomicU64 = AtomicU64::new(1);
static PENDING: Mutex<Option<HashMap<u64, Waiting>>> = Mutex::new(None);

fn with_pending<T>(f: impl FnOnce(&mut HashMap<u64, Waiting>) -> T) -> T {
    let mut guard = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

fn label(id: u64) -> String {
    format!("approval-{id}")
}

/// Shows the prompt and waits for the user. No answer within two minutes means no.
pub async fn ask(app: &AppHandle, req: ApprovalRequest) -> bool {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = oneshot::channel();
    let view = PendingApproval {
        id,
        client: req.client.clone(),
        tool: req.tool.clone(),
        project: req
            .project
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        machine: req.machine.clone(),
        arguments: serde_json::to_string_pretty(&req.arguments).unwrap_or_default(),
        expires_at: boxdeck_core::jobs::now_ms() + TIMEOUT.as_millis() as u64,
    };
    with_pending(|p| p.insert(id, Waiting { view, answer: tx }));

    let window = WebviewWindowBuilder::new(
        app,
        label(id),
        WebviewUrl::App(format!("index.html?approval={id}").into()),
    )
    .title("Boxdeck: approve an agent action")
    .inner_size(480.0, 420.0)
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

    let allowed = matches!(tokio::time::timeout(TIMEOUT, rx).await, Ok(Ok(true)));
    with_pending(|p| p.remove(&id));
    if let Some(w) = app.get_webview_window(&label(id)) {
        let _ = w.close();
    }
    allowed
}

#[tauri::command]
pub fn get_approval(id: u64) -> Option<PendingApproval> {
    with_pending(|p| p.get(&id).map(|w| w.view.clone()))
}

#[tauri::command]
pub fn answer_approval(id: u64, allow: bool) {
    if let Some(w) = with_pending(|p| p.remove(&id)) {
        let _ = w.answer.send(allow);
    }
}
