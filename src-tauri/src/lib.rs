mod approvals;
mod commands;
mod mcp_cmds;
mod settings_cmds;
mod state;
mod tray;
mod update_cmds;

use boxdeck_core::jobs::{JobEvent, JobState};
use state::AppState;
use tauri::{Manager, WindowEvent};
use tauri_plugin_notification::NotificationExt;

/// Passed by the login item, so an automatic start stays in the tray.
const AUTOSTART_ARG: &str = "--autostart";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // Must be first: a second launch just brings the running window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args([AUTOSTART_ARG])
                .build(),
        )
        .manage(AppState::new())
        .manage(update_cmds::PendingUpdate::default())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_machines,
            commands::refresh_status,
            commands::run_action,
            commands::cancel_job,
            commands::list_jobs,
            commands::job_log,
            commands::subscribe_jobs,
            commands::ssh_open,
            commands::pty_rerun_job,
            commands::pty_write,
            commands::pty_resize,
            commands::pty_close,
            commands::open_folder,
            commands::list_boxes,
            commands::check_box_updates,
            commands::prune_dry_run,
            commands::run_box_op,
            commands::search_boxes,
            commands::popular_boxes,
            settings_cmds::get_settings,
            settings_cmds::save_settings,
            settings_cmds::pick_folder,
            settings_cmds::add_bookmark,
            settings_cmds::remove_bookmark,
            settings_cmds::read_vagrantfile,
            settings_cmds::save_vagrantfile,
            settings_cmds::validate_vagrantfile,
            settings_cmds::export_settings,
            settings_cmds::import_settings,
            settings_cmds::redetect_vagrant,
            mcp_cmds::mcp_clients,
            mcp_cmds::mcp_connect,
            mcp_cmds::mcp_disconnect,
            mcp_cmds::mcp_grants,
            mcp_cmds::mcp_revoke_grant,
            mcp_cmds::list_audit,
            approvals::get_approval,
            approvals::answer_approval,
            update_cmds::pending_app_update,
            update_cmds::check_app_update,
            update_cmds::install_app_update,
        ])
        .on_window_event(|window, event| {
            // Closing an approval window without answering counts as "no".
            if let WindowEvent::Destroyed = event
                && let Some(id) = window.label().strip_prefix("approval-")
            {
                approvals::dismissed(window.app_handle(), id);
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let keep = window.state::<AppState>().settings().app.keep_in_tray;
                if keep && window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = handle.state::<AppState>().settings();
            #[cfg(target_os = "macos")]
            if settings.app.menu_bar_only {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            tray::setup(&handle)?;
            let autostarted = std::env::args().any(|a| a == AUTOSTART_ARG);
            if !(settings.app.start_hidden || autostarted) {
                tray::show_main(&handle);
            }
            commands::watch_index(&handle);
            commands::watch_audit(&handle);
            approvals::start(&handle);
            notify_finished_jobs(&handle);
            // Dev builds would only ever find the published release.
            if settings.app.check_updates_on_launch && !cfg!(debug_assertions) {
                tauri::async_runtime::spawn(update_cmds::check_on_launch(handle.clone()));
            }
            tauri::async_runtime::spawn(commands::startup(handle));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        // macOS: clicking the Dock icon while the window is hidden brings it back.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            tray::show_main(app);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (app, event);
    });
}

/// System notification when a job ends while the window isn't focused, and a tray refresh
/// on every job change.
fn notify_finished_jobs(app: &tauri::AppHandle) {
    let mut rx = app.state::<AppState>().jobs.subscribe();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            let job = match rx.recv().await {
                Ok(JobEvent::Updated { job }) => job,
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            };
            tray::request_refresh(&app);
            let body = match job.state {
                JobState::Succeeded => format!("{} finished", job.label),
                JobState::Failed => match job.exit_code {
                    Some(code) => format!("{} failed (exit {code})", job.label),
                    None => format!("{} failed", job.label),
                },
                JobState::TimedOut => format!("{} timed out", job.label),
                _ => continue,
            };
            let state = app.state::<AppState>();
            if !state.settings().app.notify_on_finish {
                continue;
            }
            let focused = app
                .get_webview_window("main")
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false);
            if !focused {
                let _ = app
                    .notification()
                    .builder()
                    .title("Boxdeck")
                    .body(body)
                    .show();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    // Parsed from the file because a second `generate_context!` clashes on the embedded Info.plist symbol.
    fn config() -> serde_json::Value {
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("valid tauri.conf.json")
    }

    #[test]
    fn bundle_identifier_matches_project_decision() {
        assert_eq!(config()["identifier"], "io.github.oops-apps.boxdeck");
    }

    #[test]
    fn csp_is_set() {
        assert!(config()["app"]["security"]["csp"].is_string());
    }

    #[test]
    fn updater_reads_signed_releases_from_github() {
        let updater = &config()["plugins"]["updater"];
        assert!(!updater["pubkey"].as_str().unwrap_or_default().is_empty());
        assert_eq!(
            updater["endpoints"][0],
            "https://github.com/oops-apps/boxdeck/releases/latest/download/latest.json"
        );
        // Signed bundles only in the release build, which has the private key.
        assert_ne!(config()["bundle"]["createUpdaterArtifacts"], true);
        let release: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.updater.conf.json")).unwrap();
        assert_eq!(release["bundle"]["createUpdaterArtifacts"], true);
    }

    #[test]
    fn window_starts_hidden_until_setup_decides() {
        assert_eq!(config()["app"]["windows"][0]["visible"], false);
    }
}
