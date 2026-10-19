//! Everything that talks to, or reads files owned by, the `vagrant` CLI.

pub mod actions;
pub mod boxes;
pub mod catalog;
pub mod details;
pub mod index;
pub mod locate;
pub mod machine_readable;
pub mod status;
pub mod synced;
pub mod vagrantfile;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// How to run Vagrant for one project: its folder plus any non-default Vagrantfile name or
/// data path recorded in the machine index. Without these, Vagrant evaluates the wrong file or
/// reports `not_created` (plan §4.1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct ProjectEnv {
    pub dir: PathBuf,
    /// `VAGRANT_VAGRANTFILE`, when the project doesn't use the default name.
    pub vagrantfile: Option<String>,
    /// `VAGRANT_DOTFILE_PATH`, when the data dir isn't `<dir>/.vagrant`.
    pub dotfile_path: Option<PathBuf>,
}

impl ProjectEnv {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            vagrantfile: None,
            dotfile_path: None,
        }
    }

    pub(crate) fn from_index(
        dir: PathBuf,
        vagrantfile: Option<&str>,
        local_data_path: Option<&str>,
    ) -> Self {
        let dotfile_path = local_data_path
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .filter(|p| *p != dir.join(".vagrant"));
        Self {
            vagrantfile: vagrantfile.filter(|v| !v.is_empty()).map(str::to_owned),
            dotfile_path,
            dir,
        }
    }

    /// Environment variables to set when running Vagrant for this project.
    pub fn vars(&self) -> Vec<(OsString, OsString)> {
        let mut vars = Vec::new();
        if let Some(name) = &self.vagrantfile {
            vars.push(("VAGRANT_VAGRANTFILE".into(), name.into()));
        }
        if let Some(path) = &self.dotfile_path {
            vars.push(("VAGRANT_DOTFILE_PATH".into(), path.into()));
        }
        vars
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_needs_no_env() {
        let env = ProjectEnv::from_index("/p".into(), None, Some("/p/.vagrant"));
        assert!(env.vars().is_empty());
    }

    #[test]
    fn custom_layout_sets_env() {
        let env = ProjectEnv::from_index("/p".into(), Some("Custom.rb"), Some("/data/p"));
        assert_eq!(
            env.vars(),
            vec![
                ("VAGRANT_VAGRANTFILE".into(), "Custom.rb".into()),
                ("VAGRANT_DOTFILE_PATH".into(), "/data/p".into()),
            ]
        );
    }
}
