import type { ProjectSettings } from "../ipc/generated/ProjectSettings";

/** Home-relative folder for display. */
export function shortDir(dir: string): string {
  return dir.replace(/^\/(Users|home)\/[^/]+/, "~").replace(/^[A-Z]:\\Users\\[^\\]+/i, "~");
}

export const COLORS = ["red", "orange", "yellow", "green", "blue", "purple", "gray"] as const;

export const COLOR_CLASS: Record<string, string> = {
  red: "bg-[#e5484d]",
  orange: "bg-[#f76b15]",
  yellow: "bg-[#e2a336]",
  green: "bg-[#30a46c]",
  blue: "bg-[#3e63dd]",
  purple: "bg-[#8e4ec6]",
  gray: "bg-[#8b8d98]",
};

export function emptyProject(): ProjectSettings {
  return {
    alias: null,
    group: null,
    color: null,
    pinned: false,
    hidden: false,
    env: {},
    up: { provider: null, provision: null },
    agent_access: "inherit",
    shell: null,
    machines: {},
  };
}

export function formatBytes(n: number | null): string {
  if (n === null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 10 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}
