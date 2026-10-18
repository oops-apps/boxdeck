import * as Dialog from "@radix-ui/react-dialog";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { api } from "../ipc/api";
import { useAppInfo, useMachines, useSettings } from "../hooks";
import type { Project } from "../ipc/generated/Project";
import type { BoxInfo } from "../ipc/generated/BoxInfo";
import type { BoxOp } from "../ipc/generated/BoxOp";
import type { BoxUpdate } from "../ipc/generated/BoxUpdate";
import { useUi } from "../state/store";
import { Button, ConfirmDialog, Icon, IconButton, Menu, cn } from "../components/ui";
import { formatBytes, shortDir } from "../lib/display";

const updateKey = (name: string, provider: string) => `${name}\u0000${provider}`;

function useBoxes() {
  const qc = useQueryClient();
  useEffect(
    () => api().onBoxesChanged(() => void qc.invalidateQueries({ queryKey: ["boxes"] })),
    [qc],
  );
  return useQuery({ queryKey: ["boxes"], queryFn: () => api().listBoxes() });
}

function useBoxOp() {
  const openTab = useUi((s) => s.openTab);
  return useMutation({
    mutationFn: (op: BoxOp) => api().runBoxOp(op),
    onSuccess: (jobId) => openTab({ key: `job-${jobId}`, kind: "job", jobId }),
  });
}

/** Other architectures installed for the same box, version and provider. */
function siblingsIn(boxes: BoxInfo[], b: BoxInfo): BoxInfo[] {
  return boxes.filter(
    (o) =>
      o !== b &&
      o.name === b.name &&
      o.version === b.version &&
      o.provider === b.provider &&
      o.architecture !== b.architecture,
  );
}

export function BoxesView() {
  const { data: boxes, isLoading, error } = useBoxes();
  const run = useBoxOp();
  const updates = useMutation({ mutationFn: () => api().checkBoxUpdates() });
  const [removing, setRemoving] = useState<BoxInfo | null>(null);
  const [pruning, setPruning] = useState(false);
  const [adding, setAdding] = useState(false);

  const byKey = useMemo(() => {
    const m = new Map<string, BoxUpdate>();
    for (const u of updates.data?.updates ?? []) m.set(updateKey(u.name, u.provider), u);
    return m;
  }, [updates.data]);

  const sorted = useMemo(
    () =>
      [...(boxes ?? [])].sort(
        (a, b) =>
          a.name.localeCompare(b.name) ||
          a.provider.localeCompare(b.provider) ||
          b.version.localeCompare(a.version, undefined, { numeric: true }),
      ),
    [boxes],
  );
  const siblings = (b: BoxInfo) => siblingsIn(boxes ?? [], b);
  const { data: settings } = useSettings();
  const { data: projects } = useMachines();
  const defaultProvider = useMemo(
    () => guessProvider(settings?.env, boxes ?? [], projects ?? []),
    [settings, boxes, projects],
  );
  const total = (boxes ?? []).reduce((n, b) => n + (b.size_bytes ?? 0), 0);

  return (
    <div className="flex h-full flex-col">
      <header
        data-tauri-drag-region="deep"
        className="flex items-center gap-2 border-b border-border h-12 shrink-0 px-4"
      >
        <h1 className="shrink-0 text-[15px] font-semibold">Boxes</h1>
        <span className="min-w-0 truncate text-xs text-muted">
          {boxes?.length ?? 0} installed · {formatBytes(total)}
        </span>
        <div className="ml-auto flex shrink-0 items-center gap-2">
          <Button onClick={() => updates.mutate()} disabled={updates.isPending}>
            <Icon name="refresh" className={cn(updates.isPending && "animate-spin")} />
            {updates.isPending ? "Checking…" : "Check for updates"}
          </Button>
          <Button onClick={() => setPruning(true)} disabled={!boxes?.length}>
            Prune…
          </Button>
          <Button variant="primary" onClick={() => setAdding(true)}>
            <Icon name="plus" className="size-3.5" /> Add box
          </Button>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-auto">
        {isLoading && <p className="px-4 py-3 text-muted">Listing boxes…</p>}
        {error && (
          <p role="alert" className="px-4 py-3 text-danger selectable">
            {String(error)}
          </p>
        )}
        {updates.error && (
          <p role="alert" className="px-4 py-2 text-danger selectable">
            Couldn't check for updates: {String(updates.error)}
          </p>
        )}
        {boxes && boxes.length === 0 && (
          <div className="mx-auto mt-16 max-w-sm text-center">
            <Icon name="stack" className="mx-auto size-8 text-muted" />
            <p className="mt-3 font-medium">No boxes installed</p>
            <p className="mt-1 text-muted">
              Boxes are downloaded on your first <code className="font-mono">vagrant up</code>, or
              you can add one now.
            </p>
          </div>
        )}
        {sorted.length > 0 && (
          <table className="w-full border-collapse text-left whitespace-nowrap">
            <thead className="sticky top-0 bg-bg text-xs text-muted">
              <tr className="border-b border-border">
                <th className="px-4 py-1.5 font-medium">Box</th>
                <th className="px-2 py-1.5 font-medium">Version</th>
                <th className="px-2 py-1.5 font-medium">Provider</th>
                <th className="px-2 py-1.5 font-medium max-lg:hidden">Arch</th>
                <th className="px-2 py-1.5 text-right font-medium">Size</th>
                <th className="px-2 py-1.5 font-medium">Used by</th>
                <th className="px-2 py-1.5 font-medium">Updates</th>
                <th className="sticky right-0 bg-bg px-4 py-1.5" />
              </tr>
            </thead>
            <tbody>
              {sorted.map((b) => {
                const u = byKey.get(updateKey(b.name, b.provider));
                const outdated = u?.status.status === "outdated" && u.status.current === b.version;
                return (
                  <tr
                    key={`${b.name}|${b.provider}|${b.version}|${b.architecture}`}
                    className="group border-b border-border hover:bg-sunken/60"
                  >
                    <td
                      className="max-w-56 truncate px-4 py-1.5 font-medium selectable"
                      title={b.name}
                    >
                      {b.name}
                    </td>
                    <td className="px-2 py-1.5 font-mono text-xs selectable">{b.version}</td>
                    <td className="px-2 py-1.5">{b.provider}</td>
                    <td className="px-2 py-1.5 text-muted max-lg:hidden">
                      {b.architecture ?? "—"}
                    </td>
                    <td className="px-2 py-1.5 text-right tabular-nums text-muted">
                      {formatBytes(b.size_bytes)}
                    </td>
                    <td className="max-w-40 truncate px-2 py-1.5">
                      {b.used_by.length === 0 ? (
                        <span className="text-muted">unused</span>
                      ) : (
                        <span
                          title={b.used_by.map((u) => `${u.machine} (${u.project})`).join("\n")}
                        >
                          {b.used_by.map((u) => u.machine).join(", ")}
                        </span>
                      )}
                    </td>
                    <td className="px-2 py-1.5">
                      <UpdateCell update={u} version={b.version} />
                    </td>
                    <td className="sticky right-0 bg-bg px-4 py-1.5 text-right group-hover:bg-[color-mix(in_srgb,var(--sunken)_60%,var(--bg))]">
                      {outdated && (
                        <Button
                          size="sm"
                          onClick={() =>
                            run.mutate({
                              op: "update",
                              name: b.name,
                              provider: b.provider,
                              architecture: b.architecture,
                            })
                          }
                        >
                          Update
                        </Button>
                      )}
                      <Menu
                        trigger={
                          <IconButton
                            icon="more"
                            label={`More actions for ${b.name} ${b.version}`}
                          />
                        }
                        items={[
                          {
                            label: "Remove this version…",
                            danger: true,
                            onSelect: () => setRemoving(b),
                          },
                        ]}
                      />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        {updates.data && (
          <details className="px-4 py-3 text-xs">
            <summary className="cursor-pointer text-muted">Vagrant's output</summary>
            <pre className="mt-2 font-mono whitespace-pre-wrap selectable">{updates.data.raw}</pre>
          </details>
        )}
      </div>

      <ConfirmDialog
        open={removing !== null}
        danger
        title={`Remove ${removing?.name} ${removing?.version}?`}
        confirmLabel={removing?.used_by.length ? "Remove anyway" : "Remove"}
        body={
          removing?.used_by.length ? (
            <>
              <p>
                In use by{" "}
                {removing.used_by.map((u) => `${u.machine} (${shortDir(u.project)})`).join(", ")}.
              </p>
              <p className="mt-2">
                Those machines keep running, but Vagrant may download the box again the next time
                they're recreated.
              </p>
            </>
          ) : (
            "The box files are deleted from disk. You can add it again later."
          )
        }
        footer={
          removing && siblings(removing).length > 0 ? (
            <p className="mt-3 text-warn">
              Vagrant removes every architecture of this version together (
              {[removing, ...siblings(removing)].map((b) => b.architecture ?? "none").join(", ")}).
            </p>
          ) : null
        }
        onCancel={() => setRemoving(null)}
        onConfirm={() => {
          if (removing) {
            run.mutate({
              op: "remove",
              name: removing.name,
              version: removing.version,
              provider: removing.provider,
              all_architectures: siblings(removing).length > 0,
            });
          }
          setRemoving(null);
        }}
      />
      {pruning && (
        <PruneDialog
          onClose={() => setPruning(false)}
          onPrune={(keepUsed) => {
            setPruning(false);
            run.mutate({ op: "prune", keep_used: keepUsed });
          }}
        />
      )}
      {adding && (
        <AddBoxDialog
          defaultProvider={defaultProvider}
          onClose={() => setAdding(false)}
          onAdd={(op) => {
            setAdding(false);
            run.mutate(op);
          }}
        />
      )}
    </div>
  );
}

function UpdateCell({ update, version }: { update?: BoxUpdate; version: string }) {
  if (!update) return <span className="text-muted">—</span>;
  const s = update.status;
  switch (s.status) {
    case "up_to_date":
      return <span className="text-ok">Up to date</span>;
    case "outdated":
      return s.current === version ? (
        <span className="font-medium text-warn">{s.latest} available</span>
      ) : (
        <span className="text-muted">older version</span>
      );
    case "no_metadata":
      return <span className="text-muted">Not from a catalog</span>;
    case "error":
      return (
        <span className="text-danger" title={s.message}>
          Check failed
        </span>
      );
  }
}

function PruneDialog({
  onClose,
  onPrune,
}: {
  onClose: () => void;
  onPrune: (keepUsed: boolean) => void;
}) {
  const [keepUsed, setKeepUsed] = useState(true);
  const preview = useQuery({
    queryKey: ["prune-dry-run", keepUsed],
    queryFn: () => api().pruneDryRun(keepUsed),
    gcTime: 0,
  });
  return (
    <ConfirmDialog
      open
      danger
      title="Prune old box versions?"
      confirmLabel="Prune"
      onCancel={onClose}
      onConfirm={() => onPrune(keepUsed)}
      body={
        <div className="text-text">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={keepUsed}
              onChange={(e) => setKeepUsed(e.target.checked)}
              className="accent-accent"
            />
            Keep versions still used by a machine
          </label>
          <p className="mt-3 text-xs text-muted">Dry run (nothing has been removed yet):</p>
          <pre className="mt-1 max-h-56 overflow-auto rounded border border-border bg-sunken p-2 font-mono text-xs whitespace-pre-wrap selectable">
            {preview.isLoading
              ? "Asking Vagrant…"
              : preview.error
                ? String(preview.error)
                : preview.data}
          </pre>
        </div>
      }
    />
  );
}

function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return debounced;
}

function shortCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}k`;
  return String(n);
}

const PROVIDER_NAMES: Record<string, string> = {
  vmware_desktop: "VMware",
  virtualbox: "VirtualBox",
  parallels: "Parallels",
  utm: "UTM",
  libvirt: "libvirt",
  hyperv: "Hyper-V",
  docker: "Docker",
};

/** Why a box won't run here, or null if it should. Boxes without architecture data are amd64. */
function mismatch(
  providers: string[],
  architectures: string[],
  provider: string,
  host: string | null,
): string | null {
  if (provider && !providers.includes(provider)) {
    return `${providers.map((p) => PROVIDER_NAMES[p] ?? p).join(", ")} only`;
  }
  const archs = architectures.length ? architectures : ["amd64"];
  if (host && !archs.includes(host)) return `no ${host} build`;
  return null;
}

/** Catalog search results under the Source field; picking one fills it in. */
function CatalogPicker({
  query,
  provider,
  onPick,
}: {
  query: string;
  provider: string;
  onPick: (name: string) => void;
}) {
  const q = useDebounced(query.trim(), 400);
  const { data: info } = useAppInfo();
  const popularQuery = useQuery({
    queryKey: ["popular-boxes"],
    queryFn: () => api().popularBoxes(),
    staleTime: Infinity,
  });
  const looksLikePath = /^(https?:\/\/|\/|[A-Za-z]:\\)/.test(q);
  const search = useQuery({
    queryKey: ["box-search", q, provider],
    queryFn: () => api().searchBoxes(q, provider || undefined),
    enabled: q.length >= 2 && !looksLikePath,
    staleTime: 5 * 60_000,
  });
  if (looksLikePath) return null;
  const host = info?.host_architecture ?? null;

  if (q.length < 2 || search.data?.error) {
    const popular = search.data?.popular ?? popularQuery.data ?? [];
    return (
      <div className="mt-1.5">
        {search.data?.error ? (
          <p className="mb-1 text-xs text-muted">
            <span title={search.data.error}>Couldn't search the catalog</span> (offline, or the
            catalog rejected this search; a narrower one like{" "}
            <code className="font-mono">bento/debian</code> often works). Popular boxes:
          </p>
        ) : (
          <p className="mb-1 text-xs text-muted">Popular:</p>
        )}
        <div className="flex flex-wrap gap-1">
          {popular.map((p) => {
            const why = mismatch(p.providers, p.architectures, provider, host);
            return (
              <button
                key={p.name}
                type="button"
                onClick={() => onPick(p.name)}
                title={why ? `${p.name}: ${why}` : p.name}
                className={cn(
                  "rounded border border-border px-1.5 py-0.5 text-xs hover:border-accent hover:bg-[var(--hover)]",
                  why && "opacity-55",
                )}
              >
                {p.label}
                {why && <span className="ml-1 text-warn">· {why}</span>}
              </button>
            );
          })}
        </div>
      </div>
    );
  }
  if (search.isLoading) return <p className="mt-1.5 text-xs text-muted">Searching the catalog…</p>;
  const boxes = search.data?.boxes ?? [];
  if (boxes.length === 0) {
    return (
      <p className="mt-1.5 text-xs text-muted">
        No catalog boxes match{provider && ` for ${provider}`}.
      </p>
    );
  }
  // Once a result is picked, the list has done its job.
  if (boxes.some((b) => b.name === q)) return null;
  return (
    <ul
      aria-label="Catalog results"
      className="mt-1.5 max-h-48 overflow-auto rounded-md border border-border bg-panel"
    >
      {boxes.slice(0, 8).map((b) => {
        const why = mismatch(b.providers, b.architectures, "", host);
        return (
          <li key={b.name}>
            <button
              type="button"
              onClick={() => onPick(b.name)}
              className={cn(
                "flex w-full items-center gap-2 border-b border-border px-2 py-1.5 text-left hover:bg-sunken",
                why && "opacity-60",
              )}
            >
              <span className="min-w-0 flex-1 truncate">
                <span className="font-medium">{b.name}</span>
                <span className="ml-1.5 font-mono text-xs text-muted">{b.version}</span>
              </span>
              {why && <span className="text-xs text-warn">{why}</span>}
              <span
                className="text-xs text-muted"
                title={`${b.downloads.toLocaleString()} downloads`}
              >
                {shortCount(b.downloads)}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

const PROVIDERS = ["vmware_desktop", "virtualbox", "parallels", "libvirt", "hyperv", "docker"];

/** The provider people most likely want: their configured default, else what they already use. */
export function guessProvider(
  env: Record<string, string> | undefined,
  boxes: BoxInfo[],
  projects: Project[],
): string {
  const configured = env?.VAGRANT_DEFAULT_PROVIDER?.trim();
  if (configured) return configured;
  const counts = new Map<string, number>();
  const used = [
    ...boxes.map((b) => b.provider),
    ...projects.flatMap((p) => p.machines.map((m) => m.provider)),
  ].filter(Boolean);
  for (const p of used) counts.set(p, (counts.get(p) ?? 0) + 1);
  return [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "";
}

function AddBoxDialog({
  defaultProvider,
  onClose,
  onAdd,
}: {
  defaultProvider: string;
  onClose: () => void;
  onAdd: (op: BoxOp) => void;
}) {
  const [source, setSource] = useState("");
  const [name, setName] = useState("");
  const [provider, setProvider] = useState(defaultProvider);
  const [architecture, setArchitecture] = useState("");
  const [version, setVersion] = useState("");
  const [checksumType, setChecksumType] = useState("sha256");
  const [checksum, setChecksum] = useState("");
  const [force, setForce] = useState(false);
  const isCatalog = /^[A-Za-z0-9][\w.-]*(\/[A-Za-z0-9][\w.-]*)?$/.test(source.trim());
  const needsName = source.trim() !== "" && !isCatalog;
  // Catalog boxes usually come for several providers, and Vagrant can't ask without a terminal.
  const needsProvider = isCatalog && provider === "";
  const valid = source.trim() !== "" && (!needsName || name.trim() !== "") && !needsProvider;

  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (!valid) return;
    const opt = (v: string) => (v.trim() === "" ? null : v.trim());
    onAdd({
      op: "add",
      source: source.trim(),
      name: opt(name),
      provider: opt(provider),
      architecture: opt(architecture),
      version: isCatalog ? opt(version) : null,
      checksum: opt(checksum) ? [checksumType, checksum.trim()] : null,
      force,
    });
  };

  const field =
    "h-7 w-full rounded-md border border-border bg-panel px-2 outline-none focus:border-accent";
  return (
    <Dialog.Root open onOpenChange={(o) => !o && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="anim-overlay fixed inset-0 z-40 bg-black/30" />
        <Dialog.Content className="anim-dialog fixed top-1/2 left-1/2 z-50 w-[480px] max-w-[calc(100vw-32px)] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-panel p-5 shadow-xl">
          <Dialog.Title className="text-[15px] font-semibold">Add a box</Dialog.Title>
          <Dialog.Description className="mt-1 text-muted">
            From Vagrant's catalog (like <code className="font-mono">bento/ubuntu-24.04</code>), a
            URL, or a <code className="font-mono">.box</code> file path.
          </Dialog.Description>
          <form
            onSubmit={submit}
            className="mt-4 grid grid-cols-[110px_1fr] items-center gap-x-3 gap-y-2.5"
          >
            <label htmlFor="box-source" className="self-start pt-1.5">
              Source
            </label>
            <div>
              <input
                id="box-source"
                autoFocus
                value={source}
                onChange={(e) => setSource(e.target.value)}
                placeholder="Search, e.g. ubuntu, or paste a URL"
                className={cn(field, "w-full")}
              />
              <CatalogPicker query={source} provider={provider} onPick={setSource} />
            </div>
            <label htmlFor="box-name">
              Name{needsName && <span className="text-danger"> *</span>}
            </label>
            <input
              id="box-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={needsName ? "Required for a URL or file" : "Optional"}
              className={field}
            />
            <label htmlFor="box-provider">
              Provider{isCatalog && <span className="text-danger"> *</span>}
            </label>
            <select
              id="box-provider"
              value={provider}
              onChange={(e) => setProvider(e.target.value)}
              className={field}
            >
              <option value="">{isCatalog ? "Choose a provider" : "From the box file"}</option>
              {[...new Set([...(defaultProvider ? [defaultProvider] : []), ...PROVIDERS])].map(
                (p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ),
              )}
            </select>
            <label htmlFor="box-arch">Architecture</label>
            <input
              id="box-arch"
              value={architecture}
              onChange={(e) => setArchitecture(e.target.value)}
              placeholder="This computer's, e.g. arm64"
              className={field}
            />
            {isCatalog && (
              <>
                <label htmlFor="box-version">Version</label>
                <input
                  id="box-version"
                  value={version}
                  onChange={(e) => setVersion(e.target.value)}
                  placeholder="Latest"
                  className={field}
                />
              </>
            )}
            <label htmlFor="box-checksum">Checksum</label>
            <div className="flex gap-2">
              <select
                aria-label="Checksum type"
                value={checksumType}
                onChange={(e) => setChecksumType(e.target.value)}
                className="h-7 rounded-md border border-border bg-panel px-1"
              >
                {["md5", "sha1", "sha256", "sha384", "sha512"].map((t) => (
                  <option key={t}>{t}</option>
                ))}
              </select>
              <input
                id="box-checksum"
                value={checksum}
                onChange={(e) => setChecksum(e.target.value)}
                placeholder="Optional"
                className={cn(field, "font-mono")}
              />
            </div>
            <span />
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={force}
                onChange={(e) => setForce(e.target.checked)}
                className="accent-accent"
              />
              Replace if already installed
            </label>
            <div className="col-span-2 mt-3 flex justify-end gap-2">
              <Button onClick={onClose}>Cancel</Button>
              <Button type="submit" variant="primary" disabled={!valid}>
                Add box
              </Button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
