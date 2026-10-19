//! What the MCP tools run against: Vagrant, a job registry, the shared settings and audit log,
//! and the bridge to the running app for approvals.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use boxdeck_core::audit::AuditLog;
pub use boxdeck_core::bridge::ApprovalRequest;
use boxdeck_core::bridge::Bridge;
use boxdeck_core::config::Settings;
use boxdeck_core::jobs::{JobId, Jobs};
use boxdeck_core::machines::{self, Project};
use boxdeck_core::policy;
use boxdeck_core::vagrant::ProjectEnv;
use boxdeck_core::vagrant::index;
use boxdeck_core::vagrant::locate::Vagrant;
use boxdeck_core::vagrant::status::StatusService;

pub type SettingsFn = Arc<dyn Fn() -> Settings + Send + Sync>;
pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
/// Called once a job an agent started has finished.
pub type JobDoneFn = Arc<dyn Fn(JobId) + Send + Sync>;

/// Reaches the user in the Boxdeck app. [`Bridge`] in production; a fake in tests.
pub trait Approver: Send + Sync {
    /// Whether the app is running to answer.
    fn app_running(&self) -> bool;
    /// Whether the user allowed shell access to this machine for the session.
    fn granted(&self, project: &Path, machine: &str) -> bool;
    /// Asks; `true` means allowed. No answer in time means no.
    fn ask(&self, req: ApprovalRequest) -> BoxFuture<bool>;
}

impl Approver for Bridge {
    fn app_running(&self) -> bool {
        Bridge::app_running(self)
    }

    fn granted(&self, project: &Path, machine: &str) -> bool {
        Bridge::granted(self, project, machine)
    }

    fn ask(&self, req: ApprovalRequest) -> BoxFuture<bool> {
        let bridge = self.clone();
        Box::pin(async move {
            bridge.ask(req).await.unwrap_or_else(|e| {
                eprintln!("boxdeck-mcp: could not ask for approval: {e}");
                false
            })
        })
    }
}

pub struct Ctx {
    pub vagrant: Result<Vagrant, String>,
    pub jobs: Jobs,
    pub status: Option<Arc<StatusService>>,
    pub settings: SettingsFn,
    pub index_path: Option<PathBuf>,
    pub vagrant_home: Option<PathBuf>,
    pub audit: Option<AuditLog>,
    pub approvals: Option<Arc<dyn Approver>>,
    pub on_job_done: Option<JobDoneFn>,
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
