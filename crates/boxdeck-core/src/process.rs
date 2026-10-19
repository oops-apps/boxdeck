//! The only place Boxdeck spawns processes. Commands are argv lists, never shell strings.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use process_wrap::tokio::{CommandWrap, KillOnDrop};

use crate::error::{Error, Result};

/// A fully specified command: program, argv, working directory and extra environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    /// Layered over the inherited (PATH-fixed) environment.
    pub env: Vec<(OsString, OsString)>,
}

impl CommandSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// Collected result of a finished command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    /// `None` when the process was ended by a signal.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// Builds the wrapped command: null stdin, piped output, own process group / job object,
/// no console window on Windows, and the whole tree killed if the handle is dropped.
pub fn wrap(spec: &CommandSpec) -> CommandWrap {
    let mut cmd = CommandWrap::with_new(&spec.program, |c| {
        c.args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = &spec.cwd {
            c.current_dir(dir);
        }
        for (k, v) in &spec.env {
            c.env(k, v);
        }
    });
    #[cfg(unix)]
    cmd.wrap(process_wrap::tokio::ProcessGroup::leader());
    #[cfg(windows)]
    {
        use process_wrap::tokio::{CreationFlags, JobObject};
        use windows::Win32::System::Threading::CREATE_NO_WINDOW;
        cmd.wrap(CreationFlags(CREATE_NO_WINDOW));
        cmd.wrap(JobObject);
    }
    cmd.wrap(KillOnDrop);
    cmd
}

/// Runs a short command to completion and collects its output.
/// On timeout the future is dropped, and `KillOnDrop` takes the whole process tree down.
pub async fn run(spec: &CommandSpec, timeout: Duration) -> Result<CommandOutput> {
    let (exit_code, stdout, stderr) = run_raw(spec, timeout).await?;
    Ok(CommandOutput {
        exit_code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Like [`run`], with stdout and stderr as bytes (file downloads).
pub async fn run_raw(
    spec: &CommandSpec,
    timeout: Duration,
) -> Result<(Option<i32>, Vec<u8>, Vec<u8>)> {
    let child = wrap(spec).spawn()?;
    let output = tokio::time::timeout(timeout, Box::into_pin(child.wait_with_output()))
        .await
        .map_err(|_| Error::Timeout(timeout.as_secs()))??;
    Ok((output.status.code(), output.stdout, output.stderr))
}
