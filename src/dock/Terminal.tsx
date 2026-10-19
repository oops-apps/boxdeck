import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTerm, type ITheme } from "@xterm/xterm";
import { useEffect, useRef, useState } from "react";

function theme(): ITheme {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(name).trim() || undefined;
  // ANSI colors come from the app palette, for job logs and for shells in SSH tabs alike.
  return {
    background: v("--term-bg") ?? "#16161a",
    foreground: v("--term-fg") ?? "#e6e6ea",
    red: v("--term-red"),
    brightRed: v("--term-red"),
    green: v("--term-green"),
    brightGreen: v("--term-green"),
    yellow: v("--term-yellow"),
    brightYellow: v("--term-yellow"),
    blue: v("--term-blue"),
    brightBlue: v("--term-blue"),
    magenta: v("--term-magenta"),
    brightMagenta: v("--term-magenta"),
    brightBlack: v("--term-dim"),
  };
}

/**
 * An xterm.js terminal that fills its container. `onReady` gets the terminal once it has a
 * size; its cleanup runs on unmount.
 */
export function TerminalView({
  readOnly,
  onReady,
  label,
}: {
  readOnly: boolean;
  onReady: (term: XTerm, fit: FitAddon) => () => void;
  label: string;
}) {
  const host = useRef<HTMLDivElement>(null);
  // The session is set up once per mount, so the first callback is the one that counts.
  const [ready] = useState(() => onReady);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const term = new XTerm({
      fontFamily: getComputedStyle(document.documentElement).getPropertyValue("--font-mono"),
      fontSize: 12,
      cursorBlink: !readOnly,
      disableStdin: readOnly,
      convertEol: readOnly,
      scrollback: 10_000,
      theme: theme(),
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    if (term.textarea) term.textarea.setAttribute("aria-label", label);
    fit.fit();
    const cleanup = ready(term, fit);
    // The theme is read once from CSS, so follow the system switching light/dark while open.
    const scheme = window.matchMedia?.("(prefers-color-scheme: dark)");
    const retheme = () => (term.options.theme = theme());
    scheme?.addEventListener("change", retheme);
    const observer = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {
        // The container can be momentarily detached while tabs switch.
      }
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      scheme?.removeEventListener("change", retheme);
      cleanup();
      term.dispose();
    };
  }, [readOnly, label, ready]);

  // Padding lives on the wrapper: FitAddon sizes the terminal to its direct parent.
  return (
    <div className="h-full w-full bg-[var(--term-bg)] py-1.5 pl-2">
      <div ref={host} className="h-full w-full" />
    </div>
  );
}
