//! Reading and safely saving a project's Vagrantfile (plan §4.4). Vagrantfiles are Ruby; Boxdeck
//! never evaluates them itself, only through `vagrant validate`.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::process::CommandSpec;
use crate::vagrant::ProjectEnv;

/// Editing anything bigger is almost certainly the wrong file.
const MAX_BYTES: u64 = 1024 * 1024;
const KEEP_BACKUPS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct VagrantfileText {
    pub path: PathBuf,
    pub content: String,
}

/// The Vagrantfile Vagrant would load for this project: the recorded name, else `Vagrantfile`,
/// else `vagrantfile`.
pub fn path(env: &ProjectEnv) -> PathBuf {
    if let Some(name) = &env.vagrantfile {
        return env.dir.join(name);
    }
    let upper = env.dir.join("Vagrantfile");
    let lower = env.dir.join("vagrantfile");
    if !upper.is_file() && lower.is_file() {
        lower
    } else {
        upper
    }
}

/// Whether a folder looks like a Vagrant project.
pub fn exists(dir: &Path, name: Option<&str>) -> bool {
    path(&ProjectEnv {
        dir: dir.to_owned(),
        vagrantfile: name.map(str::to_owned),
        dotfile_path: None,
    })
    .is_file()
}

pub fn read(env: &ProjectEnv) -> Result<VagrantfileText> {
    let path = path(env);
    let meta = std::fs::metadata(&path)?;
    if meta.len() > MAX_BYTES {
        return Err(Error::invalid("Vagrantfile", "is larger than 1 MB"));
    }
    Ok(VagrantfileText {
        content: std::fs::read_to_string(&path)?,
        path,
    })
}

/// Saves `content` if the file still matches `base` (what the editor loaded). Keeps a
/// timestamped backup under `backups_dir`, not next to the Vagrantfile, so the user's repo
/// stays clean, and writes atomically so a crash never leaves half a file.
/// Returns the backup's path.
pub fn save(env: &ProjectEnv, base: &str, content: &str, backups_dir: &Path) -> Result<PathBuf> {
    if content.len() as u64 > MAX_BYTES {
        return Err(Error::invalid("Vagrantfile", "is larger than 1 MB"));
    }
    let path = path(env);
    let current = std::fs::read_to_string(&path)?;
    if current != base {
        return Err(Error::invalid(
            "Vagrantfile",
            "changed on disk since you opened it; reload to see the new version",
        ));
    }

    let dir = backups_dir.join(backup_folder(&env.dir));
    std::fs::create_dir_all(&dir)?;
    let stamp = crate::jobs::now_ms();
    let backup = dir.join(format!("Vagrantfile-{stamp}.rb"));
    std::fs::write(&backup, &current)?;
    prune_backups(&dir)?;

    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Vagrantfile", "has no parent folder"))?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(content.as_bytes())?;
    tmp.as_file().sync_all()?;
    let perms = std::fs::metadata(&path)?.permissions();
    tmp.as_file().set_permissions(perms)?;
    tmp.persist(&path).map_err(|e| e.error)?;
    Ok(backup)
}

/// A stable, readable folder name for a project's backups.
fn backup_folder(dir: &Path) -> String {
    let s: String = dir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let s = s.trim_matches('_');
    let start = s.len().saturating_sub(100);
    s[start..].to_owned()
}

fn prune_backups(dir: &Path) -> Result<()> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("Vagrantfile-"))
        })
        .collect();
    files.sort();
    for old in files.iter().take(files.len().saturating_sub(KEEP_BACKUPS)) {
        std::fs::remove_file(old)?;
    }
    Ok(())
}

/// `vagrant validate` for the project.
pub fn validate_command(vagrant: &Path, env: &ProjectEnv) -> CommandSpec {
    let mut spec = CommandSpec::new(vagrant)
        .args(["validate", "--no-tty"])
        .cwd(&env.dir);
    for (k, v) in env.vars() {
        spec = spec.env(k, v);
    }
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &Path) -> ProjectEnv {
        ProjectEnv::new(dir)
    }

    #[test]
    fn finds_the_right_file() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(path(&project(d.path())), d.path().join("Vagrantfile"));
        std::fs::write(d.path().join("vagrantfile"), "").unwrap();
        assert!(exists(d.path(), None));
        let custom = ProjectEnv {
            vagrantfile: Some("Custom.rb".into()),
            ..project(d.path())
        };
        assert_eq!(path(&custom), d.path().join("Custom.rb"));
        assert!(!exists(d.path(), Some("Custom.rb")));
    }

    #[test]
    fn saves_with_backup_and_detects_conflicts() {
        let d = tempfile::tempdir().unwrap();
        let backups = tempfile::tempdir().unwrap();
        let file = d.path().join("Vagrantfile");
        std::fs::write(&file, "old").unwrap();
        let env = project(d.path());

        let text = read(&env).unwrap();
        assert_eq!(text.content, "old");
        let backup = save(&env, &text.content, "new", backups.path()).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "old");
        assert!(backup.starts_with(backups.path()));
        // No stray temp files next to the Vagrantfile.
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 1);

        // Someone else edited it meanwhile.
        std::fs::write(&file, "external").unwrap();
        assert!(save(&env, "new", "mine", backups.path()).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "external");
    }

    #[test]
    fn keeps_the_last_ten_backups() {
        let d = tempfile::tempdir().unwrap();
        let backups = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Vagrantfile"), "0").unwrap();
        let env = project(d.path());
        for i in 0..13 {
            save(&env, &i.to_string(), &(i + 1).to_string(), backups.path()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let dir = backups.path().join(backup_folder(d.path()));
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), KEEP_BACKUPS);
    }

    #[cfg(unix)]
    #[test]
    fn keeps_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let backups = tempfile::tempdir().unwrap();
        let file = d.path().join("Vagrantfile");
        std::fs::write(&file, "a").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        save(&project(d.path()), "a", "b", backups.path()).unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
    }

    #[test]
    fn refuses_huge_content() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Vagrantfile"), "a").unwrap();
        let big = "x".repeat(MAX_BYTES as usize + 1);
        assert!(save(&project(d.path()), "a", &big, d.path()).is_err());
    }
}
