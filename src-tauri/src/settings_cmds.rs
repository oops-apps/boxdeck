//! Settings, bookmarks, the Vagrantfile editor and settings import/export (plan §4.4).

use std::path::{Path, PathBuf};

use boxdeck_core::config::{self, Settings, SettingsStore};
use boxdeck_core::machines::Project;
use boxdeck_core::vagrant::locate::Vagrant;
use boxdeck_core::vagrant::vagrantfile::{self, VagrantfileText};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

use crate::commands::{self, CmdResult};
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

/// Validates, saves and applies settings. Launch-at-login is synced with the OS here.
#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: Settings) -> CmdResult<Settings> {
    let state = app.state::<AppState>();
    let manager = app.autolaunch();
    if manager.is_enabled().unwrap_or(false) != settings.app.launch_at_login {
        let result = if settings.app.launch_at_login {
            manager.enable()
        } else {
            manager.disable()
        };
        result.map_err(|e| format!("could not change launch at login: {e}"))?;
    }
    let (saved, path_changed) = state.update_settings(|s| *s = settings)?;
    if path_changed {
        redetect(&app).await;
    }
    commands::machines_changed(&app);
    Ok(saved)
}

/// Looks for Vagrant again, e.g. after the user installed it.
#[tauri::command]
pub async fn redetect_vagrant(app: AppHandle) {
    redetect(&app).await;
    commands::machines_changed(&app);
}

async fn redetect(app: &AppHandle) {
    let state = app.state::<AppState>();
    let result = Vagrant::detect(state.settings().vagrant_path.as_deref())
        .await
        .map_err(|e| e.to_string());
    state.set_vagrant(result);
    tauri::async_runtime::spawn(commands::sync_all(app.clone(), false));
}

async fn pick(
    app: &AppHandle,
    title: &str,
    folder: bool,
    save_name: Option<&str>,
) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let builder = app.dialog().file().set_title(title);
    let done = move |p: Option<tauri_plugin_dialog::FilePath>| {
        let _ = tx.send(p.and_then(|p| p.into_path().ok()));
    };
    match (folder, save_name) {
        (true, _) => builder.pick_folder(done),
        (false, Some(name)) => builder
            .set_file_name(name)
            .add_filter("JSON", &["json"])
            .save_file(done),
        (false, None) => builder.add_filter("JSON", &["json"]).pick_file(done),
    }
    rx.await.ok().flatten()
}

/// Folder picker for bookmarks and the Vagrant path. `None` if cancelled.
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Option<String> {
    pick(&app, "Choose a folder", true, None)
        .await
        .map(|p| p.to_string_lossy().into_owned())
}

/// Adds a project folder by hand. It must contain a Vagrantfile (or the one named by the
/// project's `VAGRANT_VAGRANTFILE` setting).
#[tauri::command]
pub async fn add_bookmark(app: AppHandle, dir: PathBuf) -> CmdResult<Vec<Project>> {
    let state = app.state::<AppState>();
    let dir = dir
        .canonicalize()
        .map_err(|_| "that folder doesn't exist".to_owned())?;
    let name = state
        .settings()
        .project(&dir)
        .and_then(|p| p.env.get("VAGRANT_VAGRANTFILE").cloned());
    if !vagrantfile::exists(&dir, name.as_deref()) {
        return Err(format!(
            "no {} in {}",
            name.as_deref().unwrap_or("Vagrantfile"),
            dir.display()
        ));
    }
    state.update_settings(|s| {
        if !s.bookmarks.contains(&dir) {
            s.bookmarks.push(dir.clone());
        }
    })?;
    let projects = state.rebuild_projects().await?;
    commands::machines_changed(&app);
    tauri::async_runtime::spawn(commands::sync_all(app.clone(), false));
    Ok(projects)
}

#[tauri::command]
pub async fn remove_bookmark(app: AppHandle, dir: PathBuf) -> CmdResult<Vec<Project>> {
    let state = app.state::<AppState>();
    state.update_settings(|s| s.bookmarks.retain(|b| b != &dir))?;
    let projects = state.rebuild_projects().await?;
    commands::machines_changed(&app);
    Ok(projects)
}

#[tauri::command]
pub fn read_vagrantfile(
    state: State<'_, AppState>,
    project_dir: PathBuf,
) -> CmdResult<VagrantfileText> {
    let env = state.project_env(&project_dir)?;
    vagrantfile::read(&env).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct ValidateResult {
    pub ok: bool,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct SaveResult {
    pub backup: String,
    pub validation: ValidateResult,
}

/// Saves the Vagrantfile if it hasn't changed since `base` was loaded, then validates it.
#[tauri::command]
pub async fn save_vagrantfile(
    app: AppHandle,
    project_dir: PathBuf,
    base: String,
    content: String,
) -> CmdResult<SaveResult> {
    let state = app.state::<AppState>();
    let env = state.project_env(&project_dir)?;
    let backups = config::data_dir()
        .map_err(|e| e.to_string())?
        .join("backups");
    let env2 = env.clone();
    let backup =
        tokio::task::spawn_blocking(move || vagrantfile::save(&env2, &base, &content, &backups))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
    let validation = validate(&state, &env.dir).await?;
    tauri::async_runtime::spawn(commands::sync_all(app.clone(), false));
    Ok(SaveResult {
        backup: backup.to_string_lossy().into_owned(),
        validation,
    })
}

#[tauri::command]
pub async fn validate_vagrantfile(
    state: State<'_, AppState>,
    project_dir: PathBuf,
) -> CmdResult<ValidateResult> {
    validate(&state, &project_dir).await
}

async fn validate(state: &AppState, project_dir: &Path) -> CmdResult<ValidateResult> {
    let env = state.project_env(project_dir)?;
    let vagrant = state.vagrant()?;
    let out = commands::query(state, vagrantfile::validate_command(&vagrant.path, &env)).await?;
    let mut output = out.stdout.trim().to_owned();
    if !out.stderr.trim().is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(out.stderr.trim());
    }
    Ok(ValidateResult {
        ok: out.success(),
        output,
    })
}

/// Writes the settings to a JSON file the user picks.
#[tauri::command]
pub async fn export_settings(app: AppHandle) -> CmdResult<Option<String>> {
    let Some(path) = pick(
        &app,
        "Export Boxdeck settings",
        false,
        Some("boxdeck-settings.json"),
    )
    .await
    else {
        return Ok(None);
    };
    let settings = app.state::<AppState>().settings();
    SettingsStore::new(&path)
        .save(&settings)
        .map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Reads settings from a JSON file the user picks. They're migrated and validated, but not
/// saved: the UI shows them for review and saves through `save_settings`.
#[tauri::command]
pub async fn import_settings(app: AppHandle) -> CmdResult<Option<Settings>> {
    let Some(path) = pick(&app, "Import Boxdeck settings", false, None).await else {
        return Ok(None);
    };
    let imported = SettingsStore::new(&path)
        .load()
        .map_err(|e| e.to_string())?;
    imported.validate().map_err(|e| e.to_string())?;
    Ok(Some(imported))
}
