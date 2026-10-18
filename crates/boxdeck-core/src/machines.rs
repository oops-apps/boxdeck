//! The machine list shown in the UI, tray and MCP: machine index entries merged with live
//! `vagrant status` results and bookmarked project folders (plan §4.1).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::vagrant::ProjectEnv;
use crate::vagrant::index::IndexedMachine;
use crate::vagrant::status::ProjectStatus;

/// Where a machine's state came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum StateSource {
    /// Vagrant's cached index state; may be stale.
    Index,
    /// A `vagrant status` call.
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct Machine {
    pub name: String,
    pub provider: String,
    /// State id, e.g. `running`, `poweroff`, `not_created`, or `unknown`.
    pub state: String,
    pub state_short: String,
    pub state_long: String,
    pub source: StateSource,
    /// Machine-index id, once the machine has been created.
    pub index_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct Project {
    pub env: ProjectEnv,
    pub machines: Vec<Machine>,
    /// Last successful or failed status check, in ms since the epoch.
    #[ts(type = "number | null")]
    pub checked_at: Option<u64>,
    /// Why the last status check failed, e.g. a Vagrantfile error. Machines then show index state.
    pub error: Option<String>,
    pub bookmarked: bool,
}

/// Merges the three sources into projects sorted by folder.
pub fn merge(
    indexed: &[IndexedMachine],
    statuses: &[ProjectStatus],
    bookmarks: &[ProjectEnv],
) -> Vec<Project> {
    let mut projects: BTreeMap<ProjectEnv, Project> = BTreeMap::new();
    for b in bookmarks {
        project(&mut projects, b).bookmarked = true;
    }
    for m in indexed {
        project(&mut projects, &m.env).machines.push(Machine {
            name: m.name.clone(),
            provider: m.provider.clone(),
            state: m.state.clone(),
            state_short: m.state.replace('_', " "),
            state_long: String::new(),
            source: StateSource::Index,
            index_id: Some(m.id.clone()),
        });
    }
    for s in statuses {
        let p = project(&mut projects, &s.env);
        p.checked_at = Some(s.checked_at);
        match &s.result {
            Ok(live) => {
                let indexed: Vec<Machine> = std::mem::take(&mut p.machines);
                p.error = None;
                p.machines = live
                    .iter()
                    .map(|l| Machine {
                        name: l.name.clone(),
                        provider: l.provider.clone(),
                        state: l.state.clone(),
                        state_short: l.state_short.clone(),
                        state_long: l.state_long.clone(),
                        source: StateSource::Live,
                        index_id: indexed
                            .iter()
                            .find(|i| i.name == l.name)
                            .and_then(|i| i.index_id.clone()),
                    })
                    .collect();
            }
            Err(e) => p.error = Some(e.clone()),
        }
    }
    projects.into_values().collect()
}

fn project<'a>(
    projects: &'a mut BTreeMap<ProjectEnv, Project>,
    env: &ProjectEnv,
) -> &'a mut Project {
    projects.entry(env.clone()).or_insert_with(|| Project {
        env: env.clone(),
        machines: Vec::new(),
        checked_at: None,
        error: None,
        bookmarked: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vagrant::status::MachineStatus;

    fn indexed(dir: &str, name: &str, state: &str) -> IndexedMachine {
        IndexedMachine {
            id: format!("{name}-id"),
            name: name.into(),
            provider: "docker".into(),
            architecture: None,
            state: state.into(),
            env: ProjectEnv::new(dir),
            box_ref: None,
        }
    }

    fn live(name: &str, state: &str) -> MachineStatus {
        MachineStatus {
            name: name.into(),
            provider: "docker".into(),
            state: state.into(),
            state_short: state.into(),
            state_long: format!("{state} long"),
        }
    }

    #[test]
    fn index_only_shows_cached_state() {
        let p = merge(&[indexed("/p", "web", "not_created")], &[], &[]);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].machines[0].source, StateSource::Index);
        assert_eq!(p[0].machines[0].state_short, "not created");
        assert_eq!(p[0].checked_at, None);
    }

    #[test]
    fn live_status_replaces_index_and_adds_uncreated_machines() {
        let status = ProjectStatus {
            env: ProjectEnv::new("/p"),
            checked_at: 5,
            result: Ok(vec![live("web", "running"), live("db", "not_created")]),
        };
        let p = merge(&[indexed("/p", "web", "poweroff")], &[status], &[]);
        let m = &p[0].machines;
        assert_eq!(m.len(), 2);
        assert_eq!(
            (m[0].name.as_str(), m[0].state.as_str()),
            ("web", "running")
        );
        assert_eq!(m[0].index_id.as_deref(), Some("web-id"));
        assert_eq!(m[0].source, StateSource::Live);
        assert_eq!(m[1].index_id, None);
        assert_eq!(p[0].checked_at, Some(5));
    }

    #[test]
    fn failed_status_keeps_index_state_and_reports_the_error() {
        let status = ProjectStatus {
            env: ProjectEnv::new("/p"),
            checked_at: 5,
            result: Err("syntax error".into()),
        };
        let p = merge(&[indexed("/p", "web", "running")], &[status], &[]);
        assert_eq!(p[0].error.as_deref(), Some("syntax error"));
        assert_eq!(p[0].machines[0].source, StateSource::Index);
    }

    #[test]
    fn bookmarks_appear_without_machines() {
        let p = merge(&[], &[], &[ProjectEnv::new("/b")]);
        assert!(p[0].bookmarked);
        assert!(p[0].machines.is_empty());
    }
}
