import { lazy, Suspense, useState, type ComponentProps } from "react";
import { useShallow } from "zustand/react/shallow";
import { isActive, prefetch } from "../hooks";
import { api } from "../ipc/api";
import type { LogLine } from "../ipc/generated/LogLine";
import { colorizeLine } from "../lib/vagrantLog";
import { DOCK_DEFAULT, onJobLine, useUi, type DockTab } from "../state/store";
import { Button, ConfirmDialog, Icon, IconButton, cn } from "../components/ui";

// xterm is only needed once a log or terminal tab opens, so it loads off the startup path.
const loadTerminal = () => import("./Terminal");
const LazyTerminal = lazy(() => loadTerminal().then((m) => ({ default: m.TerminalView })));
prefetch(loadTerminal);

function TerminalView(props: ComponentProps<typeof LazyTerminal>) {
  return (
    <Suspense fallback={<div className="h-full w-full bg-[var(--term-bg)]" />}>
      <LazyTerminal {...props} />
    </Suspense>
  );
}

export function Dock() {
  const { tabs, activeTab, setActiveTab, closeTab, dockOpen, setDockOpen, dockSize } = useUi(
    useShallow((s) => ({
      tabs: s.tabs,
      activeTab: s.activeTab,
      setActiveTab: s.setActiveTab,
      closeTab: s.closeTab,
      dockOpen: s.dockOpen,
      setDockOpen: s.setDockOpen,
      dockSize: s.dockSize,
    })),
  );
  // Only the tab titles, so job state changes don't re-render the dock.
  const titles = useUi(
    useShallow((s) =>
      s.tabs.map((t) =>
        t.kind === "job" ? (s.jobs[t.jobId]?.label ?? `job ${t.jobId}`) : t.title,
      ),
    ),
  );
  if (tabs.length === 0) return null;

  return (
    <section
      aria-label="Logs and terminals"
      className={cn(
        "anim-rise relative flex flex-col border-t border-border bg-panel",
        dockOpen ? "min-h-32" : "h-8",
      )}
      style={dockOpen ? { height: `${dockSize * 100}%` } : undefined}
    >
      {dockOpen && <ResizeHandle size={dockSize} />}
      <div role="tablist" className="flex h-8 shrink-0 items-center gap-0.5 overflow-x-auto px-1">
        {tabs.map((t, i) => (
          <div
            key={t.key}
            className={cn(
              "anim-enter flex h-6 items-center rounded-md text-xs transition-colors",
              t.key === activeTab && dockOpen
                ? "bg-sunken font-medium"
                : "text-muted hover:bg-sunken/60",
            )}
          >
            <button
              type="button"
              role="tab"
              aria-selected={t.key === activeTab}
              onClick={() => {
                setActiveTab(t.key);
                setDockOpen(true);
              }}
              className="flex items-center gap-1.5 pl-2"
            >
              <Icon name={t.kind === "job" ? "list" : "terminal"} className="size-3" />
              <span className="max-w-44 truncate">{titles[i]}</span>
            </button>
            <button
              type="button"
              aria-label={`Close ${titles[i]}`}
              onClick={() => closeTab(t.key)}
              className="ml-1 rounded p-0.5 hover:bg-border"
            >
              <Icon name="close" className="size-3" />
            </button>
          </div>
        ))}
        <IconButton
          icon="chevron"
          label={dockOpen ? "Collapse panel" : "Expand panel"}
          onClick={() => setDockOpen(!dockOpen)}
          className={cn(
            "ml-auto [&_svg]:transition-transform",
            dockOpen ? "[&_svg]:rotate-90" : "[&_svg]:-rotate-90",
          )}
        />
      </div>
      {/* Every tab stays mounted so terminals keep their sessions while hidden. */}
      <div className={cn("relative min-h-0 flex-1", !dockOpen && "hidden")}>
        {tabs.map((t) => (
          <div
            key={t.key}
            role="tabpanel"
            className={cn("absolute inset-0", t.key !== activeTab && "invisible")}
          >
            {t.kind === "job" ? <JobTab jobId={t.jobId} /> : <PtyTab tab={t} />}
          </div>
        ))}
      </div>
    </section>
  );
}

/** The dock's top edge: drag to resize, arrow keys when focused, double-click to reset. */
function ResizeHandle({ size }: { size: number }) {
  const setDockSize = useUi((s) => s.setDockSize);
  const [dragging, setDragging] = useState(false);

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    // The dock's parent is the content column it shares with the main view.
    const column = e.currentTarget.parentElement?.parentElement;
    if (!column || e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging(true);
    const rect = column.getBoundingClientRect();
    const move = (ev: PointerEvent) => setDockSize((rect.bottom - ev.clientY) / rect.height);
    const up = () => {
      setDragging(false);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  return (
    <div
      role="separator"
      aria-orientation="horizontal"
      aria-label="Resize panel"
      aria-valuemin={15}
      aria-valuemax={85}
      aria-valuenow={Math.round(size * 100)}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onDoubleClick={() => setDockSize(DOCK_DEFAULT)}
      onKeyDown={(e) => {
        if (e.key === "ArrowUp") setDockSize(size + 0.05);
        else if (e.key === "ArrowDown") setDockSize(size - 0.05);
        else return;
        e.preventDefault();
      }}
      className="group absolute inset-x-0 -top-1 z-10 h-2 cursor-row-resize outline-none"
    >
      <span
        className={cn(
          "absolute inset-x-0 top-1/2 h-0.5 -translate-y-1/2 transition-colors",
          dragging
            ? "bg-accent-solid"
            : "group-hover:bg-accent-solid/60 group-focus-visible:bg-accent-solid",
        )}
      />
    </div>
  );
}

function JobTab({ jobId }: { jobId: number }) {
  const job = useUi((s) => s.jobs[jobId]);
  const openTab = useUi((s) => s.openTab);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const risky = job && /^(up|destroy)\b/.test(job.label);

  const cancel = () => {
    if (risky && job?.state === "running") setConfirmCancel(true);
    else void api().cancelJob(jobId, job?.state === "cancelling");
  };

  const rerun = () =>
    openTab({
      key: `rerun-${jobId}-${Date.now()}`,
      kind: "pty",
      title: `${job?.label ?? "job"} (terminal)`,
      open: (cols, rows, h) => api().ptyRerunJob(jobId, cols, rows, h),
    });

  return (
    <div className="flex h-full flex-col">
      <div className="relative flex h-8 shrink-0 items-center gap-2 border-y border-border px-3 text-xs">
        {job && isActive(job) && (
          <span
            role="progressbar"
            aria-label={`${job.label} running`}
            className="absolute inset-x-0 -bottom-px h-0.5 overflow-hidden"
          >
            <span className="progress-fill anim-load block h-full rounded-full" />
          </span>
        )}
        <code className="truncate font-mono text-muted selectable">
          {job?.argv.slice(1).join(" ")}
        </code>
        <div className="ml-auto flex items-center gap-1.5">
          {job && isActive(job) && (
            <Button size="sm" onClick={cancel}>
              {job.state === "cancelling" ? "Force stop" : "Cancel"}
            </Button>
          )}
          {job?.state === "failed" && (
            <Button
              size="sm"
              onClick={rerun}
              title="For commands that need input, such as a sudo password"
            >
              <Icon name="terminal" className="size-3" /> Run in terminal
            </Button>
          )}
        </div>
      </div>
      <div className="min-h-0 flex-1">
        <TerminalView
          readOnly
          label={`Output of ${job?.label ?? "job"}`}
          onReady={(term) => {
            let last = -1;
            let loaded = false;
            const pending: LogLine[] = [];
            const write = (l: LogLine) => {
              if (l.seq <= last) return;
              last = l.seq;
              term.writeln(colorizeLine(l));
            };
            const unsubscribe = onJobLine(jobId, (l) => (loaded ? write(l) : pending.push(l)));
            void api()
              .jobLog(jobId)
              .then((lines) => {
                lines.forEach(write);
                pending.forEach(write);
                loaded = true;
              });
            return unsubscribe;
          }}
        />
      </div>
      <ConfirmDialog
        open={confirmCancel}
        title="Cancel this job?"
        confirmLabel="Cancel job"
        danger
        body="Stopping halfway can leave the machine half-created. Vagrant gets a chance to clean up first; use Force stop if it hangs."
        onCancel={() => setConfirmCancel(false)}
        onConfirm={() => {
          setConfirmCancel(false);
          void api().cancelJob(jobId, false);
        }}
      />
    </div>
  );
}

function PtyTab({ tab }: { tab: Extract<DockTab, { kind: "pty" }> }) {
  return (
    <TerminalView
      readOnly={false}
      label={tab.title}
      onReady={(term) => {
        let id: number | null = null;
        let closed = false;
        const data = term.onData((d) => id !== null && void api().ptyWrite(id, d));
        const resize = term.onResize(
          ({ cols, rows }) => id !== null && void api().ptyResize(id, cols, rows),
        );
        tab
          .open(term.cols, term.rows, {
            onData: (bytes) => term.write(bytes),
            onExit: (code) => {
              term.write(
                `\r\n\x1b[2m[session ended${code === null ? "" : `, exit ${code}`}]\x1b[0m\r\n`,
              );
            },
          })
          .then((pty) => {
            if (closed) void api().ptyClose(pty);
            else id = pty;
          })
          .catch((e: unknown) => term.write(`\x1b[31m${String(e)}\x1b[0m\r\n`));
        term.focus();
        return () => {
          closed = true;
          data.dispose();
          resize.dispose();
          if (id !== null) void api().ptyClose(id);
        };
      }}
    />
  );
}
