# Boxdeck for AI agents (MCP)

Boxdeck includes a [Model Context Protocol](https://modelcontextprotocol.io) server, so AI agents and coding assistants can list, start, stop and inspect your Vagrant machines, and, if you allow it, run commands and move files inside them. Agents use the same code paths as the app, and you decide what they may do.

Agent access is **off** until you turn it on in **Settings → MCP / Agents**.

## Connecting a client

Under **Settings → MCP / Agents → Connect a client**, click **Connect** next to your client:

| Client         | Config file Boxdeck edits                                                                                                                  |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Claude Code    | `~/.claude.json`                                                                                                                           |
| Claude Desktop | `claude_desktop_config.json` in `~/Library/Application Support/Claude` (macOS), `%APPDATA%\Claude` (Windows) or `~/.config/Claude` (Linux) |
| Cursor         | `~/.cursor/mcp.json`                                                                                                                       |
| Gemini CLI     | `~/.gemini/settings.json`                                                                                                                  |
| Codex          | `~/.codex/config.toml` (or `$CODEX_HOME/config.toml`)                                                                                      |

Connect adds one `boxdeck` entry that starts the `boxdeck-mcp` program bundled with the app. It keeps everything else in the file, including key order and comments, and **Disconnect** removes only that entry. Restart the client afterwards. If you move or update Boxdeck and the stored path goes stale, the row shows **Reconnect**.

There is no token and no network port. The client starts `boxdeck-mcp` itself over stdio, and `boxdeck-mcp` reads the same settings as the app. For any other client, copy the snippet from the same page:

```json
{
  "mcpServers": {
    "boxdeck": { "command": "/Applications/Boxdeck.app/Contents/MacOS/boxdeck-mcp", "args": [] }
  }
}
```

## Access levels

| Level              | Tools                                                                                                                                                                                    |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Read** (default) | `list_machines`, `get_machine_status`, `get_machine_details`, `list_boxes`, `check_box_updates`, `read_vagrantfile`, `validate_vagrantfile`, `get_job`, `list_jobs`                      |
| **Operate**        | Read, plus `machine_up`, `machine_halt`, `machine_suspend`, `machine_resume`, `machine_reload`, `machine_provision`, `box_add`, `box_update`, `cancel_job` (only jobs the agent started) |
| **Full**           | Operate, plus `machine_destroy`, `box_remove`, `box_prune`, `write_vagrantfile`, each approved by you                                                                                    |

Every tool carries MCP annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`) so clients can show and gate them.

Long actions such as `vagrant up` run as jobs. Action tools wait up to `wait_seconds` (default 60, max 600) and then return the result, or a job id to poll with `get_job`. Clients that send a progress token get Vagrant's output as progress notifications.

## Shell access inside VMs

With **Shell access in VMs** turned on (it needs Operate), agents get three more tools:

- `ssh_exec` runs a shell command in a running VM through `vagrant ssh -c`, optionally in a `cwd`. It returns stdout, stderr and the exit code. A command still running after `wait_seconds` becomes a job to poll with `get_job`; `timeout_seconds` (default 600, max 3600) stops it. Each stream returns at most its last 200 KB, with `truncated` set when output was cut.
- `vm_upload` copies a file or folder from the project folder into the VM (`vagrant upload`).
- `vm_download` returns a text file from the VM (up to 1 MB), or saves any file under the project folder with `save_to` (up to 50 MB).

Agents get full control inside the VM. On the host they can only reach the project folder: upload sources and download destinations must stay inside it, symlinks included.

| Mode         | What happens                                                                                                                                                                                                                                  |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Off**      | Shell tools are refused (default).                                                                                                                                                                                                            |
| **Ask once** | A VM with a two-way synced folder can change files on your computer. The first shell call to such a VM asks you; **Allow for this session** trusts that VM until Boxdeck restarts. VMs with no synced folders, or only rsync ones, don't ask. |
| **Always**   | No questions.                                                                                                                                                                                                                                 |

Each project can override the mode under **Settings → Projects**. Machines you allowed for the session are listed under the setting, each with **Revoke**. Boxdeck reads which folders are synced from Vagrant's own record in `.vagrant/machines/<name>/<provider>/synced_folders`. When there's no record yet, it asks.

## Approvals

Boxdeck opens a small window naming the agent, the tool, the project, the machine and the exact arguments:

- for every destructive tool (Full level)
- for the first shell call to a VM that can change host files (Ask once)

**Deny** is the default button, and no answer within two minutes counts as a denial.

`boxdeck-mcp` sends these requests to the app through files in Boxdeck's data folder, which only your user account can read. **If the app isn't running, nobody can answer**, so those calls are refused with a message telling the agent to open Boxdeck. Two switches under **When Boxdeck isn't running** change that, both off by default:

- **Allow shell access without asking**: Ask once lets calls through instead.
- **Allow destructive tools without approval**: agents can then destroy machines with nobody asked.

## Projects

Every project follows the global setting (all projects, or only the ones you allow) unless you set it to **Allow** or **Deny** under Settings → Projects. Agents can't see denied projects at all.

## Pausing and auditing

- **Pause Agent Access** in the tray menu refuses every call at once in every running `boxdeck-mcp`.
- Every call, allowed or not, is appended to `audit.jsonl` in Boxdeck's data folder with the time, client, tool, arguments, decision and result. Activity → Agent calls shows it.

## Security notes

- Boxdeck opens no network port for agents. Only programs running as you can start `boxdeck-mcp` or write to the data folder, and those programs could already run `vagrant` directly.
- There is no generic "run a vagrant command" tool. Inputs are checked against known projects, known machine names and allowlisted values before Vagrant runs. `ssh_exec`'s command is the only free-form input.
- `get_machine_details` returns SSH host, port and user only. Private keys are never read.
- **Tool output is untrusted.** VM logs, Vagrantfile contents and `ssh_exec` output can contain text written to steer an agent. Boxdeck can't control what an agent does with it, so it limits the damage instead: access levels, shell mode, approvals, per-project deny and the pause switch.
