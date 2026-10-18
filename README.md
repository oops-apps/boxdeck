# Boxdeck

Boxdeck is an open-source desktop app for [HashiCorp Vagrant](https://developer.hashicorp.com/vagrant) on macOS, Windows and Linux. It finds every Vagrant machine on your computer and lets you start, stop, provision and SSH into them from a window or the system tray, manage your boxes, and edit Vagrantfiles, without the terminal. It also includes an MCP server, so AI agents can work with your machines under permissions you control.

> **Status:** early development; the first release (v0.1.0) isn't out yet. Until then, [build it from source](#development).

Boxdeck is not affiliated with or endorsed by HashiCorp. It runs the `vagrant` you already have installed and ships none of Vagrant's code.

## Features

- **Every machine in one place**: grouped by project, with live state, search and filters. Project folders you add stay listed even before the first `vagrant up`.
- **Actions without a terminal**: up, halt, suspend, resume, reload, provision and destroy (with confirmation), for one machine, a selection or a whole project.
- **SSH in a tab**: a real terminal inside the app.
- **Live output**: watch each command as it runs, cancel safely (Vagrant gets to clean up first), and re-run a command in a terminal when it needs a password.
- **Tray or menu bar**: per-machine actions, "halt all", launch at login, and notifications when jobs finish.
- **Boxes**: size on disk, which machines use each box, update checks, add, update, remove and prune (with a dry run first).
- **Projects your way**: names, groups, color tags, pinning and hiding; per-project environment variables and `vagrant up` defaults.
- **Vagrantfile editor**: Ruby highlighting, a backup on every save, conflict detection, and `vagrant validate` built in.
- **AI agents (MCP)**: off by default. Read, Operate or Full access, per-project rules, a token per client, approval for destructive actions, a pause switch and an audit log. See [docs/mcp.md](docs/mcp.md).

Works with any Vagrant provider; tested with VMware Desktop (VMware Fusion) and Docker. Needs Vagrant 2.3 or newer.

## Getting started

1. Install Vagrant and a provider; for VMware that also means the separate Vagrant VMware Utility. **[Step-by-step install guide](docs/install.md)**.
2. Install Boxdeck from [Releases](https://github.com/oops-apps/boxdeck/releases) once v0.1.0 is out, or [build it](#development).
3. Open Boxdeck. Every machine Vagrant knows about appears; add other project folders with **Add project…**.

## Documentation

| Guide                                      | What's in it                                                       |
| ------------------------------------------ | ------------------------------------------------------------------ |
| [Install](docs/install.md)                 | Vagrant, each provider, and Boxdeck itself                         |
| [AI agents (MCP)](docs/mcp.md)             | Connecting a client, access levels, approvals, security            |
| [Troubleshooting](docs/troubleshooting.md) | Vagrant not found, provider errors, tray on Linux, unsigned builds |
| [Architecture](docs/architecture.md)       | How the pieces fit together, for contributors                      |
| [Releasing](RELEASING.md)                  | How maintainers cut a release                                      |
| [Changelog](CHANGELOG.md)                  | What changed in each version                                       |

## Development

You need Rust (stable), Node.js 22+, the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS, and [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) (`cargo install cargo-deny`). For manual testing, also Vagrant and a provider.

```bash
make install   # npm install
make dev       # run the app with hot reload
```

| Command            | Does                                                      | Same as                                      |
| ------------------ | --------------------------------------------------------- | -------------------------------------------- |
| `make dev`         | Run the app with hot reload; builds the MCP sidecar first | `npm run tauri dev`                          |
| `make build`       | Release bundles for this platform                         | `npm run tauri build`                        |
| `make build-debug` | A faster debug build, no installers                       | `npm run tauri build -- --debug --no-bundle` |
| `make lint`        | rustfmt, clippy, Prettier, ESLint and TypeScript checks   |                                              |
| `make fmt`         | Format Rust and frontend code                             | `cargo fmt --all && npm run format`          |
| `make test`        | Rust and frontend tests                                   | `cargo test --workspace && npm test`         |
| `make check`       | Everything CI runs: lint, tests and cargo-deny            | `npm run check`                              |
| `make types`       | Regenerate TypeScript types from Rust                     | `npm run gen:types`                          |
| `make mcp`         | Run `boxdeck-mcp` over stdio                              | `cargo run -p boxdeck-mcp`                   |
| `make clean`       | Remove build output                                       |                                              |

`make help` lists them all. See [CONTRIBUTING.md](CONTRIBUTING.md) for testing against real Vagrant.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). Please report security issues privately as described in [SECURITY.md](SECURITY.md), not in public issues.

## License

[MIT](LICENSE)
