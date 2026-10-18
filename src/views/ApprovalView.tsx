import { useEffect, useState } from "react";
import { api } from "../ipc/api";
import type { PendingApproval } from "../ipc/generated/PendingApproval";
import { Button, Icon } from "../components/ui";
import { shortDir } from "../lib/display";

/** The small window that asks before an agent runs a destructive tool. */
export function ApprovalView({ id }: { id: number }) {
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
  const answer = (allow: boolean) => void api().answerApproval(id, allow);

  return (
    <div className="flex h-full flex-col bg-panel p-5">
      <div className="flex items-center gap-2">
        <Icon name="warning" className="size-5 text-warn" />
        <h1 className="text-[15px] font-semibold">An agent wants to make a change</h1>
      </div>
      <dl className="mt-4 grid grid-cols-[80px_1fr] gap-x-3 gap-y-1.5">
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
      <p className="mt-3 text-xs text-muted">Exact arguments:</p>
      <pre className="mt-1 min-h-0 flex-1 overflow-auto rounded border border-border bg-sunken p-2 font-mono text-xs selectable">
        {req.arguments}
      </pre>
      <div className="mt-4 flex items-center gap-2">
        <span className="text-xs text-muted">Denied automatically in {left} s</span>
        <div className="ml-auto flex gap-2">
          <Button onClick={() => answer(false)} autoFocus>
            Deny
          </Button>
          <Button variant="danger" onClick={() => answer(true)}>
            Allow
          </Button>
        </div>
      </div>
    </div>
  );
}
