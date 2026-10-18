import type { Action } from "./generated/Action";
import type { ActionOptions } from "./generated/ActionOptions";
import type { AppInfo } from "./generated/AppInfo";
import type { AuditEntry } from "./generated/AuditEntry";
import type { McpStatus } from "./generated/McpStatus";
import type { PendingApproval } from "./generated/PendingApproval";
import type { BoxInfo } from "./generated/BoxInfo";
import type { BoxOp } from "./generated/BoxOp";
import type { BoxUpdates } from "./generated/BoxUpdates";
import type { CatalogSearch } from "./generated/CatalogSearch";
import type { PopularBox } from "./generated/PopularBox";
import type { JobEvent } from "./generated/JobEvent";
import type { JobInfo } from "./generated/JobInfo";
import type { LogLine } from "./generated/LogLine";
import type { Project } from "./generated/Project";
import type { SaveResult } from "./generated/SaveResult";
import type { Settings } from "./generated/Settings";
import type { VagrantfileText } from "./generated/VagrantfileText";
import type { ValidateResult } from "./generated/ValidateResult";
import type { TrayRequest } from "./generated/TrayRequest";

export type Unsubscribe = () => void;

/** A terminal session's output and exit callbacks. */
export interface PtyHandlers {
  onData: (bytes: Uint8Array) => void;
  onExit: (code: number | null) => void;
}

/** Everything the UI asks of the backend. Implemented over Tauri IPC, and by a mock in dev/tests. */
export interface Api {
  appInfo(): Promise<AppInfo>;
  listMachines(): Promise<Project[]>;
  /** `auto` marks background refreshes, which back off while Vagrant is busy. */
  refreshStatus(projectDir?: string, auto?: boolean): Promise<Project[]>;
  runAction(
    projectDir: string,
    machine: string | null,
    action: Action,
    options?: ActionOptions,
  ): Promise<number>;
  cancelJob(id: number, force: boolean): Promise<void>;
  listJobs(): Promise<JobInfo[]>;
  jobLog(id: number, tail?: number): Promise<LogLine[]>;
  subscribeJobs(onEvent: (e: JobEvent) => void): Unsubscribe;
  onMachinesChanged(cb: () => void): Unsubscribe;
  /** Requests from tray menu items that need the window, e.g. opening an SSH tab. */
  onTrayRequest(cb: (r: TrayRequest) => void): Unsubscribe;
  sshOpen(
    projectDir: string,
    machine: string,
    cols: number,
    rows: number,
    h: PtyHandlers,
  ): Promise<number>;
  ptyRerunJob(jobId: number, cols: number, rows: number, h: PtyHandlers): Promise<number>;
  ptyWrite(id: number, data: string): Promise<void>;
  ptyResize(id: number, cols: number, rows: number): Promise<void>;
  ptyClose(id: number): Promise<void>;
  openFolder(projectDir: string): Promise<void>;
  listBoxes(): Promise<BoxInfo[]>;
  checkBoxUpdates(): Promise<BoxUpdates>;
  pruneDryRun(keepUsed: boolean): Promise<string>;
  runBoxOp(op: BoxOp): Promise<number>;
  /** Searches the public box catalog; never throws for network errors (see `error`). */
  searchBoxes(query: string, provider?: string): Promise<CatalogSearch>;
  popularBoxes(): Promise<PopularBox[]>;
  onBoxesChanged(cb: () => void): Unsubscribe;
  getSettings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<Settings>;
  /** Native folder picker; null when cancelled. */
  pickFolder(): Promise<string | null>;
  addBookmark(dir: string): Promise<Project[]>;
  removeBookmark(dir: string): Promise<Project[]>;
  readVagrantfile(projectDir: string): Promise<VagrantfileText>;
  saveVagrantfile(projectDir: string, base: string, content: string): Promise<SaveResult>;
  validateVagrantfile(projectDir: string): Promise<ValidateResult>;
  /** Returns the written path, or null when cancelled. */
  exportSettings(): Promise<string | null>;
  /** Returns validated settings for review (not yet saved), or null when cancelled. */
  importSettings(): Promise<Settings | null>;
  /** Looks for Vagrant again (after installing it, or changing its path). */
  redetectVagrant(): Promise<void>;
  mcpStatus(): Promise<McpStatus>;
  /** Returns the new client's token. */
  mcpAddClient(name: string): Promise<string>;
  mcpRotateClient(name: string): Promise<string>;
  mcpRemoveClient(name: string): Promise<void>;
  mcpClientToken(name: string): Promise<string>;
  /** Agent calls, newest first. */
  listAudit(limit?: number): Promise<AuditEntry[]>;
  onAuditChanged(cb: () => void): Unsubscribe;
  getApproval(id: number): Promise<PendingApproval | null>;
  answerApproval(id: number, allow: boolean): Promise<void>;
}

/** macOS app window with the title bar merged into the content (traffic lights over the UI). */
export function hasOverlayTitleBar(): boolean {
  return isTauri() && /Mac/i.test(navigator.platform);
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

let current: Api | null = null;

export function setApi(api: Api): void {
  current = api;
}

export function api(): Api {
  if (!current) throw new Error("API not initialised");
  return current;
}
