//! Boxdeck's own updates: signed releases from GitHub through `tauri-plugin-updater` (plan §3.2).

use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::commands::CmdResult;
use crate::state::AppState;

/// Low-rate notification that the launch check found a release.
pub const UPDATE_AVAILABLE: &str = "update-available";

/// The release found by the last check; install uses it so a newer one can't slip in between.
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

impl PendingUpdate {
    fn get(&self) -> Option<Update> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, update: Option<Update>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = update;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct AppUpdate {
    pub version: String,
    pub current_version: String,
    /// Release notes from the GitHub release body.
    pub notes: Option<String>,
    /// RFC 3339 publish date.
    pub date: Option<String>,
}

impl From<&Update> for AppUpdate {
    fn from(u: &Update) -> Self {
        Self {
            version: u.version.clone(),
            current_version: u.current_version.clone(),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
            date: u.raw_json["pub_date"].as_str().map(str::to_owned),
        }
    }
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum UpdateProgress {
    Started {
        #[ts(type = "number | null")]
        total: Option<u64>,
    },
    Chunk {
        #[ts(type = "number")]
        bytes: u64,
    },
    /// Downloaded and verified; the installer is running.
    Installing,
}

/// The release found by the launch check (or the last manual one), without a network call.
#[tauri::command]
pub fn pending_app_update(pending: State<'_, PendingUpdate>) -> Option<AppUpdate> {
    pending.get().as_ref().map(AppUpdate::from)
}

#[tauri::command]
pub async fn check_app_update(app: AppHandle) -> CmdResult<Option<AppUpdate>> {
    let update = check(&app).await?;
    Ok(update.as_ref().map(AppUpdate::from))
}

/// Downloads, verifies and installs the pending update, then restarts Boxdeck. On Windows the
/// installer closes the app itself.
#[tauri::command]
pub async fn install_app_update(
    app: AppHandle,
    on_progress: Channel<UpdateProgress>,
) -> CmdResult<()> {
    let update = app
        .state::<PendingUpdate>()
        .get()
        .ok_or("no update to install; check for updates first")?;
    let active = app
        .state::<AppState>()
        .jobs
        .list()
        .iter()
        .filter(|j| !j.state.is_finished())
        .count();
    if active > 0 {
        return Err(format!(
            "{active} Vagrant job{} still running; wait for {} to finish",
            if active == 1 { " is" } else { "s are" },
            if active == 1 { "it" } else { "them" }
        ));
    }
    let mut started = false;
    update
        .download_and_install(
            |bytes, total| {
                if !started {
                    started = true;
                    let _ = on_progress.send(UpdateProgress::Started { total });
                }
                let _ = on_progress.send(UpdateProgress::Chunk {
                    bytes: bytes as u64,
                });
            },
            || {
                let _ = on_progress.send(UpdateProgress::Installing);
            },
        )
        .await
        .map_err(|e| format!("update failed: {e}"))?;
    app.restart()
}

/// One check at startup; a found release is announced to the window.
pub async fn check_on_launch(app: AppHandle) {
    match check(&app).await {
        Ok(Some(update)) => {
            let _ = app.emit(UPDATE_AVAILABLE, AppUpdate::from(&update));
        }
        Ok(None) => {}
        Err(e) => tracing::info!("launch update check: {e}"),
    }
}

async fn check(app: &AppHandle) -> CmdResult<Option<Update>> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("could not check for updates: {e}"))?;
    app.state::<PendingUpdate>().set(update.clone());
    Ok(update)
}
