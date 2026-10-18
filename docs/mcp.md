# Boxdeck for AI agents (MCP)

Boxdeck includes a [Model Context Protocol](https://modelcontextprotocol.io) server, so AI agents and coding assistants can list, start, stop and inspect your Vagrant machines. Agents use the same code paths as the app, and you decide what they may do.

Agent access is **off** until you turn it on in **Settings → MCP / Agents**.

## Two ways to connect

|                       | HTTP (recommended)                                                          | stdio                                               |
| --------------------- | --------------------------------------------------------------------------- | --------------------------------------------------- |
| How                   | `http://127.0.0.1:47816/mcp` with an `Authorization: Bearer <token>` header | The client starts the bundled `boxdeck-mcp` program |
| Needs the app running | Yes                                                                         | No                                                  |
| Destructive tools     | Asked in a Boxdeck window each time                                         | Refused, unless you allow them for headless mode    |
| Shows up in Activity  | Live                                                                        | Afterwards, from the shared audit log               |

Each client gets its **own token** (Settings → MCP / Agents → Clients → Add client), so you can rotate or revoke one without affecting the others. Settings shows ready-to-copy snippets, including a `claude mcp add` command for Claude Code.

```json
{
  "mcpServers": {
    "boxdeck": {
      "type": "http",
      "url": "http://127.0.0.1:47816/mcp",
      "headers": { "Authorization": "Bearer <token>" }
    }
  }
}
```

For stdio, point the client at `boxdeck-mcp` inside the app bundle (Settings shows the exact path). It reads the same settings as the app.

## Access levels

| Level              | Tools                                                                                                                                                                                    |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Read** (default) | `list_machines`, `get_machine_status`, `get_machine_details`, `list_boxes`, `check_box_updates`, `read_vagrantfile`, `validate_vagrantfile`, `get_job`, `list_jobs`                      |
| **Operate**        | Read, plus `machine_up`, `machine_halt`, `machine_suspend`, `machine_resume`, `machine_reload`, `machine_provision`, `box_add`, `box_update`, `cancel_job` (only jobs the agent started) |
| **Full**           | Operate, plus `machine_destroy`, `box_remove`, `box_prune`, `write_vagrantfile`, each approved by you; and `ssh_exec` if you turn it on separately                                       |

Every tool carries MCP annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`) so clients can show and gate them.

Long actions such as `vagrant up` run as jobs. Action tools wait up to `wait_seconds` (default 60, max 600) and then return the result, or a job id to poll with `get_job`. Clients that send a progress token get Vagrant's output as progress notifications.

## Approvals

When an agent calls a destructive tool over HTTP, Boxdeck opens a small window naming the agent, the tool, the project, the machine and the exact arguments. **Deny** is the default button, and no answer within two minutes counts as a denial.

## Projects

Every project follows the global setting (all projects, or only the ones you allow) unless you set it to **Allow** or **Deny** under Settings → Projects. Agents can't see denied projects at all.

## Pausing and auditing

- **Pause Agent Access** in the tray menu refuses every call at once, in both the app and a running `boxdeck-mcp`.
- Every call, allowed or not, is appended to `audit.jsonl` in Boxdeck's data folder with the time, client, tool, arguments, decision and result. Activity → Agent calls shows it.

## Security notes

- The HTTP server listens on `127.0.0.1` only, checks the `Host` header against loopback names (against DNS-rebinding attacks from web pages), rejects browser `Origin`s other than localhost, and requires a token on every request.
- There is no generic "run a vagrant command" tool. Inputs are checked against known projects, known machine names and allowlisted values before Vagrant runs.
- `get_machine_details` returns SSH host, port and user only. Private keys are never read.
- **Tool output is untrusted.** VM logs, Vagrantfile contents and `ssh_exec` output can contain text written to steer an agent. Boxdeck can't control what an agent does with it, so it limits the damage instead: access levels, approvals, per-project deny and the pause switch. Keep `ssh_exec` off unless you need it.
