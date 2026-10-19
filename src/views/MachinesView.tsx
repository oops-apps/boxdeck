import { memo, useCallback, useMemo, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import {
  activeJobFor,
  relativeTime,
  useMachines,
  useNow,
  useRefresh,
  useRunAction,
  useAppInfo,
  useNarrow,
  useSettings,
} from "../hooks";
import { VagrantSetup } from "./VagrantSetup";
import { COLOR_CLASS, shortDir } from "../lib/display";
import type { ProjectSettings } from "../ipc/generated/ProjectSettings";
import type { Settings } from "../ipc/generated/Settings";
import { api } from "../ipc/api";
import type { Action } from "../ipc/generated/Action";
import type { Machine } from "../ipc/generated/Machine";
import type { Project } from "../ipc/generated/Project";
import { machineKey, useUi } from "../state/store";
import {
  Button,
  ConfirmDialog,
  Icon,
  IconButton,
  Menu,
  StateBadge,
  Tip,
  stateStyle,
  cn,
  useChangeCount,
  type MenuItem,
} from "../components/ui";

const STATE_FILTERS = [
  { value: "all", label: "All states" },
  { value: "running", label: "Running" },
  { value: "stopped", label: "Stopped" },
  { value: "suspended", label: "Suspended" },
  { value: "not_created", label: "Not created" },
];

const SUSPENDED = ["saved", "suspended", "paused"];
// `not_running` is VMware's name for a stopped VM.
const STOPPED = ["poweroff", "stopped", "aborted", "shutoff", "not_running"];

function matchesFilter(m: Machine, filter: string): boolean {
  switch (filter) {
    case "running":
      return m.state === "running";
    case "stopped":
      return STOPPED.includes(m.state);
    case "suspended":
      return SUSPENDED.includes(m.state);
    case "not_created":
      return m.state === "not_created";
    default:
      return true;
  }
}

interface Pending {
  dir: string;
  machine: string | null;
  action: Action;
  title: string;
}

export function MachinesView() {
  const { data: projects, isLoading, error } = useMachines();
  const refresh = useRefresh();
  const run = useRunAction();
  const { data: settings } = useSettings();
  const { data: info } = useAppInfo();
  const narrow = useNarrow();
  const {
    search,
    setSearch,
    stateFilter,
    setStateFilter,
    selected,
    clearSelected,
    showHidden,
    setShowHidden,
  } = useUi(
    useShallow((s) => ({
      search: s.search,
      setSearch: s.setSearch,
      stateFilter: s.stateFilter,
      setStateFilter: s.setStateFilter,
      selected: s.selected,
      clearSelected: s.clearSelected,
      showHidden: s.showHidden,
      setShowHidden: s.setShowHidden,
    })),
  );
  const [confirm, setConfirm] = useState<Pending | null>(null);
  const [addError, setAddError] = useState<string | null>(null);

  const visible = useMemo(
    () => arrange(projects ?? [], settings, search, stateFilter, showHidden),
    [projects, settings, search, stateFilter, showHidden],
  );
  const hiddenCount = useMemo(
    () =>
      (projects ?? []).filter((p) => settings?.projects[p.env.dir]?.hidden).length +
      (projects ?? []).reduce(
        (n, p) =>
          n +
          p.machines.filter((m) => settings?.projects[p.env.dir]?.machines[m.name]?.hidden).length,
        0,
      ),
    [projects, settings],
  );

  const addProject = async () => {
    setAddError(null);
    const dir = await api().pickFolder();
    if (!dir) return;
    try {
      await api().addBookmark(dir);
    } catch (e) {
      setAddError(String(e));
    }
  };

  // Stable so memoized project groups and rows skip re-rendering when nothing of theirs changed.
  const runAction = run.mutate;
  const request = useCallback(
    (p: Pending) => {
      if (p.action === "destroy") setConfirm(p);
      else runAction(p);
    },
    [runAction],
  );

  const bulk = (action: Action) => {
    for (const key of selected) {
      const [dir, machine] = key.split("\u0000");
      run.mutate({ dir, machine, action });
    }
    clearSelected();
  };

  const total = (projects ?? []).reduce((n, p) => n + p.machines.length, 0);
  const running = (projects ?? []).reduce(
    (n, p) => n + p.machines.filter((m) => m.state === "running").length,
    0,
  );

  return (
    <div className="flex h-full flex-col">
      <header
        data-tauri-drag-region="deep"
        className="flex items-center gap-2 border-b border-border h-12 shrink-0 px-4"
      >
        <h1 className="shrink-0 text-[15px] font-semibold">Machines</h1>
        <span className="text-xs whitespace-nowrap text-muted max-lg:hidden">
          {running} running · {total} total
        </span>
        <div className="ml-auto flex min-w-0 items-center justify-end gap-2">
          <label className="relative min-w-24 shrink">
            <span className="sr-only">Search machines</span>
            <Icon
              name="search"
              className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted"
            />
            <input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search"
              className="h-7 w-48 max-w-full rounded-md border border-border bg-panel pr-2 pl-7 outline-none focus:border-accent"
            />
          </label>
          <select
            aria-label="Filter by state"
            value={stateFilter}
            onChange={(e) => setStateFilter(e.target.value)}
            className="h-7 rounded-md border border-border bg-panel px-2"
          >
            {STATE_FILTERS.map((f) => (
              <option key={f.value} value={f.value}>
                {f.label}
              </option>
            ))}
          </select>
          {hiddenCount > 0 && (
            <label className="flex shrink-0 items-center gap-1.5 text-xs whitespace-nowrap">
              <input
                type="checkbox"
                checked={showHidden}
                onChange={(e) => setShowHidden(e.target.checked)}
                className="accent-accent"
              />
              Show hidden ({hiddenCount})
            </label>
          )}
          {narrow ? (
            <>
              <IconButton
                icon="folder"
                label="Add project…"
                variant="default"
                onClick={() => void addProject()}
              />
              <IconButton
                icon="refresh"
                label={refresh.isPending ? "Checking…" : "Refresh"}
                variant="default"
                onClick={() => refresh.mutate(undefined)}
                disabled={refresh.isPending}
                className={cn(refresh.isPending && "[&_svg]:animate-spin")}
              />
            </>
          ) : (
            <>
              <Button onClick={() => void addProject()}>
                <Icon name="folder" className="size-3.5" /> Add project…
              </Button>
              <Button onClick={() => refresh.mutate(undefined)} disabled={refresh.isPending}>
                <Icon name="refresh" className={cn(refresh.isPending && "animate-spin")} />
                {refresh.isPending ? "Checking…" : "Refresh"}
              </Button>
            </>
          )}
        </div>
      </header>

      {selected.size > 0 && (
        <div
          role="toolbar"
          aria-label="Selected machines"
          className="flex items-center gap-2 border-b border-border bg-sunken px-4 py-1.5"
        >
          <span className="text-xs font-medium">{selected.size} selected</span>
          <Button size="sm" variant="primary" onClick={() => bulk("up")}>
            <Icon name="play" className="size-3" /> Up
          </Button>
          <Button size="sm" onClick={() => bulk("halt")}>
            <Icon name="stop" className="size-3 text-danger" /> Halt
          </Button>
          <Button size="sm" variant="ghost" onClick={clearSelected}>
            Clear
          </Button>
        </div>
      )}

      {addError && (
        <div
          role="alert"
          className="border-b border-border bg-danger/10 px-4 py-2 text-xs text-danger selectable"
        >
          Couldn't add that folder: {addError}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto px-4 py-3">
        {isLoading && <p className="text-muted">Reading the machine index…</p>}
        {error && (
          <p role="alert" className="text-danger selectable">
            Couldn't read Vagrant's machine index: {String(error)}
          </p>
        )}
        {info?.vagrant_error && total === 0 && <VagrantSetup info={info} />}
        {!isLoading && !error && total === 0 && !info?.vagrant_error && (
          <div className="mx-auto mt-16 max-w-sm text-center">
            <Icon name="monitor" className="mx-auto size-8 text-muted" />
            <p className="mt-3 font-medium">No Vagrant machines yet</p>
            <p className="mt-1 text-muted">
              Machines appear here once you run <code className="font-mono">vagrant up</code> in a
              project, or add a project folder to manage it from the start.
            </p>
          </div>
        )}
        <div className="flex flex-col gap-3">
          {visible.map(({ group, projects: list }) => (
            <div key={group ?? ""} className="flex flex-col gap-3">
              {group !== null && (
                <h2 className="mt-1 text-xs font-semibold tracking-wide text-muted uppercase">
                  {group}
                </h2>
              )}
              {list.map((p) => (
                <ProjectGroup
                  key={p.env.dir}
                  project={p}
                  settings={settings?.projects[p.env.dir]}
                  onAction={request}
                />
              ))}
            </div>
          ))}
        </div>
      </div>

      <ConfirmDialog
        open={confirm !== null}
        danger
        title={confirm?.title ?? ""}
        confirmLabel="Destroy"
        body={
          <>
            Vagrant will delete the virtual machine and its disks. The Vagrantfile and project
            folder are kept. This can't be undone.
          </>
        }
        onCancel={() => setConfirm(null)}
        onConfirm={() => {
          if (confirm) run.mutate(confirm);
          setConfirm(null);
        }}
      />
    </div>
  );
}

/** Filters, sorts (pinned first, then by name) and groups projects using their settings. */
export function arrange(
  projects: Project[],
  settings: Settings | undefined,
  search: string,
  stateFilter: string,
  showHidden: boolean,
): { group: string | null; projects: Project[] }[] {
  const q = search.trim().toLowerCase();
  const ps = (p: Project) => settings?.projects[p.env.dir];
  const filtered = projects
    .filter((p) => showHidden || !ps(p)?.hidden)
    .map((p) => {
      const conf = ps(p);
      const projectText = `${p.env.dir} ${conf?.alias ?? ""} ${conf?.group ?? ""}`.toLowerCase();
      return {
        ...p,
        machines: p.machines.filter((m) => {
          const mc = conf?.machines[m.name];
          if (!showHidden && mc?.hidden) return false;
          const text = `${m.name} ${mc?.alias ?? ""} ${m.provider}`.toLowerCase();
          return (
            matchesFilter(m, stateFilter) && (!q || text.includes(q) || projectText.includes(q))
          );
        }),
      };
    })
    .filter((p) => p.machines.length > 0 || (!q && stateFilter === "all"));
  const name = (p: Project) => (ps(p)?.alias ?? shortDir(p.env.dir)).toLowerCase();
  filtered.sort(
    (a, b) =>
      Number(ps(b)?.pinned ?? false) - Number(ps(a)?.pinned ?? false) ||
      name(a).localeCompare(name(b)),
  );
  const groups = new Map<string | null, Project[]>();
  for (const p of filtered) {
    const g = ps(p)?.group ?? null;
    groups.set(g, [...(groups.get(g) ?? []), p]);
  }
  const anyGroup = [...groups.keys()].some((g) => g !== null);
  return [...groups.entries()]
    .sort(([a], [b]) => (a === null ? 1 : b === null ? -1 : a.localeCompare(b)))
    .map(([group, list]) => ({
      group: group ?? (anyGroup ? "Other projects" : null),
      projects: list,
    }));
}

const ProjectGroup = memo(function ProjectGroup({
  project,
  settings,
  onAction,
}: {
  project: Project;
  settings?: ProjectSettings;
  onAction: (p: Pending) => void;
}) {
  const refresh = useRefresh();
  const editVagrantfile = useUi((s) => s.editVagrantfile);
  const openProjectSettings = useUi((s) => s.openProjectSettings);
  const now = useNow();
  const [open, setOpen] = useState(true);
  const dir = project.env.dir;
  return (
    <section className="overflow-hidden rounded-lg border border-border bg-panel">
      <div className="flex items-center gap-2 border-b border-border px-3 py-1.5">
        <button
          type="button"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
          className="flex min-w-0 items-center gap-1.5 rounded text-left"
        >
          <Icon
            name="chevron"
            className={cn("size-3 text-muted transition-transform", open && "rotate-90")}
          />
          {settings?.color && (
            <span
              aria-label={`${settings.color} tag`}
              className={cn("size-2 shrink-0 rounded-full", COLOR_CLASS[settings.color])}
            />
          )}
          <span className="truncate font-medium selectable" title={dir}>
            {settings?.alias ?? shortDir(dir)}
          </span>
          {settings?.alias && <span className="truncate text-xs text-muted">{shortDir(dir)}</span>}
          {settings?.pinned && <span className="text-xs text-muted">(pinned)</span>}
        </button>
        {project.env.vagrantfile && (
          <span className="rounded bg-sunken px-1.5 text-xs text-muted">
            {project.env.vagrantfile}
          </span>
        )}
        <span className="ml-auto text-xs text-muted">
          checked {relativeTime(project.checked_at, now)}
        </span>
        <IconButton
          icon="refresh"
          label="Check status"
          onClick={() => refresh.mutate(dir)}
          disabled={refresh.isPending}
          className={cn(refresh.isPending && "[&_svg]:animate-spin")}
        />
        <Menu
          trigger={<IconButton icon="more" label="Project actions" />}
          items={[
            {
              label: "Up all",
              onSelect: () => onAction({ dir, machine: null, action: "up", title: "" }),
            },
            {
              label: "Halt all",
              onSelect: () => onAction({ dir, machine: null, action: "halt", title: "" }),
            },
            "sep",
            { label: "Edit Vagrantfile", onSelect: () => editVagrantfile(dir) },
            { label: "Project settings…", onSelect: () => openProjectSettings(dir) },
            { label: "Open folder", onSelect: () => void api().openFolder(dir) },
            ...(project.bookmarked
              ? [
                  "sep" as const,
                  {
                    label: "Remove from Boxdeck",
                    onSelect: () => void api().removeBookmark(dir),
                  },
                ]
              : []),
          ]}
        />
      </div>
      {project.error && (
        <div
          role="alert"
          className="flex gap-2 border-b border-border bg-danger/8 px-3 py-2 text-xs"
        >
          <Icon name="warning" className="mt-px size-3.5 text-danger" />
          <div className="min-w-0">
            <p className="font-medium text-danger">Vagrant couldn't load this project</p>
            <pre className="mt-1 font-mono whitespace-pre-wrap text-muted selectable">
              {project.error}
            </pre>
          </div>
        </div>
      )}
      {open && (
        <ul>
          {project.machines.map((m) => (
            <MachineRow
              key={m.name}
              dir={dir}
              machine={m}
              alias={settings?.machines[m.name]?.alias ?? null}
              onAction={onAction}
            />
          ))}
          {project.machines.length === 0 && (
            <li className="px-3 py-2 text-muted">No machines defined.</li>
          )}
        </ul>
      )}
    </section>
  );
});

const MachineRow = memo(function MachineRow({
  dir,
  machine: m,
  alias,
  onAction,
}: {
  dir: string;
  machine: Machine;
  alias: string | null;
  onAction: (p: Pending) => void;
}) {
  const key = machineKey(dir, m.name);
  const isSelected = useUi((s) => s.selected.has(key));
  const toggleSelected = useUi((s) => s.toggleSelected);
  const openTab = useUi((s) => s.openTab);
  // Returns the job object held in the store, so other jobs' updates don't re-render this row.
  const job = useUi((s) => activeJobFor(s.jobs, dir, m.name));
  const running = m.state === "running";
  const suspended = SUSPENDED.includes(m.state);
  const created = m.state !== "not_created";
  const changes = useChangeCount(m.state);
  const act = (action: Action) =>
    onAction({ dir, machine: m.name, action, title: `Destroy ${m.name}?` });

  const openSsh = () =>
    openTab({
      key: `ssh-${key}-${Date.now()}`,
      kind: "pty",
      title: `ssh ${m.name}`,
      open: (cols, rows, h) => api().sshOpen(dir, m.name, cols, rows, h),
    });

  const menu: (MenuItem | "sep")[] = [
    { label: "Suspend", onSelect: () => act("suspend"), disabled: !running },
    { label: "Halt", onSelect: () => act("halt"), disabled: !suspended },
    { label: "Reload", onSelect: () => act("reload"), disabled: !created },
    { label: "Provision", onSelect: () => act("provision"), disabled: !running },
    "sep",
    { label: "Open folder", onSelect: () => void api().openFolder(dir) },
    "sep",
    { label: "Destroy…", onSelect: () => act("destroy"), danger: true, disabled: !created },
  ];

  return (
    <li
      className={cn(
        "group relative flex items-center gap-3 border-b border-border px-3 py-2 last:border-b-0",
        isSelected ? "bg-accent/10" : "hover:bg-sunken/60",
      )}
    >
      {changes > 0 && (
        <span
          key={changes}
          aria-hidden="true"
          className={cn("anim-wash", stateStyle(m.state).tone)}
        />
      )}
      <input
        type="checkbox"
        aria-label={`Select ${m.name}`}
        checked={isSelected}
        onChange={() => toggleSelected(key)}
        className="accent-accent"
      />
      <div className="w-32 shrink-0">
        <StateBadge
          state={m.state}
          label={m.state_short || m.state}
          stale={m.source === "index"}
          changes={changes}
        />
      </div>
      <div className="min-w-0 flex-1 truncate">
        <Tip label={m.state_long || m.state_short}>
          <span className="font-medium">{alias ?? m.name}</span>
        </Tip>
        {alias && <span className="ml-1.5 font-mono text-xs text-muted">{m.name}</span>}
        <span className="ml-2 text-xs text-muted">{m.provider}</span>
      </div>
      {/* The row already names the machine, so the chip shows just the verb. */}
      {job && (
        <Tip label={job.label}>
          <button
            type="button"
            onClick={() => openTab({ key: `job-${job.id}`, kind: "job", jobId: job.id })}
            className="anim-enter flex shrink-0 items-center gap-1.5 rounded px-1.5 text-xs whitespace-nowrap text-accent hover:bg-sunken"
          >
            <span className="size-2 animate-pulse rounded-full bg-accent" aria-hidden="true" />
            {job.state === "queued"
              ? "queued"
              : job.state === "cancelling"
                ? "stopping"
                : (job.argv[1] ?? job.label)}
          </button>
        </Tip>
      )}
      <div className="flex shrink-0 items-center gap-1">
        {suspended ? (
          <Button size="sm" variant="primary" onClick={() => act("resume")} disabled={!!job}>
            <Icon name="play" className="size-3" /> Resume
          </Button>
        ) : running ? (
          <Button size="sm" onClick={() => act("halt")} disabled={!!job}>
            <Icon name="stop" className="size-3 text-danger" /> Halt
          </Button>
        ) : (
          <Button size="sm" variant="primary" onClick={() => act("up")} disabled={!!job}>
            <Icon name="play" className="size-3" /> Up
          </Button>
        )}
        <Button size="sm" onClick={openSsh} disabled={!running}>
          <Icon name="prompt" className="size-3" /> SSH
        </Button>
        <Menu
          trigger={<IconButton icon="more" label={`More actions for ${m.name}`} />}
          items={menu}
        />
      </div>
    </li>
  );
});
