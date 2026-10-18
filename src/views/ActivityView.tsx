import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { isActive, relativeTime, useNow } from "../hooks";
import { api } from "../ipc/api";
import type { JobInfo } from "../ipc/generated/JobInfo";
import type { JobState } from "../ipc/generated/JobState";
import type { Origin } from "../ipc/generated/Origin";
import { useUi } from "../state/store";
import { Button, cn } from "../components/ui";
import { shortDir } from "../lib/display";

const STATE_LABEL: Record<JobState, string> = {
  queued: "Queued",
  running: "Running",
  cancelling: "Stopping",
  succeeded: "Succeeded",
  failed: "Failed",
  cancelled: "Cancelled",
  timed_out: "Timed out",
};

const STATE_TONE: Record<JobState, string> = {
  queued: "text-muted",
  running: "text-accent",
  cancelling: "text-warn",
  succeeded: "text-ok",
  failed: "text-danger",
  cancelled: "text-muted",
  timed_out: "text-danger",
};

export function originLabel(o: Origin): string {
  return o.kind === "mcp" ? `Agent: ${o.client}` : o.kind === "tray" ? "Tray" : "App";
}

function duration(job: JobInfo, now: number): string {
  if (job.started_at === null) return "—";
  const s = Math.round(((job.finished_at ?? now) - job.started_at) / 1000);
  return s < 60 ? `${s} s` : `${Math.floor(s / 60)} min ${s % 60} s`;
}

const DECISION_TONE: Record<string, string> = {
  allowed: "text-ok",
  approved: "text-ok",
  denied: "text-danger",
  rejected: "text-danger",
};

function AgentCalls() {
  const qc = useQueryClient();
  useEffect(
    () => api().onAuditChanged(() => void qc.invalidateQueries({ queryKey: ["audit"] })),
    [qc],
  );
  const { data, error } = useQuery({ queryKey: ["audit"], queryFn: () => api().listAudit(200) });
  const now = useNow();
  if (error)
    return (
      <p role="alert" className="px-4 py-3 text-danger">
        {String(error)}
      </p>
    );
  if (!data?.length)
    return (
      <p className="px-4 py-6 text-muted">
        No agent calls yet. Turn agents on in Settings → MCP / Agents.
      </p>
    );
  return (
    <table className="w-full border-collapse text-left">
      <thead className="sticky top-0 bg-bg text-xs text-muted">
        <tr className="border-b border-border">
          <th className="px-4 py-1.5 font-medium">When</th>
          <th className="px-2 py-1.5 font-medium">Agent</th>
          <th className="px-2 py-1.5 font-medium">Tool</th>
          <th className="px-2 py-1.5 font-medium">Decision</th>
          <th className="px-2 py-1.5 font-medium">Result</th>
        </tr>
      </thead>
      <tbody>
        {data.map((e, i) => (
          <tr
            key={`${e.time}-${i}`}
            className="border-b border-border align-top hover:bg-sunken/60"
          >
            <td className="px-4 py-1.5 whitespace-nowrap text-muted">
              {relativeTime(e.time, now)}
            </td>
            <td className="px-2 py-1.5">
              {e.client}
              <span className="ml-1 text-xs text-muted">({e.transport})</span>
            </td>
            <td className="px-2 py-1.5">
              <code className="font-mono text-xs">{e.tool}</code>
              <div
                className="max-w-80 truncate font-mono text-xs text-muted selectable"
                title={JSON.stringify(e.arguments)}
              >
                {JSON.stringify(e.arguments)}
              </div>
            </td>
            <td className={cn("px-2 py-1.5 font-medium", DECISION_TONE[e.decision])}>
              {e.decision}
            </td>
            <td className="max-w-72 px-2 py-1.5 text-muted selectable">
              {e.result}
              {e.job_id !== null && <span className="ml-1 text-xs">(job {e.job_id})</span>}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function ActivityView() {
  const [tab, setTab] = useState<"jobs" | "agents">("jobs");
  return (
    <div className="flex h-full flex-col">
      <header
        data-tauri-drag-region="deep"
        className="flex items-center gap-3 border-b border-border h-12 shrink-0 px-4"
      >
        <h1 className="text-[15px] font-semibold">Activity</h1>
        <div role="tablist" className="ml-4 flex gap-1">
          {(["jobs", "agents"] as const).map((t) => (
            <button
              key={t}
              type="button"
              role="tab"
              aria-selected={tab === t}
              onClick={() => setTab(t)}
              className={cn(
                "h-6 rounded-md px-2 text-xs transition-colors",
                tab === t
                  ? "bg-[var(--hover)] font-medium"
                  : "text-muted hover:bg-[var(--hover)]/50 hover:text-text",
              )}
            >
              {t === "jobs" ? "Jobs" : "Agent calls"}
            </button>
          ))}
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-auto">
        {tab === "jobs" ? <JobsTable /> : <AgentCalls />}
      </div>
    </div>
  );
}

function JobsTable() {
  const jobs = useUi((s) => s.jobs);
  const openTab = useUi((s) => s.openTab);
  const [agentsOnly, setAgentsOnly] = useState(false);
  const list = useMemo(
    () =>
      Object.values(jobs)
        .filter((j) => !agentsOnly || j.origin.kind === "mcp")
        .sort((a, b) => b.id - a.id),
    [jobs, agentsOnly],
  );
  const now = useNow();

  return (
    <div>
      <div className="flex items-center px-4 py-1.5">
        <label className="ml-auto flex items-center gap-1.5 text-xs">
          <input
            type="checkbox"
            checked={agentsOnly}
            onChange={(e) => setAgentsOnly(e.target.checked)}
            className="accent-accent"
          />
          Agent jobs only
        </label>
      </div>
      <div>
        {list.length === 0 ? (
          <p className="px-4 py-6 text-muted">
            Nothing has run yet. Actions you start from the app, the tray or an agent show up here.
          </p>
        ) : (
          <table className="w-full border-collapse text-left">
            <thead className="sticky top-0 bg-bg text-xs text-muted">
              <tr className="border-b border-border">
                <th className="px-4 py-1.5 font-medium">Command</th>
                <th className="px-2 py-1.5 font-medium">Project</th>
                <th className="px-2 py-1.5 font-medium">From</th>
                <th className="px-2 py-1.5 font-medium">Status</th>
                <th className="px-2 py-1.5 font-medium">Started</th>
                <th className="px-2 py-1.5 font-medium">Took</th>
                <th className="px-4 py-1.5" />
              </tr>
            </thead>
            <tbody>
              {list.map((j) => {
                const project =
                  j.scope.scope === "machine" || j.scope.scope === "project"
                    ? shortDir(j.scope.project)
                    : "—";
                return (
                  <tr
                    key={j.id}
                    className="border-b border-border hover:bg-sunken/60"
                    onDoubleClick={() => openTab({ key: `job-${j.id}`, kind: "job", jobId: j.id })}
                  >
                    <td className="px-4 py-1.5 font-medium">{j.label}</td>
                    <td className="max-w-56 truncate px-2 py-1.5 text-muted" title={j.cwd ?? ""}>
                      {project}
                    </td>
                    <td className="px-2 py-1.5">
                      <span
                        className={cn(
                          "rounded px-1.5 py-px text-xs",
                          j.origin.kind === "mcp" ? "bg-accent/15 text-accent" : "bg-sunken",
                        )}
                      >
                        {originLabel(j.origin)}
                      </span>
                    </td>
                    <td className={cn("px-2 py-1.5 font-medium", STATE_TONE[j.state])}>
                      {STATE_LABEL[j.state]}
                      {j.exit_code !== null && j.state === "failed" && ` (exit ${j.exit_code})`}
                    </td>
                    <td className="px-2 py-1.5 text-muted">{relativeTime(j.started_at, now)}</td>
                    <td className="px-2 py-1.5 text-muted">{duration(j, now)}</td>
                    <td className="px-4 py-1.5 text-right whitespace-nowrap">
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => openTab({ key: `job-${j.id}`, kind: "job", jobId: j.id })}
                      >
                        Log
                      </Button>
                      {isActive(j) && (
                        <Button
                          size="sm"
                          variant="ghost"
                          onClick={() => void api().cancelJob(j.id, false)}
                        >
                          Cancel
                        </Button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
