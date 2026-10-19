import { create } from "zustand";
import type { PtyHandlers } from "../ipc/api";
import type { JobInfo } from "../ipc/generated/JobInfo";
import type { LogLine } from "../ipc/generated/LogLine";

export type SettingsTab = "general" | "window" | "projects" | "mcp" | "about";

export type View = "machines" | "boxes" | "activity" | "settings" | "vagrantfile";

export type DockTab =
  | { key: string; kind: "job"; jobId: number }
  | {
      key: string;
      kind: "pty";
      title: string;
      /** Starts the session once the terminal knows its size. */
      open: (cols: number, rows: number, h: PtyHandlers) => Promise<number>;
    };

interface UiState {
  view: View;
  setView: (v: View) => void;
  /** Project folder open in the Vagrantfile editor. */
  editing: string | null;
  editVagrantfile: (dir: string) => void;
  settingsTab: SettingsTab;
  setSettingsTab: (t: SettingsTab) => void;
  /** Project folder to expand in Settings → Projects. */
  settingsProject: string | null;
  openProjectSettings: (dir: string | null) => void;
  /** Last manual check for a Boxdeck release (see `useCheckForUpdate`). */
  updateCheck: "idle" | "checking" | "up_to_date" | { error: string };
  showHidden: boolean;
  setShowHidden: (v: boolean) => void;

  search: string;
  setSearch: (s: string) => void;
  stateFilter: string;
  setStateFilter: (s: string) => void;

  /** Selected machines as `dir\u0000name`. */
  selected: Set<string>;
  toggleSelected: (key: string) => void;
  clearSelected: () => void;

  jobs: Record<number, JobInfo>;
  setJobs: (jobs: JobInfo[]) => void;
  upsertJob: (job: JobInfo) => void;

  dockOpen: boolean;
  setDockOpen: (open: boolean) => void;
  /** Open dock height as a share of the window's content column. */
  dockSize: number;
  setDockSize: (size: number) => void;
  tabs: DockTab[];
  activeTab: string | null;
  openTab: (tab: DockTab) => void;
  closeTab: (key: string) => void;
  setActiveTab: (key: string) => void;
}

export const machineKey = (dir: string, name: string) => `${dir}\u0000${name}`;

export const DOCK_DEFAULT = 0.38;
const DOCK_KEY = "boxdeck.dockSize";
export const clampDock = (size: number) => Math.min(0.85, Math.max(0.15, size));

// A per-window convenience, so browser storage is enough; it may be unavailable.
function savedDockSize(): number {
  try {
    const v = Number(localStorage.getItem(DOCK_KEY));
    return v > 0 ? clampDock(v) : DOCK_DEFAULT;
  } catch {
    return DOCK_DEFAULT;
  }
}

export const useUi = create<UiState>((set) => ({
  view: "machines",
  setView: (view) => set({ view }),
  editing: null,
  editVagrantfile: (editing) => set({ editing, view: "vagrantfile" }),
  settingsProject: null,
  settingsTab: "general",
  setSettingsTab: (settingsTab) => set({ settingsTab }),
  openProjectSettings: (settingsProject) =>
    set({ settingsProject, view: "settings", settingsTab: "projects" }),
  updateCheck: "idle",
  showHidden: false,
  setShowHidden: (showHidden) => set({ showHidden }),

  search: "",
  setSearch: (search) => set({ search }),
  stateFilter: "all",
  setStateFilter: (stateFilter) => set({ stateFilter }),

  selected: new Set(),
  toggleSelected: (key) =>
    set((s) => {
      const selected = new Set(s.selected);
      if (selected.has(key)) selected.delete(key);
      else selected.add(key);
      return { selected };
    }),
  clearSelected: () => set({ selected: new Set() }),

  jobs: {},
  setJobs: (list) => set({ jobs: Object.fromEntries(list.map((j) => [j.id, j])) }),
  upsertJob: (job) => set((s) => ({ jobs: { ...s.jobs, [job.id]: job } })),

  dockOpen: false,
  setDockOpen: (dockOpen) => set({ dockOpen }),
  dockSize: savedDockSize(),
  setDockSize: (size) => {
    const dockSize = clampDock(size);
    try {
      localStorage.setItem(DOCK_KEY, String(dockSize));
    } catch {
      // Not saved; the size still applies for this session.
    }
    set({ dockSize });
  },
  tabs: [],
  activeTab: null,
  openTab: (tab) =>
    set((s) => ({
      tabs: s.tabs.some((t) => t.key === tab.key) ? s.tabs : [...s.tabs, tab],
      activeTab: tab.key,
      dockOpen: true,
    })),
  closeTab: (key) =>
    set((s) => {
      const tabs = s.tabs.filter((t) => t.key !== key);
      const activeTab = s.activeTab === key ? (tabs.at(-1)?.key ?? null) : s.activeTab;
      return { tabs, activeTab, dockOpen: s.dockOpen && tabs.length > 0 };
    }),
  setActiveTab: (activeTab) => set({ activeTab }),
}));

type LineListener = (line: LogLine) => void;
const lineListeners = new Map<number, Set<LineListener>>();

/** Output lines for one job, for open log tabs. */
export function onJobLine(id: number, cb: LineListener): () => void {
  const set = lineListeners.get(id) ?? new Set();
  set.add(cb);
  lineListeners.set(id, set);
  return () => set.delete(cb);
}

export function emitJobLine(id: number, line: LogLine): void {
  lineListeners.get(id)?.forEach((cb) => cb(line));
}
