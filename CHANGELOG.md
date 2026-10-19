# Changelog

All notable changes to Boxdeck are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0-beta.1] - 2026-10-19

### Added

- Machine list from Vagrant's index with live status per project: up, halt, suspend, resume, reload, provision, destroy, and SSH in a terminal tab.
- Live job output, queueing per machine and project, and cancel that lets Vagrant clean up first.
- Tray / menu bar with per-machine actions, hide-to-tray, launch at login and notifications.
- Box management: sizes, machines using each box, update checks, add, update, remove and prune with a dry run.
- Settings: Vagrant path, environment variables, project folders, names, groups, colors, `vagrant up` defaults, and a Vagrantfile editor with validation and backups.
- MCP server for AI agents over local HTTP or stdio, with access levels, per-project access, per-client tokens, approvals for destructive actions, a pause switch and an audit log.
