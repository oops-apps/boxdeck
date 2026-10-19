//! Input validators shared by IPC commands and MCP tools (plan §4.7). Everything that ends up in
//! a `vagrant` argv or working directory passes through here first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const MAX_NAME_LEN: usize = 128;

/// Project folders and machine names Boxdeck knows about, from the machine index, bookmarks and
/// status results. Paths are stored canonicalized.
#[derive(Debug, Clone, Default)]
pub struct Known {
    projects: BTreeMap<PathBuf, BTreeSet<String>>,
}

impl Known {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a project folder; non-existent folders are ignored.
    pub fn add_project(&mut self, dir: &Path) -> Option<PathBuf> {
        let dir = dir.canonicalize().ok()?;
        self.projects.entry(dir.clone()).or_default();
        Some(dir)
    }

    pub fn add_machine(&mut self, dir: &Path, name: &str) {
        if let Some(dir) = self.add_project(dir) {
            self.projects
                .entry(dir)
                .or_default()
                .insert(name.to_owned());
        }
    }

    /// Canonical path of a known project folder.
    pub fn project_dir(&self, dir: &Path) -> Result<PathBuf> {
        let canonical = dir
            .canonicalize()
            .map_err(|_| Error::invalid("project folder", "does not exist"))?;
        if self.projects.contains_key(&canonical) {
            Ok(canonical)
        } else {
            Err(Error::invalid(
                "project folder",
                "is not a known Vagrant project",
            ))
        }
    }

    /// A machine name known in the given project.
    pub fn machine<'a>(&self, dir: &Path, name: &'a str) -> Result<&'a str> {
        let dir = self.project_dir(dir)?;
        machine_name(name)?;
        if self.projects[&dir].contains(name) {
            Ok(name)
        } else {
            Err(Error::invalid(
                "machine name",
                format!("no machine named {name:?} in this project"),
            ))
        }
    }
}

fn not_an_option(field: &'static str, value: &str) -> Result<()> {
    if value.starts_with('-') {
        return Err(Error::invalid(field, "must not start with '-'"));
    }
    Ok(())
}

fn charset(field: &'static str, value: &str, allowed: impl Fn(char) -> bool) -> Result<()> {
    if value.is_empty() || value.len() > MAX_NAME_LEN {
        return Err(Error::invalid(
            field,
            format!("must be 1 to {MAX_NAME_LEN} characters"),
        ));
    }
    not_an_option(field, value)?;
    match value.chars().find(|c| !allowed(*c)) {
        Some(c) => Err(Error::invalid(field, format!("contains {c:?}"))),
        None => Ok(()),
    }
}

/// A Vagrant machine name: letters, digits, `.`, `_`, `-`.
pub fn machine_name(name: &str) -> Result<()> {
    charset("machine name", name, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')
    })
}

/// A command for an agent to run inside a VM: free-form, but bounded.
pub fn guest_command(command: &str) -> Result<()> {
    if command.trim().is_empty() || command.len() > 8192 || command.contains('\0') {
        return Err(Error::invalid("command", "must be 1 to 8192 characters"));
    }
    Ok(())
}

/// A path inside a VM.
pub fn guest_path(path: &str) -> Result<()> {
    if path.trim().is_empty() || path.len() > 4096 || path.contains(['\0', '\n', '\r']) {
        return Err(Error::invalid(
            "guest path",
            "must be 1 to 4096 characters on one line",
        ));
    }
    Ok(())
}

/// Resolves a host path for file transfer, relative to the project, and checks that it stays
/// inside it (after symlinks). Agents may move files between the VM and the project folder, not
/// anywhere else on the host. With `existing = false` only the parent has to exist.
pub fn inside(project: &Path, path: &str, existing: bool) -> Result<PathBuf> {
    let outside = || Error::invalid("host path", "must be inside the project folder");
    let root = project.canonicalize().map_err(|_| outside())?;
    let joined = root.join(path);
    let resolved = if existing {
        joined.canonicalize().map_err(|_| outside())?
    } else {
        let name = joined
            .file_name()
            .filter(|n| *n != ".." && *n != ".")
            .ok_or_else(outside)?
            .to_owned();
        let parent = joined.parent().ok_or_else(outside)?;
        parent.canonicalize().map_err(|_| outside())?.join(name)
    };
    if resolved.starts_with(&root) && resolved != root {
        Ok(resolved)
    } else {
        Err(outside())
    }
}

/// A box name: `name` or `org/name`, each part letters, digits, `.`, `_`, `-`.
pub fn box_name(name: &str) -> Result<()> {
    charset("box name", name, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/')
    })?;
    let parts: Vec<&str> = name.split('/').collect();
    if parts.len() > 2
        || parts
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return Err(Error::invalid("box name", "must be NAME or ORG/NAME"));
    }
    Ok(())
}

/// A provider name such as `vmware_desktop` or `virtualbox`.
pub fn provider(name: &str) -> Result<()> {
    charset("provider", name, |c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-')
    })
}

/// A box architecture such as `amd64` or `arm64`.
pub fn architecture(name: &str) -> Result<()> {
    charset("architecture", name, |c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'
    })
}

/// A box version or version constraint such as `1.2.3` or `>= 1.0, < 2.0`.
pub fn box_version(v: &str) -> Result<()> {
    charset("box version", v, |c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '.' | '-' | '+' | ' ' | ',' | '<' | '>' | '=' | '~')
    })
}

/// A flag that must be one of an explicit allowlist.
pub fn flag<'a>(value: &str, allowed: &[&'a str]) -> Result<&'a str> {
    allowed
        .iter()
        .copied()
        .find(|a| *a == value)
        .ok_or_else(|| Error::invalid("flag", format!("{value:?} is not allowed here")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_paths_stay_inside_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("proj");
        std::fs::create_dir_all(project.join("sub")).unwrap();
        std::fs::write(project.join("sub/a.txt"), "x").unwrap();
        std::fs::write(dir.path().join("secret"), "x").unwrap();
        let root = project.canonicalize().unwrap();

        assert_eq!(
            inside(&project, "sub/a.txt", true).unwrap(),
            root.join("sub/a.txt")
        );
        let abs = root.join("sub/a.txt");
        assert!(inside(&project, abs.to_str().unwrap(), true).is_ok());
        assert_eq!(
            inside(&project, "sub/new.bin", false).unwrap(),
            root.join("sub/new.bin")
        );
        for bad in [
            "../secret",
            "sub/../../secret",
            ".",
            "",
            "sub/..",
            "missing/x",
        ] {
            assert!(inside(&project, bad, false).is_err(), "{bad}");
        }
        assert!(inside(&project, "sub/none.txt", true).is_err());
        let secret = dir.path().join("secret");
        assert!(inside(&project, secret.to_str().unwrap(), true).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path(), project.join("escape")).unwrap();
            assert!(inside(&project, "escape/secret", true).is_err());
            assert!(inside(&project, "escape/new", false).is_err());
        }
    }

    #[test]
    fn guest_inputs() {
        assert!(guest_command("ls -la").is_ok());
        assert!(guest_command("").is_err());
        assert!(guest_command(&"x".repeat(8193)).is_err());
        assert!(guest_path("/srv/app").is_ok());
        assert!(guest_path("a\nb").is_err());
    }

    #[test]
    fn machine_names() {
        for ok in ["web", "db-1", "app_2", "a.b"] {
            assert!(machine_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-f",
            "--provider=x",
            "a b",
            "a;b",
            "a/b",
            "$(x)",
            &"x".repeat(129),
        ] {
            assert!(machine_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn box_names() {
        for ok in ["ubuntu", "hashicorp/bionic64", "bento/ubuntu-24.04"] {
            assert!(box_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-f", "a/b/c", "/a", "a/", "../x", "a b", "http://x"] {
            assert!(box_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn providers_architectures_versions() {
        assert!(provider("vmware_desktop").is_ok());
        assert!(provider("VMware").is_err());
        assert!(architecture("arm64").is_ok());
        assert!(architecture("-x").is_err());
        assert!(box_version(">= 1.0, < 2.0").is_ok());
        assert!(box_version("1.0;rm").is_err());
    }

    #[test]
    fn flags_must_be_allowlisted() {
        assert_eq!(
            flag("--provision", &["--provision"]).unwrap(),
            "--provision"
        );
        assert!(flag("--provision-with", &["--provision"]).is_err());
    }

    #[test]
    fn known_projects_and_machines() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let mut known = Known::new();
        known.add_machine(dir.path(), "web");
        assert!(known.project_dir(dir.path()).is_ok());
        assert!(known.project_dir(other.path()).is_err());
        assert!(known.project_dir(&dir.path().join("missing")).is_err());
        assert_eq!(known.machine(dir.path(), "web").unwrap(), "web");
        assert!(known.machine(dir.path(), "db").is_err());
        assert!(known.machine(dir.path(), "-f").is_err());
    }

    #[test]
    fn project_paths_are_compared_canonically() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("p")).unwrap();
        let mut known = Known::new();
        known.add_project(&dir.path().join("p"));
        let indirect = dir.path().join("p").join("..").join("p");
        assert!(known.project_dir(&indirect).is_ok());
    }
}
