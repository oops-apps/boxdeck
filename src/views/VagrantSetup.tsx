import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "../ipc/api";
import type { AppInfo } from "../ipc/generated/AppInfo";
import { useUi } from "../state/store";
import { Button, Icon } from "../components/ui";

const INSTALL_URL = "https://developer.hashicorp.com/vagrant/install";

/** First-run help when Vagrant can't be found or is too old. */
export function VagrantSetup({ info }: { info: AppInfo }) {
  const qc = useQueryClient();
  const setView = useUi((s) => s.setView);
  const [checking, setChecking] = useState(false);
  const tooOld = info.vagrant_error?.includes("older than") ?? false;

  return (
    <div className="mx-auto mt-12 max-w-lg rounded-lg border border-border bg-panel p-6">
      <div className="flex items-center gap-2">
        <Icon name="warning" className="size-5 text-warn" />
        <h2 className="text-[15px] font-semibold">
          {tooOld ? "Your Vagrant is too old" : "Boxdeck can't find Vagrant"}
        </h2>
      </div>
      <p className="mt-2 text-muted selectable">{info.vagrant_error}</p>
      <ol className="mt-4 list-decimal space-y-2 pl-5">
        <li>
          {tooOld ? "Update" : "Install"} Vagrant 2.3 or newer from{" "}
          <code className="font-mono text-xs selectable">{INSTALL_URL}</code>, plus a provider such
          as VMware Desktop or VirtualBox.
        </li>
        <li>
          If it's installed somewhere unusual, set its path in{" "}
          <button
            type="button"
            className="text-accent underline"
            onClick={() => setView("settings")}
          >
            Settings
          </button>
          .
        </li>
        <li>
          Then look again. Step-by-step instructions for every provider are in the project's{" "}
          <code className="font-mono text-xs">docs/install.md</code>.
        </li>
      </ol>
      <div className="mt-5">
        <Button
          variant="primary"
          disabled={checking}
          onClick={async () => {
            setChecking(true);
            try {
              await api().redetectVagrant();
              await qc.invalidateQueries({ queryKey: ["app-info"] });
            } finally {
              setChecking(false);
            }
          }}
        >
          <Icon name="refresh" className={checking ? "animate-spin" : undefined} />
          {checking ? "Looking…" : "Look again"}
        </Button>
      </div>
    </div>
  );
}
