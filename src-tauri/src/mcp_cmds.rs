//! Settings → MCP: endpoint status, client tokens and the agent audit log.

use boxdeck_core::audit::{AuditEntry, AuditLog};
use boxdeck_core::config::McpClient;
use tauri::{AppHandle, Manager};

use crate::commands::CmdResult;
use crate::mcp_http::{self, McpStatus};
use crate::state::AppState;

#[tauri::command]
pub fn mcp_status(app: AppHandle) -> McpStatus {
    mcp_http::status(&app)
}

/// A random 256-bit token, hex-encoded.
fn new_token() -> String {
    rand::random::<[u8; 32]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Adds a named client and returns its token (shown so the user can copy it).
#[tauri::command]
pub fn mcp_add_client(app: AppHandle, name: String) -> CmdResult<String> {
    let name = name.trim().to_owned();
    let token = new_token();
    app.state::<AppState>().update_settings(|s| {
        if !s.mcp.clients.iter().any(|c| c.name == name) {
            s.mcp.clients.push(McpClient {
                name: name.clone(),
                token: token.clone(),
                created_at: boxdeck_core::jobs::now_ms(),
            });
        }
    })?;
    mcp_client_token(app, name)
}

/// Replaces a client's token; the old one stops working at once.
#[tauri::command]
pub fn mcp_rotate_client(app: AppHandle, name: String) -> CmdResult<String> {
    let token = new_token();
    app.state::<AppState>().update_settings(|s| {
        if let Some(c) = s.mcp.clients.iter_mut().find(|c| c.name == name) {
            c.token = token.clone();
            c.created_at = boxdeck_core::jobs::now_ms();
        }
    })?;
    Ok(token)
}

#[tauri::command]
pub fn mcp_remove_client(app: AppHandle, name: String) -> CmdResult<()> {
    app.state::<AppState>()
        .update_settings(|s| s.mcp.clients.retain(|c| c.name != name))?;
    Ok(())
}

/// The token, for the Copy button. Settings sent to the UI otherwise have tokens blanked.
#[tauri::command]
pub fn mcp_client_token(app: AppHandle, name: String) -> CmdResult<String> {
    app.state::<AppState>()
        .settings()
        .mcp
        .clients
        .into_iter()
        .find(|c| c.name == name)
        .map(|c| c.token)
        .ok_or_else(|| format!("no client named {name:?}"))
}

/// Recent agent calls from both the app and `boxdeck-mcp`, newest first.
#[tauri::command]
pub fn list_audit(limit: Option<usize>) -> CmdResult<Vec<AuditEntry>> {
    let log = AuditLog::default_log().map_err(|e| e.to_string())?;
    let mut entries = log
        .tail(limit.unwrap_or(200).min(2000))
        .map_err(|e| e.to_string())?;
    entries.reverse();
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_and_unique() {
        let a = new_token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, new_token());
    }
}
