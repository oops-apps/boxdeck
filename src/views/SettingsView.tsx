import { useState, type ReactNode } from "react";
import {
  isActive,
  useAppInfo,
  useAppUpdate,
  useCheckForUpdate,
  useMachines,
  useSaveSettings,
  useSettings,
} from "../hooks";
import { api } from "../ipc/api";
import type { AgentAccess } from "../ipc/generated/AgentAccess";
import type { ProjectSettings } from "../ipc/generated/ProjectSettings";
import type { Settings } from "../ipc/generated/Settings";
import { useUi, type SettingsTab } from "../state/store";
import { Button, Icon, cn } from "../components/ui";
import { COLORS, COLOR_CLASS, emptyProject, shortDir } from "../lib/display";
import { McpSection } from "./McpSection";

const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform);

const field = "h-7 rounded-md border border-border bg-panel px-2 outline-none focus:border-accent";

function Section({ title, children, id }: { title: string; children: ReactNode; id?: string }) {
  return (
    <section id={id} className="mb-6">
      <h2 className="mb-2 text-xs font-semibold tracking-wide text-muted uppercase">{title}</h2>
      <div className="rounded-lg border border-border bg-panel">{children}</div>
    </section>
  );
}

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

function Toggle({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <label className="flex items-center gap-2 pt-1">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="accent-accent"
      />
      {label}
    </label>
  );
}

/** Edits a string map such as environment variables. */
export function KeyValueEditor({
  value,
  onChange,
  keyLabel = "Name",
}: {
  value: Record<string, string>;
  onChange: (v: Record<string, string>) => void;
  keyLabel?: string;
}) {
  const [rows, setRows] = useState(() => Object.entries(value));
  const [seen, setSeen] = useState(value);
  const toMap = (r: [string, string][]) => Object.fromEntries(r.filter(([k]) => k.trim() !== ""));
  // Pick up outside changes (revert, import) without dropping rows still being typed.
  if (value !== seen) {
    setSeen(value);
    if (JSON.stringify(toMap(rows)) !== JSON.stringify(value)) setRows(Object.entries(value));
  }
  const commit = (next: [string, string][]) => {
    setRows(next);
    onChange(toMap(next));
  };
  return (
    <div className="flex flex-col gap-1.5">
      {rows.map(([k, v], i) => (
        <div key={i} className="flex gap-1.5">
          <input
            aria-label={`${keyLabel} ${i + 1}`}
            value={k}
            onChange={(e) => commit(rows.map((r, j) => (j === i ? [e.target.value, r[1]] : r)))}
            placeholder="VAGRANT_DEFAULT_PROVIDER"
            className={cn(field, "w-64 font-mono text-xs")}
          />
          <input
            aria-label={`Value ${i + 1}`}
            value={v}
            onChange={(e) => commit(rows.map((r, j) => (j === i ? [r[0], e.target.value] : r)))}
            className={cn(field, "min-w-0 flex-1 font-mono text-xs")}
          />
          <Button
            variant="ghost"
            aria-label={`Remove ${k || "row"}`}
            onClick={() => commit(rows.filter((_, j) => j !== i))}
            className="w-7 px-0"
          >
            <Icon name="close" className="size-3.5" />
          </Button>
        </div>
      ))}
      <div>
        <Button size="sm" onClick={() => setRows([...rows, ["", ""]])}>
          <Icon name="plus" className="size-3" /> Add variable
        </Button>
      </div>
    </div>
  );
}

const SETTINGS_TABS: { id: SettingsTab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "window", label: "Window & tray" },
  { id: "projects", label: "Projects" },
  { id: "mcp", label: "MCP / Agents" },
  { id: "about", label: "Backup & about" },
];

export function SettingsView() {
  const tab = useUi((s) => s.settingsTab);
  const setTab = useUi((s) => s.setSettingsTab);
  const { data: saved } = useSettings();
  const save = useSaveSettings();
  /** Local edits; `null` means "showing what's saved". */
  const [edits, setEdits] = useState<Settings | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  if (!saved) return <p className="p-4 text-muted">Loading settings…</p>;

  const draft = edits ?? saved;
  const dirty = edits !== null && JSON.stringify(edits) !== JSON.stringify(saved);
  const setDraft = (s: Settings | null) => setEdits(s);
  const set = (change: (s: Settings) => void) => {
    const next = structuredClone(draft);
    change(next);
    setEdits(next);
  };

  return (
    <div className="flex h-full flex-col">
      <header
        data-tauri-drag-region="deep"
        className="flex items-center gap-2 border-b border-border h-12 shrink-0 px-4"
      >
        <h1 className="text-[15px] font-semibold">Settings</h1>
        {dirty && (
          <div className="ml-auto flex items-center gap-2">
            <span className="text-xs text-muted">Unsaved changes</span>
            <Button onClick={() => setDraft(null)}>Revert</Button>
            <Button
              variant="primary"
              disabled={save.isPending}
              onClick={() =>
                save.mutate(draft, {
                  onSuccess: () => {
                    setDraft(null);
                    setNotice("Settings saved.");
                  },
                })
              }
            >
              Save
            </Button>
          </div>
        )}
      </header>
      {(save.error || notice) && (
        <div
          role={save.error ? "alert" : "status"}
          className={cn(
            "border-b border-border px-4 py-2 text-xs selectable",
            save.error ? "bg-danger/10 text-danger" : "text-ok",
          )}
        >
          {save.error ? String(save.error) : notice}
        </div>
      )}
      <div
        role="tablist"
        aria-label="Settings sections"
        className="flex gap-1 border-b border-border px-4 py-1.5"
      >
        {SETTINGS_TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            onClick={() => setTab(t.id)}
            className={cn(
              "h-7 rounded-md px-2.5 transition-colors",
              tab === t.id
                ? "bg-[var(--hover)] font-medium"
                : "text-muted hover:bg-[var(--hover)]/50 hover:text-text",
            )}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-4 py-4">
        <div className="max-w-3xl">
          {tab === "general" && <GeneralSection draft={draft} set={set} />}
          {tab === "window" && (
            <Section title="Window and tray">
              <Row label="Closing the window" hint="Boxdeck keeps running in the tray">
                <Toggle
                  label="Keep running in the tray"
                  checked={draft.app.keep_in_tray}
                  onChange={(v) => set((s) => void (s.app.keep_in_tray = v))}
                />
              </Row>
              <Row label="Startup">
                <Toggle
                  label="Launch at login"
                  checked={draft.app.launch_at_login}
                  onChange={(v) => set((s) => void (s.app.launch_at_login = v))}
                />
                <Toggle
                  label="Start hidden in the tray"
                  checked={draft.app.start_hidden}
                  onChange={(v) => set((s) => void (s.app.start_hidden = v))}
                />
              </Row>
              {isMac && (
                <Row
                  label="Menu bar only"
                  hint="No Dock icon. Takes effect after restarting Boxdeck."
                >
                  <Toggle
                    label="Show Boxdeck only in the menu bar"
                    checked={draft.app.menu_bar_only}
                    onChange={(v) => set((s) => void (s.app.menu_bar_only = v))}
                  />
                </Row>
              )}
              <Row label="Notifications">
                <Toggle
                  label="Notify when a job finishes while Boxdeck isn't focused"
                  checked={draft.app.notify_on_finish}
                  onChange={(v) => set((s) => void (s.app.notify_on_finish = v))}
                />
              </Row>
            </Section>
          )}
          {tab === "general" && (
            <Section title="Environment variables">
              <div className="px-3 py-2.5">
                <p className="mb-2 text-xs text-muted">
                  Passed to every Vagrant command. Stored in plain text in the settings file, so
                  don't put passwords or tokens here.
                </p>
                <KeyValueEditor
                  value={draft.env}
                  onChange={(env) => set((s) => void (s.env = env))}
                />
              </div>
            </Section>
          )}
          {tab === "projects" && <ProjectsSection draft={draft} set={set} />}
          {tab === "mcp" && <McpSection draft={draft} set={set} />}
          {tab === "about" && (
            <>
              <UpdatesSection draft={draft} set={set} />
              <Section title="Backup">
                <Row label="Settings file" hint="Move your setup to another computer">
                  <div className="flex gap-2">
                    <Button
                      onClick={async () => {
                        try {
                          const path = await api().exportSettings();
                          if (path) setNotice(`Exported to ${path}`);
                        } catch (e) {
                          setNotice(`Export failed: ${String(e)}`);
                        }
                      }}
                    >
                      Export…
                    </Button>
                    <Button
                      onClick={async () => {
                        try {
                          const imported = await api().importSettings();
                          if (imported) {
                            setDraft(imported);
                            setNotice("Imported. Review the changes, then Save.");
                          }
                        } catch (e) {
                          setNotice(`Import failed: ${String(e)}`);
                        }
                      }}
                    >
                      Import…
                    </Button>
                  </div>
                </Row>
              </Section>
              <Diagnostics />
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function GeneralSection({
  draft,
  set,
}: {
  draft: Settings;
  set: (f: (s: Settings) => void) => void;
}) {
  const [newPath, setNewPath] = useState("");
  return (
    <Section title="Vagrant">
      <Row label="Vagrant program" hint="Leave empty to find vagrant on PATH">
        <div className="flex gap-2">
          <input
            aria-label="Vagrant program path"
            value={draft.vagrant_path ?? ""}
            onChange={(e) => set((s) => void (s.vagrant_path = e.target.value.trim() || null))}
            placeholder="/usr/local/bin/vagrant"
            className={cn(field, "min-w-0 flex-1 font-mono text-xs")}
          />
        </div>
      </Row>
      <Row label="Extra PATH folders" hint="Searched first, e.g. for provider tools">
        <div className="flex flex-col gap-1.5">
          {draft.extra_path.map((p, i) => (
            <div key={p} className="flex items-center gap-1.5">
              <code className="min-w-0 flex-1 truncate font-mono text-xs selectable">{p}</code>
              <Button
                variant="ghost"
                aria-label={`Remove ${p}`}
                className="w-7 px-0"
                onClick={() => set((s) => void s.extra_path.splice(i, 1))}
              >
                <Icon name="close" className="size-3.5" />
              </Button>
            </div>
          ))}
          <div className="flex gap-1.5">
            <input
              aria-label="New PATH folder"
              value={newPath}
              onChange={(e) => setNewPath(e.target.value)}
              placeholder="/opt/homebrew/bin"
              className={cn(field, "min-w-0 flex-1 font-mono text-xs")}
            />
            <Button
              size="sm"
              disabled={!newPath.trim()}
              onClick={() => {
                set((s) => void s.extra_path.push(newPath.trim()));
                setNewPath("");
              }}
            >
              Add
            </Button>
          </div>
        </div>
      </Row>
      <Row label="Status checks" hint="Each check runs Vagrant, which takes a few seconds">
        <div className="flex flex-wrap items-center gap-3">
          <label className="flex items-center gap-2">
            At most
            <input
              type="number"
              min={1}
              max={8}
              aria-label="Parallel status checks"
              value={draft.status.concurrency}
              onChange={(e) => set((s) => void (s.status.concurrency = Number(e.target.value)))}
              className={cn(field, "w-14")}
            />
            at once
          </label>
          <label className="flex items-center gap-2">
            Refresh every
            <select
              aria-label="Background refresh"
              value={draft.status.interval_secs ?? 0}
              onChange={(e) => {
                const v = Number(e.target.value);
                set((s) => void (s.status.interval_secs = v === 0 ? null : v));
              }}
              className={field}
            >
              <option value={0}>Off</option>
              <option value={60}>1 minute</option>
              <option value={300}>5 minutes</option>
              <option value={900}>15 minutes</option>
            </select>
          </label>
        </div>
      </Row>
      <Row label="Concurrent jobs" hint="Actions on one machine always run one at a time">
        <input
          type="number"
          min={1}
          max={16}
          aria-label="Concurrent jobs"
          value={draft.jobs.global_cap}
          onChange={(e) => set((s) => void (s.jobs.global_cap = Number(e.target.value)))}
          className={cn(field, "w-14")}
        />
      </Row>
    </Section>
  );
}

function ProjectsSection({
  draft,
  set,
}: {
  draft: Settings;
  set: (f: (s: Settings) => void) => void;
}) {
  const { data: projects } = useMachines();
  const focus = useUi((s) => s.settingsProject);
  const [open, setOpen] = useState<string | null>(focus);
  const [seenFocus, setSeenFocus] = useState(focus);
  const [error, setError] = useState<string | null>(null);
  if (focus !== seenFocus) {
    setSeenFocus(focus);
    setOpen(focus);
  }

  const dirs = [
    ...new Set([...(projects ?? []).map((p) => p.env.dir), ...Object.keys(draft.projects)]),
  ].sort();

  const add = async () => {
    setError(null);
    const dir = await api().pickFolder();
    if (!dir) return;
    try {
      await api().addBookmark(dir);
      setOpen(dir);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <Section title="Projects" id="projects">
      <div className="flex items-center gap-2 border-b border-border px-3 py-2">
        <p className="flex-1 text-xs text-muted">
          Names, groups and defaults for each project. Folders you add stay listed even before the
          first <code className="font-mono">vagrant up</code>.
        </p>
        <Button size="sm" onClick={() => void add()}>
          <Icon name="folder" className="size-3.5" /> Add project folder…
        </Button>
      </div>
      {error && (
        <p role="alert" className="border-b border-border px-3 py-2 text-xs text-danger selectable">
          {error}
        </p>
      )}
      {dirs.length === 0 && <p className="px-3 py-3 text-muted">No projects yet.</p>}
      {dirs.map((dir) => {
        const p = draft.projects[dir] ?? emptyProject();
        const machines = projects?.find((x) => x.env.dir === dir)?.machines ?? [];
        const bookmarked = draft.bookmarks.includes(dir);
        const update = (change: (p: ProjectSettings) => void) =>
          set((s) => {
            const cur = s.projects[dir] ?? emptyProject();
            change(cur);
            s.projects[dir] = cur;
          });
        const expanded = open === dir;
        return (
          <div key={dir} className="border-b border-border last:border-b-0">
            <button
              type="button"
              aria-expanded={expanded}
              onClick={() => setOpen(expanded ? null : dir)}
              className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-sunken/60"
            >
              <Icon name="chevron" className={cn("size-3 text-muted", expanded && "rotate-90")} />
              {p.color && <span className={cn("size-2 rounded-full", COLOR_CLASS[p.color])} />}
              <span className="font-medium">{p.alias ?? shortDir(dir)}</span>
              {p.alias && <span className="truncate text-xs text-muted">{shortDir(dir)}</span>}
              <span className="ml-auto text-xs text-muted">
                {[p.group, p.hidden && "hidden", bookmarked && "added by hand"]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
            </button>
            {expanded && (
              <div className="grid grid-cols-[150px_1fr] items-center gap-x-3 gap-y-2.5 bg-sunken/40 px-8 py-3">
                <label htmlFor={`alias-${dir}`}>Display name</label>
                <input
                  id={`alias-${dir}`}
                  value={p.alias ?? ""}
                  onChange={(e) => update((x) => void (x.alias = e.target.value || null))}
                  placeholder={shortDir(dir)}
                  className={field}
                />
                <label htmlFor={`group-${dir}`}>Group</label>
                <input
                  id={`group-${dir}`}
                  value={p.group ?? ""}
                  onChange={(e) => update((x) => void (x.group = e.target.value || null))}
                  placeholder="e.g. Work"
                  className={field}
                />
                <span>Color tag</span>
                <div role="radiogroup" aria-label="Color tag" className="flex items-center gap-1.5">
                  <button
                    type="button"
                    role="radio"
                    aria-checked={p.color === null}
                    onClick={() => update((x) => void (x.color = null))}
                    className={cn(
                      "rounded border border-border px-1.5 text-xs",
                      p.color === null && "border-accent",
                    )}
                  >
                    None
                  </button>
                  {COLORS.map((c) => (
                    <button
                      key={c}
                      type="button"
                      role="radio"
                      aria-checked={p.color === c}
                      aria-label={c}
                      onClick={() => update((x) => void (x.color = c))}
                      className={cn(
                        "size-5 rounded-full border-2",
                        COLOR_CLASS[c],
                        p.color === c ? "border-text" : "border-transparent",
                      )}
                    />
                  ))}
                </div>
                <span>Show</span>
                <div className="flex gap-4">
                  <Toggle
                    label="Pin to top"
                    checked={p.pinned}
                    onChange={(v) => update((x) => void (x.pinned = v))}
                  />
                  <Toggle
                    label="Hide from the list"
                    checked={p.hidden}
                    onChange={(v) => update((x) => void (x.hidden = v))}
                  />
                </div>
                <label htmlFor={`provider-${dir}`}>vagrant up defaults</label>
                <div className="flex flex-wrap items-center gap-3">
                  <input
                    id={`provider-${dir}`}
                    aria-label="Default provider"
                    value={p.up.provider ?? ""}
                    onChange={(e) =>
                      update((x) => void (x.up.provider = e.target.value.trim() || null))
                    }
                    placeholder="Provider, e.g. vmware_desktop"
                    className={cn(field, "w-56")}
                  />
                  <select
                    aria-label="Provisioning on up"
                    value={p.up.provision === null ? "" : String(p.up.provision)}
                    onChange={(e) =>
                      update(
                        (x) =>
                          void (x.up.provision =
                            e.target.value === "" ? null : e.target.value === "true"),
                      )
                    }
                    className={field}
                  >
                    <option value="">Provision as Vagrant decides</option>
                    <option value="true">Always provision</option>
                    <option value="false">Never provision</option>
                  </select>
                </div>
                <label htmlFor={`agents-${dir}`}>Agent access</label>
                <select
                  id={`agents-${dir}`}
                  value={p.agent_access}
                  onChange={(e) =>
                    update((x) => void (x.agent_access = e.target.value as AgentAccess))
                  }
                  className={cn(field, "w-56")}
                >
                  <option value="inherit">Use the MCP setting</option>
                  <option value="allow">Allow</option>
                  <option value="deny">Deny (agents can't see it)</option>
                </select>
                <span className="self-start pt-1">Environment</span>
                <KeyValueEditor
                  value={p.env}
                  onChange={(env) => update((x) => void (x.env = env))}
                />
                {machines.length > 0 && (
                  <>
                    <span className="self-start pt-1">Machine names</span>
                    <div className="flex flex-col gap-1.5">
                      {machines.map((m) => (
                        <div key={m.name} className="flex items-center gap-2">
                          <span className="w-28 truncate font-mono text-xs">{m.name}</span>
                          <input
                            aria-label={`Display name for ${m.name}`}
                            value={p.machines[m.name]?.alias ?? ""}
                            onChange={(e) =>
                              update((x) => {
                                const cur = x.machines[m.name] ?? {
                                  alias: null,
                                  color: null,
                                  pinned: false,
                                  hidden: false,
                                };
                                cur.alias = e.target.value || null;
                                x.machines[m.name] = cur;
                              })
                            }
                            placeholder="Display name"
                            className={cn(field, "w-48")}
                          />
                          <Toggle
                            label="Hide"
                            checked={p.machines[m.name]?.hidden ?? false}
                            onChange={(v) =>
                              update((x) => {
                                const cur = x.machines[m.name] ?? {
                                  alias: null,
                                  color: null,
                                  pinned: false,
                                  hidden: false,
                                };
                                cur.hidden = v;
                                x.machines[m.name] = cur;
                              })
                            }
                          />
                        </div>
                      ))}
                    </div>
                  </>
                )}
                {bookmarked && (
                  <>
                    <span />
                    <div>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="text-danger"
                        onClick={() => void api().removeBookmark(dir)}
                      >
                        Remove from Boxdeck
                      </Button>
                    </div>
                  </>
                )}
              </div>
            )}
          </div>
        );
      })}
    </Section>
  );
}

function Diagnostics() {
  const { data: info } = useAppInfo();
  const rows: [string, string][] = [
    ["Boxdeck", info?.version ?? "…"],
    ["Vagrant", info?.vagrant ? `${info.vagrant.version} at ${info.vagrant.path}` : "not found"],
    ["Machine index", info?.index_path ?? "unknown"],
  ];
  return (
    <Section title="Diagnostics">
      <dl className="grid grid-cols-[140px_1fr] gap-x-4 gap-y-1.5 p-3">
        {rows.map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="text-muted">{k}</dt>
            <dd className="font-mono text-xs break-all selectable">{v}</dd>
          </div>
        ))}
      </dl>
      {info?.vagrant_error && (
        <p role="alert" className="border-t border-border px-3 py-2 text-danger selectable">
          {info.vagrant_error}
        </p>
      )}
    </Section>
  );
}

const mb = (bytes: number) => (bytes / 1_000_000).toFixed(1);

/** Changelog Markdown, reduced to its headings and bullets; anything else shows as plain text. */
function ReleaseNotes({ text }: { text: string }) {
  const lines = text.split("\n").filter((l) => l.trim() !== "");
  return (
    <div className="mb-2 flex max-h-48 flex-col gap-1 overflow-auto rounded-md border border-border bg-sunken p-2 text-xs selectable">
      {lines.map((line, i) => {
        const heading = /^#+\s+(.*)/.exec(line);
        if (heading)
          return (
            <p key={i} className="mt-1 font-semibold first:mt-0">
              {heading[1]}
            </p>
          );
        const bullet = /^\s*[-*]\s+(.*)/.exec(line);
        if (bullet)
          return (
            <p key={i} className="pl-3 -indent-3">
              • {bullet[1]}
            </p>
          );
        return <p key={i}>{line}</p>;
      })}
    </div>
  );
}

function UpdatesSection({
  draft,
  set,
}: {
  draft: Settings;
  set: (f: (s: Settings) => void) => void;
}) {
  const { data: info } = useAppInfo();
  const { data: update } = useAppUpdate();
  const check = useCheckForUpdate();
  const status = useUi((s) => s.updateCheck);
  const busy = useUi((s) => Object.values(s.jobs).filter(isActive).length);
  const [installError, setInstallError] = useState<string | null>(null);
  const [progress, setProgress] = useState<{
    done: number;
    total: number | null;
    phase: "downloading" | "installing" | "restarting";
  } | null>(null);
  const checking = status === "checking";
  const error = installError ?? (typeof status === "object" ? status.error : null);

  const install = async () => {
    setInstallError(null);
    setProgress({ done: 0, total: null, phase: "downloading" });
    try {
      await api().installAppUpdate((p) =>
        setProgress((cur) => {
          const base = cur ?? { done: 0, total: null, phase: "downloading" as const };
          if (p.kind === "started") return { ...base, total: p.total };
          if (p.kind === "chunk") return { ...base, done: base.done + p.bytes };
          return { ...base, phase: "installing" };
        }),
      );
      setProgress((cur) => cur && { ...cur, phase: "restarting" });
    } catch (e) {
      setProgress(null);
      setInstallError(String(e));
    }
  };

  const released = update?.date
    ? new Date(update.date).toLocaleDateString(undefined, { dateStyle: "medium" })
    : null;
  const pct = progress?.total ? Math.min(100, (progress.done / progress.total) * 100) : null;

  return (
    <Section title="Updates" id="updates">
      <Row label="Boxdeck version">
        <div className="flex flex-wrap items-center gap-3 pt-0.5">
          <span className="font-mono text-xs selectable">{info?.version ?? "…"}</span>
          <Button
            size="sm"
            disabled={checking || progress !== null}
            onClick={() => {
              setInstallError(null);
              void check();
            }}
          >
            {checking ? "Checking…" : "Check for updates"}
          </Button>
          {status === "up_to_date" && !update && (
            <span role="status" className="text-xs text-ok">
              You're up to date.
            </span>
          )}
        </div>
        {error && (
          <p role="alert" className="mt-2 text-xs text-danger selectable">
            {error}
          </p>
        )}
      </Row>
      {update && (
        <Row
          label={`Boxdeck ${update.version} is available`}
          hint={released ? `Released ${released}` : undefined}
        >
          {update.notes && <ReleaseNotes text={update.notes} />}
          {progress ? (
            <div role="status" aria-live="polite" className="flex flex-col gap-1.5">
              <div
                role="progressbar"
                aria-label="Update download"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={pct === null ? undefined : Math.round(pct)}
                className="h-1.5 overflow-hidden rounded-full bg-sunken"
              >
                <div
                  className={cn(
                    "progress-fill h-full rounded-full transition-[width]",
                    pct === null && "anim-load",
                  )}
                  style={pct === null ? undefined : { width: `${pct}%` }}
                />
              </div>
              <span className="text-xs text-muted">
                {progress.phase === "downloading" &&
                  (progress.total
                    ? `Downloading ${mb(progress.done)} of ${mb(progress.total)} MB…`
                    : `Downloading ${mb(progress.done)} MB…`)}
                {progress.phase === "installing" && "Installing…"}
                {progress.phase === "restarting" && "Restarting Boxdeck…"}
              </span>
            </div>
          ) : (
            <div className="flex flex-wrap items-center gap-3">
              <Button variant="primary" disabled={busy > 0} onClick={() => void install()}>
                Install and restart
              </Button>
              {busy > 0 && (
                <span className="text-xs text-muted">
                  Wait for running Vagrant jobs to finish first.
                </span>
              )}
            </div>
          )}
        </Row>
      )}
      <Row label="Automatic check">
        <Toggle
          label="Check for updates when Boxdeck starts"
          checked={draft.app.check_updates_on_launch}
          onChange={(v) => set((s) => void (s.app.check_updates_on_launch = v))}
        />
      </Row>
    </Section>
  );
}
