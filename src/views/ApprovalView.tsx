import { useEffect, useState } from "react";
import { api } from "../ipc/api";
import type { PendingApproval } from "../ipc/generated/PendingApproval";
import { Button, cn } from "../components/ui";
import { shortDir } from "../lib/display";

/** The small window that asks before an agent runs a destructive tool, or first gets a shell
 * in a VM that can write to the host. */
export function ApprovalView({ id }: { id: string }) {
  const [req, setReq] = useState<PendingApproval | null | undefined>(undefined);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    void api().getApproval(id).then(setReq);
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [id]);

  if (req === undefined) return <p className="p-4 text-muted">Loading…</p>;
  if (req === null)
    return <p className="p-4 text-muted">This request has already been answered.</p>;

  const left = Math.max(0, Math.round((req.expires_at - now) / 1000));
  const answer = (allow: boolean, session = false) => void api().answerApproval(id, allow, session);
  const shell = req.grantable;

  return (
    <div className="flex h-full flex-col bg-panel p-5">
      <div className="flex items-center gap-2">
        {/* The website's approval signal: a glowing dot, red for destructive tools. */}
        <span
          aria-hidden="true"
          className={cn(
            "size-2 shrink-0 rounded-full",
            shell
              ? "bg-warn shadow-[0_0_10px_var(--warn)]"
              : "bg-danger shadow-[0_0_10px_var(--danger)]",
          )}
        />
        <h1 className="text-[15px] font-semibold">
          {shell ? "An agent wants shell access to a VM" : "An agent wants to make a change"}
        </h1>
      </div>
      <dl className="mt-4 grid grid-cols-[80px_1fr] gap-x-3 gap-y-1.5 rounded-lg bg-sunken px-3.5 py-3">
        <dt className="text-muted">Agent</dt>
        <dd className="font-medium">{req.client}</dd>
        <dt className="text-muted">Action</dt>
        <dd className="font-mono">{req.tool}</dd>
        {req.project && (
          <>
            <dt className="text-muted">Project</dt>
            <dd className="truncate selectable" title={req.project}>
              {shortDir(req.project)}
            </dd>
          </>
        )}
        {req.machine && (
          <>
            <dt className="text-muted">Machine</dt>
            <dd className="font-medium">{req.machine}</dd>
          </>
        )}
      </dl>
      {shell && (
        <p className="mt-3 text-xs">
          {req.host_paths.length > 0 ? (
            <>
              Commands in this VM can change files on this computer through its synced folders:{" "}
              {req.host_paths.map((p, i) => (
                <span key={p}>
                  {i > 0 && ", "}
                  <code className="font-mono selectable" title={p}>
                    {shortDir(p)}
                  </code>
                </span>
              ))}
              .
            </>
          ) : (
            "Boxdeck can't tell which host folders this VM can change, so it asks."
          )}
        </p>
      )}
      <p className="mt-3 text-xs text-muted">Exact arguments:</p>
      <pre className="mt-1 min-h-0 flex-1 overflow-auto rounded border border-border bg-sunken p-2 font-mono text-xs selectable">
        {req.arguments}
      </pre>
      {/* Wraps so the three shell buttons still fit the 480 px window. */}
      <div className="mt-4 flex flex-wrap items-center justify-end gap-2">
        <span className="mr-auto text-xs text-muted">Denied automatically in {left} s</span>
        <div className="flex gap-2">
          <Button onClick={() => answer(false)} autoFocus>
            Deny
          </Button>
          {shell ? (
            <>
              <Button onClick={() => answer(true)}>Allow once</Button>
              <Button variant="primary" onClick={() => answer(true, true)}>
                Allow for this session
              </Button>
            </>
          ) : (
            <Button variant="danger" onClick={() => answer(true)}>
              Allow
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
