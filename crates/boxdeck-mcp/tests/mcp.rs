//! Drives the real MCP server through an in-memory rmcp client, with `fake-vagrant` standing in
//! for Vagrant.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use boxdeck_core::audit::AuditLog;
use boxdeck_core::config::{AccessLevel, AgentAccess, ProjectSettings, Settings, ShellMode};
use boxdeck_core::jobs::{JobRequest, Jobs, JobsConfig, Origin, Scope};
use boxdeck_core::process::CommandSpec;
use boxdeck_core::vagrant::locate::{Features, Vagrant};
use boxdeck_core::vagrant::status::StatusService;
use boxdeck_mcp::ctx::BoxFuture;
use boxdeck_mcp::{ApprovalRequest, Approver, BoxdeckServer, Ctx};
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientRequest, ProgressNotificationParam, Request,
};
use rmcp::service::{NotificationContext, PeerRequestOptions, RunningService};
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::{Value, json};

fn fake_vagrant() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let ok = std::process::Command::new(cargo)
            .args(["build", "-q", "-p", "fake-vagrant"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success();
        assert!(ok, "building fake-vagrant failed");
        std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"))
            .join("debug")
            .join(format!("vagrant{}", std::env::consts::EXE_SUFFIX))
    })
}

#[derive(Default, Clone)]
struct Client {
    progress: Arc<Mutex<Vec<String>>>,
}

impl ClientHandler for Client {
    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _cx: NotificationContext<RoleClient>,
    ) {
        if let Some(m) = params.message {
            self.progress.lock().unwrap().push(m);
        }
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    project: PathBuf,
    settings: Arc<Mutex<Settings>>,
    jobs: Jobs,
    audit: AuditLog,
    client: RunningService<RoleClient, Client>,
    progress: Arc<Mutex<Vec<String>>>,
}

/// One project with a `web` machine. The fake answers every command with the same script:
/// machine-readable status lines, then the argv it got, then `sleep_ms` of work.
async fn harness(access: AccessLevel, sleep_ms: u64) -> Harness {
    harness_with(access, sleep_ms, None).await
}

/// Approval requests the fake app saw, as `tool machine grantable`.
type Answers = Arc<Mutex<Vec<String>>>;

/// Stands in for the running Boxdeck app on the approval bridge.
struct FakeApp {
    answer: bool,
    seen: Answers,
    granted: bool,
}

impl Approver for FakeApp {
    fn app_running(&self) -> bool {
        true
    }

    fn granted(&self, _project: &Path, _machine: &str) -> bool {
        self.granted
    }

    fn ask(&self, req: ApprovalRequest) -> BoxFuture<bool> {
        self.seen.lock().unwrap().push(format!(
            "{} {} {}",
            req.tool,
            req.machine.unwrap_or_default(),
            req.grantable
        ));
        let answer = self.answer;
        Box::pin(async move { answer })
    }
}

/// `app`: the app is running and answers every approval with this.
async fn harness_with(access: AccessLevel, sleep_ms: u64, app: Option<(bool, Answers)>) -> Harness {
    harness_app(
        access,
        sleep_ms,
        app.map(|(answer, seen)| FakeApp {
            answer,
            seen,
            granted: false,
        }),
    )
    .await
}

async fn harness_app(access: AccessLevel, sleep_ms: u64, app: Option<FakeApp>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("proj").canonicalize().unwrap_or_else(|_| {
        std::fs::create_dir(dir.path().join("proj")).unwrap();
        dir.path().join("proj").canonicalize().unwrap()
    });
    std::fs::write(project.join("Vagrantfile"), "Vagrant.configure(\"2\") {}\n").unwrap();
    let index = dir.path().join("index");
    let entry = json!({"version": 1, "machines": {"abc": {
        "name": "web", "provider": "docker", "state": "poweroff",
        "vagrantfile_path": project, "local_data_path": project.join(".vagrant")}}});
    std::fs::write(&index, entry.to_string()).unwrap();
    let script = dir.path().join("fake.script");
    std::fs::write(
        &script,
        format!(
            "out 1,web,provider-name,docker\nout 1,web,state,running\nout \
             1,web,state-human-short,running\nargs\nsleep {sleep_ms}\n"
        ),
    )
    .unwrap();

    let mut s = Settings::default();
    s.mcp.enabled = true;
    s.mcp.access = access;
    s.env.insert(
        "FAKE_VAGRANT_SCRIPT".into(),
        script.to_string_lossy().into(),
    );
    let settings = Arc::new(Mutex::new(s));
    let jobs = Jobs::new(JobsConfig::default());
    let audit = AuditLog::new(dir.path().join("audit.jsonl"));
    let reader = settings.clone();
    let ctx = Ctx {
        vagrant: Ok(Vagrant {
            path: fake_vagrant().to_owned(),
            version: semver_249(),
            features: Features {
                box_architecture: true,
            },
        }),
        jobs: jobs.clone(),
        status: Some(Arc::new(StatusService::new(fake_vagrant().to_owned(), 2))),
        settings: Arc::new(move || reader.lock().unwrap().clone()),
        index_path: Some(index),
        vagrant_home: None,
        audit: Some(audit.clone()),
        approvals: app.map(|a| Arc::new(a) as Arc<dyn Approver>),
        on_job_done: None,
    };
    let (server_io, client_io) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        let running = BoxdeckServer::new(Arc::new(ctx))
            .serve(server_io)
            .await
            .unwrap();
        let _ = running.waiting().await;
    });
    let handler = Client::default();
    let progress = handler.progress.clone();
    let client = handler.serve(client_io).await.unwrap();
    Harness {
        _dir: dir,
        project,
        settings,
        jobs,
        audit,
        client,
        progress,
    }
}

fn semver_249() -> semver::Version {
    boxdeck_core::vagrant::locate::parse_version("Vagrant 2.4.9").unwrap()
}

impl Harness {
    async fn call(&self, tool: &'static str, args: Value) -> CallToolResult {
        let mut params = CallToolRequestParams::new(tool);
        if let Value::Object(map) = args {
            params = params.with_arguments(map);
        }
        self.client.call_tool(params).await.unwrap()
    }

    fn dir(&self) -> String {
        self.project.to_string_lossy().into_owned()
    }
}

fn denied(r: &CallToolResult) -> String {
    assert_eq!(r.is_error, Some(true), "expected an error: {r:?}");
    serde_json::to_string(&r.content).unwrap()
}

#[tokio::test]
async fn tool_list_schemas_and_annotations() {
    let h = harness(AccessLevel::Read, 0).await;
    let tools = h.client.list_all_tools().await.unwrap();
    let summary: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "annotations": t.annotations,
                "input": t.input_schema.get("properties").map(|p| {
                    let mut keys: Vec<_> = p.as_object().unwrap().keys().cloned().collect();
                    keys.sort();
                    keys
                }),
                "has_output_schema": t.output_schema.is_some(),
            })
        })
        .collect();
    insta::assert_json_snapshot!(summary);
}

#[tokio::test]
async fn everything_is_refused_while_mcp_is_off_or_paused() {
    let h = harness(AccessLevel::Operate, 0).await;
    h.settings.lock().unwrap().mcp.enabled = false;
    assert!(denied(&h.call("list_machines", json!({})).await).contains("turned off"));
    {
        let mut s = h.settings.lock().unwrap();
        s.mcp.enabled = true;
        s.mcp.paused = true;
    }
    assert!(denied(&h.call("list_machines", json!({})).await).contains("paused"));
    let log = h.audit.tail(10).unwrap();
    assert_eq!(log.len(), 2);
    assert!(log.iter().all(|e| e.decision == "denied"));
    assert_eq!(log[0].client, "stdio:rmcp");
}

#[tokio::test]
async fn read_level_lists_but_cannot_act() {
    let h = harness(AccessLevel::Read, 0).await;
    let r = h.call("list_machines", json!({})).await;
    assert_eq!(r.is_error, Some(false));
    let machines = &r.structured_content.unwrap()["machines"];
    assert_eq!(machines[0]["name"], "web");
    assert_eq!(machines[0]["state"], "poweroff");
    assert_eq!(machines[0]["state_source"], "index");

    let msg = denied(
        &h.call("machine_up", json!({"project": h.dir(), "machine": "web"}))
            .await,
    );
    assert!(msg.contains("Operate"));
    assert!(h.jobs.list().is_empty());
}

#[tokio::test]
async fn denied_projects_are_invisible() {
    let h = harness(AccessLevel::Operate, 0).await;
    h.settings.lock().unwrap().projects.insert(
        h.dir(),
        ProjectSettings {
            agent_access: AgentAccess::Deny,
            ..Default::default()
        },
    );
    let r = h.call("list_machines", json!({})).await;
    assert_eq!(r.structured_content.unwrap()["machines"], json!([]));
    let msg = denied(
        &h.call("get_machine_status", json!({"project": h.dir()}))
            .await,
    );
    assert!(msg.contains("Unknown project folder"));
}

#[tokio::test]
async fn up_waits_streams_progress_and_records_the_job() {
    let h = harness(AccessLevel::Operate, 50).await;
    let params = CallToolRequestParams::new("machine_up").with_arguments(
        json!({"project": h.dir(), "machine": "web", "wait_seconds": 30})
            .as_object()
            .unwrap()
            .clone(),
    );
    let handle = h
        .client
        .send_cancellable_request(
            ClientRequest::CallToolRequest(Request::new(params)),
            PeerRequestOptions::no_options(),
        )
        .await
        .unwrap();
    let response = handle.await_response().await.unwrap();
    let result: CallToolResult = match response {
        rmcp::model::ServerResult::CallToolResult(r) => r,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(result.is_error, Some(false), "{result:?}");
    let out = result.structured_content.unwrap();
    assert_eq!(out["job"]["state"], "succeeded");
    assert_eq!(out["job"]["origin"], "mcp:stdio:rmcp");
    let tail: Vec<String> = serde_json::from_value(out["log_tail"].clone()).unwrap();
    assert!(tail.contains(&"up web --no-tty".to_owned()), "{tail:?}");

    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        h.progress
            .lock()
            .unwrap()
            .contains(&"up web --no-tty".to_owned())
    );
    let entry = h.audit.tail(1).unwrap().remove(0);
    assert_eq!(entry.tool, "machine_up");
    assert_eq!(entry.decision, "allowed");
    assert!(entry.job_id.is_some());
}

#[tokio::test]
async fn short_wait_returns_a_job_to_poll() {
    let h = harness(AccessLevel::Operate, 1500).await;
    let r = h
        .call(
            "machine_halt",
            json!({"project": h.dir(), "machine": "web", "wait_seconds": 0}),
        )
        .await;
    let out = r.structured_content.unwrap();
    assert_eq!(out["job"]["state"], "running");
    assert!(out["hint"].as_str().unwrap().contains("get_job"));
    let id = out["job"]["job_id"].as_u64().unwrap();

    let mut state = String::new();
    for _ in 0..50 {
        let r = h.call("get_job", json!({"job_id": id})).await;
        state = r.structured_content.unwrap()["job"]["state"]
            .as_str()
            .unwrap()
            .to_owned();
        if state != "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(state, "succeeded");
}

#[tokio::test]
async fn bad_input_is_rejected_before_vagrant_runs() {
    let h = harness(AccessLevel::Operate, 0).await;
    let msg = denied(
        &h.call("machine_up", json!({"project": h.dir(), "machine": "nope"}))
            .await,
    );
    assert!(msg.contains("No machine named"));
    let msg = denied(
        &h.call(
            "machine_up",
            json!({"project": h.dir(), "machine": "web", "provider": "--evil"}),
        )
        .await,
    );
    assert!(msg.contains("provider"));
    let msg = denied(
        &h.call("machine_up", json!({"project": "/does/not/exist"}))
            .await,
    );
    assert!(msg.contains("Unknown project folder"));
    assert!(h.jobs.list().is_empty());
}

#[tokio::test]
async fn agents_only_cancel_their_own_jobs() {
    let h = harness(AccessLevel::Operate, 0).await;
    let id = h.jobs.submit(JobRequest {
        label: "up web".into(),
        origin: Origin::Ui,
        scope: Scope::Unlocked,
        spec: CommandSpec::new(fake_vagrant()),
        timeout: None,
    });
    let msg = denied(&h.call("cancel_job", json!({"job_id": id})).await);
    assert!(msg.contains("only cancel jobs they started"));
}

#[tokio::test]
async fn reads_the_vagrantfile() {
    let h = harness(AccessLevel::Read, 0).await;
    let r = h
        .call("read_vagrantfile", json!({"project": h.dir()}))
        .await;
    let out = r.structured_content.unwrap();
    assert!(
        out["content"]
            .as_str()
            .unwrap()
            .contains("Vagrant.configure")
    );
}

#[tokio::test]
async fn destructive_tools_are_refused_headless_by_default() {
    let h = harness(AccessLevel::Full, 0).await;
    let args = json!({"project": h.dir(), "machine": "web"});
    assert!(denied(&h.call("machine_destroy", args.clone()).await).contains("Open Boxdeck"));
    assert!(h.jobs.list().is_empty());
    h.settings.lock().unwrap().mcp.headless_destructive = true;
    let r = h.call("machine_destroy", args).await;
    assert_eq!(r.is_error, Some(false), "{r:?}");
    let tail = r.structured_content.unwrap()["log_tail"].to_string();
    assert!(tail.contains("destroy web -f --no-tty"), "{tail}");
}

#[tokio::test]
async fn the_app_asks_the_user_and_respects_the_answer() {
    let seen: Answers = Arc::default();
    let h = harness_with(AccessLevel::Full, 0, Some((false, seen.clone()))).await;
    let args = json!({"project": h.dir(), "machine": "web"});
    assert!(denied(&h.call("machine_destroy", args.clone()).await).contains("didn't approve"));
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["machine_destroy web false"]
    );
    assert_eq!(h.audit.tail(1).unwrap()[0].decision, "rejected");
    assert!(h.jobs.list().is_empty());

    let seen: Answers = Arc::default();
    let h = harness_with(AccessLevel::Full, 0, Some((true, seen.clone()))).await;
    let r = h
        .call(
            "machine_destroy",
            json!({"project": h.dir(), "machine": "web"}),
        )
        .await;
    assert_eq!(r.is_error, Some(false), "{r:?}");
    assert_eq!(h.audit.tail(1).unwrap()[0].decision, "approved");
}

/// Writes the `synced_folders` record Vagrant keeps for `web`.
fn synced(h: &Harness, json: &str) {
    let dir = h.project.join(".vagrant/machines/web/docker");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("synced_folders"), json).unwrap();
}

#[tokio::test]
async fn ssh_exec_needs_shell_mode_and_operate_and_returns_output() {
    let h = harness(AccessLevel::Operate, 0).await;
    let args = json!({"project": h.dir(), "machine": "web", "command": "uname -a"});
    assert!(denied(&h.call("ssh_exec", args.clone()).await).contains("Shell access"));
    h.settings.lock().unwrap().mcp.shell = ShellMode::Always;
    let r = h.call("ssh_exec", args).await;
    assert_eq!(r.is_error, Some(false), "{r:?}");
    let out = r.structured_content.unwrap();
    assert_eq!(out["exit_code"], 0);
    assert_eq!(out["running"], false);
    assert_eq!(out["truncated"], false);
    let stdout = out["stdout"].as_str().unwrap();
    assert!(stdout.contains("ssh web -c uname -a --no-tty"), "{stdout}");

    h.settings.lock().unwrap().mcp.access = AccessLevel::Read;
    let args = json!({"project": h.dir(), "machine": "web", "command": "ls"});
    assert!(denied(&h.call("ssh_exec", args).await).contains("Operate"));
}

#[tokio::test]
async fn ssh_exec_runs_in_cwd_and_long_commands_become_jobs() {
    let h = harness(AccessLevel::Operate, 1500).await;
    h.settings.lock().unwrap().mcp.shell = ShellMode::Always;
    let r = h
        .call(
            "ssh_exec",
            json!({"project": h.dir(), "machine": "web", "command": "make", "cwd": "/srv/app",
                   "wait_seconds": 0}),
        )
        .await;
    let out = r.structured_content.unwrap();
    assert_eq!(out["running"], true, "{out}");
    let id = out["job_id"].as_u64().unwrap();
    assert!(out["hint"].as_str().unwrap().contains("get_job"));
    let info = h.jobs.wait(id).await.unwrap();
    assert_eq!(info.exit_code, Some(0));
    let polled = h.call("get_job", json!({"job_id": id})).await;
    let tail = polled.structured_content.unwrap()["log_tail"].to_string();
    assert!(tail.contains("cd '/srv/app' && make"), "{tail}");
}

#[tokio::test]
async fn ask_once_asks_for_host_writable_machines_only() {
    let seen: Answers = Arc::default();
    let h = harness_with(AccessLevel::Operate, 0, Some((true, seen.clone()))).await;
    h.settings.lock().unwrap().mcp.shell = ShellMode::AskOnce;
    let args = json!({"project": h.dir(), "machine": "web", "command": "ls"});

    // No record yet counts as writable: asks, and offers the session grant.
    assert_eq!(h.call("ssh_exec", args.clone()).await.is_error, Some(false));
    assert_eq!(seen.lock().unwrap().as_slice(), ["ssh_exec web true"]);
    assert_eq!(h.audit.tail(1).unwrap()[0].decision, "approved");

    // rsync only copies host to guest: no question.
    synced(
        &h,
        r#"{"rsync":{"/vagrant":{"hostpath":"/x","guestpath":"/vagrant"}}}"#,
    );
    assert_eq!(h.call("ssh_exec", args.clone()).await.is_error, Some(false));
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(h.audit.tail(1).unwrap()[0].decision, "allowed");

    synced(
        &h,
        r#"{"virtualbox":{"/vagrant":{"hostpath":"/x","guestpath":"/vagrant"}}}"#,
    );
    assert_eq!(h.call("ssh_exec", args).await.is_error, Some(false));
    assert_eq!(seen.lock().unwrap().len(), 2);

    let refused = harness_with(AccessLevel::Operate, 0, Some((false, seen.clone()))).await;
    refused.settings.lock().unwrap().mcp.shell = ShellMode::AskOnce;
    let args = json!({"project": refused.dir(), "machine": "web", "command": "ls"});
    assert!(denied(&refused.call("ssh_exec", args).await).contains("didn't approve"));
}

#[tokio::test]
async fn ask_once_respects_grants_and_the_headless_switch() {
    let seen: Answers = Arc::default();
    let granted = FakeApp {
        answer: false,
        seen: seen.clone(),
        granted: true,
    };
    let h = harness_app(AccessLevel::Operate, 0, Some(granted)).await;
    h.settings.lock().unwrap().mcp.shell = ShellMode::AskOnce;
    let args = json!({"project": h.dir(), "machine": "web", "command": "ls"});
    assert_eq!(h.call("ssh_exec", args).await.is_error, Some(false));
    assert!(seen.lock().unwrap().is_empty());

    let h = harness(AccessLevel::Operate, 0).await;
    h.settings.lock().unwrap().mcp.shell = ShellMode::AskOnce;
    let args = json!({"project": h.dir(), "machine": "web", "command": "ls"});
    assert!(denied(&h.call("ssh_exec", args.clone()).await).contains("Open the Boxdeck app"));
    h.settings.lock().unwrap().mcp.shell_headless = true;
    assert_eq!(h.call("ssh_exec", args).await.is_error, Some(false));
}

#[tokio::test]
async fn vm_upload_only_sends_files_from_the_project() {
    let h = harness(AccessLevel::Operate, 0).await;
    h.settings.lock().unwrap().mcp.shell = ShellMode::Always;
    std::fs::write(h.project.join("build.tar"), "x").unwrap();
    let r = h
        .call(
            "vm_upload",
            json!({"project": h.dir(), "machine": "web", "source": "build.tar",
                   "destination": "/tmp/build.tar"}),
        )
        .await;
    assert_eq!(r.is_error, Some(false), "{r:?}");
    let tail = r.structured_content.unwrap()["log_tail"].to_string();
    let source = h.project.join("build.tar");
    assert!(
        tail.contains(&format!(
            "upload {} /tmp/build.tar web --no-tty",
            source.display()
        )),
        "{tail}"
    );
    let jobs = h.jobs.list().len();
    let r = h
        .call(
            "vm_upload",
            json!({"project": h.dir(), "machine": "web", "source": "../index",
                   "destination": "/tmp/x"}),
        )
        .await;
    assert!(denied(&r).contains("inside the project"));
    assert_eq!(h.jobs.list().len(), jobs);
}

#[tokio::test]
async fn vm_download_returns_text_or_saves_inside_the_project() {
    let h = harness(AccessLevel::Operate, 0).await;
    h.settings.lock().unwrap().mcp.shell = ShellMode::Always;
    let script = h.project.join("download.script");
    std::fs::write(&script, "out 12\nout hello\n").unwrap();
    h.settings.lock().unwrap().env.insert(
        "FAKE_VAGRANT_SCRIPT".into(),
        script.to_string_lossy().into(),
    );
    let r = h
        .call(
            "vm_download",
            json!({"project": h.dir(), "machine": "web", "path": "/etc/motd"}),
        )
        .await;
    let out = r.structured_content.unwrap();
    assert_eq!(out["content"], "hello\n");
    assert_eq!(out["size_bytes"], 12);
    assert_eq!(out["truncated"], true);

    let r = h
        .call(
            "vm_download",
            json!({"project": h.dir(), "machine": "web", "path": "/etc/motd",
                   "save_to": "motd.txt"}),
        )
        .await;
    assert_eq!(r.is_error, Some(false), "{r:?}");
    assert_eq!(
        std::fs::read_to_string(h.project.join("motd.txt")).unwrap(),
        "hello\n"
    );
    let r = h
        .call(
            "vm_download",
            json!({"project": h.dir(), "machine": "web", "path": "/etc/motd",
                   "save_to": "../motd.txt"}),
        )
        .await;
    assert!(denied(&r).contains("inside the project"));
}

#[tokio::test]
async fn write_vagrantfile_refuses_stale_edits() {
    let seen: Answers = Arc::default();
    let h = harness_with(AccessLevel::Full, 0, Some((true, seen))).await;
    let r = h
        .call(
            "write_vagrantfile",
            json!({"project": h.dir(), "content": "x", "expected_content": "not what's there"}),
        )
        .await;
    assert!(denied(&r).contains("changed since you read it"));
    assert_eq!(
        std::fs::read_to_string(h.project.join("Vagrantfile")).unwrap(),
        "Vagrant.configure(\"2\") {}\n"
    );
}
