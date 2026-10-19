# Changelog

All notable changes to Boxdeck are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
