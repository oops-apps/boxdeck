# Changelog

All notable changes to Boxdeck are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Agents can work inside VMs: `ssh_exec` runs a command (optionally in a folder) and returns its output, or a job to follow when it runs long; `vm_upload` and `vm_download` copy files between the VM and the project folder. Turn it on under Settings → MCP / Agents → Shell access in VMs; it needs Operate access.
- Shell access asks once per VM whose synced folders can change files on your computer, with **Allow for this session**. Each project can override the mode, and session grants can be revoked in Settings.
- One-click **Connect** for Claude Code, Claude Desktop, Cursor, Gemini CLI and Codex. Boxdeck adds only its own entry to the client's config, keeps everything else, and offers **Reconnect** when the app has moved.

### Changed

- Agents connect over stdio only, with no token. Approvals reach the app through Boxdeck's data folder, so nothing listens on a network port.
- Settings migrate automatically: if `ssh_exec` was on, shell access becomes Ask once.
- New switch to allow shell access while the Boxdeck app isn't running, separate from the one for destructive tools.

### Removed

- The in-app HTTP endpoint, its port setting and per-client tokens. Clients set up with the HTTP URL stop connecting: use Connect in Settings → MCP / Agents, then restart the client.

## [0.1.0-beta.4] - 2026-10-19

### Changed

- Betas are published as regular releases, so the in-app updater finds them. Updating from 0.1.0-beta.3 starts with this release.
- Releases no longer include separate `boxdeck-mcp` downloads. Use the copy inside the app; Settings → MCP / Agents shows its path.

## [0.1.0-beta.3] - 2026-10-19

### Added

- In-app updates: Boxdeck checks GitHub Releases at startup (can be turned off), shows the release notes, and installs a signed update and restarts from Settings → Backup & about or the tray.
- Job output in color: Vagrant's `==>` steps, details, forwarded ports and errors are easy to tell apart.

### Changed

- The app's look follows the website: state dots (a lit green dot for running), Up and Resume as the main action, a red stop icon on Halt, a tinted selected row, the sweeping progress bar while a job runs, and the website's dark palette.
- The sidebar uses a monitor icon for Machines and the box icon for Boxes.
- The macOS menu-bar icon is now the app icon's cube, with its top face lit while machines run.
- Drag the top edge of the log and terminal panel to resize it; Boxdeck remembers the size.

## [0.1.0-beta.2] - 2026-10-19

### Changed

- macOS builds are ad-hoc signed, so macOS asks you to confirm the first launch instead of reporting the app as damaged.

## [0.1.0-beta.1] - 2026-10-19

### Added

- Machine list from Vagrant's index with live status per project: up, halt, suspend, resume, reload, provision, destroy, and SSH in a terminal tab.
- Live job output, queueing per machine and project, and cancel that lets Vagrant clean up first.
- Tray / menu bar with per-machine actions, hide-to-tray, launch at login and notifications.
- Box management: sizes, machines using each box, update checks, add, update, remove and prune with a dry run.
- Settings: Vagrant path, environment variables, project folders, names, groups, colors, `vagrant up` defaults, and a Vagrantfile editor with validation and backups.
- MCP server for AI agents over local HTTP or stdio, with access levels, per-project access, per-client tokens, approvals for destructive actions, a pause switch and an audit log.
