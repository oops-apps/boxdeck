//! Tray / menu bar (plan §4.3). Every action is reachable from the menu, because Linux tray
//! icons emit no click events.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use boxdeck_core::jobs::{JobInfo, JobState, Origin, Scope};
use boxdeck_core::machines::Project;
use boxdeck_core::vagrant::actions::Action;
use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{
    CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, Submenu,
    SubmenuBuilder,
};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::Notify;

use crate::commands;
use crate::state::AppState;

const TRAY_ID: &str = "main";
/// Machines listed directly in the menu; the rest are a click away in the window.
const MAX_MACHINES: usize = 25;
/// Window-side requests from tray items, e.g. opening an SSH tab.
pub const TRAY_REQUEST: &str = "tray-request";

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum TrayRequest {
    Ssh {
        project_dir: String,
        machine: String,
    },
    ShowMachine {
        project_dir: String,
        machine: String,
    },
    ShowSettings,
    /// Open the updates section and check now.
    CheckUpdates,
}

/// Menu ids point into this table instead of carrying paths.
static TARGETS: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());
static REFRESH: OnceLock<Notify> = OnceLock::new();

fn notify() -> &'static Notify {
    REFRESH.get_or_init(Notify::new)
}

/// Asks for a (debounced) menu rebuild.
pub fn request_refresh(_app: &AppHandle) {
    notify().notify_one();
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, &[], &[])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Boxdeck")
        .menu(&menu)
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            // Windows: left click opens the window; the menu is on right click. Linux never
            // sends this event, so everything is in the menu too.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
                && !cfg!(target_os = "macos")
            {
                show_main(tray.app_handle());
            }
        });
    builder = if cfg!(target_os = "macos") {
        builder.icon(icon(false)).icon_as_template(true)
    } else if let Some(icon) = app.default_window_icon() {
        builder.icon(icon.clone())
    } else {
        builder
    };
    builder.build(app)?;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            notify().notified().await;
            // Coalesce bursts (a job update plus a status refresh) into one rebuild.
            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Err(e) = rebuild(&app) {
                tracing::warn!("tray menu rebuild failed: {e}");
            }
        }
    });
    Ok(())
}

fn rebuild(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let projects = state.current_projects();
    let jobs = state.jobs.list();
    let menu = build_menu(app, &projects, &jobs)?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
        let running = projects
            .iter()
            .flat_map(|p| &p.machines)
            .filter(|m| m.state == "running")
            .count();
        tray.set_tooltip(Some(format!("Boxdeck: {running} running")))?;
        set_icon(&tray, running > 0)?;
    }
    Ok(())
}

fn set_icon<R: Runtime>(tray: &TrayIcon<R>, running: bool) -> tauri::Result<()> {
    if cfg!(target_os = "macos") {
        tray.set_icon(Some(icon(running)))?;
        tray.set_icon_as_template(true)?;
    }
    Ok(())
}

fn glyph(state: &str) -> &'static str {
    match state {
        "running" => "●",
        "saved" | "suspended" | "paused" => "◐",
        "not_created" => "○",
        "aborted" => "▲",
        _ => "■",
    }
}

fn active_job<'a>(jobs: &'a [JobInfo], dir: &PathBuf, name: &str) -> Option<&'a JobInfo> {
    jobs.iter().find(|j| {
        matches!(
            j.state,
            JobState::Queued | JobState::Running | JobState::Cancelling
        ) && match &j.scope {
            Scope::Machine { project, name: n } => project == dir && n == name,
            Scope::Project { project } => project == dir,
            _ => false,
        }
    })
}

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    projects: &[Project],
    jobs: &[JobInfo],
) -> tauri::Result<Menu<R>> {
    let all: Vec<_> = projects
        .iter()
        .flat_map(|p| p.machines.iter().map(move |m| (p, m)))
        .collect();
    let running = all.iter().filter(|(_, m)| m.state == "running").count();
    let agent_jobs = jobs
        .iter()
        .filter(|j| matches!(j.origin, Origin::Mcp(_)))
        .filter(|j| matches!(j.state, JobState::Running | JobState::Queued))
        .count();

    let mut menu = MenuBuilder::new(app);
    let summary = if all.is_empty() {
        "No Vagrant machines".to_owned()
    } else {
        format!("{running} running · {} stopped", all.len() - running)
    };
    menu = menu.item(
        &MenuItemBuilder::with_id("summary", summary)
            .enabled(false)
            .build(app)?,
    );
    if agent_jobs > 0 {
        let label = format!(
            "Agent: {agent_jobs} job{} running",
            if agent_jobs == 1 { "" } else { "s" }
        );
        menu = menu.item(
            &MenuItemBuilder::with_id("agents", label)
                .enabled(false)
                .build(app)?,
        );
    }
    menu = menu.separator();

    let multiple_projects = projects.len() > 1;
    let mut targets = Vec::new();
    for (p, m) in all.iter().take(MAX_MACHINES) {
        let idx = targets.len();
        targets.push((p.env.dir.clone(), m.name.clone()));
        let project_name = p
            .env
            .dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let job = active_job(jobs, &p.env.dir, &m.name);
        let mut title = format!("{} {}", glyph(&m.state), m.name);
        if multiple_projects {
            title.push_str(&format!(" — {project_name}"));
        }
        if let Some(job) = job {
            title.push_str(&format!("  ({}…)", job.label));
        }
        menu = menu.item(&machine_submenu(app, idx, &title, &m.state, job.is_some())?);
    }
    if all.len() > MAX_MACHINES {
        let more = format!("{} more in Boxdeck…", all.len() - MAX_MACHINES);
        menu = menu.item(&MenuItemBuilder::with_id("show", more).build(app)?);
    }
    *TARGETS.lock().unwrap_or_else(|e| e.into_inner()) = targets;
    if !all.is_empty() {
        menu = menu.separator();
    }

    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    menu = menu
        .item(
            &MenuItemBuilder::with_id("halt-all", "Halt All Running")
                .enabled(running > 0)
                .build(app)?,
        )
        .item(&MenuItemBuilder::with_id("refresh", "Refresh Status").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("show", "Open Boxdeck").build(app)?)
        .item(&MenuItemBuilder::with_id("settings", "Settings…").build(app)?);
    let mcp = app.state::<AppState>().settings().mcp;
    if mcp.enabled {
        menu = menu.item(
            &CheckMenuItemBuilder::with_id("pause-agents", "Pause Agent Access")
                .checked(mcp.paused)
                .build(app)?,
        );
    }
    menu = menu
        .item(
            &CheckMenuItemBuilder::with_id("autostart", "Launch at Login")
                .checked(autostart)
                .build(app)?,
        )
        .item(&MenuItemBuilder::with_id("updates", "Check for Updates…").build(app)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&MenuItemBuilder::with_id("quit", "Quit Boxdeck").build(app)?);
    menu.build()
}

fn machine_submenu<R: Runtime>(
    app: &AppHandle<R>,
    idx: usize,
    title: &str,
    state: &str,
    busy: bool,
) -> tauri::Result<Submenu<R>> {
    let running = state == "running";
    let suspended = matches!(state, "saved" | "suspended" | "paused");
    let created = state != "not_created";
    let item = |id: String, label: &str, enabled: bool| {
        MenuItemBuilder::with_id(id, label)
            .enabled(enabled && !busy)
            .build(app)
    };
    let act = |a: &str| format!("act:{a}:{idx}");
    SubmenuBuilder::new(app, title)
        .item(&item(act("up"), "Up", !running && !suspended)?)
        .item(&item(act("halt"), "Halt", running || suspended)?)
        .item(&item(act("suspend"), "Suspend", running)?)
        .item(&item(act("resume"), "Resume", suspended)?)
        .item(&item(act("reload"), "Reload", created)?)
        .item(&item(act("provision"), "Provision", running)?)
        .separator()
        .item(
            &MenuItemBuilder::with_id(format!("ssh:{idx}"), "SSH")
                .enabled(running)
                .build(app)?,
        )
        .item(&MenuItemBuilder::with_id(format!("open:{idx}"), "Open Folder").build(app)?)
        .item(&MenuItemBuilder::with_id(format!("showm:{idx}"), "Show in Boxdeck").build(app)?)
        .build()
}

fn target(idx: &str) -> Option<(PathBuf, String)> {
    let idx: usize = idx.parse().ok()?;
    TARGETS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(idx)
        .cloned()
}

fn parse_action(name: &str) -> Option<Action> {
    Some(match name {
        "up" => Action::Up,
        "halt" => Action::Halt,
        "suspend" => Action::Suspend,
        "resume" => Action::Resume,
        "reload" => Action::Reload,
        "provision" => Action::Provision,
        // Destroy is deliberately not offered from the tray.
        _ => return None,
    })
}

fn on_menu(app: &AppHandle, id: &str) {
    let mut parts = id.split(':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("act"), Some(action), Some(idx)) => {
            if let (Some(action), Some((dir, name))) = (parse_action(action), target(idx))
                && let Err(e) = commands::start_action(
                    app,
                    &dir,
                    Some(&name),
                    action,
                    &Default::default(),
                    Origin::Tray,
                )
            {
                tracing::warn!("tray action failed: {e}");
            }
        }
        (Some("ssh"), Some(idx), None) => {
            if let Some((dir, machine)) = target(idx) {
                show_main(app);
                let _ = app.emit(
                    TRAY_REQUEST,
                    TrayRequest::Ssh {
                        project_dir: dir.to_string_lossy().into_owned(),
                        machine,
                    },
                );
            }
        }
        (Some("showm"), Some(idx), None) => {
            if let Some((dir, machine)) = target(idx) {
                show_main(app);
                let _ = app.emit(
                    TRAY_REQUEST,
                    TrayRequest::ShowMachine {
                        project_dir: dir.to_string_lossy().into_owned(),
                        machine,
                    },
                );
            }
        }
        (Some("open"), Some(idx), None) => {
            if let Some((dir, _)) = target(idx) {
                let state = app.state::<AppState>();
                let _ = commands::open_folder(app.clone(), state, dir);
            }
        }
        (Some("halt-all"), None, None) => halt_all(app),
        (Some("refresh"), None, None) => {
            tauri::async_runtime::spawn(commands::sync_all(app.clone(), false));
        }
        (Some("show"), None, None) => show_main(app),
        (Some("settings"), None, None) => {
            show_main(app);
            let _ = app.emit(TRAY_REQUEST, TrayRequest::ShowSettings);
        }
        (Some("updates"), None, None) => {
            show_main(app);
            let _ = app.emit(TRAY_REQUEST, TrayRequest::CheckUpdates);
        }
        (Some("autostart"), None, None) => toggle_autostart(app),
        (Some("pause-agents"), None, None) => {
            let state = app.state::<AppState>();
            // Saved to the settings file, so a running boxdeck-mcp stops too.
            if let Err(e) = state.update_settings(|s| s.mcp.paused = !s.mcp.paused) {
                tracing::warn!("could not pause agent access: {e}");
            }
            let _ = app.emit(crate::commands::MACHINES_CHANGED, ());
            request_refresh(app);
        }
        (Some("quit"), None, None) => app.exit(0),
        _ => {}
    }
}

fn halt_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    for p in state.current_projects() {
        for m in p.machines.iter().filter(|m| m.state == "running") {
            if let Err(e) = commands::start_action(
                app,
                &p.env.dir,
                Some(&m.name),
                Action::Halt,
                &Default::default(),
                Origin::Tray,
            ) {
                tracing::warn!("halt {} failed: {e}", m.name);
            }
        }
    }
}

fn toggle_autostart(app: &AppHandle) {
    let manager = app.autolaunch();
    let enable = !manager.is_enabled().unwrap_or(false);
    let result = if enable {
        manager.enable()
    } else {
        manager.disable()
    };
    match result {
        Ok(()) => {
            let state = app.state::<AppState>();
            if let Err(e) = state.update_settings(|s| s.app.launch_at_login = enable) {
                tracing::warn!("could not save settings: {e}");
            }
        }
        Err(e) => tracing::warn!("could not change launch at login: {e}"),
    }
    request_refresh(app);
}

/// Shows, unminimizes and focuses the main window.
pub fn show_main<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The menu-bar template (black + alpha): the app icon's cube, its top face lit while any
/// machine runs. Sources and the export script: `icons/tray/*.svg`, `scripts/export-tray-icons.sh`.
pub fn icon(running: bool) -> Image<'static> {
    let png: &'static [u8] = if running {
        include_bytes!("../icons/tray/tray-running.png")
    } else {
        include_bytes!("../icons/tray/tray.png")
    };
    Image::from_bytes(png).expect("bundled tray icon is a valid PNG")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_icons_decode_as_44px_templates() {
        for running in [false, true] {
            let i = icon(running);
            assert_eq!((i.width(), i.height()), (44, 44));
            // A template is black; macOS supplies the color from the alpha.
            assert!(i.rgba().chunks(4).all(|p| p[3] == 0 || p[..3] == [0, 0, 0]));
        }
    }

    #[test]
    fn running_icon_has_more_ink() {
        let ink = |i: Image<'_>| i.rgba().chunks(4).filter(|p| p[3] > 0).count();
        assert!(ink(icon(true)) > ink(icon(false)));
    }

    #[test]
    fn destroy_is_not_a_tray_action() {
        assert!(parse_action("destroy").is_none());
        assert!(matches!(parse_action("halt"), Some(Action::Halt)));
    }
}
