//! Job runner integration tests against the scriptable `fake-vagrant` binary.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use boxdeck_core::jobs::{
    JobEvent, JobInfo, JobRequest, JobState, Jobs, JobsConfig, Origin, Scope, Stream,
};
use boxdeck_core::process::CommandSpec;
use boxdeck_core::vagrant::{ProjectEnv, actions};

/// Builds the fake once per test binary and returns its path.
fn fake_vagrant() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = std::process::Command::new(cargo)
            .args(["build", "-q", "-p", "fake-vagrant"])
            .current_dir(&root)
            .status()
            .expect("run cargo build");
        assert!(status.success(), "building fake-vagrant failed");
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"));
        target
            .join("debug")
            .join(format!("vagrant{}", std::env::consts::EXE_SUFFIX))
    })
}

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// A job that runs `script` in fake-vagrant.
    fn job(&self, name: &str, scope: Scope, script: &str) -> JobRequest {
        let file = self.path(&format!("{name}.script"));
        std::fs::write(&file, script).unwrap();
        JobRequest {
            label: name.into(),
            origin: Origin::Ui,
            scope,
            spec: CommandSpec::new(fake_vagrant())
                .arg(name)
                .cwd(self.dir.path())
                .env("FAKE_VAGRANT_SCRIPT", &file),
            timeout: None,
        }
    }
}

fn machine(name: &str) -> Scope {
    Scope::Machine {
        project: "/p".into(),
        name: name.into(),
    }
}

fn jobs() -> Jobs {
    Jobs::new(JobsConfig {
        grace: Duration::from_millis(500),
        ..Default::default()
    })
}

async fn wait(jobs: &Jobs, id: u64) -> JobInfo {
    tokio::time::timeout(Duration::from_secs(20), jobs.wait(id))
        .await
        .expect("job finished in time")
        .expect("job exists")
}

/// Waits until the job has printed a line, i.e. its process is up and running the script.
async fn wait_for_output(jobs: &Jobs, id: u64, text: &str) {
    for _ in 0..200 {
        if jobs
            .log(id, usize::MAX)
            .is_some_and(|l| l.iter().any(|l| l.text == text))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("job {id} never printed {text:?}");
}

fn texts(jobs: &Jobs, id: u64, stream: Stream) -> Vec<String> {
    jobs.log(id, usize::MAX)
        .unwrap()
        .into_iter()
        .filter(|l| l.stream == stream)
        .map(|l| l.text)
        .collect()
}

#[tokio::test]
async fn streams_output_in_order_and_reports_success() {
    let f = Fixture::new();
    let jobs = jobs();
    let mut events = jobs.subscribe();
    let id = jobs.submit(f.job(
        "up",
        machine("web"),
        "out one\nerr warn\nout two\nsleep 50\nout three\n",
    ));
    let info = wait(&jobs, id).await;
    assert_eq!(info.state, JobState::Succeeded);
    assert_eq!(info.exit_code, Some(0));
    assert!(info.started_at.is_some() && info.finished_at.is_some());
    assert_eq!(texts(&jobs, id, Stream::Stdout), ["one", "two", "three"]);
    assert_eq!(texts(&jobs, id, Stream::Stderr), ["warn"]);

    let mut streamed = Vec::new();
    let mut states = Vec::new();
    while let Ok(event) = events.try_recv() {
        match event {
            JobEvent::Output { line, .. } if line.stream == Stream::Stdout => {
                streamed.push(line.text)
            }
            JobEvent::Updated { job } => states.push(job.state),
            _ => {}
        }
    }
    assert_eq!(streamed, ["one", "two", "three"]);
    assert_eq!(
        states,
        [JobState::Queued, JobState::Running, JobState::Succeeded]
    );
}

#[tokio::test]
async fn failure_keeps_the_exit_code() {
    let f = Fixture::new();
    let jobs = jobs();
    let id = jobs.submit(f.job("up", machine("web"), "err boom\nexit 3\n"));
    let info = wait(&jobs, id).await;
    assert_eq!(info.state, JobState::Failed);
    assert_eq!(info.exit_code, Some(3));
}

#[tokio::test]
async fn ring_buffer_keeps_the_last_lines() {
    let f = Fixture::new();
    let jobs = Jobs::new(JobsConfig {
        ring_lines: 3,
        ..Default::default()
    });
    let script: String = (1..=10).map(|i| format!("out {i}\n")).collect();
    let id = jobs.submit(f.job("up", machine("web"), &script));
    wait(&jobs, id).await;
    let log = jobs.log(id, usize::MAX).unwrap();
    assert_eq!(
        log.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        ["8", "9", "10"]
    );
    assert_eq!(log[0].seq, 7);
    assert_eq!(jobs.log(id, 1).unwrap()[0].text, "10");
}

#[tokio::test]
async fn jobs_on_one_machine_queue_and_others_run_in_parallel() {
    let f = Fixture::new();
    let jobs = jobs();
    let first = jobs.submit(f.job("a", machine("web"), "out started\nsleep 400\n"));
    let second = jobs.submit(f.job("b", machine("web"), "out second\n"));
    let other = jobs.submit(f.job("c", machine("db"), "out other\n"));
    wait_for_output(&jobs, first, "started").await;
    assert_eq!(jobs.get(second).unwrap().state, JobState::Queued);
    assert_eq!(wait(&jobs, other).await.state, JobState::Succeeded);
    assert_eq!(jobs.get(second).unwrap().state, JobState::Queued);
    let first_done = wait(&jobs, first).await.finished_at.unwrap();
    let second_started = wait(&jobs, second).await.started_at.unwrap();
    assert!(second_started >= first_done);
}

#[tokio::test]
async fn project_wide_jobs_lock_every_machine() {
    let f = Fixture::new();
    let jobs = jobs();
    let whole = jobs.submit(f.job(
        "up",
        Scope::Project {
            project: "/p".into(),
        },
        "out started\nsleep 300\n",
    ));
    let one = jobs.submit(f.job("halt", machine("db"), "out ok\n"));
    let elsewhere = jobs.submit(f.job(
        "halt",
        Scope::Machine {
            project: "/q".into(),
            name: "db".into(),
        },
        "out ok\n",
    ));
    wait_for_output(&jobs, whole, "started").await;
    assert_eq!(jobs.get(one).unwrap().state, JobState::Queued);
    assert_eq!(wait(&jobs, elsewhere).await.state, JobState::Succeeded);
    wait(&jobs, whole).await;
    assert_eq!(wait(&jobs, one).await.state, JobState::Succeeded);
}

#[tokio::test]
async fn global_cap_limits_running_jobs() {
    let f = Fixture::new();
    let jobs = Jobs::new(JobsConfig {
        global_cap: 1,
        ..Default::default()
    });
    let a = jobs.submit(f.job("a", machine("web"), "out started\nsleep 300\n"));
    let b = jobs.submit(f.job("b", machine("db"), "out ok\n"));
    wait_for_output(&jobs, a, "started").await;
    assert_eq!(jobs.get(b).unwrap().state, JobState::Queued);
    wait(&jobs, a).await;
    assert_eq!(wait(&jobs, b).await.state, JobState::Succeeded);
}

#[tokio::test]
async fn box_jobs_run_one_at_a_time() {
    let f = Fixture::new();
    let jobs = jobs();
    let a = jobs.submit(f.job("a", Scope::Boxes, "out started\nsleep 300\n"));
    let b = jobs.submit(f.job("b", Scope::Boxes, "out ok\n"));
    wait_for_output(&jobs, a, "started").await;
    assert_eq!(jobs.get(b).unwrap().state, JobState::Queued);
    wait(&jobs, b).await;
}

#[tokio::test]
async fn cancelling_a_queued_job_drops_it() {
    let f = Fixture::new();
    let jobs = jobs();
    let a = jobs.submit(f.job("a", machine("web"), "out started\nsleep 300\n"));
    let b = jobs.submit(f.job("b", machine("web"), "out never\n"));
    jobs.cancel(b, false).unwrap();
    assert_eq!(jobs.get(b).unwrap().state, JobState::Cancelled);
    wait(&jobs, a).await;
    assert!(jobs.log(b, usize::MAX).unwrap().is_empty());
    assert!(jobs.cancel(b, false).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn graceful_cancel_lets_vagrant_clean_up() {
    let f = Fixture::new();
    let jobs = jobs();
    let id = jobs.submit(f.job("up", machine("web"), "out started\nsleep 30000\n"));
    wait_for_output(&jobs, id, "started").await;
    jobs.cancel(id, false).unwrap();
    assert_eq!(jobs.get(id).unwrap().state, JobState::Cancelling);
    let info = wait(&jobs, id).await;
    assert_eq!(info.state, JobState::Cancelled);
    assert_eq!(info.exit_code, Some(130));
    assert!(
        texts(&jobs, id, Stream::Stdout)
            .contains(&"==> Waiting for cleanup before exiting...".into())
    );
}

#[tokio::test]
async fn ignored_interrupt_is_followed_by_a_tree_kill() {
    let f = Fixture::new();
    let jobs = jobs();
    let pidfile = f.path("child.pid");
    let id = jobs.submit(f.job(
        "up",
        machine("web"),
        &format!(
            "ignore-interrupt\nspawn-child {}\nout started\nsleep 30000\n",
            pidfile.display()
        ),
    ));
    wait_for_output(&jobs, id, "started").await;
    let child: u32 = std::fs::read_to_string(&pidfile).unwrap().parse().unwrap();
    assert!(alive(child));
    jobs.cancel(id, false).unwrap();
    // Grace is 500 ms in these tests; then the whole tree goes.
    let info = wait(&jobs, id).await;
    assert_eq!(info.state, JobState::Cancelled);
    assert!(
        wait_until_dead(child).await,
        "child {child} survived the tree kill"
    );
}

#[tokio::test]
async fn force_cancel_kills_the_tree_immediately() {
    let f = Fixture::new();
    let jobs = Jobs::new(JobsConfig {
        grace: Duration::from_secs(60),
        ..Default::default()
    });
    let pidfile = f.path("child.pid");
    let id = jobs.submit(f.job(
        "up",
        machine("web"),
        &format!(
            "ignore-interrupt\nspawn-child {}\nout started\nsleep 30000\n",
            pidfile.display()
        ),
    ));
    wait_for_output(&jobs, id, "started").await;
    let child: u32 = std::fs::read_to_string(&pidfile).unwrap().parse().unwrap();
    jobs.cancel(id, true).unwrap();
    assert_eq!(wait(&jobs, id).await.state, JobState::Cancelled);
    assert!(
        wait_until_dead(child).await,
        "child {child} survived the tree kill"
    );
}

#[tokio::test]
async fn timeout_kills_the_job() {
    let f = Fixture::new();
    let jobs = jobs();
    let mut req = f.job("up", machine("web"), "ignore-interrupt\nsleep 30000\n");
    req.timeout = Some(Duration::from_millis(300));
    let id = jobs.submit(req);
    assert_eq!(wait(&jobs, id).await.state, JobState::TimedOut);
}

#[tokio::test]
async fn waiting_for_input_fails_fast() {
    let f = Fixture::new();
    let jobs = jobs();
    let id = jobs.submit(f.job("up", machine("web"), "read-stdin\nout unreachable\n"));
    let info = wait(&jobs, id).await;
    assert_eq!(info.state, JobState::Failed);
    assert!(
        texts(&jobs, id, Stream::Stderr)
            .iter()
            .any(|l| l.contains("requires"))
    );
    assert!(texts(&jobs, id, Stream::Stdout).is_empty());
}

#[tokio::test]
async fn spawn_failure_is_reported() {
    let jobs = jobs();
    let id = jobs.submit(JobRequest {
        label: "up".into(),
        origin: Origin::Mcp("agent".into()),
        scope: machine("web"),
        spec: CommandSpec::new("/definitely/not/vagrant"),
        timeout: None,
    });
    let info = wait(&jobs, id).await;
    assert_eq!(info.state, JobState::Failed);
    assert!(info.error.is_some());
    assert_eq!(info.origin, Origin::Mcp("agent".into()));
}

#[tokio::test]
async fn action_argv_and_project_env_reach_vagrant() {
    let f = Fixture::new();
    let script = f.path("args.script");
    std::fs::write(&script, "args\nenv VAGRANT_VAGRANTFILE\n").unwrap();
    let env = ProjectEnv {
        dir: f.dir.path().to_owned(),
        vagrantfile: Some("Custom.rb".into()),
        dotfile_path: None,
    };
    let spec = actions::build(
        fake_vagrant(),
        &env,
        actions::Action::Destroy,
        Some("web"),
        &Default::default(),
    )
    .unwrap()
    .env("FAKE_VAGRANT_SCRIPT", &script);
    let jobs = jobs();
    let id = jobs.submit(JobRequest {
        label: "destroy web".into(),
        origin: Origin::Ui,
        scope: machine("web"),
        spec,
        timeout: None,
    });
    wait(&jobs, id).await;
    assert_eq!(
        texts(&jobs, id, Stream::Stdout),
        ["destroy web -f --no-tty", "VAGRANT_VAGRANTFILE=Custom.rb"]
    );
}

#[tokio::test]
async fn list_is_newest_first() {
    let f = Fixture::new();
    let jobs = jobs();
    let a = jobs.submit(f.job("a", machine("web"), "out a\n"));
    let b = jobs.submit(f.job("b", machine("db"), "out b\n"));
    wait(&jobs, a).await;
    wait(&jobs, b).await;
    let ids: Vec<_> = jobs.list().iter().map(|j| j.id).collect();
    assert_eq!(ids, [b, a]);
}

fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    }
}

async fn wait_until_dead(pid: u32) -> bool {
    for _ in 0..100 {
        if !alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

#[tokio::test]
async fn status_holds_and_jobs_never_overlap_in_one_project() {
    let f = Fixture::new();
    let jobs = jobs();
    let hold = jobs
        .try_hold_project(Path::new("/p"))
        .expect("idle project");
    // A job submitted while the project is held waits for the hold.
    let id = jobs.submit(f.job("up", machine("web"), "out ran\n"));
    let other = jobs.submit(f.job(
        "up",
        Scope::Machine {
            project: "/q".into(),
            name: "web".into(),
        },
        "out ok\n",
    ));
    assert_eq!(wait(&jobs, other).await.state, JobState::Succeeded);
    assert_eq!(jobs.get(id).unwrap().state, JobState::Queued);
    // And no second hold while that job is pending.
    drop(hold);
    assert_eq!(wait(&jobs, id).await.state, JobState::Succeeded);

    let long = jobs.submit(f.job("halt", machine("web"), "out started\nsleep 300\n"));
    assert!(jobs.try_hold_project(Path::new("/p")).is_none());
    assert!(jobs.is_busy(Path::new("/p")));
    assert!(!jobs.is_busy(Path::new("/q")));
    wait(&jobs, long).await;
    assert!(jobs.try_hold_project(Path::new("/p")).is_some());
}

/// Regression: Tauri calls sync commands on the GUI main thread, which has no Tokio runtime.
/// Submitting from there used to panic in `tokio::spawn` and abort the app.
#[test]
fn submit_from_a_thread_without_a_runtime() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let f = Fixture::new();
    let jobs = Jobs::new(JobsConfig {
        runtime: Some(rt.handle().clone()),
        ..Default::default()
    });
    let req = f.job("add", Scope::Boxes, "out added\n");
    let id = std::thread::spawn(move || jobs.submit(req))
        .join()
        .expect("submit must not panic");
    let _ = id;
}

#[test]
fn without_any_runtime_the_job_fails_instead_of_panicking() {
    let f = Fixture::new();
    let jobs = Jobs::new(JobsConfig::default());
    let id = jobs.submit(f.job("add", Scope::Boxes, "out added\n"));
    let info = jobs.get(id).unwrap();
    assert_eq!(info.state, JobState::Failed);
    assert!(info.error.unwrap().contains("no async runtime"));
}
