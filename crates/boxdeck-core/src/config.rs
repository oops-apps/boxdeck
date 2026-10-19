//! Versioned settings JSON, shared by the app (the only writer) and `boxdeck-mcp` (reader).

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Current settings schema version. Bump it and add a step to [`migrate`] when the shape changes.
pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(export)]
pub struct Settings {
    pub version: u32,
    /// Explicit `vagrant` binary; `None` means find it on `PATH`.
    pub vagrant_path: Option<PathBuf>,
    /// Prepended to `PATH` for every Vagrant call.
    pub extra_path: Vec<PathBuf>,
    /// Environment variables for every Vagrant call. Stored in plain JSON; not for secrets.
    pub env: BTreeMap<String, String>,
    pub status: StatusSettings,
    pub jobs: JobSettings,
    pub app: AppSettings,
    /// Project folders added by hand; they stay listed after `destroy` and before first `up`.
    pub bookmarks: Vec<PathBuf>,
    /// Per-project customisation, keyed by the project folder as Vagrant records it.
    pub projects: BTreeMap<String, ProjectSettings>,
    pub mcp: McpSettings,
}

/// What agents may do (plan §4.8). Off until the user turns it on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(export)]
pub struct McpSettings {
    pub enabled: bool,
    pub access: AccessLevel,
    /// Kill switch: every call is refused while set. Shared with `boxdeck-mcp` via this file.
    pub paused: bool,
    /// Headless (stdio) mode can't ask for approval, so destructive tools are refused there
    /// unless this is on.
    pub headless_destructive: bool,
    /// `ssh_exec` runs arbitrary commands in a VM, so it needs its own opt-in.
    pub ssh_exec: bool,
    /// Projects without an explicit allow/deny follow this.
    pub default_access: DefaultProjectAccess,
    /// Port of the in-app HTTP endpoint (M6b).
    pub port: u16,
    /// Named bearer tokens, one per client (M6b). Never exported.
    pub clients: Vec<McpClient>,
    /// The audit log rotates when it grows past this.
    #[ts(type = "number")]
    pub audit_max_bytes: u64,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            access: AccessLevel::Read,
            paused: false,
            headless_destructive: false,
            ssh_exec: false,
            default_access: DefaultProjectAccess::AllProjects,
            port: 47_816,
            clients: Vec::new(),
            audit_max_bytes: 5_000_000,
        }
    }
}

/// Ordered: each level includes the ones before it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ts_rs::TS,
)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AccessLevel {
    /// Look only.
    #[default]
    Read,
    /// Lifecycle actions and box add/update.
    Operate,
    /// Destructive tools too, each one approved.
    Full,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum DefaultProjectAccess {
    #[default]
    AllProjects,
    OnlyAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct McpClient {
    pub name: String,
    pub token: String,
    #[ts(type = "number")]
    pub created_at: u64,
}

/// Colour tags the UI knows how to draw.
pub const COLOR_TAGS: &[&str] = &["red", "orange", "yellow", "green", "blue", "purple", "gray"];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(export)]
pub struct ProjectSettings {
    pub alias: Option<String>,
    /// Groups projects in the machine list, e.g. "Work" or "Clients".
    pub group: Option<String>,
    pub color: Option<String>,
    pub pinned: bool,
    pub hidden: bool,
    /// Added to every Vagrant call for this project, e.g. `VAGRANT_VAGRANTFILE`.
    pub env: BTreeMap<String, String>,
    /// Defaults for `vagrant up` started from the app or tray.
    pub up: UpDefaults,
    /// Whether agents (MCP) may see and act on this project.
    pub agent_access: AgentAccess,
    pub machines: BTreeMap<String, MachineSettings>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(export)]
pub struct UpDefaults {
    pub provider: Option<String>,
    /// `Some(false)` passes `--no-provision`.
    pub provision: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AgentAccess {
    /// Follow the global MCP setting.
    #[default]
    Inherit,
    Deny,
    Allow,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(export)]
pub struct MachineSettings {
    pub alias: Option<String>,
    pub color: Option<String>,
    pub pinned: bool,
    pub hidden: bool,
}

/// Window, tray and startup behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(export)]
pub struct AppSettings {
    /// Closing the window hides it to the tray instead of quitting.
    pub keep_in_tray: bool,
    pub start_hidden: bool,
    /// macOS: no Dock icon, tray only. Takes effect on next launch.
    pub menu_bar_only: bool,
    pub launch_at_login: bool,
    /// System notification when a job finishes while the window isn't focused.
    pub notify_on_finish: bool,
    /// Look for a new Boxdeck release once at startup.
    pub check_updates_on_launch: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            keep_in_tray: true,
            start_hidden: false,
            menu_bar_only: false,
            launch_at_login: false,
            notify_on_finish: true,
            check_updates_on_launch: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(export)]
pub struct StatusSettings {
    /// Parallel `vagrant status` calls.
    pub concurrency: usize,
    /// Background refresh interval; `None` means off.
    #[ts(type = "number | null")]
    pub interval_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(export)]
pub struct JobSettings {
    /// Maximum jobs running at once across all machines.
    pub global_cap: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            vagrant_path: None,
            extra_path: Vec::new(),
            // Skips Vagrant's version-check network call on every invocation.
            env: BTreeMap::from([("VAGRANT_CHECKPOINT_DISABLE".to_owned(), "1".to_owned())]),
            status: StatusSettings::default(),
            jobs: JobSettings::default(),
            app: AppSettings::default(),
            bookmarks: Vec::new(),
            projects: BTreeMap::new(),
            mcp: McpSettings::default(),
        }
    }
}

impl Default for StatusSettings {
    fn default() -> Self {
        Self {
            concurrency: 2,
            interval_secs: None,
        }
    }
}

impl Default for JobSettings {
    fn default() -> Self {
        Self { global_cap: 3 }
    }
}

impl Settings {
    pub fn project(&self, dir: &Path) -> Option<&ProjectSettings> {
        self.projects.get(&dir.to_string_lossy().into_owned())
    }

    /// Global plus per-project environment; project values win.
    pub fn project_env(&self, dir: &Path) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        let mut env = self.command_env();
        if let Some(p) = self.project(dir) {
            env.extend(p.env.iter().map(|(k, v)| (k.into(), v.into())));
        }
        env
    }

    /// Rejects values that would break Vagrant calls or the UI. Run before saving anything a
    /// user or an imported file supplied.
    pub fn validate(&self) -> Result<()> {
        if let Some(p) = &self.vagrant_path
            && !p.is_absolute()
        {
            return Err(Error::invalid("vagrant path", "must be an absolute path"));
        }
        if !(1..=8).contains(&self.status.concurrency) {
            return Err(Error::invalid("status concurrency", "must be 1 to 8"));
        }
        if self.status.interval_secs.is_some_and(|s| s < 10) {
            return Err(Error::invalid(
                "refresh interval",
                "must be at least 10 seconds",
            ));
        }
        if !(1..=16).contains(&self.jobs.global_cap) {
            return Err(Error::invalid("concurrent jobs", "must be 1 to 16"));
        }
        env_names(&self.env)?;
        if self.mcp.port < 1024 {
            return Err(Error::invalid("MCP port", "must be 1024 or higher"));
        }
        for c in &self.mcp.clients {
            label("MCP client name", &Some(c.name.clone()))?;
        }
        for b in &self.bookmarks {
            if !b.is_absolute() {
                return Err(Error::invalid("bookmark", "must be an absolute path"));
            }
        }
        for (dir, p) in &self.projects {
            if !Path::new(dir).is_absolute() {
                return Err(Error::invalid("project", "key must be an absolute path"));
            }
            label("alias", &p.alias)?;
            label("group", &p.group)?;
            color(&p.color)?;
            env_names(&p.env)?;
            if let Some(provider) = &p.up.provider {
                crate::validate::provider(provider)?;
            }
            for (name, m) in &p.machines {
                crate::validate::machine_name(name)?;
                label("alias", &m.alias)?;
                color(&m.color)?;
            }
        }
        Ok(())
    }
}

fn env_names(env: &BTreeMap<String, String>) -> Result<()> {
    for (k, v) in env {
        let mut chars = k.chars();
        let ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok || k.len() > 128 {
            return Err(Error::invalid(
                "environment variable",
                format!("{k:?} is not a valid name"),
            ));
        }
        if v.contains('\0') {
            return Err(Error::invalid(
                "environment variable",
                "value contains a NUL byte",
            ));
        }
    }
    Ok(())
}

fn label(field: &'static str, value: &Option<String>) -> Result<()> {
    if let Some(v) = value
        && (v.trim().is_empty() || v.chars().count() > 64 || v.chars().any(char::is_control))
    {
        return Err(Error::invalid(
            field,
            "must be 1 to 64 printable characters",
        ));
    }
    Ok(())
}

fn color(value: &Option<String>) -> Result<()> {
    if let Some(c) = value
        && !COLOR_TAGS.contains(&c.as_str())
    {
        return Err(Error::invalid(
            "color",
            format!("{c:?} is not one of {COLOR_TAGS:?}"),
        ));
    }
    Ok(())
}

impl Settings {
    /// Environment for every Vagrant call: the configured variables, plus `PATH` with
    /// `extra_path` prepended when set.
    pub fn command_env(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        let mut env: Vec<(std::ffi::OsString, std::ffi::OsString)> =
            self.env.iter().map(|(k, v)| (k.into(), v.into())).collect();
        if !self.extra_path.is_empty() {
            let current = std::env::var_os("PATH").unwrap_or_default();
            let paths = self
                .extra_path
                .iter()
                .cloned()
                .chain(std::env::split_paths(&current));
            if let Ok(joined) = std::env::join_paths(paths) {
                env.push(("PATH".into(), joined));
            }
        }
        env
    }
}

/// Upgrades raw settings JSON from version `_from` to [`SETTINGS_VERSION`], one version at a
/// time. Version 1 is the first, so there are no steps yet.
pub fn migrate(value: serde_json::Value, _from: u32) -> serde_json::Value {
    value
}

/// Per-OS data directory (backups, audit log).
pub fn data_dir() -> Result<PathBuf> {
    directories::ProjectDirs::from("io.github", "oops-apps", "Boxdeck")
        .map(|d| d.data_dir().to_owned())
        .ok_or(Error::NoConfigDir)
}

/// Default settings file in the per-OS config directory.
pub fn default_path() -> Result<PathBuf> {
    directories::ProjectDirs::from("io.github", "oops-apps", "Boxdeck")
        .map(|d| d.config_dir().join("settings.json"))
        .ok_or(Error::NoConfigDir)
}

/// Loads and saves the settings file.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads settings, applying migrations. A missing file gives defaults.
    pub fn load(&self) -> Result<Settings> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(e) => return Err(e.into()),
        };
        let invalid = |source| Error::SettingsInvalid {
            path: self.path.clone(),
            source,
        };
        let value: serde_json::Value = serde_json::from_str(&text).map_err(invalid)?;
        let found = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1) as u32;
        if found > SETTINGS_VERSION {
            // Refuse rather than silently dropping fields a newer Boxdeck wrote.
            return Err(Error::SettingsTooNew {
                path: self.path.clone(),
                found,
                supported: SETTINGS_VERSION,
            });
        }
        let mut settings: Settings =
            serde_json::from_value(migrate(value, found)).map_err(invalid)?;
        settings.version = SETTINGS_VERSION;
        Ok(settings)
    }

    /// Writes settings atomically (temp file in the same folder, then rename), user-only on Unix.
    pub fn save(&self, settings: &Settings) -> Result<()> {
        let dir = self
            .path
            .parent()
            .ok_or_else(|| Error::invalid("settings path", "has no parent folder"))?;
        std::fs::create_dir_all(dir)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tmp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        let json =
            serde_json::to_vec_pretty(settings).map_err(|source| Error::SettingsInvalid {
                path: self.path.clone(),
                source,
            })?;
        tmp.write_all(&json)?;
        tmp.write_all(b"\n")?;
        tmp.as_file().sync_all()?;
        tmp.persist(&self.path).map_err(|e| e.error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_env_prepends_extra_path() {
        let s = Settings {
            extra_path: vec!["/opt/tools".into()],
            ..Default::default()
        };
        let env = s.command_env();
        assert!(env.contains(&("VAGRANT_CHECKPOINT_DISABLE".into(), "1".into())));
        let path = &env.iter().find(|(k, _)| k == "PATH").unwrap().1;
        assert_eq!(
            std::env::split_paths(path).next().unwrap(),
            std::path::PathBuf::from("/opt/tools")
        );
        assert!(
            Settings::default()
                .command_env()
                .iter()
                .all(|(k, _)| k != "PATH")
        );
    }

    #[test]
    fn update_check_defaults_on_for_older_settings_files() {
        let app: AppSettings = serde_json::from_str(r#"{"keep_in_tray": false}"#).unwrap();
        assert!(app.check_updates_on_launch);
    }

    #[test]
    fn validation_catches_bad_values() {
        assert!(Settings::default().validate().is_ok());
        let bad_env = Settings {
            env: BTreeMap::from([("1BAD".into(), "x".into())]),
            ..Default::default()
        };
        assert!(bad_env.validate().is_err());
        let mut s = Settings::default();
        s.projects.insert(
            "/p".into(),
            ProjectSettings {
                color: Some("chartreuse".into()),
                ..Default::default()
            },
        );
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.projects
            .insert("relative".into(), ProjectSettings::default());
        assert!(s.validate().is_err());
        let s = Settings {
            status: StatusSettings {
                concurrency: 0,
                interval_secs: None,
            },
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn project_env_overrides_global() {
        let mut s = Settings {
            env: BTreeMap::from([("A".into(), "global".into())]),
            ..Default::default()
        };
        s.projects.insert(
            "/p".into(),
            ProjectSettings {
                env: BTreeMap::from([("A".into(), "project".into())]),
                ..Default::default()
            },
        );
        let env = s.project_env(Path::new("/p"));
        let last_a = env.iter().rev().find(|(k, _)| k == "A").unwrap();
        assert_eq!(last_a.1, "project");
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = SettingsStore::new(dir.path().join("settings.json"))
            .load()
            .unwrap();
        assert_eq!(s, Settings::default());
        assert_eq!(s.env["VAGRANT_CHECKPOINT_DISABLE"], "1");
    }

    #[test]
    fn round_trips_and_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("a").join("settings.json"));
        let s = Settings {
            vagrant_path: Some("/opt/vagrant/bin/vagrant".into()),
            status: StatusSettings {
                concurrency: 4,
                interval_secs: Some(60),
            },
            ..Default::default()
        };
        store.save(&s).unwrap();
        assert_eq!(store.load().unwrap(), s);
    }

    #[cfg(unix)]
    #[test]
    fn file_is_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));
        store.save(&Settings::default()).unwrap();
        let mode = std::fs::metadata(store.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(
            &path,
            r#"{"version":1,"status":{"concurrency":5},"future":true}"#,
        )
        .unwrap();
        let s = SettingsStore::new(&path).load().unwrap();
        assert_eq!(s.status.concurrency, 5);
        assert_eq!(s.jobs.global_cap, 3);
    }

    #[test]
    fn newer_version_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"version":99}"#).unwrap();
        assert!(matches!(
            SettingsStore::new(&path).load(),
            Err(Error::SettingsTooNew { found: 99, .. })
        ));
    }

    #[test]
    fn invalid_json_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{").unwrap();
        assert!(matches!(
            SettingsStore::new(&path).load(),
            Err(Error::SettingsInvalid { .. })
        ));
    }
}
