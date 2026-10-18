//! Append-only JSONL log of agent tool calls (plan §4.8). The app and `boxdeck-mcp` may both
//! append, so every write happens under an exclusive lock on `audit.lock`, one line per call;
//! only the app rotates.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::policy::Transport;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct AuditEntry {
    /// Milliseconds since the Unix epoch.
    #[ts(type = "number")]
    pub time: u64,
    pub transport: Transport,
    /// Token name (HTTP) or `stdio:<clientInfo name>`.
    pub client: String,
    pub tool: String,
    #[ts(type = "unknown")]
    pub arguments: serde_json::Value,
    /// `allowed`, `denied`, `approved` or `rejected`.
    pub decision: String,
    #[ts(type = "number | null")]
    pub job_id: Option<u64>,
    /// `ok`, or the error message.
    pub result: String,
}

#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    /// The log in Boxdeck's data directory.
    pub fn default_log() -> Result<Self> {
        Ok(Self::new(crate::config::data_dir()?.join("audit.jsonl")))
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> Result<File> {
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("audit.lock"))?;
        lock.lock()?;
        Ok(lock)
    }

    /// Appends one entry as a single write, under the lock.
    pub fn append(&self, entry: &AuditEntry) -> Result<()> {
        let mut line = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
        line.push(b'\n');
        let lock = self.lock()?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(&line)?;
        drop(lock);
        Ok(())
    }

    /// The last `n` entries, oldest first. Unparseable lines are skipped.
    pub fn tail(&self, n: usize) -> Result<Vec<AuditEntry>> {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut entries: Vec<AuditEntry> = BufReader::new(file)
            .lines()
            .map_while(|l| l.ok())
            .filter_map(|l| serde_json::from_str(&l).ok())
            .collect();
        let start = entries.len().saturating_sub(n);
        Ok(entries.split_off(start))
    }

    /// Moves the log to `audit.1.jsonl` once it's bigger than `max_bytes`. App only.
    pub fn rotate_if_needed(&self, max_bytes: u64) -> Result<bool> {
        let lock = self.lock()?;
        let big = std::fs::metadata(&self.path).is_ok_and(|m| m.len() > max_bytes);
        if big {
            std::fs::rename(&self.path, self.path.with_extension("1.jsonl"))?;
        }
        drop(lock);
        Ok(big)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(i: u64) -> AuditEntry {
        AuditEntry {
            time: i,
            transport: Transport::Stdio,
            client: "stdio:test".into(),
            tool: "list_machines".into(),
            arguments: serde_json::json!({ "i": i }),
            decision: "allowed".into(),
            job_id: None,
            result: "ok".into(),
        }
    }

    #[test]
    fn appends_and_reads_the_tail() {
        let d = tempfile::tempdir().unwrap();
        let log = AuditLog::new(d.path().join("audit.jsonl"));
        assert!(log.tail(5).unwrap().is_empty());
        for i in 0..10 {
            log.append(&entry(i)).unwrap();
        }
        let tail = log.tail(3).unwrap();
        assert_eq!(tail.iter().map(|e| e.time).collect::<Vec<_>>(), [7, 8, 9]);
    }

    #[test]
    fn concurrent_writers_never_interleave_lines() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("audit.jsonl");
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let log = AuditLog::new(&path);
                std::thread::spawn(move || {
                    for i in 0..50 {
                        let mut e = entry(t * 1000 + i);
                        e.result = "x".repeat(2000);
                        log.append(&e).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 400);
        assert!(
            text.lines()
                .all(|l| serde_json::from_str::<AuditEntry>(l).is_ok())
        );
    }

    #[test]
    fn rotates_past_the_limit() {
        let d = tempfile::tempdir().unwrap();
        let log = AuditLog::new(d.path().join("audit.jsonl"));
        log.append(&entry(1)).unwrap();
        assert!(!log.rotate_if_needed(1_000_000).unwrap());
        assert!(log.rotate_if_needed(10).unwrap());
        assert!(d.path().join("audit.1.jsonl").is_file());
        assert!(log.tail(10).unwrap().is_empty());
    }
}
