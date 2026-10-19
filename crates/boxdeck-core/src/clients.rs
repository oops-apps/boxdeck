//! Registers `boxdeck-mcp` in MCP clients' own config files (plan §4.8, Settings → MCP →
//! Connect). Only the `boxdeck` entry is ever added or removed; everything else in the file,
//! including key order and TOML comments, is kept.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::error::{Error, Result};

/// The entry name in every client's config.
pub const ENTRY: &str = "boxdeck";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ClientKind {
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
    GeminiCli,
    Codex,
}

impl ClientKind {
    pub const ALL: [Self; 5] = [
        Self::ClaudeCode,
        Self::ClaudeDesktop,
        Self::Cursor,
        Self::GeminiCli,
        Self::Codex,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Cursor => "Cursor",
            Self::GeminiCli => "Gemini CLI",
            Self::Codex => "Codex",
        }
    }
}

/// Where clients keep their config: the home folder and the OS app-config folder
/// (`~/Library/Application Support`, `%APPDATA%`, `~/.config`). Injected so tests never touch
/// the real ones.
#[derive(Debug, Clone)]
pub struct Homes {
    pub home: PathBuf,
    pub app_config: PathBuf,
    /// `$CODEX_HOME`, when set.
    pub codex_home: Option<PathBuf>,
}

impl Homes {
    pub fn detect() -> Result<Self> {
        let dirs = directories::BaseDirs::new().ok_or(Error::NoConfigDir)?;
        Ok(Self {
            home: dirs.home_dir().to_owned(),
            app_config: dirs.config_dir().to_owned(),
            codex_home: std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute()),
        })
    }

    /// The client's config file, and the folder whose presence means it's installed.
    fn locate(&self, kind: ClientKind) -> (PathBuf, PathBuf) {
        let h = &self.home;
        match kind {
            ClientKind::ClaudeCode => (h.join(".claude.json"), h.join(".claude")),
            ClientKind::ClaudeDesktop => {
                let dir = self.app_config.join("Claude");
                (dir.join("claude_desktop_config.json"), dir)
            }
            ClientKind::Cursor => (h.join(".cursor/mcp.json"), h.join(".cursor")),
            ClientKind::GeminiCli => (h.join(".gemini/settings.json"), h.join(".gemini")),
            ClientKind::Codex => {
                let dir = self.codex_home.clone().unwrap_or_else(|| h.join(".codex"));
                (dir.join("config.toml"), dir)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct ClientStatus {
    pub kind: ClientKind,
    pub name: String,
    pub config_path: PathBuf,
    /// Its config folder exists.
    pub installed: bool,
    /// The command registered for `boxdeck`, if any.
    pub registered: Option<PathBuf>,
    /// Registered, but not to this Boxdeck's `boxdeck-mcp` (moved, updated or deleted).
    pub stale: bool,
    /// The config file exists but couldn't be read; Connect won't overwrite it.
    pub error: Option<String>,
}

/// Every supported client's state, for the Connect list.
pub fn list(homes: &Homes, server: &Path) -> Vec<ClientStatus> {
    ClientKind::ALL
        .iter()
        .map(|&k| status(homes, k, server))
        .collect()
}

pub fn status(homes: &Homes, kind: ClientKind, server: &Path) -> ClientStatus {
    let (config_path, dir) = homes.locate(kind);
    let (registered, error) = match registered(kind, &config_path) {
        Ok(r) => (r, None),
        Err(e) => (None, Some(e.to_string())),
    };
    let stale = registered
        .as_ref()
        .is_some_and(|r| r != server || !r.exists());
    ClientStatus {
        kind,
        name: kind.name().to_owned(),
        installed: dir.is_dir() || config_path.is_file(),
        config_path,
        registered,
        stale,
        error,
    }
}

/// Adds or replaces the `boxdeck` entry.
pub fn connect(homes: &Homes, kind: ClientKind, server: &Path) -> Result<()> {
    let (path, _) = homes.locate(kind);
    let path = resolve_link(&path);
    let text = read_optional(&path)?;
    let out = match kind {
        ClientKind::Codex => {
            let mut doc = parse_toml(&path, &text)?;
            let servers = doc
                .entry("mcp_servers")
                .or_insert_with(|| {
                    let mut t = toml_edit::Table::new();
                    t.set_implicit(true);
                    toml_edit::Item::Table(t)
                })
                .as_table_like_mut()
                .ok_or_else(|| invalid(&path, "mcp_servers isn't a table"))?;
            let mut entry = toml_edit::Table::new();
            entry["command"] = toml_edit::value(server.to_string_lossy().as_ref());
            entry["args"] = toml_edit::value(toml_edit::Array::new());
            servers.insert(ENTRY, toml_edit::Item::Table(entry));
            doc.to_string()
        }
        _ => {
            let mut root = parse_json(&path, &text)?;
            let servers = root
                .entry("mcpServers")
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or_else(|| invalid(&path, "mcpServers isn't an object"))?;
            let mut entry = json!({ "command": server, "args": [] });
            if kind == ClientKind::ClaudeCode {
                entry["type"] = "stdio".into();
            }
            servers.insert(ENTRY.into(), entry);
            json_text(&root)?
        }
    };
    write_keeping_mode(&path, out.as_bytes())
}

/// Removes the `boxdeck` entry, and the servers table if that leaves it empty.
pub fn disconnect(homes: &Homes, kind: ClientKind) -> Result<()> {
    let (path, _) = homes.locate(kind);
    let path = resolve_link(&path);
    if !path.exists() {
        return Ok(());
    }
    let text = read_optional(&path)?;
    let out = match kind {
        ClientKind::Codex => {
            let mut doc = parse_toml(&path, &text)?;
            let Some(servers) = doc
                .get_mut("mcp_servers")
                .and_then(|i| i.as_table_like_mut())
            else {
                return Ok(());
            };
            if servers.remove(ENTRY).is_none() {
                return Ok(());
            }
            if servers.is_empty() {
                doc.remove("mcp_servers");
            }
            doc.to_string()
        }
        _ => {
            let mut root = parse_json(&path, &text)?;
            let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
                return Ok(());
            };
            if servers.shift_remove(ENTRY).is_none() {
                return Ok(());
            }
            if servers.is_empty() {
                root.shift_remove("mcpServers");
            }
            json_text(&root)?
        }
    };
    write_keeping_mode(&path, out.as_bytes())
}

/// Copies `boxdeck-mcp` to `dir` once per `version` and returns the copy. For Linux AppImages,
/// whose mount path changes on every launch, so registered configs need a stable path.
pub fn stable_copy(server: &Path, dir: &Path, version: &str) -> Result<PathBuf> {
    let name = server
        .file_name()
        .ok_or_else(|| invalid(server, "has no file name"))?;
    let dest = dir.join(name);
    let marker = dir.join("boxdeck-mcp.version");
    if dest.is_file() && std::fs::read_to_string(&marker).is_ok_and(|v| v == version) {
        return Ok(dest);
    }
    std::fs::create_dir_all(dir)?;
    let tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::fs::copy(server, tmp.path())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o755))?;
    }
    tmp.persist(&dest).map_err(|e| e.error)?;
    std::fs::write(&marker, version)?;
    Ok(dest)
}

fn registered(kind: ClientKind, path: &Path) -> Result<Option<PathBuf>> {
    let text = read_optional(path)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let command = match kind {
        ClientKind::Codex => parse_toml(path, &text)?
            .get("mcp_servers")
            .and_then(|s| s.get(ENTRY))
            .and_then(|e| e.get("command"))
            .and_then(|c| c.as_str())
            .map(PathBuf::from),
        _ => parse_json(path, &text)?
            .get("mcpServers")
            .and_then(|s| s.get(ENTRY))
            .and_then(|e| e.get("command"))
            .and_then(Value::as_str)
            .map(PathBuf::from),
    };
    Ok(command)
}

fn invalid(path: &Path, why: &str) -> Error {
    Error::invalid("client config", format!("{}: {why}", path.display()))
}

fn read_optional(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

fn parse_json(path: &Path, text: &str) -> Result<Map<String, Value>> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(invalid(path, "isn't a JSON object")),
        Err(e) => Err(invalid(path, &format!("isn't valid JSON ({e})"))),
    }
}

fn parse_toml(path: &Path, text: &str) -> Result<toml_edit::DocumentMut> {
    text.parse()
        .map_err(|e| invalid(path, &format!("isn't valid TOML ({e})")))
}

fn json_text(root: &Map<String, Value>) -> Result<String> {
    let mut text = serde_json::to_string_pretty(root).map_err(std::io::Error::other)?;
    text.push('\n');
    Ok(text)
}

/// Dotfile managers often symlink these files; write through the link instead of replacing it.
fn resolve_link(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

fn write_keeping_mode(path: &Path, body: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| invalid(path, "has no parent folder"))?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(body)?;
    if let Ok(meta) = std::fs::metadata(path) {
        tmp.as_file().set_permissions(meta.permissions())?;
    }
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn homes(dir: &Path) -> Homes {
        Homes {
            home: dir.join("home"),
            app_config: dir.join("appcfg"),
            codex_home: None,
        }
    }

    fn server(dir: &Path) -> PathBuf {
        let p = dir.join("Boxdeck.app/boxdeck-mcp");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "").unwrap();
        p
    }

    #[test]
    fn json_round_trip_keeps_other_keys_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let (h, srv) = (homes(dir.path()), server(dir.path()));
        let cfg = h.home.join(".claude.json");
        std::fs::create_dir_all(&h.home).unwrap();
        let original = r#"{"zeta":1,"mcpServers":{"other":{"command":"x"}},"alpha":[1,2]}"#;
        std::fs::write(&cfg, original).unwrap();

        connect(&h, ClientKind::ClaudeCode, &srv).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["zeta", "mcpServers", "alpha"]);
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["boxdeck"]["command"], srv.to_str().unwrap());
        assert_eq!(v["mcpServers"]["boxdeck"]["type"], "stdio");

        let st = status(&h, ClientKind::ClaudeCode, &srv);
        assert_eq!(st.registered.as_deref(), Some(srv.as_path()));
        assert!(st.installed && !st.stale && st.error.is_none());

        disconnect(&h, ClientKind::ClaudeCode).unwrap();
        let back: Value = serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(back, serde_json::from_str::<Value>(original).unwrap());
    }

    #[test]
    fn creates_missing_files_and_drops_empty_tables() {
        let dir = tempfile::tempdir().unwrap();
        let (h, srv) = (homes(dir.path()), server(dir.path()));
        for kind in [
            ClientKind::ClaudeDesktop,
            ClientKind::Cursor,
            ClientKind::GeminiCli,
        ] {
            assert!(!status(&h, kind, &srv).installed);
            connect(&h, kind, &srv).unwrap();
            let st = status(&h, kind, &srv);
            assert!(st.installed, "{kind:?}");
            assert_eq!(st.registered.as_deref(), Some(srv.as_path()));
            let v: Value =
                serde_json::from_str(&std::fs::read_to_string(&st.config_path).unwrap()).unwrap();
            assert!(v["mcpServers"]["boxdeck"].get("type").is_none());
            disconnect(&h, kind).unwrap();
            assert_eq!(
                std::fs::read_to_string(&st.config_path).unwrap().trim(),
                "{}"
            );
        }
        assert!(
            h.app_config
                .join("Claude/claude_desktop_config.json")
                .exists()
        );
    }

    #[test]
    fn codex_toml_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let (h, srv) = (homes(dir.path()), server(dir.path()));
        let cfg = h.home.join(".codex/config.toml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        let original =
            "# my settings\nmodel = \"o4\" # fast\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        std::fs::write(&cfg, original).unwrap();

        connect(&h, ClientKind::Codex, &srv).unwrap();
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(
            text.starts_with("# my settings\nmodel = \"o4\" # fast\n"),
            "{text}"
        );
        assert!(text.contains("[mcp_servers.boxdeck]"), "{text}");
        let st = status(&h, ClientKind::Codex, &srv);
        assert_eq!(st.registered.as_deref(), Some(srv.as_path()));

        disconnect(&h, ClientKind::Codex).unwrap();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), original);
    }

    #[test]
    fn codex_home_is_respected() {
        let dir = tempfile::tempdir().unwrap();
        let srv = server(dir.path());
        let mut h = homes(dir.path());
        h.codex_home = Some(dir.path().join("codexhome"));
        connect(&h, ClientKind::Codex, &srv).unwrap();
        assert!(dir.path().join("codexhome/config.toml").exists());
    }

    #[test]
    fn stale_when_the_registered_path_moved() {
        let dir = tempfile::tempdir().unwrap();
        let (h, srv) = (homes(dir.path()), server(dir.path()));
        connect(&h, ClientKind::Cursor, &dir.path().join("old/boxdeck-mcp")).unwrap();
        let st = status(&h, ClientKind::Cursor, &srv);
        assert!(st.stale);
        connect(&h, ClientKind::Cursor, &srv).unwrap();
        assert!(!status(&h, ClientKind::Cursor, &srv).stale);
    }

    #[test]
    fn stable_copy_is_refreshed_per_version() {
        let dir = tempfile::tempdir().unwrap();
        let srv = server(dir.path());
        std::fs::write(&srv, "v1").unwrap();
        let bin = dir.path().join("bin");
        let copy = stable_copy(&srv, &bin, "1.0.0").unwrap();
        assert_eq!(copy, bin.join("boxdeck-mcp"));
        std::fs::write(&srv, "v2").unwrap();
        stable_copy(&srv, &bin, "1.0.0").unwrap();
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "v1");
        stable_copy(&srv, &bin, "1.1.0").unwrap();
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "v2");
    }

    #[test]
    fn broken_files_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (h, srv) = (homes(dir.path()), server(dir.path()));
        let cfg = h.home.join(".cursor/mcp.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, "{ oops").unwrap();
        assert!(connect(&h, ClientKind::Cursor, &srv).is_err());
        assert!(disconnect(&h, ClientKind::Cursor).is_err());
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "{ oops");
        assert!(status(&h, ClientKind::Cursor, &srv).error.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_symlinks_and_keeps_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (h, srv) = (homes(dir.path()), server(dir.path()));
        let real = dir.path().join("dotfiles/claude.json");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "{}").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::create_dir_all(&h.home).unwrap();
        std::os::unix::fs::symlink(&real, h.home.join(".claude.json")).unwrap();

        connect(&h, ClientKind::ClaudeCode, &srv).unwrap();
        assert!(
            std::fs::symlink_metadata(h.home.join(".claude.json"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(std::fs::read_to_string(&real).unwrap().contains("boxdeck"));
        let mode = std::fs::metadata(&real).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
