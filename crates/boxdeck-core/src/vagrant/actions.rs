//! Builds argv for machine lifecycle actions. All inputs are validated here, so callers (IPC,
//! tray, MCP) can't inject flags or reach a shell.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::process::CommandSpec;
use crate::vagrant::ProjectEnv;
use crate::validate;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Action {
    Up,
    Halt,
    Suspend,
    Resume,
    Reload,
    Provision,
    Destroy,
}

impl Action {
    pub fn command(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Halt => "halt",
            Self::Suspend => "suspend",
            Self::Resume => "resume",
            Self::Reload => "reload",
            Self::Provision => "provision",
            Self::Destroy => "destroy",
        }
    }

    /// Whether the action can lose data and needs confirmation (or approval) first.
    pub fn is_destructive(self) -> bool {
        matches!(self, Self::Destroy)
    }
}

/// Optional extras, each checked against the action it's used with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default)]
#[ts(export)]
pub struct ActionOptions {
    /// `--provision` (true) or `--no-provision` (false) for up/reload/resume.
    pub provision: Option<bool>,
    /// `--provider` for up.
    pub provider: Option<String>,
    /// `--provision-with` for up/reload/provision.
    pub provisioners: Vec<String>,
}

/// argv for an action on one machine, or the whole project when `machine` is `None`.
/// Always non-interactive (`--no-tty`); `destroy` always gets `-f`, so callers must have
/// confirmed or passed a policy check first.
pub fn build(
    vagrant: &Path,
    env: &ProjectEnv,
    action: Action,
    machine: Option<&str>,
    opts: &ActionOptions,
) -> Result<CommandSpec> {
    let mut args: Vec<String> = vec![action.command().to_owned()];
    if let Some(name) = machine {
        validate::machine_name(name)?;
        args.push(name.to_owned());
    }
    if let Some(p) = opts.provision {
        validate::flag(action.command(), &["up", "reload", "resume"])
            .map_err(|_| invalid_option("provision", action))?;
        args.push(if p { "--provision" } else { "--no-provision" }.to_owned());
    }
    if let Some(provider) = &opts.provider {
        if action != Action::Up {
            return Err(invalid_option("provider", action));
        }
        validate::provider(provider)?;
        args.extend(["--provider".to_owned(), provider.clone()]);
    }
    if !opts.provisioners.is_empty() {
        validate::flag(action.command(), &["up", "reload", "provision"])
            .map_err(|_| invalid_option("provisioners", action))?;
        for p in &opts.provisioners {
            validate::machine_name(p).map_err(|_| {
                crate::Error::invalid("provisioner", format!("{p:?} is not a valid name"))
            })?;
        }
        args.extend(["--provision-with".to_owned(), opts.provisioners.join(",")]);
    }
    if action == Action::Destroy {
        args.push("-f".to_owned());
    }
    args.push("--no-tty".to_owned());

    let mut spec = CommandSpec::new(vagrant).args(args).cwd(&env.dir);
    for (k, v) in env.vars() {
        spec = spec.env(k, v);
    }
    Ok(spec)
}

/// `vagrant ssh MACHINE` for a PTY session. Interactive, so no `--no-tty`.
pub fn ssh(vagrant: &Path, env: &ProjectEnv, machine: &str) -> Result<CommandSpec> {
    validate::machine_name(machine)?;
    let mut spec = CommandSpec::new(vagrant)
        .args(["ssh", machine])
        .cwd(&env.dir);
    for (k, v) in env.vars() {
        spec = spec.env(k, v);
    }
    Ok(spec)
}

fn invalid_option(option: &'static str, action: Action) -> crate::Error {
    crate::Error::invalid(
        option,
        format!("can't be used with `vagrant {}`", action.command()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(action: Action, machine: Option<&str>, opts: &ActionOptions) -> Result<Vec<String>> {
        build(
            Path::new("vagrant"),
            &ProjectEnv::new("/p"),
            action,
            machine,
            opts,
        )
        .map(|s| {
            s.args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        })
    }

    #[test]
    fn simple_actions() {
        assert_eq!(
            argv(Action::Halt, Some("web"), &Default::default()).unwrap(),
            ["halt", "web", "--no-tty"]
        );
        assert_eq!(
            argv(Action::Up, None, &Default::default()).unwrap(),
            ["up", "--no-tty"]
        );
    }

    #[test]
    fn destroy_is_forced_after_our_confirmation() {
        assert_eq!(
            argv(Action::Destroy, Some("web"), &Default::default()).unwrap(),
            ["destroy", "web", "-f", "--no-tty"]
        );
        assert!(Action::Destroy.is_destructive());
        assert!(!Action::Halt.is_destructive());
    }

    #[test]
    fn up_options() {
        let opts = ActionOptions {
            provision: Some(false),
            provider: Some("vmware_desktop".into()),
            provisioners: vec!["shell".into(), "ansible".into()],
        };
        assert_eq!(
            argv(Action::Up, Some("web"), &opts).unwrap(),
            [
                "up",
                "web",
                "--no-provision",
                "--provider",
                "vmware_desktop",
                "--provision-with",
                "shell,ansible",
                "--no-tty"
            ]
        );
    }

    #[test]
    fn rejects_options_on_the_wrong_action_and_bad_values() {
        let provider = ActionOptions {
            provider: Some("virtualbox".into()),
            ..Default::default()
        };
        assert!(argv(Action::Halt, None, &provider).is_err());
        let provision = ActionOptions {
            provision: Some(true),
            ..Default::default()
        };
        assert!(argv(Action::Destroy, None, &provision).is_err());
        let bad = ActionOptions {
            provisioners: vec!["a,b".into()],
            ..Default::default()
        };
        assert!(argv(Action::Provision, None, &bad).is_err());
        assert!(argv(Action::Up, Some("--provider=x"), &Default::default()).is_err());
    }

    #[test]
    fn project_env_is_applied() {
        let env = ProjectEnv {
            dir: "/p".into(),
            vagrantfile: Some("Custom.rb".into()),
            dotfile_path: None,
        };
        let spec = build(
            Path::new("vagrant"),
            &env,
            Action::Up,
            None,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(spec.cwd.as_deref(), Some(Path::new("/p")));
        assert_eq!(spec.env.len(), 1);
    }
}
