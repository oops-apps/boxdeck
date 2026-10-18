//! Forwarded ports and SSH connection info for one machine. Private keys are never read or
//! returned; only host, port and user.

use std::path::Path;

use serde::Serialize;

use crate::error::Result;
use crate::process::CommandSpec;
use crate::vagrant::{ProjectEnv, machine_readable};
use crate::validate;

fn spec(vagrant: &Path, env: &ProjectEnv, args: &[&str]) -> CommandSpec {
    let mut spec = CommandSpec::new(vagrant).args(args).cwd(&env.dir);
    for (k, v) in env.vars() {
        spec = spec.env(k, v);
    }
    spec
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ForwardedPort {
    pub guest: u16,
    pub host: u16,
}

/// `vagrant port NAME --machine-readable`; never `--no-tty` (plan §3.1).
pub fn port_command(vagrant: &Path, env: &ProjectEnv, machine: &str) -> Result<CommandSpec> {
    validate::machine_name(machine)?;
    Ok(spec(vagrant, env, &["port", machine, "--machine-readable"]))
}

pub fn parse_ports(stdout: &str) -> Vec<ForwardedPort> {
    machine_readable::parse(stdout)
        .into_iter()
        .filter(|l| l.kind == "forwarded_port")
        .filter_map(|l| {
            Some(ForwardedPort {
                guest: l.data.first()?.parse().ok()?,
                host: l.data.get(1)?.parse().ok()?,
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SshInfo {
    pub host: String,
    pub port: u16,
    pub user: String,
}

pub fn ssh_config_command(vagrant: &Path, env: &ProjectEnv, machine: &str) -> Result<CommandSpec> {
    validate::machine_name(machine)?;
    Ok(spec(vagrant, env, &["ssh-config", machine, "--no-tty"]))
}

/// Picks `HostName`, `Port` and `User` out of `vagrant ssh-config` text; everything else
/// (including `IdentityFile`) is dropped.
pub fn parse_ssh_config(stdout: &str) -> Option<SshInfo> {
    let mut host = None;
    let mut port = None;
    let mut user = None;
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some("HostName"), Some(v)) => host = Some(v.to_owned()),
            (Some("Port"), Some(v)) => port = v.parse().ok(),
            (Some("User"), Some(v)) => user = Some(v.to_owned()),
            _ => {}
        }
    }
    Some(SshInfo {
        host: host?,
        port: port?,
        user: user?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports() {
        let out = "1,web,forwarded_port,22,2222\n1,web,forwarded_port,80,8080\n1,,ui,info,x\n";
        assert_eq!(
            parse_ports(out),
            [
                ForwardedPort {
                    guest: 22,
                    host: 2222
                },
                ForwardedPort {
                    guest: 80,
                    host: 8080
                }
            ]
        );
    }

    #[test]
    fn ssh_config_without_key_paths() {
        let out = "Host web\n  HostName 127.0.0.1\n  User vagrant\n  Port 2222\n  \
                   IdentityFile /home/u/.vagrant/machines/web/private_key\n";
        let info = parse_ssh_config(out).unwrap();
        assert_eq!(info.host, "127.0.0.1");
        assert_eq!(info.port, 2222);
        assert_eq!(info.user, "vagrant");
        assert!(!format!("{info:?}").contains("private_key"));
        assert!(parse_ssh_config("nothing").is_none());
    }
}
