import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { useSettings } from "../hooks";
import { api } from "../ipc/api";
import type { AccessLevel } from "../ipc/generated/AccessLevel";
import type { DefaultProjectAccess } from "../ipc/generated/DefaultProjectAccess";
import type { Settings } from "../ipc/generated/Settings";
import { Button, ConfirmDialog, Icon, cn } from "../components/ui";

const field = "h-7 rounded-md border border-border bg-panel px-2 outline-none focus:border-accent";

// Colored by how much an agent can change: read-only, operate, destructive.
const LEVELS: { value: AccessLevel; label: string; hint: string; tone: string }[] = [
  {
    value: "read",
    label: "Read",
    hint: "List machines, boxes, jobs and Vagrantfiles",
    tone: "text-ok",
  },
  {
    value: "operate",
    label: "Operate",
    hint: "Also start, stop, reload and provision; add and update boxes",
    tone: "text-accent",
  },
  {
    value: "full",
    label: "Full",
    hint: "Also destroy, remove boxes and edit Vagrantfiles, each approved by you",
    tone: "text-danger",
  },
];

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex items-start gap-4 border-b border-border px-3 py-2.5 last:border-b-0">
      <div className="w-56 shrink-0 pt-1">
        <div>{label}</div>
        {hint && <div className="mt-0.5 text-xs text-muted">{hint}</div>}
      </div>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}

function Copy({ text, label = "Copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  return (
    <Button
      size="sm"
      onClick={() => {
        void navigator.clipboard?.writeText(text).then(() => {
          setDone(true);
          setTimeout(() => setDone(false), 1500);
        });
      }}
    >
      {done ? "Copied" : label}
    </Button>
  );
}

function Snippet({ title, text }: { title: string; text: string }) {
  return (
    <div className="mb-5 last:mb-0">
      <div className="mb-1.5 flex items-center justify-between gap-2">
        <span className="text-xs font-medium">{title}</span>
        <Copy text={text} />
      </div>
      {/* Wrap instead of scrolling sideways: commands are long, and Copy grabs the whole thing. */}
      <pre className="rounded-md border border-border bg-sunken px-3 py-2.5 font-mono text-xs leading-relaxed break-all whitespace-pre-wrap selectable">
        {text}
      </pre>
    </div>
  );
}

export function McpSection({
  draft,
  set,
}: {
  draft: Settings;
  set: (f: (s: Settings) => void) => void;
}) {
  const qc = useQueryClient();
  const { data: saved } = useSettings();
  const status = useQuery({
    queryKey: ["mcp-status", saved?.mcp.enabled, saved?.mcp.port],
    queryFn: () => api().mcpStatus(),
  });
  const [newName, setNewName] = useState("");
  const [shown, setShown] = useState<{ name: string; token: string } | null>(null);
  const [revoking, setRevoking] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mcp = draft.mcp;
  const clients = saved?.mcp.clients ?? [];

  const refresh = () => qc.invalidateQueries({ queryKey: ["settings"] });
  const run = async (f: () => Promise<void>) => {
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(String(e));
    }
  };

  const url = status.data?.url ?? `http://127.0.0.1:${mcp.port}/mcp`;
  const token = shown?.token ?? "<token>";
  const httpJson = JSON.stringify(
    {
      mcpServers: { boxdeck: { type: "http", url, headers: { Authorization: `Bearer ${token}` } } },
    },
    null,
    2,
  );
  const stdioJson = JSON.stringify(
    { mcpServers: { boxdeck: { command: status.data?.stdio_path ?? "boxdeck-mcp" } } },
    null,
    2,
  );
  const claudeCmd = `claude mcp add --transport http boxdeck ${url} --header "Authorization: Bearer ${token}"`;

  return (
    <section id="mcp" className="mb-6">
      <h2 className="mb-2 text-xs font-semibold tracking-wide text-muted uppercase">
        MCP server for AI agents
      </h2>
      <div className="rounded-lg border border-border bg-panel">
        <Row label="Agent access" hint="Lets AI agents (MCP clients) manage your machines">
          <label className="flex items-center gap-2 pt-1">
            <input
              type="checkbox"
              checked={mcp.enabled}
              onChange={(e) => set((s) => void (s.mcp.enabled = e.target.checked))}
              className="accent-accent"
            />
            Allow agents to connect
          </label>
          {saved?.mcp.enabled && (
            <p
              role="status"
              className={cn("mt-1 text-xs", status.data?.error ? "text-danger" : "text-muted")}
            >
              {status.data?.error
                ? `Not running: ${status.data.error}`
                : status.data?.running
                  ? `Listening on ${status.data.url}`
                  : "Starting…"}
              {saved.mcp.paused && " · paused from the tray"}
            </p>
          )}
        </Row>
        <Row label="What agents may do">
          <div role="radiogroup" aria-label="Access level" className="flex flex-col gap-1.5 pt-1">
            {LEVELS.map((l) => (
              <label key={l.value} className="flex items-start gap-2">
                <input
                  type="radio"
                  name="mcp-access"
                  checked={mcp.access === l.value}
                  onChange={() => set((s) => void (s.mcp.access = l.value))}
                  className="mt-0.5 accent-accent"
                />
                <span>
                  <span className={cn("font-medium", l.tone)}>{l.label}</span>{" "}
                  <span className="text-xs text-muted">{l.hint}</span>
                </span>
              </label>
            ))}
          </div>
        </Row>
        <Row label="Projects" hint="Override per project under Projects">
          <select
            aria-label="Default project access"
            value={mcp.default_access}
            onChange={(e) =>
              set((s) => void (s.mcp.default_access = e.target.value as DefaultProjectAccess))
            }
            className={field}
          >
            <option value="all_projects">Agents see all projects</option>
            <option value="only_allowed">Only projects set to Allow</option>
          </select>
        </Row>
        <Row label="Risky options" hint="Both off by default">
          <label className="flex items-start gap-2 pt-1">
            <input
              type="checkbox"
              checked={mcp.ssh_exec}
              onChange={(e) => set((s) => void (s.mcp.ssh_exec = e.target.checked))}
              className="mt-0.5 accent-accent"
            />
            <span>
              Allow <code className="font-mono">ssh_exec</code>{" "}
              <span className="text-xs text-muted">
                runs any command inside a VM, approved each time. Needs Full access.
              </span>
            </span>
          </label>
          <label className="mt-1.5 flex items-start gap-2">
            <input
              type="checkbox"
              checked={mcp.headless_destructive}
              onChange={(e) => set((s) => void (s.mcp.headless_destructive = e.target.checked))}
              className="mt-0.5 accent-accent"
            />
            <span>
              Allow destructive tools without approval in headless mode{" "}
              <span className="text-xs text-danger">
                boxdeck-mcp can then destroy machines with nobody asked.
              </span>
            </span>
          </label>
        </Row>
        <Row label="Port" hint="Restarts the endpoint when saved">
          <input
            type="number"
            min={1024}
            max={65535}
            aria-label="MCP port"
            value={mcp.port}
            onChange={(e) => set((s) => void (s.mcp.port = Number(e.target.value)))}
            className={cn(field, "w-24")}
          />
        </Row>
        <Row label="Clients" hint="Each agent gets its own token, so you can revoke one at a time">
          <div className="flex flex-col gap-1.5">
            {clients.map((c) => (
              <div key={c.name} className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate font-medium">{c.name}</span>
                <Button
                  size="sm"
                  onClick={() =>
                    void run(async () =>
                      setShown({ name: c.name, token: await api().mcpClientToken(c.name) }),
                    )
                  }
                >
                  Show token
                </Button>
                <Button
                  size="sm"
                  onClick={() =>
                    void run(async () =>
                      setShown({ name: c.name, token: await api().mcpRotateClient(c.name) }),
                    )
                  }
                >
                  Rotate
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  className="text-danger"
                  onClick={() => setRevoking(c.name)}
                >
                  Revoke
                </Button>
              </div>
            ))}
            <form
              className="flex gap-1.5"
              onSubmit={(e) => {
                e.preventDefault();
                const name = newName.trim();
                if (!name) return;
                void run(async () => {
                  const t = await api().mcpAddClient(name);
                  setShown({ name, token: t });
                  setNewName("");
                  await refresh();
                });
              }}
            >
              <input
                aria-label="New client name"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder="e.g. Claude Code"
                className={cn(field, "w-56")}
              />
              <Button size="sm" type="submit" disabled={!newName.trim()}>
                <Icon name="plus" className="size-3" /> Add client
              </Button>
            </form>
            {shown && (
              <div className="mt-1 rounded border border-border bg-sunken p-2">
                <div className="flex items-center gap-2">
                  <span className="text-xs">Token for {shown.name}:</span>
                  <code className="min-w-0 flex-1 truncate font-mono text-xs selectable">
                    {shown.token}
                  </code>
                  <Copy text={shown.token} />
                  <Button size="sm" variant="ghost" onClick={() => setShown(null)}>
                    Hide
                  </Button>
                </div>
                <p className="mt-1 text-xs text-muted">
                  Anyone with this token can act as this client.
                </p>
              </div>
            )}
            {error && (
              <p role="alert" className="text-xs text-danger selectable">
                {error}
              </p>
            )}
          </div>
        </Row>
        <div className="px-3 py-2.5">
          <p className="mb-2 font-medium">Connect a client</p>
          <p className="mb-3 text-xs text-muted">
            {shown
              ? `Snippets include ${shown.name}'s token.`
              : "Show a client's token to fill it in."}{" "}
            Use HTTP while Boxdeck is running; stdio works without it but can't ask you to approve
            destructive actions.
          </p>
          <Snippet title="Claude Code" text={claudeCmd} />
          <Snippet title="HTTP (most clients' mcpServers config)" text={httpJson} />
          <Snippet title="stdio" text={stdioJson} />
        </div>
      </div>
      <ConfirmDialog
        open={revoking !== null}
        danger
        title={`Revoke ${revoking}?`}
        confirmLabel="Revoke"
        body="Its token stops working immediately. The agent needs a new one to connect again."
        onCancel={() => setRevoking(null)}
        onConfirm={() => {
          const name = revoking;
          setRevoking(null);
          if (name)
            void run(async () => {
              await api().mcpRemoveClient(name);
              if (shown?.name === name) setShown(null);
              await refresh();
            });
        }}
      />
    </section>
  );
}
