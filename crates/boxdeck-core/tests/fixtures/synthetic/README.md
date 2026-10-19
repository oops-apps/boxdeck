# Synthetic fixtures

Hand-written stand-ins for output we can't capture yet. Each one names its source and gets
replaced by a real capture (`scripts/capture-fixtures.sh`) when one is available.

| File | Based on |
|---|---|
| `box-list-2.3.stdout` | `plugins/commands/box/command/list.rb` before 2.4, which emitted `box-name`, `box-provider` and `box-version` but no `box-architecture` |
| `synced_folders.json` | `.vagrant/machines/<name>/<provider>/synced_folders` as written by `save_synced_folders` in `lib/vagrant/action/builtin/mixin_synced_folders.rb`: a JSON object keyed by synced-folder type, then by folder id, with `hostpath`, `guestpath` and `disabled` |
