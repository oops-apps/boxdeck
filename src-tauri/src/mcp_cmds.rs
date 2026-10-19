//! Settings → MCP: Connect/Disconnect for MCP clients, session grants and the audit log.

use std::path::{Path, PathBuf};

use boxdeck_core::audit::{AuditEntry, AuditLog};
use boxdeck_core::bridge::{Bridge, Grant};
use boxdeck_core::clients::{self, ClientKind, ClientStatus, Homes};
use serde::Serialize;

use crate::commands::CmdResult;

/// The bundled `boxdeck-mcp`, next to the app's own binary.
fn bundled_server() -> PathBuf {
    let name = format!("boxdeck-mcp{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// The path clients should launch. An AppImage is mounted somewhere new on every launch, so it
/// registers a copy in the data dir instead.
fn server_path() -> PathBuf {
    let bundled = bundled_server();
    if std::env::var_os("APPIMAGE").is_none() {
        return bundled;
    }
    boxdeck_core::config::data_dir()
        .and_then(|d| clients::stable_copy(&bundled, &d.join("bin"), boxdeck_core::VERSION))
        .unwrap_or_else(|e| {
            tracing::warn!("could not copy boxdeck-mcp out of the AppImage: {e}");
            bundled
        })
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export)]
pub struct McpClients {
    /// What Connect registers; also shown for clients that aren't listed.
    pub server_path: String,
    pub clients: Vec<ClientStatus>,
}

fn homes() -> CmdResult<Homes> {
    Homes::detect().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mcp_clients() -> CmdResult<McpClients> {
    let server = server_path();
    Ok(McpClients {
        server_path: server.to_string_lossy().into_owned(),
        clients: clients::list(&homes()?, &server),
    })
}

#[tauri::command]
pub fn mcp_connect(kind: ClientKind) -> CmdResult<McpClients> {
    clients::connect(&homes()?, kind, &server_path()).map_err(|e| e.to_string())?;
    mcp_clients()
}

#[tauri::command]
pub fn mcp_disconnect(kind: ClientKind) -> CmdResult<McpClients> {
    clients::disconnect(&homes()?, kind).map_err(|e| e.to_string())?;
    mcp_clients()
}

fn bridge() -> CmdResult<Bridge> {
    Bridge::default_bridge().map_err(|e| e.to_string())
}

/// Machines the user allowed shell access to for this session.
#[tauri::command]
pub fn mcp_grants() -> CmdResult<Vec<Grant>> {
    Ok(bridge()?.grants())
}

#[tauri::command]
pub fn mcp_revoke_grant(project: String, machine: String) -> CmdResult<Vec<Grant>> {
    let bridge = bridge()?;
    bridge
        .revoke(Path::new(&project), &machine)
        .map_err(|e| e.to_string())?;
    Ok(bridge.grants())
}

/// Recent agent calls, newest first.
#[tauri::command]
pub fn list_audit(limit: Option<usize>) -> CmdResult<Vec<AuditEntry>> {
    let log = AuditLog::default_log().map_err(|e| e.to_string())?;
    let mut entries = log
        .tail(limit.unwrap_or(200).min(2000))
        .map_err(|e| e.to_string())?;
    entries.reverse();
    Ok(entries)
}
