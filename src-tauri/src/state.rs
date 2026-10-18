//! App-wide state shared by IPC commands, the tray and (later) the MCP endpoint.
//! All Vagrant logic lives in `boxdeck-core`; this only wires it together.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use boxdeck_core::config::{Settings, SettingsStore};
use boxdeck_core::jobs::{Jobs, JobsConfig};
use boxdeck_core::machines::{self, Project};
use boxdeck_core::pty::PtySession;
use boxdeck_core::vagrant::ProjectEnv;
use boxdeck_core::vagrant::index;
use boxdeck_core::vagrant::locate::Vagrant;
use boxdeck_core::vagrant::status::StatusService;
use boxdeck_core::validate::Known;
use boxdeck_core::watch::FileWatcher;

/// Result of locating Vagrant at startup (or after a settings change).
pub enum VagrantState {
    Detecting,
    Ready(Vagrant),
    Failed(String),
}

pub struct AppState {
    pub settings: RwLock<Settings>,
    store: Option<SettingsStore>,
    pub vagrant: RwLock<VagrantState>,
    pub status: RwLock<Option<Arc<StatusService>>>,
    pub jobs: Jobs,
    pub index_path: Option<PathBuf>,
    pub vagrant_home: Option<PathBuf>,
    /// Latest merged view, also the source of truth for validating project folders.
    projects: RwLock<Vec<Project>>,
    known: RwLock<Known>,
    pub watcher: Mutex<Option<FileWatcher>>,
    pub ptys: Mutex<HashMap<u32, PtySession>>,
    next_pty: AtomicU32,
    /// When the machine index last changed (ms since the epoch); some Vagrant action is
    /// usually running right then.
    pub last_index_change: AtomicU64,
}

fn read<T>(l: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(|e| e.into_inner())
}

fn write<T>(l: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(|e| e.into_inner())
}

impl AppState {
    pub fn new() -> Self {
        let store = boxdeck_core::config::default_path()
            .ok()
            .map(SettingsStore::new);
        let settings = match store.as_ref().map(SettingsStore::load) {
            Some(Ok(s)) => s,
            Some(Err(e)) => {
                tracing::warn!("using default settings: {e}");
                Settings::default()
            }
            None => Settings::default(),
        };
        let jobs = Jobs::new(JobsConfig {
            global_cap: settings.jobs.global_cap,
            // Tauri runs sync commands and tray handlers on the main thread, which has no Tokio
            // runtime; jobs run on Tauri's.
            runtime: Some(tauri::async_runtime::handle().inner().clone()),
            ..Default::default()
        });
        Self {
            index_path: index::default_vagrant_home().map(|h| index::index_path(&h)),
            vagrant_home: index::default_vagrant_home(),
            settings: RwLock::new(settings),
            store,
            vagrant: RwLock::new(VagrantState::Detecting),
            status: RwLock::new(None),
            jobs,
            projects: RwLock::new(Vec::new()),
            known: RwLock::new(Known::new()),
            watcher: Mutex::new(None),
            ptys: Mutex::new(HashMap::new()),
            next_pty: AtomicU32::new(1),
            last_index_change: AtomicU64::new(0),
        }
    }

    pub fn settings(&self) -> Settings {
        read(&self.settings).clone()
    }

    /// Changes settings, validates and writes them, then applies what can change live.
    /// Returns the new settings and whether the Vagrant path changed (needs re-detection).
    pub fn update_settings(
        &self,
        change: impl FnOnce(&mut Settings),
    ) -> Result<(Settings, bool), String> {
        let mut settings = write(&self.settings);
        let mut next = settings.clone();
        change(&mut next);
        next.validate().map_err(|e| e.to_string())?;
        if let Some(store) = &self.store {
            store.save(&next).map_err(|e| e.to_string())?;
        }
        let path_changed = next.vagrant_path != settings.vagrant_path;
        if next.status.concurrency != settings.status.concurrency
            && let VagrantState::Ready(v) = &*read(&self.vagrant)
        {
            *write(&self.status) = Some(Arc::new(StatusService::new(
                v.path.clone(),
                next.status.concurrency,
            )));
        }
        self.jobs.set_global_cap(next.jobs.global_cap);
        *settings = next.clone();
        Ok((next, path_changed))
    }

    pub fn vagrant(&self) -> Result<Vagrant, String> {
        match &*read(&self.vagrant) {
            VagrantState::Ready(v) => Ok(v.clone()),
            VagrantState::Detecting => Err("still looking for vagrant".into()),
            VagrantState::Failed(e) => Err(e.clone()),
        }
    }

    pub fn set_vagrant(&self, result: Result<Vagrant, String>) {
        let settings = self.settings();
        *write(&self.status) = result.as_ref().ok().map(|v| {
            Arc::new(StatusService::new(
                v.path.clone(),
                settings.status.concurrency,
            ))
        });
        *write(&self.vagrant) = match result {
            Ok(v) => VagrantState::Ready(v),
            Err(e) => VagrantState::Failed(e),
        };
    }

    pub fn status_service(&self) -> Option<Arc<StatusService>> {
        read(&self.status).clone()
    }

    /// Re-reads the machine index and merges it with cached status results.
    /// Every machine in Vagrant's index.
    pub async fn read_index(&self) -> Result<Vec<index::IndexedMachine>, String> {
        match self.index_path.clone() {
            Some(path) => tokio::task::spawn_blocking(move || index::read(&path))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string()),
            None => Ok(Vec::new()),
        }
    }

    pub async fn rebuild_projects(&self) -> Result<Vec<Project>, String> {
        let indexed = self.read_index().await?;
        let statuses = match self.status_service() {
            Some(s) => s.all_cached().await,
            None => Vec::new(),
        };
        let bookmarks: Vec<ProjectEnv> = self
            .settings()
            .bookmarks
            .iter()
            .map(ProjectEnv::new)
            .collect();
        let projects = machines::merge(&indexed, &statuses, &bookmarks);
        let mut known = Known::new();
        for p in &projects {
            known.add_project(&p.env.dir);
            for m in &p.machines {
                known.add_machine(&p.env.dir, &m.name);
            }
        }
        *write(&self.known) = known;
        *write(&self.projects) = projects.clone();
        Ok(projects)
    }

    /// The last merged machine list, without re-reading anything.
    pub fn current_projects(&self) -> Vec<Project> {
        read(&self.projects).clone()
    }

    /// The server-side environment for a project folder the UI sent. Client-supplied
    /// Vagrantfile names or data paths are never trusted.
    pub fn project_env(&self, dir: &Path) -> Result<ProjectEnv, String> {
        let canonical = read(&self.known)
            .project_dir(dir)
            .map_err(|e| e.to_string())?;
        read(&self.projects)
            .iter()
            .find(|p| p.env.dir.canonicalize().is_ok_and(|d| d == canonical))
            .map(|p| p.env.clone())
            .ok_or_else(|| "project folder is not known".to_owned())
    }

    pub fn check_machine(&self, dir: &Path, name: &str) -> Result<(), String> {
        read(&self.known)
            .machine(dir, name)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn next_pty_id(&self) -> u32 {
        self.next_pty.fetch_add(1, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxdeck_core::jobs::{JobRequest, JobState, Origin, Scope};
    use boxdeck_core::vagrant::actions::{self, Action};

    /// End-to-end against a real Vagrant and Docker. Needs `BOXDECK_E2E_HOME` (a VAGRANT_HOME
    /// with one Docker-backed project where `web` is running and `db` isn't created).
    #[tokio::test]
    #[ignore = "needs vagrant + docker; set BOXDECK_E2E_HOME"]
    async fn real_vagrant_round_trip() {
        let home = PathBuf::from(std::env::var("BOXDECK_E2E_HOME").expect("BOXDECK_E2E_HOME"));
        let mut state = AppState::new();
        state.index_path = Some(index::index_path(&home));
        state
            .settings
            .write()
            .unwrap()
            .env
            .insert("VAGRANT_HOME".into(), home.to_string_lossy().into_owned());
        state.set_vagrant(Vagrant::detect(None).await.map_err(|e| e.to_string()));

        let projects = state.rebuild_projects().await.unwrap();
        assert_eq!(projects.len(), 1);
        let dir = projects[0].env.dir.clone();
        assert_eq!(
            projects[0].machines.len(),
            1,
            "index only knows created machines"
        );

        let service = state.status_service().unwrap();
        service
            .refresh(&projects[0].env, state.settings().project_env(&dir))
            .await;
        let projects = state.rebuild_projects().await.unwrap();
        let names: Vec<_> = projects[0]
            .machines
            .iter()
            .map(|m| (m.name.as_str(), m.state.as_str()))
            .collect();
        assert_eq!(names, [("web", "running"), ("db", "not_created")]);

        // The UI sends the folder back as a string; it must resolve to the same project.
        let env = state.project_env(Path::new(&dir)).unwrap();
        state.check_machine(&env.dir, "db").unwrap();
        assert!(state.check_machine(&env.dir, "nope").is_err());

        let mut spec = actions::build(
            &state.vagrant().unwrap().path,
            &env,
            Action::Halt,
            Some("web"),
            &Default::default(),
        )
        .unwrap();
        spec.env.splice(0..0, state.settings().command_env());
        let id = state.jobs.submit(JobRequest {
            label: "halt web".into(),
            origin: Origin::Ui,
            scope: Scope::Machine {
                project: env.dir.clone(),
                name: "web".into(),
            },
            spec,
            timeout: None,
        });
        let job = state.jobs.wait(id).await.unwrap();
        assert_eq!(
            job.state,
            JobState::Succeeded,
            "{:?}",
            state.jobs.log(id, 50)
        );
        service
            .refresh(&env, state.settings().project_env(&env.dir))
            .await;
        let projects = state.rebuild_projects().await.unwrap();
        assert_eq!(projects[0].machines[0].state, "stopped");
    }
}
