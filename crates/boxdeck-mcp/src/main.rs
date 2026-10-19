//! `boxdeck-mcp`: Boxdeck's MCP server over stdio, for clients that launch servers themselves
//! or when the app isn't running. stdout carries the protocol, so diagnostics go to stderr.

use std::process::ExitCode;
use std::sync::{Arc, RwLock};

use boxdeck_core::audit::AuditLog;
use boxdeck_core::bridge::Bridge;
use boxdeck_core::config::{self, Settings, SettingsStore};
use boxdeck_core::jobs::{Jobs, JobsConfig};
use boxdeck_core::vagrant::index;
use boxdeck_core::vagrant::locate::{self, Vagrant};
use boxdeck_core::vagrant::status::StatusService;
use boxdeck_mcp::{BoxdeckServer, Ctx};
use rmcp::ServiceExt;

const HELP: &str = "boxdeck-mcp: Boxdeck's MCP server over stdio.

Point your MCP client at this program, or use Connect in Boxdeck's Settings -> MCP. It uses
the Boxdeck app's settings (turn agent access on there). Calls that need your approval are
shown in the app while it's running; without it they are refused unless you allowed them.

Options:
  --version   print the version
  --help      print this help";

fn main() -> ExitCode {
    // Before the runtime starts: it sets environment variables.
    locate::fix_path();
    match std::env::args().nth(1).as_deref() {
        Some("--version") => {
            println!("boxdeck-mcp {}", boxdeck_mcp::server_version());
            return ExitCode::SUCCESS;
        }
        Some("--help" | "-h") => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("boxdeck-mcp: unknown option {other:?}\n\n{HELP}");
            return ExitCode::from(2);
        }
        None => {}
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("boxdeck-mcp: {e}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(serve()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("boxdeck-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}

fn load(store: Option<&SettingsStore>) -> Settings {
    match store.map(SettingsStore::load) {
        Some(Ok(s)) => s,
        Some(Err(e)) => {
            eprintln!("boxdeck-mcp: using default settings: {e}");
            Settings::default()
        }
        None => Settings::default(),
    }
}

async fn serve() -> Result<(), String> {
    let store = config::default_path().ok().map(SettingsStore::new);
    let settings = Arc::new(RwLock::new(load(store.as_ref())));

    // The app is the only writer; follow its changes (e.g. the pause switch) live.
    let _watch = store.as_ref().and_then(|store| {
        let path = store.path().to_owned();
        let shared = settings.clone();
        let store = store.clone();
        boxdeck_core::watch::watch_file(&path, move || {
            *shared.write().unwrap_or_else(|e| e.into_inner()) = load(Some(&store));
        })
        .map_err(|e| eprintln!("boxdeck-mcp: not watching settings: {e}"))
        .ok()
    });

    let initial = settings.read().unwrap_or_else(|e| e.into_inner()).clone();
    let vagrant = Vagrant::detect(initial.vagrant_path.as_deref())
        .await
        .map_err(|e| e.to_string());
    if let Err(e) = &vagrant {
        eprintln!("boxdeck-mcp: {e}");
    }
    let status = vagrant.as_ref().ok().map(|v| {
        Arc::new(StatusService::new(
            v.path.clone(),
            initial.status.concurrency,
        ))
    });
    let home = index::default_vagrant_home();
    let reader = settings.clone();
    let ctx = Ctx {
        vagrant,
        jobs: Jobs::new(JobsConfig {
            global_cap: initial.jobs.global_cap,
            ..Default::default()
        }),
        status,
        settings: Arc::new(move || reader.read().unwrap_or_else(|e| e.into_inner()).clone()),
        index_path: home.as_ref().map(|h| index::index_path(h)),
        vagrant_home: home,
        audit: AuditLog::default_log().ok(),
        approvals: Bridge::default_bridge()
            .ok()
            .map(|b| Arc::new(b) as Arc<dyn boxdeck_mcp::Approver>),
        on_job_done: None,
    };
    let service = BoxdeckServer::new(Arc::new(ctx))
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| e.to_string())?;
    service.waiting().await.map_err(|e| e.to_string())?;
    Ok(())
}
