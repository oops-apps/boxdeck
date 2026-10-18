# Boxdeck

Boxdeck is an open-source (MIT) desktop GUI for HashiCorp Vagrant, built with Tauri 2, Rust and React + TypeScript. It also includes an MCP server so AI agents can manage Vagrant machines under user-controlled permissions.

The full design is in `docs/plan.md`. It's long, so it is not imported here: read only the sections the current task needs (the section numbers are below).

## Project decisions

Fill these in before M0; they are fixed after.

- GitHub owner: `oops-apps` → bundle id `io.github.oops-apps.boxdeck`
- Minimum Vagrant: 2.3 (feature-detect 2.4 for `box-architecture`)
- Priority providers for QA: `vmware_desktop` (VMware Fusion on macOS)
- Current milestone: **v0.1.0 release (needs maintainer)** (update this line when a milestone is merged)

## Where things are

- `crates/boxdeck-core/`: all Vagrant logic, jobs, config, MCP policy, audit. **No Tauri dependency, ever.** The headless `boxdeck-mcp` binary links it.
- `crates/boxdeck-mcp/`: `rmcp` tool router (lib) plus the stdio server (bin).
- `src-tauri/`: the Tauri app: IPC commands, tray, in-app MCP HTTP endpoint, approval dialogs. Keep it thin; logic belongs in core.
- `src/`: React UI. Types in `src/ipc/generated/` come from `ts-rs`; never edit them by hand.
- `tests/fake-vagrant/`: scriptable fake `vagrant` (a Rust bin crate in the workspace) used by integration and e2e tests.
- `scripts/`: `capture-fixtures` (records real Vagrant output) and `build-sidecar` (builds `boxdeck-mcp` into `src-tauri/binaries/`).
- `crates/boxdeck-core/tests/fixtures/`: real captured Vagrant outputs. Add a fixture for every parser case.
- `.testbed/` (gitignored): throwaway Vagrant projects and VMs for local testing. Create test projects here, not in `/tmp`, and destroy their VMs when done.

## Commands

Keep this list in sync with `package.json` and the `Makefile` (`make help` lists shortcuts: `dev`, `build`, `build-debug`, `lint`, `fmt`, `test`, `check`, `types`, `sidecar`, `mcp`, `clean`).

- `npm install`: install frontend deps
- `npm run tauri dev`: run the app
- `npm run check`: **the full gate**. Runs everything below. Run it before saying a task is done.
  - `npm run check:rs`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check`
  - `npm run format:check && npm run lint && npm run typecheck && npm test`
- `npm run format`: apply Prettier; `cargo fmt --all` for Rust
- `cargo test -p boxdeck-core <name>`: run one Rust test; prefer this while iterating
- `npm run gen:types`: regenerate TS types from Rust (`ts-rs`); added in M2

## Dependencies

- IMPORTANT: use only the libraries listed in `docs/plan.md` §3.5. Before adding any other dependency, or changing a major version, stop and ask me, with the reason and the alternatives you considered.
- Stable releases only. No `-rc`, `-beta` or `-alpha` versions, and no git dependencies except `fix-path-env`, pinned to a commit `rev`.
- Don't install tools or packages on the host (cargo install, global npm, brew). If something is missing, stop and tell me what to install.
- Prefer an existing library over custom code. Only the items in plan §3.6 are custom by design. If you're about to write something non-trivial that isn't on that list, check for a maintained crate or package first and tell me what you found.
- Don't use `tauri-plugin-shell`, `tauri-plugin-fs` or `tauri-plugin-store`. Settings live in `boxdeck-core::config`.

## Non-negotiable rules

- **Spawning processes:** always go through `boxdeck-core`'s runner (`process-wrap`): argv only, never a shell string, `cwd` = project dir, stdin null. Action jobs add `--no-tty`. Two exceptions: `--machine-readable` queries must **not** get `--no-tty` (it overrides machine-readable output, plan §3.1), and PTY sessions (`vagrant ssh` in the terminal tab) get a real terminal and no `--no-tty`.
- **Project env:** pass the index entry's `VAGRANT_VAGRANTFILE` and `VAGRANT_DOTFILE_PATH` when they differ from the defaults (plan §4.1).
- **Cancel:** interrupt first, kill the process tree only after the grace period or on force (plan §4.2).
- **Destroying:** `destroy` and `box remove` run with `-f` only after our own confirmation or policy check.
- **Machine index:** `$VAGRANT_HOME/data/machine-index/index` is **read-only** for us. Read it under a shared lock. Never write it; change state only through the `vagrant` CLI.
- **Machine names:** don't rely on `global-status --machine-readable` for them; its `target` field is always empty (a Vagrant bug, plan §3.1).
- **Validation:** every IPC command and every MCP tool validates input with the shared validators in core: known project dirs, known machine names, allowlisted box names and flags.
- **Tray:** every tray action must be reachable from the menu. Linux emits no tray click events.
- **Streaming:** use `tauri::ipc::Channel` for job output to the UI. Use events only for low-rate "changed" notifications.
- **Logging:** never log or return SSH private-key contents or MCP client tokens.
- **PATH:** call `fix_path_env::fix()` first in `main()` of both the app and `boxdeck-mcp`.

## Testing and verification

- Write parser, validator and policy code test first, against fixtures.
- Never invent Vagrant output and present it as captured. Hand-written fixtures go in `tests/fixtures/synthetic/` with a comment naming their source; real ones come from `scripts/capture-fixtures`, which I run.
- Job runner and MCP behavior get integration tests against `fake-vagrant`, never real VMs in CI.
- A task is done only when `npm run check` passes. Show the command output as evidence; don't just say it passes.
- For UI changes, run `npm run dev` in the browser pane with IPC mocked (`@tauri-apps/api/mocks`), check the change, and say what you checked. I do the native-window check in `npm run tauri dev`.
- Fix root causes. Don't silence clippy, skip tests or add `#[allow]` without asking.

## Workflow

- Work one milestone at a time (plan §8). Start each with a plan for my approval: which files, in what order, and how each part will be verified.
- Small commits, each message a single line in Conventional Commits form (`feat(core): …`, `fix(mcp): …`, `test: …`, `chore: …`).
- One branch per milestone: `m<N>-<slug>`, e.g. `m1a-vagrant-read`. Don't push or open PRs unless I ask.
- Out-of-scope work goes in `docs/followups.md`, not into the current branch.
- If the plan is wrong or unclear, say so and propose a change to `docs/plan.md` instead of silently diverging.
- When compacting, keep the current milestone, the list of modified files, open TODOs and the exact test commands.
