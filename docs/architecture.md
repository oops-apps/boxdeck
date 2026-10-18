# Architecture

Boxdeck is a Cargo workspace plus a React frontend.

```
React UI (src/)  ──invoke / Channel──►  Tauri app (src-tauri/)  ──►  boxdeck-core  ──►  vagrant CLI
                                          │  tray, approvals,           ▲
                                          │  MCP over HTTP ─────────────┤
MCP clients ──stdio──► boxdeck-mcp ───────────────────────────────────────┘
```

| Crate / folder        | Role                                                                                                                                                                                                                                                                                  |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/boxdeck-core` | Everything that knows about Vagrant: locating it, parsing `--machine-readable` output, reading the machine index (read-only, under a shared lock), the job runner (queues, per-machine locks, cancel), boxes, settings, the MCP access policy and the audit log. No Tauri dependency. |
| `crates/boxdeck-mcp`  | The MCP tools (via `rmcp`) on top of core, plus the `boxdeck-mcp` stdio binary.                                                                                                                                                                                                       |
| `src-tauri`           | The desktop app: IPC commands, tray, the in-app MCP HTTP endpoint, approval windows. Thin glue over core.                                                                                                                                                                             |
| `src`                 | React + TypeScript UI. Types in `src/ipc/generated/` are generated from Rust with `ts-rs` (`npm run gen:types`).                                                                                                                                                                      |
| `tests/fake-vagrant`  | A scriptable stand-in for `vagrant`, used by integration tests.                                                                                                                                                                                                                       |

## Key decisions

- **Vagrant is only ever run as a separate process**, with an argument list (never a shell string), its project folder as working directory and stdin closed. Actions get `--no-tty`; `--machine-readable` queries must not, because `--no-tty` overrides machine-readable output.
- **Machine names come from Vagrant's machine index**, not `global-status`, whose machine-readable output has no names.
- **Live state comes from one `vagrant status` per project**, with a concurrency limit and a cache, refreshed on demand, on focus, after jobs, when the index file changes, and optionally on an interval.
- **Cancel interrupts first** (Vagrant cleans up), then kills the whole process tree.
- **Settings are versioned JSON** written atomically by the app; `boxdeck-mcp` reads the same file and follows changes, so the pause switch works for both.
