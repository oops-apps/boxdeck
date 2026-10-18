//! In-app MCP endpoint over Streamable HTTP (plan §4.8): `http://127.0.0.1:<port>/mcp`, one
//! named bearer token per client, loopback Host/Origin checks, and approvals in the app window.

use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use boxdeck_core::audit::AuditLog;
use boxdeck_core::policy::Transport;
use boxdeck_mcp::{BoxdeckServer, ClientName, Ctx};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::{approvals, commands};

#[derive(Debug, Clone, Default, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct McpStatus {
    pub running: bool,
    pub port: Option<u16>,
    /// Why the endpoint isn't running although it's turned on, e.g. the port is taken.
    pub error: Option<String>,
    pub url: Option<String>,
    /// The bundled `boxdeck-mcp` for stdio clients.
    pub stdio_path: String,
}

struct Running {
    port: u16,
    stop: Box<dyn FnOnce() + Send>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The context MCP tools run against, sharing the app's jobs, status cache and settings.
fn ctx(app: &AppHandle) -> Ctx {
    let state = app.state::<AppState>();
    let settings_app = app.clone();
    let approve_app = app.clone();
    let done_app = app.clone();
    Ctx {
        vagrant: state.vagrant(),
        jobs: state.jobs.clone(),
        status: state.status_service(),
        settings: Arc::new(move || settings_app.state::<AppState>().settings()),
        index_path: state.index_path.clone(),
        vagrant_home: state.vagrant_home.clone(),
        transport: Transport::Http,
        audit: AuditLog::default_log().ok(),
        approve: Some(Arc::new(move |req| {
            let app = approve_app.clone();
            Box::pin(async move { approvals::ask(&app, req).await })
        })),
        on_job_done: Some(Arc::new(move |_| {
            let app = done_app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = app.state::<AppState>().rebuild_projects().await;
                commands::machines_changed(&app);
            });
        })),
        client_from: Some(Arc::new(|ext| {
            ext.get::<axum::http::request::Parts>()
                .and_then(|p| p.extensions.get::<ClientName>())
                .map(|c| c.0.clone())
        })),
    }
}

/// Compares in constant time, so a wrong token can't be guessed byte by byte from timings.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Maps a bearer token to its client's name.
pub type TokenLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Rejects requests without a known token and tags the rest with the client's name.
async fn auth(State(lookup): State<TokenLookup>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    match (!token.is_empty()).then(|| lookup(token)).flatten() {
        Some(name) => {
            req.extensions_mut().insert(ClientName(name));
            next.run(req).await
        }
        None => (
            StatusCode::UNAUTHORIZED,
            "Missing or unknown token. Copy it from Boxdeck → Settings → MCP.",
        )
            .into_response(),
    }
}

/// Starts, stops or restarts the endpoint to match the settings.
pub async fn reconcile(app: &AppHandle) {
    let settings = app.state::<AppState>().settings();
    let want = settings.mcp.enabled.then_some(settings.mcp.port);
    let have = lock(&RUNNING).as_ref().map(|r| r.port);
    if want == have {
        return;
    }
    if let Some(running) = lock(&RUNNING).take() {
        (running.stop)();
    }
    *lock(&LAST_ERROR) = None;
    let Some(port) = want else { return };
    match start(app, port).await {
        Ok(running) => *lock(&RUNNING) = Some(running),
        // No silent fallback: a different port would break every configured client.
        Err(e) => *lock(&LAST_ERROR) = Some(e),
    }
}

/// The endpoint: loopback-only Host and Origin checks from rmcp (against DNS rebinding),
/// bearer-token auth in front. A missing `Origin` (normal for non-browser clients) passes.
pub fn router(
    make_ctx: impl Fn() -> Ctx + Send + Sync + 'static,
    lookup: TokenLookup,
) -> (axum::Router, impl FnOnce() + Send + Clone + 'static) {
    let config = StreamableHttpServerConfig::default()
        .with_allowed_hosts(["localhost", "127.0.0.1", "::1"])
        .with_allowed_origins(["http://localhost:*", "http://127.0.0.1:*"]);
    let cancel = config.cancellation_token.clone();
    let service = StreamableHttpService::new(
        move || Ok(BoxdeckServer::new(Arc::new(make_ctx()))),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn_with_state(lookup, auth));
    (router, move || cancel.cancel())
}

async fn start(app: &AppHandle, port: u16) -> Result<Running, String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("can't listen on 127.0.0.1:{port}: {e}"))?;
    let ctx_app = app.clone();
    let lookup_app = app.clone();
    let lookup: TokenLookup = Arc::new(move |token| {
        lookup_app
            .state::<AppState>()
            .settings()
            .mcp
            .clients
            .into_iter()
            .find(|c| same(c.token.as_bytes(), token.as_bytes()))
            .map(|c| c.name)
    });
    let (router, stop) = router(move || ctx(&ctx_app), lookup);
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.await;
            })
            .await;
    });
    Ok(Running {
        port,
        stop: Box::new(move || {
            stop();
            let _ = tx.send(());
        }),
    })
}

pub fn stdio_path() -> String {
    let name = format!("boxdeck-mcp{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&name)))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or(name)
}

pub fn status(app: &AppHandle) -> McpStatus {
    let settings = app.state::<AppState>().settings();
    let port = lock(&RUNNING).as_ref().map(|r| r.port);
    McpStatus {
        running: port.is_some(),
        port: port.or(settings.mcp.enabled.then_some(settings.mcp.port)),
        error: lock(&LAST_ERROR).clone(),
        url: port.map(|p| format!("http://127.0.0.1:{p}/mcp")),
        stdio_path: stdio_path(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxdeck_core::config::Settings;
    use boxdeck_core::jobs::{Jobs, JobsConfig};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn test_ctx() -> Ctx {
        let mut s = Settings::default();
        s.mcp.enabled = true;
        Ctx {
            vagrant: Err("no vagrant in tests".into()),
            jobs: Jobs::new(JobsConfig::default()),
            status: None,
            settings: Arc::new(move || s.clone()),
            index_path: None,
            vagrant_home: None,
            transport: Transport::Http,
            audit: None,
            approve: None,
            on_job_done: None,
            client_from: Some(Arc::new(|ext| {
                ext.get::<axum::http::request::Parts>()
                    .and_then(|p| p.extensions.get::<ClientName>())
                    .map(|c| c.0.clone())
            })),
        }
    }

    async fn spawn() -> u16 {
        let lookup: TokenLookup =
            Arc::new(|t| (t == "good-token").then(|| "Test client".to_owned()));
        let (router, _stop) = router(test_ctx, lookup);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        port
    }

    /// One raw HTTP/1.1 POST; returns the status code and body.
    async fn post(port: u16, headers: &[(&str, String)], body: &str) -> (u16, String) {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let mut req = "POST /mcp HTTP/1.1\r\n".to_owned();
        for (k, v) in headers {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str(&format!(
            "Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).await.unwrap();
        let code = out[9..12].parse().unwrap();
        (code, out)
    }

    const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;

    #[tokio::test]
    async fn needs_a_known_token() {
        let port = spawn().await;
        let host = ("Host", format!("127.0.0.1:{port}"));
        let (code, _) = post(port, std::slice::from_ref(&host), INIT).await;
        assert_eq!(code, 401);
        let bad = ("Authorization", "Bearer nope".to_owned());
        let (code, _) = post(port, &[host.clone(), bad], INIT).await;
        assert_eq!(code, 401);
        let good = ("Authorization", "Bearer good-token".to_owned());
        let (code, body) = post(port, &[host, good], INIT).await;
        assert_eq!(code, 200, "{body}");
        assert!(body.contains("\"boxdeck\""), "{body}");
    }

    #[tokio::test]
    async fn rejects_foreign_hosts_and_origins_but_not_missing_origin() {
        let port = spawn().await;
        let good = ("Authorization", "Bearer good-token".to_owned());
        let (code, _) = post(port, &[("Host", "evil.example".into()), good.clone()], INIT).await;
        assert_eq!(code, 403, "DNS-rebinding Host must be refused");
        let host = ("Host", format!("127.0.0.1:{port}"));
        let evil = ("Origin", "https://evil.example".to_owned());
        let (code, _) = post(port, &[host.clone(), good.clone(), evil], INIT).await;
        assert_eq!(code, 403, "foreign Origin must be refused");
        let local = ("Origin", format!("http://localhost:{port}"));
        let (code, _) = post(port, &[host, good, local], INIT).await;
        assert_eq!(code, 200);
    }

    #[test]
    fn token_comparison() {
        assert!(same(b"abc", b"abc"));
        assert!(!same(b"abc", b"abd"));
        assert!(!same(b"abc", b"abcd"));
        assert!(!same(b"", b"a"));
    }
}
