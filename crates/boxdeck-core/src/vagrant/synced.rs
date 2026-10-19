//! Which host folders a machine's synced folders expose, read from the `synced_folders` file
//! Vagrant writes into the machine's data dir on `up`/`reload`. Shell access asks first when the
//! VM can write to the host (plan §4.8).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::ProjectEnv;

#[derive(Debug, Deserialize)]
struct Folder {
    hostpath: Option<PathBuf>,
    #[serde(default)]
    disabled: bool,
}

/// What the VM can do to host files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAccess {
    /// Only one-way copies (rsync) or no synced folders.
    None,
    /// The VM can write these host folders.
    Writable(Vec<PathBuf>),
    /// No readable record (never brought up, or an unknown format): treat as writable.
    Unknown,
}

impl HostAccess {
    pub fn writable(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Parses a `synced_folders` file. rsync only copies host to guest, so it can't change the host.
pub fn parse(text: &str) -> Option<HostAccess> {
    let by_type: BTreeMap<String, BTreeMap<String, Folder>> = serde_json::from_str(text).ok()?;
    let mut paths: Vec<PathBuf> = by_type
        .into_iter()
        .filter(|(kind, _)| kind != "rsync")
        .flat_map(|(_, folders)| folders.into_values())
        .filter(|f| !f.disabled)
        .filter_map(|f| f.hostpath)
        .collect();
    paths.sort();
    paths.dedup();
    Some(if paths.is_empty() {
        HostAccess::None
    } else {
        HostAccess::Writable(paths)
    })
}

/// Reads the record for `machine`, whatever provider it was brought up with.
pub fn host_access(env: &ProjectEnv, machine: &str) -> HostAccess {
    let data = env
        .dotfile_path
        .clone()
        .unwrap_or_else(|| env.dir.join(".vagrant"));
    let machine_dir = data.join("machines").join(machine);
    let found = std::fs::read_dir(&machine_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|provider| read(&provider.path().join("synced_folders")))
        .reduce(merge);
    found.unwrap_or(HostAccess::Unknown)
}

fn read(path: &Path) -> Option<HostAccess> {
    parse(&std::fs::read_to_string(path).ok()?)
}

fn merge(a: HostAccess, b: HostAccess) -> HostAccess {
    match (a, b) {
        (HostAccess::Writable(mut x), HostAccess::Writable(y)) => {
            x.extend(y);
            x.sort();
            x.dedup();
            HostAccess::Writable(x)
        }
        (w @ HostAccess::Writable(_), _) | (_, w @ HostAccess::Writable(_)) => w,
        (a, _) => a,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/synthetic/synced_folders.json");

    #[test]
    fn rsync_and_disabled_folders_dont_count() {
        assert_eq!(
            parse(FIXTURE),
            Some(HostAccess::Writable(vec!["/Users/dev/proj".into()]))
        );
    }

    #[test]
    fn rsync_only_is_not_writable() {
        let text = r#"{"rsync":{"/vagrant":{"guestpath":"/vagrant","hostpath":"/p"}}}"#;
        assert_eq!(parse(text), Some(HostAccess::None));
        assert_eq!(parse("{}"), Some(HostAccess::None));
    }

    #[test]
    fn garbage_is_unknown() {
        assert_eq!(parse("not json"), None);
        let dir = tempfile::tempdir().unwrap();
        let env = ProjectEnv::new(dir.path());
        assert_eq!(host_access(&env, "default"), HostAccess::Unknown);
        assert!(HostAccess::Unknown.writable());
    }

    #[test]
    fn reads_from_the_data_dir_and_respects_dotfile_path() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("custom-data");
        let provider = data.join("machines/web/vmware_desktop");
        std::fs::create_dir_all(&provider).unwrap();
        std::fs::write(provider.join("synced_folders"), FIXTURE).unwrap();
        let mut env = ProjectEnv::new(dir.path());
        assert_eq!(host_access(&env, "web"), HostAccess::Unknown);
        env.dotfile_path = Some(data);
        assert_eq!(
            host_access(&env, "web"),
            HostAccess::Writable(vec!["/Users/dev/proj".into()])
        );
    }
}
