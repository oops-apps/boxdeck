//! Against a real `vagrant` on PATH, in a throwaway VAGRANT_HOME. Run with `--ignored`.

use std::time::Duration;

use boxdeck_core::jobs::{JobRequest, JobState, Jobs, JobsConfig, Origin, Scope};
use boxdeck_core::process::{self, CommandSpec};
use boxdeck_core::vagrant::boxes::{self, BoxOp};
use boxdeck_core::vagrant::locate::Vagrant;

fn with_home(spec: CommandSpec, home: &std::path::Path) -> CommandSpec {
    spec.env("VAGRANT_HOME", home)
        .env("VAGRANT_CHECKPOINT_DISABLE", "1")
}

#[tokio::test]
#[ignore = "needs a real vagrant on PATH; run with --ignored"]
async fn box_add_list_remove_round_trip() {
    let vagrant = Vagrant::detect(None).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");

    // A minimal offline box: metadata.json only.
    let src = dir.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("metadata.json"), r#"{"provider":"virtualbox"}"#).unwrap();
    std::fs::write(src.join("payload"), vec![7u8; 4096]).unwrap();
    let file = dir.path().join("test.box");
    let tar = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&file)
        .arg("-C")
        .arg(&src)
        .arg(".")
        .status()
        .unwrap();
    assert!(tar.success());

    let jobs = Jobs::new(JobsConfig::default());
    let run = |op: BoxOp| {
        let spec = with_home(boxes::build_op(&vagrant.path, &op).unwrap(), &home);
        jobs.submit(JobRequest {
            label: op.label(),
            origin: Origin::Ui,
            scope: Scope::Boxes,
            spec,
            timeout: Some(Duration::from_secs(120)),
        })
    };

    let add = run(BoxOp::Add {
        source: file.to_string_lossy().into_owned(),
        name: Some("boxdeck/live".into()),
        provider: None,
        architecture: Some("amd64".into()),
        version: None,
        checksum: None,
        force: false,
    });
    let info = jobs.wait(add).await.unwrap();
    assert_eq!(info.state, JobState::Succeeded, "{:?}", jobs.log(add, 30));

    let out = process::run(
        &with_home(boxes::list_command(&vagrant.path), &home),
        Duration::from_secs(60),
    )
    .await
    .unwrap();
    let mut list = boxes::parse_list(&out.stdout);
    boxes::annotate(&mut list, &[], Some(&home));
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "boxdeck/live");
    assert_eq!(list[0].architecture.as_deref(), Some("amd64"));
    assert!(list[0].size_bytes.unwrap() >= 4096, "{:?}", list[0]);

    let remove = run(BoxOp::Remove {
        name: "boxdeck/live".into(),
        version: Some(list[0].version.clone()),
        provider: Some("virtualbox".into()),
        all_architectures: false,
    });
    let info = jobs.wait(remove).await.unwrap();
    assert_eq!(
        info.state,
        JobState::Succeeded,
        "{:?}",
        jobs.log(remove, 30)
    );
    let out = process::run(
        &with_home(boxes::list_command(&vagrant.path), &home),
        Duration::from_secs(60),
    )
    .await
    .unwrap();
    assert!(boxes::parse_list(&out.stdout).is_empty());
}
