//! Searching the public box catalog through `vagrant cloud search`, so the app needs no HTTP
//! client of its own and no account (searches are anonymous).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::process::CommandSpec;
use crate::validate;

/// One search result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct CatalogBox {
    pub name: String,
    /// Latest version.
    pub version: String,
    #[ts(type = "number")]
    pub downloads: u64,
    pub providers: Vec<String>,
    /// Architectures across all providers. The catalog's own architecture filter is loose, so
    /// callers should check this list.
    pub architectures: Vec<String>,
}

/// A box worth suggesting before the user searches (and when the catalog can't be reached).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct PopularBox {
    pub name: String,
    pub label: String,
    pub providers: Vec<String>,
    /// Empty when the box predates architecture metadata (in practice amd64).
    pub architectures: Vec<String>,
}

const BENTO: &[&str] = &["vmware_desktop", "virtualbox", "parallels", "utm"];
const BOTH: &[&str] = &["arm64", "amd64"];

/// Checked against the catalog on 2026-10-19 (`vagrant cloud search`).
const POPULAR: &[(&str, &str, &[&str], &[&str])] = &[
    ("bento/ubuntu-24.04", "Ubuntu 24.04 LTS", BENTO, BOTH),
    ("bento/ubuntu-22.04", "Ubuntu 22.04 LTS", BENTO, BOTH),
    (
        "bento/debian-12",
        "Debian 12",
        &["vmware_desktop", "virtualbox", "parallels"],
        BOTH,
    ),
    ("bento/fedora-latest", "Fedora", BENTO, BOTH),
    ("bento/rockylinux-9", "Rocky Linux 9", BENTO, BOTH),
    ("bento/almalinux-9", "AlmaLinux 9", BENTO, BOTH),
    // Canonical's own box; its last release is 22.04 and it's VirtualBox/amd64 only.
    (
        "ubuntu/jammy64",
        "Ubuntu 22.04 (Canonical)",
        &["virtualbox"],
        &[],
    ),
    (
        "laravel/homestead",
        "Laravel Homestead",
        &["virtualbox", "parallels"],
        BOTH,
    ),
];

pub fn popular() -> Vec<PopularBox> {
    let owned = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect();
    POPULAR
        .iter()
        .map(|(name, label, providers, archs)| PopularBox {
            name: (*name).to_owned(),
            label: (*label).to_owned(),
            providers: owned(providers),
            architectures: owned(archs),
        })
        .collect()
}

/// This computer's architecture as box metadata names it, if it's one boxes commonly use.
pub fn host_architecture() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "aarch64" => Some("arm64"),
        "x86_64" => Some("amd64"),
        _ => None,
    }
}

fn query(q: &str) -> Result<&str> {
    let q = q.trim();
    if q.is_empty() || q.len() > 100 {
        return Err(Error::invalid("search", "must be 1 to 100 characters"));
    }
    if q.starts_with('-') {
        return Err(Error::invalid("search", "must not start with '-'"));
    }
    if let Some(c) = q
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-' | '/')))
    {
        return Err(Error::invalid("search", format!("contains {c:?}")));
    }
    Ok(q)
}

/// `vagrant cloud search --json … QUERY`, anonymous. Never with `--no-tty`: `cloud search` then
/// takes the rest of the command line, flags included, as the query. It doesn't prompt with
/// `--no-auth`, and stdin is closed anyway.
pub fn search_command(
    vagrant: &Path,
    q: &str,
    provider: Option<&str>,
    architecture: Option<&str>,
    limit: u32,
) -> Result<CommandSpec> {
    let q = query(q)?;
    let mut args: Vec<String> = vec![
        "cloud".into(),
        "search".into(),
        "--json".into(),
        "--no-auth".into(),
        "--limit".into(),
        limit.clamp(1, 50).to_string(),
    ];
    if let Some(p) = provider {
        validate::provider(p)?;
        args.extend(["--provider".into(), p.into()]);
    }
    if let Some(a) = architecture {
        validate::architecture(a)?;
        args.extend(["--architecture".into(), a.into()]);
    }
    args.push(q.into());
    Ok(CommandSpec::new(vagrant).args(args))
}

#[derive(Deserialize)]
struct Raw {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    downloads: String,
    #[serde(default)]
    providers: String,
    #[serde(default)]
    architectures: String,
}

fn list(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in s.split(',').map(str::trim).filter(|i| !i.is_empty()) {
        if !out.iter().any(|o| o == item) {
            out.push(item.to_owned());
        }
    }
    out
}

/// Parses `cloud search --json`. Vagrant may print notices before the JSON, so parsing starts
/// at the first `[`. Downloads arrive as text like `"130,558"`.
pub fn parse_search(stdout: &str) -> Result<Vec<CatalogBox>> {
    let start = stdout
        .find('[')
        .ok_or_else(|| Error::invalid("search results", "no JSON in Vagrant's output"))?;
    let raw: Vec<Raw> = serde_json::from_str(stdout[start..].trim())
        .map_err(|e| Error::invalid("search results", e.to_string()))?;
    Ok(raw
        .into_iter()
        .filter(|r| validate::box_name(&r.name).is_ok())
        .map(|r| CatalogBox {
            downloads: r
                .downloads
                .chars()
                .filter(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or(0),
            providers: list(&r.providers),
            architectures: list(&r.architectures),
            name: r.name,
            version: r.version,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(spec: &CommandSpec) -> Vec<String> {
        spec.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn popular_boxes_are_valid_names() {
        let list = popular();
        assert!(list.iter().any(|b| b.name == "laravel/homestead"));
        for b in &list {
            assert!(validate::box_name(&b.name).is_ok(), "{}", b.name);
            assert!(!b.providers.is_empty());
        }
    }

    #[test]
    fn builds_an_anonymous_search() {
        let spec = search_command(
            Path::new("vagrant"),
            " ubuntu 24 ",
            Some("vmware_desktop"),
            Some("arm64"),
            10,
        )
        .unwrap();
        assert_eq!(
            args(&spec),
            [
                "cloud",
                "search",
                "--json",
                "--no-auth",
                "--limit",
                "10",
                "--provider",
                "vmware_desktop",
                "--architecture",
                "arm64",
                "ubuntu 24"
            ]
        );
    }

    #[test]
    fn rejects_unsafe_queries() {
        for q in ["", "-x", "a;b", "a\nb", &"x".repeat(101), "$(id)"] {
            assert!(
                search_command(Path::new("v"), q, None, None, 5).is_err(),
                "{q:?}"
            );
        }
        assert!(search_command(Path::new("v"), "ok", Some("--x"), None, 5).is_err());
    }

    #[test]
    fn parses_after_notices_and_dedups_lists() {
        let out = "Searching...\n[{\"name\":\"a/b\",\"version\":\"1\",\"downloads\":\"1,234\",\
                   \"providers\":\"x, y\",\"architectures\":\"arm64, amd64, arm64\"}]\n";
        let r = parse_search(out).unwrap();
        assert_eq!(r[0].downloads, 1234);
        assert_eq!(r[0].architectures, ["arm64", "amd64"]);
        assert!(parse_search("no results").is_err());
    }
}
