import { StreamLanguage } from "@codemirror/language";
import { ruby } from "@codemirror/legacy-modes/mode/ruby";
import CodeMirror from "@uiw/react-codemirror";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState } from "react";
import { useRunAction } from "../hooks";
import { api } from "../ipc/api";
import type { ValidateResult } from "../ipc/generated/ValidateResult";
import { useUi } from "../state/store";
import { Button, Icon, cn } from "../components/ui";
import { shortDir } from "../lib/display";

const rubyLang = StreamLanguage.define(ruby);

function useDark(): boolean {
  const query = "(prefers-color-scheme: dark)";
  const [dark, setDark] = useState(() => window.matchMedia?.(query).matches ?? false);
  useEffect(() => {
    const mq = window.matchMedia?.(query);
    if (!mq) return;
    const on = (e: MediaQueryListEvent) => setDark(e.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return dark;
}

export function VagrantfileView() {
  const dir = useUi((s) => s.editing);
  const setView = useUi((s) => s.setView);
  if (!dir) return null;
  return <Editor key={dir} dir={dir} onClose={() => setView("machines")} />;
}

function Editor({ dir, onClose }: { dir: string; onClose: () => void }) {
  const file = useQuery({
    queryKey: ["vagrantfile", dir],
    queryFn: () => api().readVagrantfile(dir),
    gcTime: 0,
    staleTime: Infinity,
  });
  const [base, setBase] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [saving, setSaving] = useState(false);
  const [result, setResult] = useState<
    | { kind: "saved"; validation: ValidateResult }
    | { kind: "validated"; validation: ValidateResult }
    | { kind: "error"; message: string }
    | null
  >(null);
  const run = useRunAction();
  const dark = useDark();
  const extensions = useMemo(() => [rubyLang], []);

  if (file.data && base === null) {
    setBase(file.data.content);
    setText(file.data.content);
  }

  const dirty = base !== null && text !== base;
  const problem = result !== null && (result.kind === "error" || !result.validation.ok);

  const save = async () => {
    if (base === null) return;
    setSaving(true);
    try {
      const r = await api().saveVagrantfile(dir, base, text);
      setBase(text);
      setResult({ kind: "saved", validation: r.validation });
    } catch (e) {
      setResult({ kind: "error", message: String(e) });
    } finally {
      setSaving(false);
    }
  };

  const reload = async () => {
    const fresh = await file.refetch();
    if (fresh.data) {
      setBase(fresh.data.content);
      setText(fresh.data.content);
      setResult(null);
    }
  };

  return (
    <div
      className="flex h-full flex-col"
      onKeyDown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key === "s") {
          e.preventDefault();
          if (dirty && !saving) void save();
        }
      }}
    >
      <header
        data-tauri-drag-region="deep"
        className="flex items-center gap-2 border-b border-border h-12 shrink-0 px-4"
      >
        <Button
          variant="ghost"
          onClick={onClose}
          aria-label="Back to machines"
          className="w-7 px-0"
        >
          <Icon name="chevron" className="size-3.5 rotate-180" />
        </Button>
        <div className="min-w-0">
          <h1 className="text-[15px] font-semibold">Vagrantfile</h1>
          <p className="truncate text-xs text-muted selectable" title={file.data?.path}>
            {file.data ? file.data.path : shortDir(dir)}
          </p>
        </div>
        <div className="ml-auto flex items-center gap-2">
          {dirty && <span className="text-xs text-muted">Unsaved changes</span>}
          <Button onClick={() => void reload()}>Reload</Button>
          <Button
            onClick={async () => {
              try {
                setResult({ kind: "validated", validation: await api().validateVagrantfile(dir) });
              } catch (e) {
                setResult({ kind: "error", message: String(e) });
              }
            }}
            disabled={dirty}
            title={dirty ? "Save first; Vagrant validates the file on disk" : undefined}
          >
            Validate
          </Button>
          <Button variant="primary" onClick={() => void save()} disabled={!dirty || saving}>
            {saving ? "Saving…" : "Save"}
          </Button>
        </div>
      </header>
      {result && (
        <div
          role={problem ? "alert" : "status"}
          className={cn(
            "border-b border-border px-4 py-2 text-xs",
            problem ? "bg-danger/10" : "bg-ok/10",
          )}
        >
          {result.kind === "error" ? (
            <p className="text-danger selectable">{result.message}</p>
          ) : (
            <>
              <p className={cn("font-medium", result.validation.ok ? "text-ok" : "text-danger")}>
                {result.kind === "saved" ? "Saved. " : ""}
                {result.validation.ok
                  ? "Vagrant says the file is valid."
                  : "Vagrant found a problem:"}
              </p>
              {!result.validation.ok && (
                <pre className="mt-1 font-mono whitespace-pre-wrap selectable">
                  {result.validation.output}
                </pre>
              )}
              {result.kind === "saved" && result.validation.ok && (
                <p className="mt-1 text-muted">
                  Running machines pick up changes on reload.{" "}
                  <button
                    type="button"
                    className="text-accent underline"
                    onClick={() => run.mutate({ dir, machine: null, action: "reload" })}
                  >
                    Reload all machines in this project
                  </button>
                </p>
              )}
            </>
          )}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto">
        {file.isLoading && <p className="p-4 text-muted">Opening…</p>}
        {file.error && (
          <p role="alert" className="p-4 text-danger selectable">
            {String(file.error)}
          </p>
        )}
        {base !== null && (
          <CodeMirror
            value={text}
            onChange={setText}
            extensions={extensions}
            theme={dark ? "dark" : "light"}
            height="100%"
            className="h-full text-[13px]"
            aria-label="Vagrantfile contents"
            basicSetup={{ foldGutter: false, highlightActiveLine: true }}
          />
        )}
      </div>
    </div>
  );
}
