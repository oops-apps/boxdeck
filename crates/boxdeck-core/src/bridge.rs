//! How `boxdeck-mcp` asks the running app for approval (plan §4.8): request and answer files in
//! the per-user data dir, no network. The app holds a lock on `app.lock` while it runs, so the
//! MCP side can tell whether anyone is there to answer. The app also keeps the session grants
//! ("allow this machine until Boxdeck restarts") here.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// No answer within this long means no.
pub const TIMEOUT: Duration = Duration::from_secs(120);
const POLL: Duration = Duration::from_millis(250);
const REQUEST: &str = ".request.json";
const ANSWER: &str = ".answer.json";

/// Everything the approval prompt shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub client: String,
    pub tool: String,
    pub project: Option<PathBuf>,
    pub machine: Option<String>,
    pub arguments: serde_json::Value,
    /// Shell access to a machine: the user may allow it for the rest of the session.
    #[serde(default)]
    pub grantable: bool,
    /// Host folders the VM can write, shown with shell requests.
    #[serde(default)]
    pub host_paths: Vec<PathBuf>,
    /// Milliseconds since the epoch.
    #[serde(default)]
    pub created_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Answer {
    allow: bool,
}

/// One machine the user let agents use a shell on until the app restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct Grant {
    pub project: PathBuf,
    pub machine: String,
    #[ts(type = "number")]
    pub granted_at: u64,
}

#[derive(Debug, Clone)]
pub struct Bridge {
    dir: PathBuf,
}

impl Bridge {
    /// The bridge in Boxdeck's data directory.
    pub fn default_bridge() -> Result<Self> {
        Ok(Self::new(crate::config::data_dir()?))
    }

    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn approvals(&self) -> PathBuf {
        self.dir.join("approvals")
    }

    pub fn approvals_dir(&self) -> Result<PathBuf> {
        let dir = self.approvals();
        private_dir(&dir)?;
        Ok(dir)
    }

    fn presence_file(&self) -> Result<File> {
        private_dir(&self.dir)?;
        Ok(OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.dir.join("app.lock"))?)
    }

    /// App side: held for the app's lifetime; dropping the file releases it.
    pub fn hold_presence(&self) -> Result<File> {
        let file = self.presence_file()?;
        file.try_lock()
            .map_err(|_| Error::invalid("app.lock", "another Boxdeck app holds it"))?;
        Ok(file)
    }

    /// MCP side: whether the app is running to answer.
    pub fn app_running(&self) -> bool {
        let Ok(file) = self.presence_file() else {
            return false;
        };
        match file.try_lock() {
            Ok(()) => {
                let _ = file.unlock();
                false
            }
            Err(std::fs::TryLockError::WouldBlock) => true,
            Err(std::fs::TryLockError::Error(_)) => false,
        }
    }

    /// MCP side: files a request and waits for the app's answer. `false` when the app doesn't
    /// answer in time or stops running.
    pub async fn ask(&self, mut req: ApprovalRequest) -> Result<bool> {
        let dir = self.approvals_dir()?;
        let id = new_id();
        req.created_at = crate::jobs::now_ms();
        let request = dir.join(format!("{id}{REQUEST}"));
        let answer = dir.join(format!("{id}{ANSWER}"));
        write_atomic(&dir, &request, &serde_json::to_vec(&req).map_err(io)?)?;
        let deadline = tokio::time::Instant::now() + TIMEOUT;
        let allowed = loop {
            if let Ok(text) = std::fs::read(&answer) {
                break serde_json::from_slice::<Answer>(&text).is_ok_and(|a| a.allow);
            }
            if tokio::time::Instant::now() >= deadline || !self.app_running() {
                break false;
            }
            tokio::time::sleep(POLL).await;
        };
        let _ = std::fs::remove_file(&request);
        let _ = std::fs::remove_file(&answer);
        Ok(allowed)
    }

    /// App side: unanswered requests, oldest first.
    pub fn pending(&self) -> Vec<(String, ApprovalRequest)> {
        let mut found: Vec<_> = std::fs::read_dir(self.approvals())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let id = name.strip_suffix(REQUEST)?.to_owned();
                if self.approvals().join(format!("{id}{ANSWER}")).exists() {
                    return None;
                }
                let req = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
                Some((id, req))
            })
            .collect();
        found.sort_by_key(|(_, r): &(String, ApprovalRequest)| r.created_at);
        found
    }

    /// App side: answers a request. Unknown or already answered ids are ignored, so a window
    /// closing after its answer can't overwrite it.
    pub fn answer(&self, id: &str, allow: bool) -> Result<()> {
        if !valid_id(id) {
            return Err(Error::invalid("approval id", "is malformed"));
        }
        let dir = self.approvals_dir()?;
        if !dir.join(format!("{id}{REQUEST}")).exists()
            || dir.join(format!("{id}{ANSWER}")).exists()
        {
            return Ok(());
        }
        let body = serde_json::to_vec(&Answer { allow }).map_err(io)?;
        write_atomic(&dir, &dir.join(format!("{id}{ANSWER}")), &body)
    }

    /// App side, at startup: drops leftovers from a previous run, grants included.
    pub fn reset(&self) -> Result<()> {
        if let Ok(entries) = std::fs::read_dir(self.approvals()) {
            for e in entries.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        self.save_grants(&[])
    }

    fn grants_path(&self) -> PathBuf {
        self.dir.join("grants.json")
    }

    pub fn grants(&self) -> Vec<Grant> {
        std::fs::read(self.grants_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_grants(&self, grants: &[Grant]) -> Result<()> {
        private_dir(&self.dir)?;
        let body = serde_json::to_vec_pretty(grants).map_err(io)?;
        write_atomic(&self.dir, &self.grants_path(), &body)
    }

    pub fn granted(&self, project: &Path, machine: &str) -> bool {
        self.grants()
            .iter()
            .any(|g| g.project == project && g.machine == machine)
    }

    /// App side: allows shell access to a machine until the app restarts.
    pub fn grant(&self, project: &Path, machine: &str) -> Result<()> {
        let mut grants = self.grants();
        if !grants
            .iter()
            .any(|g| g.project == project && g.machine == machine)
        {
            grants.push(Grant {
                project: project.to_owned(),
                machine: machine.to_owned(),
                granted_at: crate::jobs::now_ms(),
            });
        }
        self.save_grants(&grants)
    }

    pub fn revoke(&self, project: &Path, machine: &str) -> Result<()> {
        let mut grants = self.grants();
        grants.retain(|g| !(g.project == project && g.machine == machine));
        self.save_grants(&grants)
    }
}

fn io(e: serde_json::Error) -> Error {
    std::io::Error::other(e).into()
}

fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    // Unique across processes without a random source: pid, time and a counter.
    format!(
        "{}-{}-{}",
        std::process::id(),
        crate::jobs::now_ms(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_digit() || b == b'-')
}

fn private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    Ok(())
}

fn write_atomic(dir: &Path, path: &Path, body: &[u8]) -> Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(body)?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ApprovalRequest {
        ApprovalRequest {
            client: "stdio:test".into(),
            tool: "ssh_exec".into(),
            project: Some("/p".into()),
            machine: Some("web".into()),
            arguments: serde_json::json!({"command": "ls"}),
            grantable: true,
            host_paths: vec!["/p".into()],
            created_at: 0,
        }
    }

    #[test]
    fn presence_follows_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path());
        assert!(!bridge.app_running());
        let held = bridge.hold_presence().unwrap();
        assert!(bridge.app_running());
        assert!(bridge.hold_presence().is_err());
        drop(held);
        assert!(!bridge.app_running());
    }

    #[tokio::test]
    async fn app_answers_a_request() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path());
        let _held = bridge.hold_presence().unwrap();
        let app = bridge.clone();
        let answering = tokio::spawn(async move {
            loop {
                if let Some((id, req)) = app.pending().pop() {
                    assert_eq!(req.tool, "ssh_exec");
                    assert!(req.created_at > 0);
                    app.answer(&id, true).unwrap();
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        assert!(bridge.ask(request()).await.unwrap());
        answering.await.unwrap();
        // Both files are cleaned up.
        assert_eq!(
            std::fs::read_dir(bridge.approvals()).unwrap().count(),
            0,
            "leftover files"
        );
    }

    #[tokio::test]
    async fn no_app_means_no() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path());
        assert!(!bridge.ask(request()).await.unwrap());
    }

    #[test]
    fn the_first_answer_wins() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path());
        let approvals = bridge.approvals_dir().unwrap();
        std::fs::write(approvals.join("1-1-1.request.json"), "{}").unwrap();
        bridge.answer("1-1-1", true).unwrap();
        bridge.answer("1-1-1", false).unwrap();
        let answer = std::fs::read_to_string(approvals.join("1-1-1.answer.json")).unwrap();
        assert!(answer.contains("true"), "{answer}");
    }

    #[test]
    fn answer_rejects_path_tricks() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path());
        assert!(bridge.answer("../grants", true).is_err());
        // Unknown but well-formed ids are a no-op.
        bridge.answer("1-2-3", true).unwrap();
        assert!(!bridge.approvals().join("1-2-3.answer.json").exists());
    }

    #[test]
    fn grants_add_revoke_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path());
        let p = Path::new("/p");
        assert!(!bridge.granted(p, "web"));
        bridge.grant(p, "web").unwrap();
        bridge.grant(p, "web").unwrap();
        assert_eq!(bridge.grants().len(), 1);
        assert!(bridge.granted(p, "web"));
        assert!(!bridge.granted(p, "db"));
        bridge.revoke(p, "web").unwrap();
        assert!(!bridge.granted(p, "web"));
        bridge.grant(p, "db").unwrap();
        std::fs::create_dir_all(bridge.approvals()).unwrap();
        std::fs::write(bridge.approvals().join("9-9-9.request.json"), "{}").unwrap();
        bridge.reset().unwrap();
        assert!(bridge.grants().is_empty());
        assert!(bridge.pending().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn folders_are_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(dir.path().join("data"));
        let approvals = bridge.approvals_dir().unwrap();
        let mode = std::fs::metadata(approvals).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
