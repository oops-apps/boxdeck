//! Box management (plan §4.5): listing, sizes, in-use info, update checks and validated
//! add/update/remove/prune commands.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::process::CommandSpec;
use crate::vagrant::index::{BoxRef, IndexedMachine};
use crate::vagrant::machine_readable;
use crate::validate;

/// An installed box (one name/version/provider/architecture combination).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct BoxInfo {
    pub name: String,
    pub provider: String,
    pub version: String,
    /// `None` before Vagrant 2.4, or for boxes added without one.
    pub architecture: Option<String>,
    /// `info.json` contents, e.g. author or homepage.
    pub info: BTreeMap<String, String>,
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
    pub used_by: Vec<BoxUser>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct BoxUser {
    pub machine: String,
    pub project: PathBuf,
}

/// `box list -i --machine-readable`; never `--no-tty` (plan §3.1).
pub fn list_command(vagrant: &Path) -> CommandSpec {
    CommandSpec::new(vagrant).args(["box", "list", "-i", "--machine-readable"])
}

/// Parses `box list --machine-readable`. Each box starts at a `box-name` line.
pub fn parse_list(stdout: &str) -> Vec<BoxInfo> {
    let mut boxes: Vec<BoxInfo> = Vec::new();
    for line in machine_readable::parse(stdout) {
        let value = line.first().to_owned();
        if line.kind == "box-name" {
            boxes.push(BoxInfo {
                name: value,
                provider: String::new(),
                version: String::new(),
                architecture: None,
                info: BTreeMap::new(),
                size_bytes: None,
                used_by: Vec::new(),
            });
            continue;
        }
        let Some(b) = boxes.last_mut() else { continue };
        match line.kind.as_str() {
            "box-provider" => b.provider = value,
            "box-version" => b.version = value,
            "box-architecture" if value != "n/a" && !value.is_empty() => {
                b.architecture = Some(value)
            }
            "box-info" => {
                if let Some(v) = line.data.get(1) {
                    b.info.insert(value, v.clone());
                }
            }
            _ => {}
        }
    }
    boxes
}

/// Vagrant's on-disk name for a box: `/` (and `:` on Windows) escaped.
fn dir_name(name: &str) -> String {
    let name = if cfg!(windows) {
        name.replace(':', "-VAGRANTCOLON-")
    } else {
        name.to_owned()
    };
    name.replace('/', "-VAGRANTSLASH-")
}

/// Where a box lives under `VAGRANT_HOME`: `boxes/NAME/VERSION/[ARCH/]PROVIDER`.
pub fn box_dir(vagrant_home: &Path, b: &BoxInfo) -> PathBuf {
    let mut dir = vagrant_home
        .join("boxes")
        .join(dir_name(&b.name))
        .join(&b.version);
    if let Some(arch) = &b.architecture {
        dir.push(arch);
    }
    dir.join(&b.provider)
}

/// Total size of the files under `dir`; `None` if it can't be read.
pub fn dir_size(dir: &Path) -> Option<u64> {
    if !dir.is_dir() {
        return None;
    }
    Some(
        walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum(),
    )
}

/// Same rule as Vagrant's `Box#in_use?`: name, provider, architecture and version all match.
pub fn matches(b: &BoxInfo, r: &BoxRef) -> bool {
    r.name == b.name
        && r.provider == b.provider
        && r.version == b.version
        && r.architecture == b.architecture
}

/// Fills `used_by` and `size_bytes`.
pub fn annotate(boxes: &mut [BoxInfo], indexed: &[IndexedMachine], vagrant_home: Option<&Path>) {
    for b in boxes.iter_mut() {
        b.used_by = indexed
            .iter()
            .filter(|m| m.box_ref.as_ref().is_some_and(|r| matches(b, r)))
            .map(|m| BoxUser {
                machine: m.name.clone(),
                project: m.env.dir.clone(),
            })
            .collect();
        b.size_bytes = vagrant_home.and_then(|h| dir_size(&box_dir(h, b)));
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(tag = "status", rename_all = "snake_case")]
#[ts(export)]
pub enum UpdateStatus {
    UpToDate {
        version: String,
    },
    Outdated {
        current: String,
        latest: String,
    },
    /// Added from a file or URL rather than a catalog, so there's nothing to compare.
    NoMetadata,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct BoxUpdate {
    pub name: String,
    pub provider: String,
    pub status: UpdateStatus,
}

/// `box outdated --global --machine-readable`. Vagrant has no structured output for this, so
/// messages arrive as `ui` lines and are parsed by [`parse_outdated`].
pub fn outdated_command(vagrant: &Path) -> CommandSpec {
    CommandSpec::new(vagrant).args(["box", "outdated", "--global", "--machine-readable"])
}

/// Parses the messages of `box outdated --global` (templates in Vagrant's `en.yml`:
/// `box_up_to_date`, `box_outdated`, `box_outdated_no_metadata`, `box_outdated_metadata_error`).
/// Accepts machine-readable or plain output; unrecognised lines are skipped, and callers show
/// the raw output next to the result.
pub fn parse_outdated(stdout: &str) -> Vec<BoxUpdate> {
    let (lines, plain) = machine_readable::split(stdout);
    let messages: Vec<String> = if lines.is_empty() {
        plain.into_iter().map(str::to_owned).collect()
    } else {
        lines
            .into_iter()
            .filter(|l| l.kind == "ui")
            .filter_map(|l| l.data.get(1).cloned())
            .collect()
    };
    messages
        .iter()
        .filter_map(|m| parse_outdated_line(m))
        .collect()
}

fn parse_outdated_line(msg: &str) -> Option<BoxUpdate> {
    let rest = msg.trim().strip_prefix("* '")?;
    let (name, rest) = rest.split_once("' for '")?;
    let (provider, rest) = rest.split_once('\'')?;
    let status = if let Some(r) = rest.strip_prefix(" (v") {
        UpdateStatus::UpToDate {
            version: r.split_once(") is up to date")?.0.to_owned(),
        }
    } else if let Some(r) = rest.strip_prefix(" is outdated! Current: ") {
        let (current, latest) = r.split_once(". Latest: ")?;
        UpdateStatus::Outdated {
            current: current.to_owned(),
            latest: latest.trim_end().to_owned(),
        }
    } else if rest.starts_with(" wasn't added from a catalog") {
        UpdateStatus::NoMetadata
    } else {
        let r = rest.strip_prefix(": Error loading metadata: ")?;
        UpdateStatus::Error {
            message: r.to_owned(),
        }
    };
    Some(BoxUpdate {
        name: name.to_owned(),
        provider: provider.to_owned(),
        status,
    })
}

/// `box prune -n`: what a prune would remove, as Vagrant's own text.
pub fn prune_dry_run_command(vagrant: &Path, keep_used: bool) -> CommandSpec {
    let mut spec = CommandSpec::new(vagrant).args(["box", "prune", "-n"]);
    if keep_used {
        spec = spec.arg("-k");
    }
    spec
}

/// A box mutation, run as a job.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ts_rs::TS)]
#[serde(tag = "op", rename_all = "snake_case")]
#[ts(export)]
pub enum BoxOp {
    Add {
        /// Catalog name (`org/name`), `http(s)://` URL, or absolute path to a `.box` file.
        source: String,
        /// Required when the source is a URL or file.
        name: Option<String>,
        provider: Option<String>,
        architecture: Option<String>,
        version: Option<String>,
        /// `(type, value)`, e.g. `("sha256", "ab12…")`.
        checksum: Option<(String, String)>,
        force: bool,
    },
    Update {
        name: String,
        provider: Option<String>,
        architecture: Option<String>,
    },
    Remove {
        name: String,
        /// `None` removes every version.
        version: Option<String>,
        provider: Option<String>,
        /// Needed when several architectures share the version. Vagrant 2.4.9 can't remove a
        /// single one: it compares `--architecture` (a String) with installed ones (Symbols),
        /// so that flag never matches and is never passed.
        all_architectures: bool,
    },
    Prune {
        /// Keep boxes still used by a machine (`-k`).
        keep_used: bool,
    },
}

impl BoxOp {
    pub fn label(&self) -> String {
        match self {
            Self::Add { source, name, .. } => {
                format!("box add {}", name.as_deref().unwrap_or(source))
            }
            Self::Update { name, .. } => format!("box update {name}"),
            Self::Remove { name, version, .. } => match version {
                Some(v) => format!("box remove {name} {v}"),
                None => format!("box remove {name} (all versions)"),
            },
            Self::Prune { .. } => "box prune".to_owned(),
        }
    }

    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::Remove { .. } | Self::Prune { .. })
    }
}

fn source(s: &str) -> Result<()> {
    if s.starts_with("https://") || s.starts_with("http://") {
        if s.chars().any(|c| c.is_whitespace() || c.is_control()) || s.len() > 2048 {
            return Err(Error::invalid("box source", "is not a valid URL"));
        }
        return Ok(());
    }
    let path = Path::new(s);
    if path.is_absolute() {
        return if path.is_file() {
            Ok(())
        } else {
            Err(Error::invalid("box source", "file does not exist"))
        };
    }
    validate::box_name(s)
}

const CHECKSUM_TYPES: &[&str] = &["md5", "sha1", "sha256", "sha384", "sha512"];

/// Builds a validated, non-interactive argv. Remove and prune get `-f`, so callers must have
/// confirmed (or passed policy) first.
pub fn build_op(vagrant: &Path, op: &BoxOp) -> Result<CommandSpec> {
    let mut args: Vec<String> = vec!["box".into()];
    let opt = |args: &mut Vec<String>, flag: &str, value: &Option<String>| {
        if let Some(v) = value {
            args.extend([flag.to_owned(), v.clone()]);
        }
    };
    match op {
        BoxOp::Add {
            source: src,
            name,
            provider,
            architecture,
            version,
            checksum,
            force,
        } => {
            source(src)?;
            let is_catalog = validate::box_name(src).is_ok();
            if !is_catalog && name.is_none() {
                return Err(Error::invalid(
                    "box name",
                    "is required when adding from a URL or file",
                ));
            }
            // Catalog boxes are often published for several providers; without one, Vagrant
            // stops to ask, which fails without a terminal.
            if is_catalog && provider.is_none() {
                return Err(Error::invalid(
                    "provider",
                    "is required for catalog boxes, e.g. vmware_desktop or virtualbox",
                ));
            }
            check_opts(name.as_deref(), provider, architecture, version)?;
            args.extend(["add".into(), src.clone()]);
            opt(&mut args, "--name", name);
            opt(&mut args, "--provider", provider);
            opt(&mut args, "--architecture", architecture);
            opt(&mut args, "--box-version", version);
            if let Some((kind, value)) = checksum {
                validate::flag(kind, CHECKSUM_TYPES)?;
                if value.is_empty() || !value.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(Error::invalid("checksum", "must be hexadecimal"));
                }
                args.extend([
                    "--checksum".into(),
                    value.clone(),
                    "--checksum-type".into(),
                    kind.clone(),
                ]);
            }
            if *force {
                args.push("--force".into());
            }
        }
        BoxOp::Update {
            name,
            provider,
            architecture,
        } => {
            check_opts(Some(name), provider, architecture, &None)?;
            args.extend(["update".into(), "--box".into(), name.clone()]);
            opt(&mut args, "--provider", provider);
            opt(&mut args, "--architecture", architecture);
        }
        BoxOp::Remove {
            name,
            version,
            provider,
            all_architectures,
        } => {
            check_opts(Some(name), provider, &None, version)?;
            args.extend(["remove".into(), name.clone()]);
            match version {
                Some(v) => args.extend(["--box-version".into(), v.clone()]),
                None => args.push("--all".into()),
            }
            opt(&mut args, "--provider", provider);
            if *all_architectures {
                args.push("--all-architectures".into());
            }
            args.push("-f".into());
        }
        BoxOp::Prune { keep_used } => {
            args.extend(["prune".into(), "-f".into()]);
            if *keep_used {
                args.push("-k".into());
            }
        }
    }
    args.push("--no-tty".into());
    Ok(CommandSpec::new(vagrant).args(args))
}

fn check_opts(
    name: Option<&str>,
    provider: &Option<String>,
    architecture: &Option<String>,
    version: &Option<String>,
) -> Result<()> {
    if let Some(n) = name {
        validate::box_name(n)?;
    }
    if let Some(p) = provider {
        validate::provider(p)?;
    }
    if let Some(a) = architecture {
        validate::architecture(a)?;
    }
    if let Some(v) = version {
        validate::box_version(v)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(op: &BoxOp) -> Result<Vec<String>> {
        build_op(Path::new("vagrant"), op).map(|s| {
            s.args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        })
    }

    #[test]
    fn list_groups_lines_per_box() {
        let out = "1,,ui,info,a (vmware_desktop%!(VAGRANT_COMMA) 1.0)\n\
                   1,,box-name,org/a\n1,,box-provider,vmware_desktop\n1,,box-version,1.0\n\
                   1,,box-architecture,arm64\n1,,box-info,author,me\n\
                   1,,box-name,b\n1,,box-provider,virtualbox\n1,,box-version,0\n\
                   1,,box-architecture,n/a\n";
        let b = parse_list(out);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].architecture.as_deref(), Some("arm64"));
        assert_eq!(b[0].info["author"], "me");
        assert_eq!(b[1].architecture, None);
    }

    #[test]
    fn box_dirs_escape_slashes_and_include_architecture() {
        let mut b =
            parse_list("1,,box-name,org/a\n1,,box-provider,p\n1,,box-version,1.0\n").remove(0);
        assert_eq!(
            box_dir(Path::new("/h"), &b),
            Path::new("/h/boxes/org-VAGRANTSLASH-a/1.0/p")
        );
        b.architecture = Some("arm64".into());
        assert_eq!(
            box_dir(Path::new("/h"), &b),
            Path::new("/h/boxes/org-VAGRANTSLASH-a/1.0/arm64/p")
        );
    }

    #[test]
    fn sizes_sum_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("a"), [0u8; 10]).unwrap();
        std::fs::write(dir.path().join("sub/b"), [0u8; 5]).unwrap();
        assert_eq!(dir_size(dir.path()), Some(15));
        assert_eq!(dir_size(&dir.path().join("missing")), None);
    }

    #[test]
    fn outdated_messages() {
        let out = "1,,ui,success,* 'a/b' for 'virtualbox' (v1.2.3) is up to date\n\
                   1,,ui,warn,* 'c' for 'vmware_desktop' is outdated! Current: 1.0. Latest: 2.0\n\
                   1,,ui,info,* 'd' for 'docker' wasn't added from a catalog%!(VAGRANT_COMMA) no version information\n\
                   1,,ui,error,* 'e' for 'p': Error loading metadata: timeout%!(VAGRANT_COMMA) retry\n\
                   1,,ui,info,Checking for updates...\n";
        let u = parse_outdated(out);
        assert_eq!(u.len(), 4);
        assert_eq!(
            u[0].status,
            UpdateStatus::UpToDate {
                version: "1.2.3".into()
            }
        );
        assert_eq!(
            u[1].status,
            UpdateStatus::Outdated {
                current: "1.0".into(),
                latest: "2.0".into()
            }
        );
        assert_eq!(u[2].status, UpdateStatus::NoMetadata);
        assert_eq!(
            u[3].status,
            UpdateStatus::Error {
                message: "timeout, retry".into()
            }
        );
    }

    #[test]
    fn outdated_plain_text_too() {
        let u = parse_outdated("* 'x' for 'p' is outdated! Current: 1. Latest: 2\n");
        assert_eq!(u[0].name, "x");
    }

    #[test]
    fn remove_is_forced_and_needs_a_version_or_all() {
        let one = BoxOp::Remove {
            name: "org/a".into(),
            version: Some("1.0".into()),
            provider: Some("virtualbox".into()),
            all_architectures: true,
        };
        assert_eq!(
            args(&one).unwrap(),
            [
                "box",
                "remove",
                "org/a",
                "--box-version",
                "1.0",
                "--provider",
                "virtualbox",
                "--all-architectures",
                "-f",
                "--no-tty"
            ]
        );
        let all = BoxOp::Remove {
            name: "a".into(),
            version: None,
            provider: None,
            all_architectures: false,
        };
        assert_eq!(
            args(&all).unwrap(),
            ["box", "remove", "a", "--all", "-f", "--no-tty"]
        );
        assert!(all.is_destructive());
    }

    #[test]
    fn add_from_catalog_url_or_file() {
        let catalog = BoxOp::Add {
            source: "bento/ubuntu-24.04".into(),
            name: None,
            provider: Some("vmware_desktop".into()),
            architecture: Some("arm64".into()),
            version: None,
            checksum: None,
            force: false,
        };
        assert_eq!(
            args(&catalog).unwrap(),
            [
                "box",
                "add",
                "bento/ubuntu-24.04",
                "--provider",
                "vmware_desktop",
                "--architecture",
                "arm64",
                "--no-tty"
            ]
        );
        let url = BoxOp::Add {
            source: "https://example.com/x.box".into(),
            name: None,
            provider: None,
            architecture: None,
            version: None,
            checksum: Some(("sha256".into(), "abc123".into())),
            force: true,
        };
        assert!(args(&url).is_err(), "URL needs a name");
        let url = BoxOp::Add {
            source: "https://example.com/x.box".into(),
            name: Some("my/box".into()),
            provider: None,
            architecture: None,
            version: None,
            checksum: Some(("sha256".into(), "abc123".into())),
            force: true,
        };
        let a = args(&url).unwrap();
        assert!(a.ends_with(&["--force".into(), "--no-tty".into()]));
        assert!(a.contains(&"--checksum-type".into()));
    }

    #[test]
    fn catalog_boxes_need_a_provider() {
        let op = BoxOp::Add {
            source: "bento/ubuntu-24.04".into(),
            name: None,
            provider: None,
            architecture: None,
            version: None,
            checksum: None,
            force: false,
        };
        let err = args(&op).unwrap_err().to_string();
        assert!(err.contains("provider"), "{err}");
    }

    #[test]
    fn rejects_bad_inputs() {
        for source in ["-f", "ftp://x", "/no/such/file.box", "a b", "https://x/ y"] {
            let op = BoxOp::Add {
                source: source.into(),
                name: Some("n".into()),
                provider: None,
                architecture: None,
                version: None,
                checksum: None,
                force: false,
            };
            assert!(args(&op).is_err(), "{source}");
        }
        let bad_sum = BoxOp::Add {
            source: "a".into(),
            name: None,
            provider: Some("virtualbox".into()),
            architecture: None,
            version: None,
            checksum: Some(("crc".into(), "00".into())),
            force: false,
        };
        assert!(args(&bad_sum).is_err());
        let update = BoxOp::Update {
            name: "a".into(),
            provider: Some("--x".into()),
            architecture: None,
        };
        assert!(args(&update).is_err());
    }

    #[test]
    fn prune_variants() {
        assert_eq!(
            args(&BoxOp::Prune { keep_used: true }).unwrap(),
            ["box", "prune", "-f", "-k", "--no-tty"]
        );
        let dry = prune_dry_run_command(Path::new("vagrant"), false);
        assert!(!dry.args.iter().any(|a| a == "--no-tty"));
    }
}
