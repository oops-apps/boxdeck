//! Who may call which MCP tool (plan §4.8). Every tool call is decided here.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{AccessLevel, AgentAccess, DefaultProjectAccess, Settings, ShellMode};

/// How a call reached Boxdeck, as recorded in the audit log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Transport {
    /// The in-app HTTP endpoint, removed in M6c; kept so older audit lines still parse.
    Http,
    /// `boxdeck-mcp`, launched by the client.
    Stdio,
}

/// What a tool needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolNeeds {
    pub level: AccessLevel,
    pub destructive: bool,
    /// Works inside a VM; governed by the shell mode.
    pub shell: bool,
}

impl ToolNeeds {
    pub const READ: Self = Self {
        level: AccessLevel::Read,
        destructive: false,
        shell: false,
    };
    pub const OPERATE: Self = Self {
        level: AccessLevel::Operate,
        destructive: false,
        shell: false,
    };
    pub const FULL: Self = Self {
        level: AccessLevel::Full,
        destructive: true,
        shell: false,
    };
    pub const SHELL: Self = Self {
        level: AccessLevel::Operate,
        destructive: false,
        shell: true,
    };
}

/// What the gate knows about a call besides the settings.
#[derive(Debug, Clone, Copy, Default)]
pub struct Situation<'a> {
    /// The project the call targets, if any.
    pub project: Option<&'a Path>,
    /// The app is running, so it can show an approval.
    pub app_running: bool,
    /// Shell calls: the VM can write host files through a synced folder.
    pub host_writable: bool,
    /// Shell calls: the user allowed this machine for the session.
    pub granted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Ask the user in the app. `grantable`: they may allow the machine for the session.
    NeedsApproval {
        grantable: bool,
    },
    /// Refused; the message goes back to the agent.
    Deny(String),
}

/// Whether agents may see and act on a project at all.
pub fn project_allowed(settings: &Settings, dir: &Path) -> bool {
    let explicit = settings.project(dir).map(|p| p.agent_access);
    match explicit {
        Some(AgentAccess::Allow) => true,
        Some(AgentAccess::Deny) => false,
        Some(AgentAccess::Inherit) | None => {
            settings.mcp.default_access == DefaultProjectAccess::AllProjects
        }
    }
}

/// The shell mode for a project: its override, or the global one.
pub fn shell_mode(settings: &Settings, project: Option<&Path>) -> ShellMode {
    project
        .and_then(|dir| settings.project(dir))
        .and_then(|p| p.shell)
        .unwrap_or(settings.mcp.shell)
}

/// Decides a tool call.
pub fn decide(settings: &Settings, needs: ToolNeeds, at: Situation<'_>) -> Decision {
    let mcp = &settings.mcp;
    if !mcp.enabled {
        return Decision::Deny("Agent access is turned off in Boxdeck's settings.".into());
    }
    if mcp.paused {
        return Decision::Deny("Agent access is paused in Boxdeck.".into());
    }
    if needs.level > mcp.access {
        return Decision::Deny(format!(
            "This tool needs the {:?} access level; Boxdeck allows {:?}.",
            needs.level, mcp.access
        ));
    }
    if let Some(dir) = at.project
        && !project_allowed(settings, dir)
    {
        // Same message as an unknown project, so a denied one doesn't reveal itself.
        return Decision::Deny("Unknown project folder.".into());
    }
    if needs.shell {
        match shell_mode(settings, at.project) {
            ShellMode::Off => {
                return Decision::Deny(
                    "Shell access to VMs is turned off in Boxdeck's settings.".into(),
                );
            }
            ShellMode::Always => {}
            ShellMode::AskOnce if !at.host_writable => {}
            ShellMode::AskOnce if at.app_running && at.granted => {}
            ShellMode::AskOnce if at.app_running => {
                return Decision::NeedsApproval { grantable: true };
            }
            ShellMode::AskOnce if mcp.shell_headless => {}
            ShellMode::AskOnce => {
                return Decision::Deny(
                    "This VM can change files on the host, so Boxdeck asks once before shell \
                     access. Open the Boxdeck app to approve it."
                        .into(),
                );
            }
        }
    }
    if needs.destructive {
        if at.app_running {
            return Decision::NeedsApproval { grantable: false };
        }
        if !mcp.headless_destructive {
            return Decision::Deny(
                "Destructive tools need approval in the Boxdeck app. Open Boxdeck and try again."
                    .into(),
            );
        }
    }
    Decision::Allow
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectSettings;

    fn on(access: AccessLevel) -> Settings {
        let mut s = Settings::default();
        s.mcp.enabled = true;
        s.mcp.access = access;
        s
    }

    fn running() -> Situation<'static> {
        Situation {
            app_running: true,
            ..Default::default()
        }
    }

    fn kind(d: &Decision) -> &'static str {
        match d {
            Decision::Allow => "allow",
            Decision::NeedsApproval { grantable: true } => "grant",
            Decision::NeedsApproval { grantable: false } => "approve",
            Decision::Deny(_) => "deny",
        }
    }

    const ALL: [ToolNeeds; 4] = [
        ToolNeeds::READ,
        ToolNeeds::OPERATE,
        ToolNeeds::FULL,
        ToolNeeds::SHELL,
    ];

    #[test]
    fn off_or_paused_denies_everything() {
        let mut paused = on(AccessLevel::Full);
        paused.mcp.paused = true;
        paused.mcp.shell = ShellMode::Always;
        for s in [Settings::default(), paused] {
            for needs in ALL {
                for app_running in [false, true] {
                    let at = Situation {
                        app_running,
                        ..Default::default()
                    };
                    assert_eq!(kind(&decide(&s, needs, at)), "deny");
                }
            }
        }
    }

    #[test]
    fn every_tool_level_against_every_access_level() {
        use AccessLevel::*;
        for access in [Read, Operate, Full] {
            let mut s = on(access);
            s.mcp.shell = ShellMode::Always;
            for needs in ALL {
                for app_running in [false, true] {
                    let at = Situation {
                        app_running,
                        ..Default::default()
                    };
                    let expected = if needs.level > access {
                        "deny"
                    } else if needs.destructive && app_running {
                        "approve"
                    } else if needs.destructive {
                        "deny"
                    } else {
                        "allow"
                    };
                    let got = kind(&decide(&s, needs, at));
                    assert_eq!(got, expected, "{access:?} {needs:?} app={app_running}");
                }
            }
        }
    }

    #[test]
    fn headless_destructive_opt_in() {
        let mut s = on(AccessLevel::Full);
        s.mcp.headless_destructive = true;
        assert_eq!(
            decide(&s, ToolNeeds::FULL, Situation::default()),
            Decision::Allow
        );
        assert_eq!(kind(&decide(&s, ToolNeeds::FULL, running())), "approve");
    }

    /// Every shell-mode combination: mode, host-writable, granted, app running, headless switch.
    #[test]
    fn shell_mode_table() {
        use ShellMode::*;
        for mode in [Off, AskOnce, Always] {
            for host_writable in [false, true] {
                for granted in [false, true] {
                    for app_running in [false, true] {
                        for headless in [false, true] {
                            let mut s = on(AccessLevel::Operate);
                            s.mcp.shell = mode;
                            s.mcp.shell_headless = headless;
                            let at = Situation {
                                project: None,
                                app_running,
                                host_writable,
                                granted,
                            };
                            let expected = match mode {
                                Off => "deny",
                                Always => "allow",
                                AskOnce if !host_writable => "allow",
                                AskOnce if app_running && granted => "allow",
                                AskOnce if app_running => "grant",
                                AskOnce if headless => "allow",
                                AskOnce => "deny",
                            };
                            let got = kind(&decide(&s, ToolNeeds::SHELL, at));
                            assert_eq!(got, expected, "{mode:?} {at:?} headless={headless}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn shell_needs_operate_not_full() {
        let mut s = on(AccessLevel::Read);
        s.mcp.shell = ShellMode::Always;
        assert_eq!(kind(&decide(&s, ToolNeeds::SHELL, running())), "deny");
        s.mcp.access = AccessLevel::Operate;
        assert_eq!(kind(&decide(&s, ToolNeeds::SHELL, running())), "allow");
    }

    #[test]
    fn project_overrides_shell_mode() {
        let mut s = on(AccessLevel::Operate);
        let p = Path::new("/p");
        let at = Situation {
            project: Some(p),
            app_running: true,
            host_writable: true,
            granted: false,
        };
        let set = |s: &mut Settings, mode: Option<ShellMode>| {
            s.projects.insert(
                "/p".into(),
                ProjectSettings {
                    shell: mode,
                    ..Default::default()
                },
            );
        };
        assert_eq!(kind(&decide(&s, ToolNeeds::SHELL, at)), "deny");
        set(&mut s, Some(ShellMode::Always));
        assert_eq!(kind(&decide(&s, ToolNeeds::SHELL, at)), "allow");
        s.mcp.shell = ShellMode::Always;
        set(&mut s, Some(ShellMode::Off));
        assert_eq!(kind(&decide(&s, ToolNeeds::SHELL, at)), "deny");
        set(&mut s, None);
        assert_eq!(kind(&decide(&s, ToolNeeds::SHELL, at)), "allow");
    }

    #[test]
    fn project_rules() {
        let mut s = on(AccessLevel::Operate);
        let p = Path::new("/p");
        let at = Situation {
            project: Some(p),
            ..Default::default()
        };
        let set = |s: &mut Settings, a: AgentAccess| {
            s.projects.insert(
                "/p".into(),
                ProjectSettings {
                    agent_access: a,
                    ..Default::default()
                },
            );
        };
        assert_eq!(decide(&s, ToolNeeds::READ, at), Decision::Allow);
        set(&mut s, AgentAccess::Deny);
        assert_eq!(
            decide(&s, ToolNeeds::READ, at),
            Decision::Deny("Unknown project folder.".into())
        );
        s.mcp.default_access = DefaultProjectAccess::OnlyAllowed;
        set(&mut s, AgentAccess::Inherit);
        assert!(!project_allowed(&s, p));
        set(&mut s, AgentAccess::Allow);
        assert!(project_allowed(&s, p));
    }
}
