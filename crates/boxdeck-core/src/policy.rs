//! Who may call which MCP tool (plan §4.8). Every tool call is decided here, in both the
//! in-app HTTP endpoint and the headless `boxdeck-mcp`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::{AccessLevel, AgentAccess, DefaultProjectAccess, Settings};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Transport {
    /// In-app Streamable HTTP; can ask the user for approval.
    Http,
    /// Headless `boxdeck-mcp`; can't ask anyone.
    Stdio,
}

/// What a tool needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolNeeds {
    pub level: AccessLevel,
    pub destructive: bool,
    /// `ssh_exec` has its own opt-in on top of Full.
    pub ssh_exec: bool,
}

impl ToolNeeds {
    pub const READ: Self = Self {
        level: AccessLevel::Read,
        destructive: false,
        ssh_exec: false,
    };
    pub const OPERATE: Self = Self {
        level: AccessLevel::Operate,
        destructive: false,
        ssh_exec: false,
    };
    pub const FULL: Self = Self {
        level: AccessLevel::Full,
        destructive: true,
        ssh_exec: false,
    };
    pub const SSH_EXEC: Self = Self {
        level: AccessLevel::Full,
        destructive: true,
        ssh_exec: true,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Ask the user (HTTP only).
    NeedsApproval,
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

/// Decides a tool call. `project` is the project the call targets, if any.
pub fn decide(
    settings: &Settings,
    needs: ToolNeeds,
    transport: Transport,
    project: Option<&Path>,
) -> Decision {
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
    if needs.ssh_exec && !mcp.ssh_exec {
        return Decision::Deny("ssh_exec is turned off in Boxdeck's settings.".into());
    }
    if let Some(dir) = project
        && !project_allowed(settings, dir)
    {
        // Same message as an unknown project, so a denied one doesn't reveal itself.
        return Decision::Deny("Unknown project folder.".into());
    }
    if needs.destructive {
        return match transport {
            Transport::Http => Decision::NeedsApproval,
            Transport::Stdio if mcp.headless_destructive => Decision::Allow,
            Transport::Stdio => Decision::Deny(
                "Destructive tools need approval, which headless mode can't ask for. Connect over \
                 HTTP with the Boxdeck app running."
                    .into(),
            ),
        };
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

    const ALL: [ToolNeeds; 4] = [
        ToolNeeds::READ,
        ToolNeeds::OPERATE,
        ToolNeeds::FULL,
        ToolNeeds::SSH_EXEC,
    ];
    const TRANSPORTS: [Transport; 2] = [Transport::Http, Transport::Stdio];

    #[test]
    fn off_or_paused_denies_everything() {
        let mut paused = on(AccessLevel::Full);
        paused.mcp.paused = true;
        for s in [Settings::default(), paused] {
            for needs in ALL {
                for t in TRANSPORTS {
                    assert!(matches!(decide(&s, needs, t, None), Decision::Deny(_)));
                }
            }
        }
    }

    #[test]
    fn every_tool_level_against_every_access_level() {
        use AccessLevel::*;
        for access in [Read, Operate, Full] {
            let s = on(access);
            for needs in ALL {
                for t in TRANSPORTS {
                    let d = decide(&s, needs, t, None);
                    let expected = if needs.level > access || needs.ssh_exec {
                        "deny"
                    } else if needs.destructive && t == Transport::Http {
                        "approve"
                    } else if needs.destructive {
                        "deny"
                    } else {
                        "allow"
                    };
                    let got = match d {
                        Decision::Allow => "allow",
                        Decision::NeedsApproval => "approve",
                        Decision::Deny(_) => "deny",
                    };
                    assert_eq!(got, expected, "{access:?} {needs:?} {t:?}");
                }
            }
        }
    }

    #[test]
    fn headless_destructive_opt_in() {
        let mut s = on(AccessLevel::Full);
        s.mcp.headless_destructive = true;
        assert_eq!(
            decide(&s, ToolNeeds::FULL, Transport::Stdio, None),
            Decision::Allow
        );
        assert_eq!(
            decide(&s, ToolNeeds::FULL, Transport::Http, None),
            Decision::NeedsApproval
        );
    }

    #[test]
    fn ssh_exec_needs_its_own_switch() {
        let mut s = on(AccessLevel::Full);
        assert!(matches!(
            decide(&s, ToolNeeds::SSH_EXEC, Transport::Http, None),
            Decision::Deny(_)
        ));
        s.mcp.ssh_exec = true;
        assert_eq!(
            decide(&s, ToolNeeds::SSH_EXEC, Transport::Http, None),
            Decision::NeedsApproval
        );
    }

    #[test]
    fn project_rules() {
        let mut s = on(AccessLevel::Operate);
        let p = Path::new("/p");
        let set = |s: &mut Settings, a: AgentAccess| {
            s.projects.insert(
                "/p".into(),
                ProjectSettings {
                    agent_access: a,
                    ..Default::default()
                },
            );
        };
        assert_eq!(
            decide(&s, ToolNeeds::READ, Transport::Stdio, Some(p)),
            Decision::Allow
        );
        set(&mut s, AgentAccess::Deny);
        assert_eq!(
            decide(&s, ToolNeeds::READ, Transport::Stdio, Some(p)),
            Decision::Deny("Unknown project folder.".into())
        );
        s.mcp.default_access = DefaultProjectAccess::OnlyAllowed;
        set(&mut s, AgentAccess::Inherit);
        assert!(!project_allowed(&s, p));
        set(&mut s, AgentAccess::Allow);
        assert!(project_allowed(&s, p));
    }
}
