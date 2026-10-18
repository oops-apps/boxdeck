//! Parsers checked against output captured from real Vagrant by `scripts/capture-fixtures.sh`.

use std::path::{Path, PathBuf};

use boxdeck_core::Error;
use boxdeck_core::vagrant::{index, locate, machine_readable, status};

const CAPTURED: &str = "2.4.9-darwin";

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/captured")
        .join(CAPTURED)
}

fn read(name: &str) -> String {
    std::fs::read_to_string(dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn exit_code(name: &str) -> i32 {
    read(&format!("{name}.exit")).trim().parse().unwrap()
}

#[test]
fn lists_machines_from_a_real_index() {
    let path = dir().join("machine-index.json");
    let machines = index::read(&path).unwrap();
    let summary: Vec<_> = machines
        .iter()
        .map(|m| (m.name.as_str(), m.provider.as_str(), m.state.as_str()))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("custom", "docker", "running"),
            ("web", "docker", "stopped")
        ]
    );
    let custom = &machines[0];
    assert_eq!(custom.env.dir, Path::new("/tmp/boxdeck-capture/dcustom"));
    assert_eq!(custom.env.vagrantfile.as_deref(), Some("Custom.rb"));
    assert_eq!(custom.env.dotfile_path, None);
    assert_eq!(machines[1].env.vagrantfile, None);
    assert_eq!(custom.id.len(), 32);
}

#[test]
fn index_is_empty_after_destroy() {
    let path = dir().join("machine-index-after-destroy.json");
    assert!(index::read(&path).unwrap().is_empty());
}

#[test]
fn status_running_stopped_and_not_created() {
    let running = status::parse(&read("status-multi-running.stdout"));
    assert_eq!(running.len(), 2);
    assert_eq!(
        (running[0].name.as_str(), running[0].state.as_str()),
        ("web", "running")
    );
    assert_eq!(running[0].provider, "docker");
    assert!(running[0].state_long.contains('\n'));
    assert!(running[0].state_long.contains("`vagrant halt`, see logs"));
    assert_eq!(
        (running[1].name.as_str(), running[1].state.as_str()),
        ("db", "not_created")
    );
    assert_eq!(running[1].state_short, "not created");

    let stopped = status::parse(&read("status-multi-stopped.stdout"));
    assert_eq!(stopped[0].state, "stopped");

    let fresh = status::parse(&read("status-multi-not-created.stdout"));
    assert!(fresh.iter().all(|m| m.state == "not_created"));

    let one = status::parse(&read("status-multi-one.stdout"));
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].name, "web");
}

#[test]
fn custom_vagrantfile_needs_the_env_var() {
    assert_eq!(exit_code("status-custom-vagrantfile-missing"), 1);
    let with_env = status::parse(&read("status-vagrantfile-env.stdout"));
    assert_eq!(with_env.len(), 1);
    assert_eq!(with_env[0].name, "custom");
}

#[test]
fn no_tty_turns_machine_readable_output_into_text() {
    // Why queries must never pass --no-tty (plan §3.1).
    let out = read("status-no-tty-overrides.stdout");
    assert!(machine_readable::parse(&out).is_empty());
    assert!(out.contains("Current machine states:"));
}

#[test]
fn errors_from_error_exit_lines() {
    let name = "status-unknown-machine";
    let err = machine_readable::failure(
        &read(&format!("{name}.stdout")),
        &read(&format!("{name}.stderr")),
        Some(exit_code(name)),
    );
    match err {
        Error::Vagrant { class, message, .. } => {
            assert_eq!(class.as_deref(), Some("Vagrant::Errors::MachineNotFound"));
            assert!(message.starts_with("The machine with the name 'nope'"));
        }
        other => panic!("unexpected {other:?}"),
    }

    let name = "status-no-provider";
    match machine_readable::failure(&read(&format!("{name}.stdout")), "", Some(1)) {
        Error::Vagrant { class, .. } => {
            assert_eq!(class.as_deref(), Some("Vagrant::Errors::NoDefaultProvider"))
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn early_errors_are_plain_text() {
    let name = "status-syntax-error";
    assert_eq!(exit_code(name), 1);
    let stdout = read(&format!("{name}.stdout"));
    assert!(machine_readable::parse(&stdout).is_empty());
    match machine_readable::failure(&stdout, &read(&format!("{name}.stderr")), Some(1)) {
        Error::Vagrant { class, message, .. } => {
            assert!(class.is_none());
            assert!(message.contains("syntax error"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn prompt_under_no_tty_fails_fast() {
    assert_eq!(exit_code("destroy-prompt-no-tty"), 1);
    assert!(read("destroy-prompt-no-tty.stderr").contains("requires\na TTY"));
}

#[test]
fn box_list_lines_with_architecture_and_info() {
    let lines = machine_readable::parse(&read("box-list-info.stdout"));
    let kinds: Vec<_> = lines
        .iter()
        .filter(|l| l.kind.starts_with("box-"))
        .map(|l| l.kind.as_str())
        .collect();
    assert_eq!(kinds.iter().filter(|k| **k == "box-name").count(), 3);
    assert_eq!(
        kinds.iter().filter(|k| **k == "box-architecture").count(),
        3
    );
    let homepage = lines
        .iter()
        .find(|l| l.kind == "box-info" && l.first() == "homepage")
        .unwrap();
    assert_eq!(homepage.data[1], "https://example.com/a,b");
    assert!(
        machine_readable::parse(&read("box-list-empty.stdout"))
            .iter()
            .all(|l| l.kind == "ui")
    );
}

#[test]
fn box_list_before_2_4_has_no_architecture() {
    let out = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic/box-list-2.3.stdout"),
    )
    .unwrap();
    let lines = machine_readable::parse(&out);
    assert!(lines.iter().any(|l| l.kind == "box-name"));
    assert!(!lines.iter().any(|l| l.kind == "box-architecture"));
}

#[test]
fn global_status_has_no_machine_names() {
    // Why names come from the index instead (plan §3.1).
    let lines = machine_readable::parse(&read("global-status.stdout"));
    assert!(lines.iter().any(|l| l.kind == "machine-id"));
    assert!(lines.iter().all(|l| l.target.is_empty()));
}

#[test]
fn version_output() {
    let v = locate::parse_version(&read("version.stdout")).unwrap();
    assert_eq!(v, semver::Version::new(2, 4, 9));
}

#[tokio::test]
#[ignore = "needs a real vagrant on PATH; run with --ignored"]
async fn detects_installed_vagrant() {
    let v = locate::Vagrant::detect(None).await.unwrap();
    assert!(v.version >= locate::MIN_VERSION);
}

#[test]
fn box_list_parses_into_boxes() {
    use boxdeck_core::vagrant::boxes;
    let list = boxes::parse_list(&read("box-list-info.stdout"));
    let summary: Vec<_> = list
        .iter()
        .map(|b| {
            (
                b.name.as_str(),
                b.provider.as_str(),
                b.architecture.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("boxdeck/arch", "virtualbox", Some("amd64")),
            ("boxdeck/arch", "vmware_desktop", Some("arm64")),
            ("boxdeck/test", "vmware_desktop", Some("arm64")),
        ]
    );
    assert_eq!(list[0].info["homepage"], "https://example.com/a,b");
    assert!(boxes::parse_list(&read("box-list-empty.stdout")).is_empty());
}

#[test]
fn box_outdated_from_real_output() {
    use boxdeck_core::vagrant::boxes::{self, UpdateStatus};
    for name in [
        "box-outdated-global-mr.stdout",
        "box-outdated-global.stdout",
    ] {
        let updates = boxes::parse_outdated(&read(name));
        assert_eq!(updates.len(), 3, "{name}");
        assert!(updates.iter().all(|u| u.status == UpdateStatus::NoMetadata));
        assert_eq!(updates[0].name, "boxdeck/test");
    }
}

#[test]
fn catalog_search_from_real_output() {
    use boxdeck_core::vagrant::catalog;
    let results = catalog::parse_search(&read("cloud-search.stdout")).unwrap();
    assert_eq!(results.len(), 5);
    let b = results
        .iter()
        .find(|b| b.name == "bento/ubuntu-24.04")
        .unwrap();
    assert!(b.downloads > 100_000);
    assert!(b.providers.contains(&"vmware_desktop".to_owned()));
    assert_eq!(b.architectures, ["arm64", "amd64"]);
    // The catalog's architecture filter is loose: an amd64-only box came back for arm64.
    let loose = results
        .iter()
        .find(|b| b.name == "bento/ubuntu-20.04")
        .unwrap();
    assert_eq!(loose.architectures, ["amd64"]);
}
