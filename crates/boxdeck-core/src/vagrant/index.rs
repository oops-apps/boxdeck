//! Read-only access to Vagrant's machine index (`$VAGRANT_HOME/data/machine-index/index`).
//!
//! Vagrant holds an exclusive `flock` on `index.lock` for every read and write and rewrites the
//! index in place, so we read under a shared lock on the same file and retry if we still catch a
//! partial write. We never write the index or create its lock file.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::vagrant::ProjectEnv;

const READ_ATTEMPTS: u32 = 5;
const RETRY_DELAY: Duration = Duration::from_millis(50);

/// Path of the index file under a Vagrant home directory.
pub fn index_path(vagrant_home: &Path) -> PathBuf {
    vagrant_home
        .join("data")
        .join("machine-index")
        .join("index")
}

/// `VAGRANT_HOME` if set, otherwise `~/.vagrant.d`.
pub fn default_vagrant_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("VAGRANT_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(home));
    }
    directories::BaseDirs::new().map(|d| d.home_dir().join(".vagrant.d"))
}

/// Vagrant writes `vagrantfile_name` as an array of names (or null); accept a bare string too.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum VagrantfileName {
    One(String),
    Many(Vec<String>),
}

impl VagrantfileName {
    pub fn first(&self) -> Option<&str> {
        match self {
            Self::One(s) => Some(s.as_str()),
            Self::Many(v) => v.first().map(String::as_str),
        }
    }
}

/// One raw index entry. Every field is optional: the format is internal to Vagrant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct RawEntry {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub architecture: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub vagrantfile_path: Option<String>,
    #[serde(default)]
    pub vagrantfile_name: Option<VagrantfileName>,
    #[serde(default)]
    pub local_data_path: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub extra_data: ExtraData,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ExtraData {
    /// The box the machine was created from; what `vagrant box remove` checks for "in use".
    #[serde(default, rename = "box")]
    pub box_ref: Option<BoxRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct BoxRef {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub architecture: Option<String>,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Deserialize)]
struct RawIndex {
    version: Option<u64>,
    #[serde(default)]
    machines: BTreeMap<String, RawEntry>,
}

/// A machine known to Vagrant, as recorded in the index (state may be stale).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct IndexedMachine {
    /// Full index id (uuid without dashes).
    pub id: String,
    pub name: String,
    pub provider: String,
    pub architecture: Option<String>,
    /// Last state Vagrant recorded, e.g. `running`, `poweroff`.
    pub state: String,
    pub env: ProjectEnv,
    /// The box it was created from, when Vagrant recorded one.
    pub box_ref: Option<BoxRef>,
}

/// Parses index JSON. Entries without a name or project folder are dropped, since nothing can
/// be done with them.
pub fn parse(json: &str, path: &Path) -> Result<Vec<IndexedMachine>> {
    let raw: RawIndex = serde_json::from_str(json).map_err(|_| Error::CorruptIndex {
        path: path.to_owned(),
    })?;
    match raw.version {
        Some(1) => {}
        Some(v) => {
            return Err(Error::UnsupportedIndexVersion {
                path: path.to_owned(),
                version: v,
            });
        }
        None => {
            return Err(Error::CorruptIndex {
                path: path.to_owned(),
            });
        }
    }
    let mut machines: Vec<IndexedMachine> = raw
        .machines
        .into_iter()
        .filter_map(|(id, e)| {
            let name = e.name.filter(|n| !n.is_empty())?;
            let dir = PathBuf::from(e.vagrantfile_path.filter(|p| !p.is_empty())?);
            let env = ProjectEnv::from_index(
                dir,
                e.vagrantfile_name.as_ref().and_then(VagrantfileName::first),
                e.local_data_path.as_deref(),
            );
            Some(IndexedMachine {
                id,
                name,
                provider: e.provider.unwrap_or_default(),
                architecture: e.architecture,
                state: e.state.unwrap_or_else(|| "unknown".to_owned()),
                env,
                box_ref: e.extra_data.box_ref,
            })
        })
        .collect();
    machines.sort_by(|a, b| (&a.env.dir, &a.name).cmp(&(&b.env.dir, &b.name)));
    Ok(machines)
}

/// Reads the index under a shared lock. A missing index means no machines yet.
pub fn read(path: &Path) -> Result<Vec<IndexedMachine>> {
    let mut last_err = None;
    for attempt in 0..READ_ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(RETRY_DELAY);
        }
        let json = match read_locked(path) {
            Ok(Some(json)) => json,
            Ok(None) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        match parse(&json, path) {
            Err(e @ Error::CorruptIndex { .. }) => last_err = Some(e),
            other => return other,
        }
    }
    Err(last_err.unwrap_or(Error::CorruptIndex {
        path: path.to_owned(),
    }))
}

fn read_locked(path: &Path) -> Result<Option<String>> {
    let lock_path = {
        let mut p = path.as_os_str().to_owned();
        p.push(".lock");
        PathBuf::from(p)
    };
    // Vagrant creates the lock file on first use; if it's absent, nobody is writing.
    let lock = File::open(&lock_path).ok();
    if let Some(lock) = &lock {
        lock.lock_shared()?;
    }
    let result = match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    };
    if let Some(lock) = &lock {
        lock.unlock()?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> &'static Path {
        Path::new("/x/index")
    }

    #[test]
    fn accepts_string_or_array_vagrantfile_name() {
        let json = r#"{"version":1,"machines":{
            "a":{"name":"one","vagrantfile_path":"/p","vagrantfile_name":["Custom.rb"]},
            "b":{"name":"two","vagrantfile_path":"/q","vagrantfile_name":"Other.rb"},
            "c":{"name":"three","vagrantfile_path":"/r","vagrantfile_name":null}}}"#;
        let m = parse(json, p()).unwrap();
        assert_eq!(m[0].env.vagrantfile.as_deref(), Some("Custom.rb"));
        assert_eq!(m[1].env.vagrantfile.as_deref(), Some("Other.rb"));
        assert_eq!(m[2].env.vagrantfile, None);
    }

    #[test]
    fn tolerates_missing_fields_and_drops_unusable_entries() {
        let json = r#"{"version":1,"machines":{
            "a":{"name":"ok","vagrantfile_path":"/p"},
            "b":{"vagrantfile_path":"/p"},
            "c":{"name":"no-dir"},
            "d":{"name":"empty-dir","vagrantfile_path":""}}}"#;
        let m = parse(json, p()).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].state, "unknown");
        assert_eq!(m[0].provider, "");
    }

    #[test]
    fn rejects_partial_json_and_unknown_versions() {
        assert!(matches!(
            parse(r#"{"version":1,"machi"#, p()),
            Err(Error::CorruptIndex { .. })
        ));
        assert!(matches!(
            parse(r#"{"version":2,"machines":{}}"#, p()),
            Err(Error::UnsupportedIndexVersion { version: 2, .. })
        ));
        assert!(matches!(
            parse(r#"{"machines":{}}"#, p()),
            Err(Error::CorruptIndex { .. })
        ));
    }

    #[test]
    fn reads_the_box_a_machine_uses() {
        let json = r#"{"version":1,"machines":{"a":{"name":"web","vagrantfile_path":"/p",
            "extra_data":{"box":{"name":"bento/ubuntu-24.04","provider":"vmware_desktop",
            "architecture":"arm64","version":"202508.03.0"}}}}}"#;
        let m = parse(json, p()).unwrap();
        let b = m[0].box_ref.as_ref().unwrap();
        assert_eq!(b.name, "bento/ubuntu-24.04");
        assert_eq!(b.architecture.as_deref(), Some("arm64"));
    }

    #[test]
    fn missing_index_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&dir.path().join("index")).unwrap().is_empty());
    }

    #[test]
    fn reads_under_existing_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index");
        std::fs::write(&path, r#"{"version":1,"machines":{}}"#).unwrap();
        std::fs::write(dir.path().join("index.lock"), "").unwrap();
        assert!(read(&path).unwrap().is_empty());
    }

    #[test]
    fn gives_up_on_persistently_corrupt_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index");
        std::fs::write(&path, "{").unwrap();
        assert!(matches!(read(&path), Err(Error::CorruptIndex { .. })));
    }
}
