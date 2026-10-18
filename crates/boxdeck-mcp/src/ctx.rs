//! What the MCP tools run against. The headless binary builds its own; the app (M6b) shares
//! its job registry, status cache and settings, so behaviour is identical in both.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use boxdeck_core::audit::AuditLog;
use boxdeck_core::config::Settings;
use boxdeck_core::jobs::{JobId, Jobs};
use boxdeck_core::machines::{self, Project};
use boxdeck_core::policy::{self, Transport};
use boxdeck_core::vagrant::ProjectEnv;
use boxdeck_core::vagrant::index;
use boxdeck_core::vagrant::locate::Vagrant;
use boxdeck_core::vagrant::status::StatusService;

pub type SettingsFn = Arc<dyn Fn() -> Settings + Send + Sync>;
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
/// Asks the user to allow a destructive call (HTTP mode). `true` means allowed.
pub type ApproveFn = Arc<dyn Fn(ApprovalRequest) -> BoxFuture<bool> + Send + Sync>;
/// Called once a job an agent started has finished (the app refreshes its views).
pub type JobDoneFn = Arc<dyn Fn(JobId) + Send + Sync>;

/// Everything the approval prompt shows.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ApprovalRequest {
    pub client: String,
    pub tool: String,
    pub project: Option<PathBuf>,
    pub machine: Option<String>,
    pub arguments: serde_json::Value,
}

/// The authenticated client's name, attached to HTTP requests by the app's auth layer.
#[derive(Debug, Clone)]
pub struct ClientName(pub String);

/// Pulls the client name out of a request's extensions (HTTP mode).
pub type ClientFn = Arc<dyn Fn(&rmcp::model::Extensions) -> Option<String> + Send + Sync>;

pub struct Ctx {
    pub vagrant: Result<Vagrant, String>,
    pub jobs: Jobs,
    pub status: Option<Arc<StatusService>>,
    pub settings: SettingsFn,
    pub index_path: Option<PathBuf>,
    pub vagrant_home: Option<PathBuf>,
    pub transport: Transport,
    pub audit: Option<AuditLog>,
    pub approve: Option<ApproveFn>,
    pub on_job_done: Option<JobDoneFn>,
    pub client_from: Option<ClientFn>,
}

impl Ctx {
    pub fn vagrant(&self) -> Result<&Vagrant, String> {
        self.vagrant.as_ref().map_err(Clone::clone)
    }

    /// Projects agents may see: index, cached status and bookmarks, minus denied projects.
    pub async fn projects(&self) -> Result<Vec<Project>, String> {
        let indexed = match self.index_path.clone() {
            Some(path) => tokio::task::spawn_blocking(move || index::read(&path))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?,
            None => Vec::new(),
        };
        let statuses = match &self.status {
            Some(s) => s.all_cached().await,
            None => Vec::new(),
        };
        let settings = (self.settings)();
        let bookmarks: Vec<ProjectEnv> = settings.bookmarks.iter().map(ProjectEnv::new).collect();
        Ok(machines::merge(&indexed, &statuses, &bookmarks)
            .into_iter()
            .filter(|p| policy::project_allowed(&settings, &p.env.dir))
            .collect())
    }

    /// The visible project at `dir` (compared canonically).
    pub async fn resolve(&self, dir: &str) -> Result<Project, String> {
        let unknown = || "Unknown project folder.".to_owned();
        let wanted = Path::new(dir).canonicalize().map_err(|_| unknown())?;
        self.projects()
            .await?
            .into_iter()
            .find(|p| p.env.dir.canonicalize().is_ok_and(|d| d == wanted))
            .ok_or_else(unknown)
    }

    /// Re-checks a project's status (after a job, or on request). Skipped while a job runs in
    /// the project, because `vagrant status` would take the machine lock and break the job;
    /// the cached result stays until then.
    pub async fn refresh(&self, env: &ProjectEnv) -> Result<(), String> {
        let service = self
            .status
            .as_ref()
            .ok_or_else(|| "Vagrant isn't available.".to_owned())?;
        let vars = (self.settings)().project_env(&env.dir);
        match service.refresh_if_idle(&self.jobs, env, vars).await {
            Some(status) => status.result.map(|_| ()),
            None => Ok(()),
        }
    }
}
