//! Live machine state via `vagrant status --machine-readable`, one call per project folder.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{Mutex, Semaphore};

use crate::error::{Error, Result};
use crate::process::{self, CommandSpec};
use crate::vagrant::{ProjectEnv, machine_readable};

const STATUS_TIMEOUT: Duration = Duration::from_secs(120);

/// State of one machine as reported by `vagrant status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct MachineStatus {
    pub name: String,
    pub provider: String,
    /// State id, e.g. `running`, `poweroff`, `not_created`.
    pub state: String,
    pub state_short: String,
    pub state_long: String,
}

/// Parses `status --machine-readable` stdout into one entry per machine, in output order.
pub fn parse(stdout: &str) -> Vec<MachineStatus> {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, MachineStatus> = HashMap::new();
    for line in machine_readable::parse(stdout) {
        if line.target.is_empty() {
            continue;
        }
        let field = match line.kind.as_str() {
            "provider-name" | "state" | "state-human-short" | "state-human-long" => line.kind,
            _ => continue,
        };
        let entry = by_name.entry(line.target.clone()).or_insert_with(|| {
            order.push(line.target.clone());
            MachineStatus {
                name: line.target.clone(),
                provider: String::new(),
                state: String::new(),
                state_short: String::new(),
                state_long: String::new(),
            }
        });
        let value = line.data.into_iter().next().unwrap_or_default();
        match field.as_str() {
            "provider-name" => entry.provider = value,
            "state" => entry.state = value,
            "state-human-short" => entry.state_short = value,
            _ => entry.state_long = value,
        }
    }
    order
        .into_iter()
        .filter_map(|n| by_name.remove(&n))
        .collect()
}

/// Builds the status command. Never pass `--no-tty` here: it overrides `--machine-readable`.
pub fn command(vagrant: &std::path::Path, env: &ProjectEnv) -> CommandSpec {
    let mut spec = CommandSpec::new(vagrant)
        .args(["status", "--machine-readable"])
        .cwd(&env.dir);
    for (k, v) in env.vars() {
        spec = spec.env(k, v);
    }
    spec
}

/// A status result with when it was taken. Errors are kept so the UI can show a badge
/// (e.g. a Vagrantfile that fails to evaluate) instead of dropping the project.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectStatus {
    pub env: ProjectEnv,
    /// Milliseconds since the Unix epoch.
    pub checked_at: u64,
    pub result: std::result::Result<Vec<MachineStatus>, String>,
}

/// Runs status calls with a concurrency limit and caches the latest result per project.
pub struct StatusService {
    vagrant: PathBuf,
    limit: Arc<Semaphore>,
    cache: Mutex<HashMap<ProjectEnv, ProjectStatus>>,
}

impl StatusService {
    pub fn new(vagrant: PathBuf, concurrency: usize) -> Self {
        Self {
            vagrant,
            limit: Arc::new(Semaphore::new(concurrency.max(1))),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Runs `vagrant status` for one project now and caches the result. `extra_env` is the
    /// settings environment for that project (global plus per-project variables).
    pub async fn refresh(
        &self,
        env: &ProjectEnv,
        extra_env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    ) -> ProjectStatus {
        let result = self.query(env, extra_env).await.map_err(|e| e.to_string());
        let status = ProjectStatus {
            env: env.clone(),
            checked_at: crate::jobs::now_ms(),
            result,
        };
        self.cache.lock().await.insert(env.clone(), status.clone());
        status
    }

    /// Like [`Self::refresh`], but only when no job is queued or running in the project, and
    /// with jobs there held back until it's done (see [`crate::jobs::Jobs::try_hold_project`]).
    /// Returns `None` when it skipped.
    pub async fn refresh_if_idle(
        &self,
        jobs: &crate::jobs::Jobs,
        env: &ProjectEnv,
        extra_env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    ) -> Option<ProjectStatus> {
        let _hold = jobs.try_hold_project(&env.dir)?;
        Some(self.refresh(env, extra_env).await)
    }

    /// Drops a project's cached result, e.g. when the machine index shows it's out of date.
    pub async fn forget(&self, env: &ProjectEnv) {
        self.cache.lock().await.remove(env);
    }

    /// Last cached result for a project, if any.
    pub async fn cached(&self, env: &ProjectEnv) -> Option<ProjectStatus> {
        self.cache.lock().await.get(env).cloned()
    }

    /// Every cached result.
    pub async fn all_cached(&self) -> Vec<ProjectStatus> {
        self.cache.lock().await.values().cloned().collect()
    }

    async fn query(
        &self,
        env: &ProjectEnv,
        extra_env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    ) -> Result<Vec<MachineStatus>> {
        let _permit = self
            .limit
            .acquire()
            .await
            .map_err(|_| Error::invalid("status service", "shut down"))?;
        let mut spec = command(&self.vagrant, env);
        // Project vars go last so they win over global settings.
        let project_vars = std::mem::take(&mut spec.env);
        spec.env = extra_env;
        spec.env.extend(project_vars);
        let out = process::run(&spec, STATUS_TIMEOUT).await?;
        if !out.success() {
            return Err(machine_readable::failure(
                &out.stdout,
                &out.stderr,
                out.exit_code,
            ));
        }
        Ok(parse(&out.stdout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_lines_by_target_and_skips_others() {
        let out = "1,web,metadata,provider,docker\n\
                   1,web,provider-name,docker\n\
                   1,web,state,running\n\
                   1,db,state,not_created\n\
                   1,,ui,info,Current machine states:\n\
                   1,web,state-human-long,a%!(VAGRANT_COMMA)b\\nc\n";
        let s = parse(out);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "web");
        assert_eq!(s[0].provider, "docker");
        assert_eq!(s[0].state, "running");
        assert_eq!(s[0].state_long, "a,b\nc");
        assert_eq!(s[1].name, "db");
        assert_eq!(s[1].provider, "");
    }

    #[test]
    fn command_never_uses_no_tty_and_sets_project_env() {
        let env = ProjectEnv {
            dir: "/p".into(),
            vagrantfile: Some("Custom.rb".into()),
            dotfile_path: None,
        };
        let spec = command(std::path::Path::new("/usr/bin/vagrant"), &env);
        assert_eq!(spec.args, vec!["status", "--machine-readable"]);
        assert!(!spec.args.iter().any(|a| a == "--no-tty"));
        assert_eq!(spec.cwd.as_deref(), Some(std::path::Path::new("/p")));
        assert_eq!(
            spec.env,
            vec![("VAGRANT_VAGRANTFILE".into(), "Custom.rb".into())]
        );
    }
}
