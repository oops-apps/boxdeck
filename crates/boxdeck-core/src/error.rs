use std::path::PathBuf;

/// Errors from `boxdeck-core`. Messages are shown to users and returned to MCP clients,
/// so they must never contain secrets.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("vagrant was not found on PATH; install Vagrant or set its path in Settings")]
    VagrantNotFound,

    #[error("the configured vagrant path does not exist or is not a file: {0}")]
    VagrantPathInvalid(PathBuf),

    #[error("could not read the Vagrant version from {0:?}")]
    UnparsableVersion(String),

    #[error("Vagrant {found} is older than the minimum supported version {minimum}")]
    VagrantTooOld {
        found: semver::Version,
        minimum: semver::Version,
    },

    #[error("vagrant failed: {message}")]
    Vagrant {
        /// Ruby error class from an `error-exit` line, when Vagrant printed one.
        class: Option<String>,
        message: String,
        exit_code: Option<i32>,
    },

    #[error("vagrant did not finish within {0} seconds")]
    Timeout(u64),

    #[error("the machine index at {path} is not valid JSON")]
    CorruptIndex { path: PathBuf },

    #[error("the machine index at {path} has unsupported version {version}")]
    UnsupportedIndexVersion { path: PathBuf, version: u64 },

    #[error("invalid {field}: {reason}")]
    Invalid { field: &'static str, reason: String },

    #[error(
        "settings file {path} has version {found}, newer than this Boxdeck supports ({supported})"
    )]
    SettingsTooNew {
        path: PathBuf,
        found: u32,
        supported: u32,
    },

    #[error("settings file {path} is not valid: {source}")]
    SettingsInvalid {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("file watch failed: {0}")]
    Watch(String),

    #[error("terminal session failed: {0}")]
    Pty(String),

    #[error("no per-user config directory is available on this system")]
    NoConfigDir,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        Self::Invalid {
            field,
            reason: reason.into(),
        }
    }
}
