//! Finding the `vagrant` binary and its version.

use std::path::{Path, PathBuf};
use std::time::Duration;

use semver::Version;

use crate::error::{Error, Result};
use crate::process::{self, CommandSpec};

/// Oldest Vagrant Boxdeck supports (plan §3.1).
pub const MIN_VERSION: Version = Version::new(2, 3, 0);
/// `box list` reports `box-architecture` from this version on.
const BOX_ARCHITECTURE_SINCE: Version = Version::new(2, 4, 0);

const VERSION_TIMEOUT: Duration = Duration::from_secs(30);

/// Imports the login shell's `PATH` so Homebrew and provider tools are found when the app is
/// launched from Finder/Dock or by an MCP client. Call first in `main()`, before any threads
/// start, because it sets environment variables. On Windows it only re-applies the inherited
/// `PATH` (fix-path-env behavior at the pinned revision). Failures are logged and ignored.
pub fn fix_path() {
    if let Err(e) = fix_path_env::fix() {
        tracing::warn!("could not import the login shell PATH: {e}");
    }
}

/// The configured override if set, otherwise `vagrant` found on `PATH`.
pub fn locate(override_path: Option<&Path>) -> Result<PathBuf> {
    match override_path {
        Some(p) if p.is_file() => Ok(p.to_owned()),
        Some(p) => Err(Error::VagrantPathInvalid(p.to_owned())),
        None => which::which("vagrant").map_err(|_| Error::VagrantNotFound),
    }
}

/// Parses `vagrant --version` output such as `Vagrant 2.4.9` or `Vagrant 2.4.10.dev`.
pub fn parse_version(output: &str) -> Result<Version> {
    let unparsable = || Error::UnparsableVersion(output.trim().to_owned());
    let word = output
        .split_whitespace()
        .skip_while(|w| !w.eq_ignore_ascii_case("vagrant"))
        .nth(1)
        .ok_or_else(unparsable)?;
    let mut nums = word.split(['.', '-', '+']).map(|p| p.parse::<u64>());
    let major = nums.next().and_then(|r| r.ok()).ok_or_else(unparsable)?;
    let minor = nums.next().and_then(|r| r.ok()).ok_or_else(unparsable)?;
    let patch = nums.next().and_then(|r| r.ok()).unwrap_or(0);
    Ok(Version::new(major, minor, patch))
}

/// Features that depend on the installed Vagrant version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct Features {
    pub box_architecture: bool,
}

impl Features {
    pub fn for_version(v: &Version) -> Self {
        Self {
            box_architecture: *v >= BOX_ARCHITECTURE_SINCE,
        }
    }
}

/// A located, version-checked Vagrant.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct Vagrant {
    pub path: PathBuf,
    pub version: Version,
    pub features: Features,
}

impl Vagrant {
    /// Locates Vagrant and checks it meets [`MIN_VERSION`].
    pub async fn detect(override_path: Option<&Path>) -> Result<Self> {
        let path = locate(override_path)?;
        let spec = CommandSpec::new(&path)
            .arg("--version")
            .env("VAGRANT_CHECKPOINT_DISABLE", "1");
        let out = process::run(&spec, VERSION_TIMEOUT).await?;
        if !out.success() {
            return Err(Error::Vagrant {
                class: None,
                message: format!("`vagrant --version` failed: {}", out.stderr.trim()),
                exit_code: out.exit_code,
            });
        }
        let version = parse_version(&out.stdout)?;
        if version < MIN_VERSION {
            return Err(Error::VagrantTooOld {
                found: version,
                minimum: MIN_VERSION,
            });
        }
        Ok(Self {
            features: Features::for_version(&version),
            path,
            version,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_and_dev_versions() {
        assert_eq!(
            parse_version("Vagrant 2.4.9\n").unwrap(),
            Version::new(2, 4, 9)
        );
        assert_eq!(
            parse_version("Vagrant 2.4.10.dev").unwrap(),
            Version::new(2, 4, 10)
        );
        assert_eq!(
            parse_version("Installed Version: Vagrant 2.3").unwrap(),
            Version::new(2, 3, 0)
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_version("").is_err());
        assert!(parse_version("vagrant").is_err());
        assert!(parse_version("Vagrant x.y").is_err());
    }

    #[test]
    fn gates_architecture_on_2_4() {
        assert!(!Features::for_version(&Version::new(2, 3, 7)).box_architecture);
        assert!(Features::for_version(&Version::new(2, 4, 0)).box_architecture);
    }

    #[test]
    fn override_must_be_a_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            locate(Some(dir.path())),
            Err(Error::VagrantPathInvalid(_))
        ));
        let file = dir.path().join("vagrant");
        std::fs::write(&file, "").unwrap();
        assert_eq!(locate(Some(&file)).unwrap(), file);
    }
}
