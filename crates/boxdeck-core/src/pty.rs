//! Interactive commands (`vagrant ssh`, or a job re-run that needs a password) in a real
//! pseudo-terminal, streamed to an in-app terminal tab.

use std::io::{Read, Write};
use std::sync::Mutex;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::error::{Error, Result};
use crate::process::CommandSpec;

/// A running PTY session. Dropping it kills the process.
pub struct PtySession {
    master: Box<dyn MasterPty + Send>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

fn pty_err(e: impl std::fmt::Display) -> Error {
    Error::Pty(e.to_string())
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

impl PtySession {
    /// Starts `spec` in a new PTY. `on_output` gets raw terminal bytes from a reader thread;
    /// `on_exit` gets the exit code once the process ends and all output was delivered.
    pub fn spawn(
        spec: &CommandSpec,
        cols: u16,
        rows: u16,
        mut on_output: impl FnMut(&[u8]) + Send + 'static,
        on_exit: impl FnOnce(Option<u32>) + Send + 'static,
    ) -> Result<Self> {
        let pair = native_pty_system()
            .openpty(size(cols, rows))
            .map_err(pty_err)?;
        let mut cmd = CommandBuilder::new(&spec.program);
        cmd.args(&spec.args);
        if let Some(dir) = &spec.cwd {
            cmd.cwd(dir);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        cmd.env("TERM", "xterm-256color");
        let mut child = pair.slave.spawn_command(cmd).map_err(pty_err)?;
        // Our copy of the slave end must close, or the reader never sees EOF.
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(pty_err)?;
        let writer = pair.master.take_writer().map_err(pty_err)?;
        let killer = child.clone_killer();

        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => on_output(&buf[..n]),
                }
            }
            let code = child.wait().ok().map(|s| s.exit_code());
            on_exit(code);
        });

        Ok(Self {
            master: pair.master,
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
        })
    }

    /// Sends keyboard input.
    pub fn write(&self, data: &[u8]) -> Result<()> {
        let mut w = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        w.write_all(data)?;
        w.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(size(cols, rows)).map_err(pty_err)
    }

    pub fn kill(&self) {
        let _ = self.killer.lock().unwrap_or_else(|e| e.into_inner()).kill();
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn runs_interactively_and_reports_exit() {
        let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
        let (exit_tx, exit_rx) = mpsc::channel();
        let spec = CommandSpec::new("/bin/sh").args(["-c", "read x; echo got:$x; exit 7"]);
        let s = PtySession::spawn(
            &spec,
            80,
            24,
            move |b| {
                let _ = out_tx.send(b.to_vec());
            },
            move |code| {
                let _ = exit_tx.send(code);
            },
        )
        .unwrap();
        s.resize(100, 30).unwrap();
        s.write(b"hello\n").unwrap();
        let code = exit_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(code, Some(7));
        let out: Vec<u8> = out_rx.try_iter().flatten().collect();
        assert!(String::from_utf8_lossy(&out).contains("got:hello"));
    }
}
