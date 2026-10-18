# Contributing to Boxdeck

Thanks for your interest in Boxdeck. Bug reports, ideas and pull requests are all welcome.

## Before you start

- For anything bigger than a small fix, open an issue first so we can agree on the approach.
- By contributing, you agree that your work is licensed under the [MIT License](LICENSE).
- Everyone taking part is expected to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Development setup

Follow [Development](README.md#development) in the README; `make help` lists the shortcuts. The repository is a Cargo workspace:

| Path                   | What it is                                                         |
| ---------------------- | ------------------------------------------------------------------ |
| `crates/boxdeck-core/` | Vagrant logic, jobs, settings and MCP policy. No Tauri dependency. |
| `crates/boxdeck-mcp/`  | The MCP server (library plus the `boxdeck-mcp` stdio binary).      |
| `src-tauri/`           | The Tauri app: IPC commands, tray, approvals. Kept thin.           |
| `src/`                 | The React + TypeScript UI.                                         |
| `tests/fake-vagrant/`  | A fake `vagrant` binary used by integration tests.                 |
| `scripts/`             | Fixture capture and the sidecar build.                             |

## Testing

- **Unit and integration tests** run without Vagrant: `make test`. The job runner and MCP tests use `tests/fake-vagrant`, a scriptable stand-in for `vagrant`.
- **Parsers are tested against real Vagrant output**, recorded by `scripts/capture-fixtures.sh` into `crates/boxdeck-core/tests/fixtures/captured/<version>-<os>/`. Never hand-write output and present it as captured; hand-written fixtures go in `tests/fixtures/synthetic/` with a note on their source.

  ```bash
  ./scripts/capture-fixtures.sh                  # offline captures (fake boxes, no VMs)
  ./scripts/capture-fixtures.sh --with-docker    # plus a Docker-backed machine
  ./scripts/capture-fixtures.sh --with-vmware    # plus a small VMware VM (needs the VMware Utility)
  ```

  The script scrubs your home folder, user name and host name from the output. Check the files before committing anyway.

- **Live tests** against a real Vagrant are marked `#[ignore]`. Run them with `cargo test -p boxdeck-core --test live_vagrant -- --ignored`.
- **Throwaway projects** for manual testing go in `.testbed/` at the repository root. It's gitignored, and the capture script works there too. Destroy any VMs you create when you're done.
- **UI changes** can be checked in a plain browser with `npm run dev`, which uses a built-in mock backend instead of Tauri.

## Making a change

1. Create a branch from `main`.
2. Keep the change focused; unrelated cleanups go in their own pull request.
3. Add or update tests. Parsers and validators are tested against fixtures in `crates/boxdeck-core/tests/fixtures/`.
4. Run the full check (`make check` does the same) and make sure it passes:

   ```bash
   npm run check
   ```

5. Write commit messages in the [Conventional Commits](https://www.conventionalcommits.org/) style, as a single line, for example `feat(core): parse box list output` or `fix(ui): keep log panel scrolled to the end`.
6. Open a pull request and fill in the template.

## Ground rules for the code

- Processes are spawned only through `boxdeck-core`'s runner, with an argument list and never a shell string.
- Boxdeck never writes Vagrant's machine index; all changes go through the `vagrant` CLI.
- Never run a Vagrant command on a project while a job runs there, not even `vagrant status`: Vagrant locks each machine, and the running action fails with `MachineLocked`. Status queries go through `Jobs::try_hold_project`.
- New dependencies need a reason in the pull request. Prefer well-maintained, permissively licensed crates and packages; `cargo deny check` enforces the license list.
