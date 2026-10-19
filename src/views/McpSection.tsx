import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { useSettings } from "../hooks";
import { api } from "../ipc/api";
import type { AccessLevel } from "../ipc/generated/AccessLevel";
import type { ClientStatus } from "../ipc/generated/ClientStatus";
import type { DefaultProjectAccess } from "../ipc/generated/DefaultProjectAccess";
import type { Settings } from "../ipc/generated/Settings";
import type { ShellMode } from "../ipc/generated/ShellMode";
import { Button, cn } from "../components/ui";
import { shortDir } from "../lib/display";

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
    hint: "Also start, stop, reload and provision; add and update boxes; shell access if on",
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

const SHELL_MODES: { value: ShellMode; label: string }[] = [
  { value: "off", label: "Off" },
  { value: "ask_once", label: "Ask once per VM that can change host files" },
  { value: "always", label: "Always allow" },
];

function ClientRow({
  c,
  busy,
  onConnect,
  onDisconnect,
}: {
  c: ClientStatus;
  busy: boolean;
  onConnect: () => void;
  onDisconnect: () => void;
}) {
  const state = c.error
    ? { text: "Config can't be read", tone: "text-danger" }
    : c.stale
      ? { text: "Points to an old copy of Boxdeck", tone: "text-warn" }
      : c.registered
        ? { text: "Connected", tone: "text-ok" }
        : c.installed
          ? { text: "Not connected", tone: "text-muted" }
          : { text: "Not found on this computer", tone: "text-muted" };
  return (
    <div className="flex items-center gap-3 border-b border-border py-2 last:border-b-0">
      <div className="min-w-0 flex-1">
        <div className="font-medium">{c.name}</div>
        <div className="truncate text-xs text-muted selectable" title={c.error ?? c.config_path}>
          <span className={state.tone}>{state.text}</span> · {shortDir(c.config_path)}
        </div>
      </div>
      {c.registered && !c.stale ? (
        <Button size="sm" disabled={busy} onClick={onDisconnect}>
          Disconnect
        </Button>
      ) : (
        <Button
          size="sm"
          variant={c.installed ? "primary" : "default"}
          disabled={busy || c.error !== null}
          onClick={onConnect}
        >
          {c.stale ? "Reconnect" : "Connect"}
        </Button>
      )}
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
  const clients = useQuery({ queryKey: ["mcp-clients"], queryFn: () => api().mcpClients() });
  const grants = useQuery({ queryKey: ["mcp-grants"], queryFn: () => api().mcpGrants() });
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ text: string; error: boolean } | null>(null);
  const mcp = draft.mcp;

  const change = async (f: () => Promise<unknown>, done: string) => {
    setBusy(true);
    setNotice(null);
    try {
      await f();
      setNotice({ text: done, error: false });
    } catch (e) {
      setNotice({ text: String(e), error: true });
    } finally {
      setBusy(false);
      await qc.invalidateQueries({ queryKey: ["mcp-clients"] });
    }
  };

  const server = clients.data?.server_path ?? "boxdeck-mcp";
  const json = JSON.stringify({ mcpServers: { boxdeck: { command: server, args: [] } } }, null, 2);

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
          {saved?.mcp.enabled && saved.mcp.paused && (
            <p role="status" className="mt-1 text-xs text-warn">
              Paused from the tray
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
        <Row
          label="Shell access in VMs"
          hint="Run commands and copy files with ssh_exec, vm_upload and vm_download. Needs Operate. Override per project under Projects"
        >
          <select
            aria-label="Shell access"
            value={mcp.shell}
            onChange={(e) => set((s) => void (s.mcp.shell = e.target.value as ShellMode))}
            className={field}
          >
            {SHELL_MODES.map((m) => (
              <option key={m.value} value={m.value}>
                {m.label}
              </option>
            ))}
          </select>
          <p className="mt-1 text-xs text-muted">
            Agents get full control inside the VM. A VM with synced folders can usually change files
            in its project folder on this computer, so &ldquo;Ask once&rdquo; asks the first time
            for those VMs. Host files agents copy in or out stay inside the project folder.
          </p>
          {(grants.data?.length ?? 0) > 0 && (
            <div className="mt-2 flex flex-col gap-1">
              <span className="text-xs font-medium">Allowed until Boxdeck restarts</span>
              {grants.data?.map((g) => (
                <div key={`${g.project}/${g.machine}`} className="flex items-center gap-2">
                  <span className="min-w-0 flex-1 truncate text-xs" title={g.project}>
                    <span className="font-medium">{g.machine}</span> in {shortDir(g.project)}
                  </span>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="text-danger"
                    onClick={() =>
                      void api()
                        .mcpRevokeGrant(g.project, g.machine)
                        .then((left) => qc.setQueryData(["mcp-grants"], left))
                    }
                  >
                    Revoke
                  </Button>
                </div>
              ))}
            </div>
          )}
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
        <Row label="When Boxdeck isn't running" hint="Nobody can approve then. Both off by default">
          <label className="flex items-start gap-2 pt-1">
            <input
              type="checkbox"
              checked={mcp.shell_headless}
              onChange={(e) => set((s) => void (s.mcp.shell_headless = e.target.checked))}
              className="mt-0.5 accent-accent"
            />
            <span>
              Allow shell access without asking{" "}
              <span className="text-xs text-muted">
                otherwise &ldquo;Ask once&rdquo; refuses until you open Boxdeck.
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
              Allow destructive tools without approval{" "}
              <span className="text-xs text-danger">
                agents can then destroy machines with nobody asked.
              </span>
            </span>
          </label>
        </Row>
        <div className="px-3 py-2.5">
          <p className="font-medium">Connect a client</p>
          <p className="mb-2 text-xs text-muted">
            Adds a <code className="font-mono">boxdeck</code> entry to the client&rsquo;s own MCP
            config and leaves everything else in it alone. No token needed: the client starts
            Boxdeck&rsquo;s MCP server itself. Restart the client afterwards.
          </p>
          {clients.data?.clients.map((c) => (
            <ClientRow
              key={c.kind}
              c={c}
              busy={busy}
              onConnect={() =>
                void change(() => api().mcpConnect(c.kind), `Connected ${c.name}. Restart it.`)
              }
              onDisconnect={() =>
                void change(() => api().mcpDisconnect(c.kind), `Disconnected ${c.name}.`)
              }
            />
          ))}
          {notice && (
            <p
              role={notice.error ? "alert" : "status"}
              className={cn("mt-1 text-xs selectable", notice.error ? "text-danger" : "text-ok")}
            >
              {notice.text}
            </p>
          )}
          <div className="mt-4">
            <Snippet title="Any other client (mcpServers config)" text={json} />
          </div>
        </div>
      </div>
    </section>
  );
}
