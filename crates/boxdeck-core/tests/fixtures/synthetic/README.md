# Synthetic fixtures

Hand-written stand-ins for output we can't capture yet. Each one names its source and gets
replaced by a real capture (`scripts/capture-fixtures.sh`) when one is available.

| File | Based on |
|---|---|
| `box-list-2.3.stdout` | `plugins/commands/box/command/list.rb` before 2.4, which emitted `box-name`, `box-provider` and `box-version` but no `box-architecture` |
