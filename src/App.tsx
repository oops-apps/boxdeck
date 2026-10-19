import * as Tooltip from "@radix-ui/react-tooltip";
import { lazy, Suspense, useState } from "react";
import { useAppInfo, useAppUpdate, useJobsSync, useRefreshOnFocus, useTrayRequests } from "./hooks";
import { hasOverlayTitleBar } from "./ipc/api";
import { useUi, type View } from "./state/store";
import { isActive, prefetch } from "./hooks";
import { Dock } from "./dock/Dock";
import { ActivityView } from "./views/ActivityView";
import { BoxesView } from "./views/BoxesView";
import { MachinesView } from "./views/MachinesView";
import { SettingsView } from "./views/SettingsView";
import { Button, Icon, cn, type IconName } from "./components/ui";

// CodeMirror is most of the bundle and only the editor needs it, so it loads off the startup path.
const loadVagrantfileView = () => import("./views/VagrantfileView");
const VagrantfileView = lazy(() =>
  loadVagrantfileView().then((m) => ({ default: m.VagrantfileView })),
);
prefetch(loadVagrantfileView);

const NAV: { view: View; label: string; icon: IconName }[] = [
  { view: "machines", label: "Machines", icon: "monitor" },
  { view: "boxes", label: "Boxes", icon: "box" },
  { view: "activity", label: "Activity", icon: "activity" },
  { view: "settings", label: "Settings", icon: "settings" },
];

function App() {
  useJobsSync();
  useRefreshOnFocus();
  useTrayRequests();
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const busy = useUi((s) => Object.values(s.jobs).filter(isActive).length);
  const { data: info } = useAppInfo();
  const { data: update } = useAppUpdate();
  const settingsTab = useUi((s) => s.settingsTab);
  const setSettingsTab = useUi((s) => s.setSettingsTab);
  /** Version the user said "Later" to; a newer one shows the banner again. */
  const [dismissed, setDismissed] = useState<string | null>(null);
  const overlay = hasOverlayTitleBar();
  const showUpdate =
    update && update.version !== dismissed && !(view === "settings" && settingsTab === "about");

  return (
    <Tooltip.Provider>
      <div className="flex h-full">
        <nav
          aria-label="Main"
          data-tauri-drag-region="deep"
          className={cn(
            "flex w-48 shrink-0 flex-col border-r border-border bg-sunken px-2 pb-3",
            // Room for the macOS traffic lights, which sit over the sidebar's top corner.
            overlay ? "pt-12" : "pt-3",
          )}
        >
          <div className="mb-4 px-2">
            <h1 className="text-[15px] font-semibold tracking-tight">Boxdeck</h1>
          </div>
          <ul className="flex flex-col gap-0.5">
            {NAV.map((n) => (
              <li key={n.view}>
                <button
                  type="button"
                  aria-current={view === n.view ? "page" : undefined}
                  onClick={() => setView(n.view)}
                  className={cn(
                    "flex h-7 w-full items-center gap-2 rounded-md px-2 text-left transition-colors",
                    view === n.view
                      ? "bg-panel font-medium shadow-sm"
                      : "text-muted hover:bg-panel/60",
                  )}
                >
                  <Icon name={n.icon} />
                  {n.label}
                  {n.view === "activity" && busy > 0 && (
                    <span className="anim-enter ml-auto rounded-full bg-accent-solid px-1.5 text-[11px] text-accent-text">
                      {busy}
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
          <div className="mt-auto px-2 text-xs text-muted">
            {info?.detecting && "Looking for Vagrant…"}
            {info?.vagrant && `Vagrant ${info.vagrant.version}`}
            {info && !info.vagrant && !info.detecting && (
              <span className="text-danger">Vagrant not found</span>
            )}
          </div>
        </nav>
        <div className="flex min-w-0 flex-1 flex-col">
          {info?.vagrant_error && (
            <div
              role="alert"
              className="border-b border-border bg-danger/10 px-4 py-2 text-xs selectable"
            >
              <strong className="text-danger">Vagrant isn't available.</strong> {info.vagrant_error}
            </div>
          )}
          {showUpdate && (
            <div
              role="status"
              className="flex items-center gap-2 border-b border-border bg-accent/15 px-4 py-1.5 text-xs"
            >
              <span>
                <strong>Boxdeck {update.version}</strong> is available.
              </span>
              <Button
                size="sm"
                variant="primary"
                onClick={() => {
                  setView("settings");
                  setSettingsTab("about");
                }}
              >
                View update
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setDismissed(update.version)}>
                Later
              </Button>
            </div>
          )}
          <main className="min-h-0 flex-1">
            {view === "machines" && <MachinesView />}
            {view === "boxes" && <BoxesView />}
            {view === "activity" && <ActivityView />}
            {view === "settings" && <SettingsView />}
            {view === "vagrantfile" && (
              <Suspense fallback={null}>
                <VagrantfileView />
              </Suspense>
            )}
          </main>
          <Dock />
        </div>
      </div>
    </Tooltip.Provider>
  );
}

export default App;
