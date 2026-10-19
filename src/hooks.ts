import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { api } from "./ipc/api";
import type { Action } from "./ipc/generated/Action";
import type { AppUpdate } from "./ipc/generated/AppUpdate";
import type { JobInfo } from "./ipc/generated/JobInfo";
import type { Project } from "./ipc/generated/Project";
import type { Settings } from "./ipc/generated/Settings";
import { emitJobLine, useUi } from "./state/store";

export function useAppInfo() {
  return useQuery({ queryKey: ["app-info"], queryFn: () => api().appInfo() });
}

/** A newer Boxdeck release, once a check (at launch or by hand) has found one. */
export function useAppUpdate() {
  const qc = useQueryClient();
  useEffect(
    () => api().onAppUpdateAvailable((u) => qc.setQueryData<AppUpdate | null>(["app-update"], u)),
    [qc],
  );
  return useQuery({ queryKey: ["app-update"], queryFn: () => api().pendingAppUpdate() });
}

/** Checks for a Boxdeck release now; the outcome lands in `updateCheck` and `useAppUpdate`. */
export function useCheckForUpdate() {
  const qc = useQueryClient();
  return useCallback(async () => {
    useUi.setState({ updateCheck: "checking" });
    try {
      const found = await api().checkAppUpdate();
      qc.setQueryData<AppUpdate | null>(["app-update"], found);
      useUi.setState({ updateCheck: found ? "idle" : "up_to_date" });
    } catch (e) {
      useUi.setState({ updateCheck: { error: String(e) } });
    }
  }, [qc]);
}

export function useMachines() {
  const qc = useQueryClient();
  useEffect(
    () =>
      api().onMachinesChanged(() => {
        void qc.invalidateQueries({ queryKey: ["machines"] });
        void qc.invalidateQueries({ queryKey: ["app-info"] });
        void qc.invalidateQueries({ queryKey: ["settings"] });
      }),
    [qc],
  );
  return useQuery({ queryKey: ["machines"], queryFn: () => api().listMachines() });
}

export function useSettings() {
  return useQuery({ queryKey: ["settings"], queryFn: () => api().getSettings() });
}

export function useSaveSettings() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (s: Settings) => api().saveSettings(s),
    onSuccess: (saved) => {
      qc.setQueryData(["settings"], saved);
      void qc.invalidateQueries({ queryKey: ["machines"] });
      void qc.invalidateQueries({ queryKey: ["app-info"] });
    },
  });
}

export function useRefresh() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (projectDir?: string) => api().refreshStatus(projectDir),
    onSuccess: (projects) => qc.setQueryData<Project[]>(["machines"], projects),
  });
}

const FOCUS_REFRESH_MS = 30_000;

/** Refreshes live status when the window regains focus, at most every 30 s (plan §4.1). */
export function useRefreshOnFocus() {
  const qc = useQueryClient();
  useEffect(() => {
    let last = 0;
    const onFocus = () => {
      if (Date.now() - last < FOCUS_REFRESH_MS) return;
      last = Date.now();
      // Automatic: the backend skips it while Vagrant is busy.
      void api()
        .refreshStatus(undefined, true)
        .then((projects) => qc.setQueryData<Project[]>(["machines"], projects));
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [qc]);
}

/** Loads the job list once, then follows live updates and output. */
export function useJobsSync() {
  const setJobs = useUi((s) => s.setJobs);
  const upsertJob = useUi((s) => s.upsertJob);
  useEffect(() => {
    let alive = true;
    const unsubscribe = api().subscribeJobs((e) => {
      if (e.type === "updated") upsertJob(e.job);
      else emitJobLine(e.id, e.line);
    });
    void api()
      .listJobs()
      .then((jobs) => {
        if (!alive) return;
        // Merge so updates that arrived before the list loaded aren't lost.
        const current = useUi.getState().jobs;
        setJobs(jobs.map((j) => current[j.id] ?? j));
      });
    return () => {
      alive = false;
      unsubscribe();
    };
  }, [setJobs, upsertJob]);
}

/** Acts on tray menu items that need the window. */
export function useTrayRequests() {
  const openTab = useUi((s) => s.openTab);
  const setView = useUi((s) => s.setView);
  const setSearch = useUi((s) => s.setSearch);
  const setSettingsTab = useUi((s) => s.setSettingsTab);
  const checkForUpdate = useCheckForUpdate();
  useEffect(
    () =>
      api().onTrayRequest((r) => {
        if (r.kind === "ssh") {
          setView("machines");
          openTab({
            key: `ssh-${r.project_dir}-${r.machine}-${Date.now()}`,
            kind: "pty",
            title: `ssh ${r.machine}`,
            open: (cols, rows, h) => api().sshOpen(r.project_dir, r.machine, cols, rows, h),
          });
        } else if (r.kind === "show_machine") {
          setView("machines");
          setSearch(r.machine);
        } else if (r.kind === "check_updates") {
          setView("settings");
          setSettingsTab("about");
          void checkForUpdate();
        } else {
          setView("settings");
        }
      }),
    [openTab, setView, setSearch, setSettingsTab, checkForUpdate],
  );
}

export function isActive(job: JobInfo): boolean {
  return job.state === "queued" || job.state === "running" || job.state === "cancelling";
}

/** The newest unfinished job for a machine (or a project-wide one), if any. */
export function activeJobFor(
  jobs: Record<number, JobInfo>,
  dir: string,
  name: string,
): JobInfo | undefined {
  return Object.values(jobs)
    .filter(isActive)
    .filter(
      (j) =>
        (j.scope.scope === "machine" && j.scope.project === dir && j.scope.name === name) ||
        (j.scope.scope === "project" && j.scope.project === dir),
    )
    .sort((a, b) => b.id - a.id)[0];
}

export function useRunAction() {
  const openTab = useUi((s) => s.openTab);
  return useMutation({
    mutationFn: async (v: { dir: string; machine: string | null; action: Action }) =>
      api().runAction(v.dir, v.machine, v.action),
    onSuccess: (jobId) => openTab({ key: `job-${jobId}`, kind: "job", jobId }),
  });
}

const NARROW = "(max-width: 1023px)";

// Module-level so useSyncExternalStore keeps one subscription instead of resubscribing per render.
function subscribeNarrow(cb: () => void) {
  const mq = window.matchMedia?.(NARROW);
  mq?.addEventListener("change", cb);
  return () => mq?.removeEventListener("change", cb);
}

const isNarrow = () => window.matchMedia?.(NARROW).matches ?? false;

/** True below Tailwind's `lg` breakpoint, where toolbars drop their button labels. */
export function useNarrow(): boolean {
  return useSyncExternalStore(subscribeNarrow, isNarrow);
}

/** Loads a lazily split module once the window has settled, so first use doesn't wait on it. */
export function prefetch(load: () => Promise<unknown>) {
  setTimeout(() => void load(), 2_000);
}

/** The current time, updated every `intervalMs`, for "N min ago" labels. */
export function useNow(intervalMs = 15_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(t);
  }, [intervalMs]);
  return now;
}

export function relativeTime(ms: number | null, now: number): string {
  if (ms === null) return "never";
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 10) return "just now";
  if (s < 60) return `${s} s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  return `${h} h ago`;
}
