import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Api, PtyHandlers } from "./api";
import type { AppUpdate } from "./generated/AppUpdate";
import type { JobEvent } from "./generated/JobEvent";
import type { TrayRequest } from "./generated/TrayRequest";
import type { UpdateProgress } from "./generated/UpdateProgress";

function ptyChannels(h: PtyHandlers) {
  const onData = new Channel<ArrayBuffer>();
  onData.onmessage = (buf) => h.onData(new Uint8Array(buf));
  const onExit = new Channel<number | null>();
  onExit.onmessage = (code) => h.onExit(code);
  return { onData, onExit };
}

export const tauriApi: Api = {
  appInfo: () => invoke("app_info"),
  listMachines: () => invoke("list_machines"),
  refreshStatus: (projectDir, auto) =>
    invoke("refresh_status", { projectDir: projectDir ?? null, auto: auto ?? false }),
  runAction: (projectDir, machine, action, options) =>
    invoke("run_action", { projectDir, machine, action, options: options ?? null }),
  cancelJob: (id, force) => invoke("cancel_job", { id, force }),
  listJobs: () => invoke("list_jobs"),
  jobLog: (id, tail) => invoke("job_log", { id, tail: tail ?? null }),
  subscribeJobs(onEvent) {
    const channel = new Channel<JobEvent>();
    let active = true;
    channel.onmessage = (e) => {
      if (active) onEvent(e);
    };
    void invoke("subscribe_jobs", { onEvent: channel });
    // Dropping our handler is enough: the backend stops once sends fail.
    return () => {
      active = false;
    };
  },
  onMachinesChanged(cb) {
    const un = listen("machines-changed", () => cb());
    return () => {
      void un.then((f) => f());
    };
  },
  onTrayRequest(cb) {
    const un = listen<TrayRequest>("tray-request", (e) => cb(e.payload));
    return () => {
      void un.then((f) => f());
    };
  },
  sshOpen: (projectDir, machine, cols, rows, h) =>
    invoke("ssh_open", { projectDir, machine, cols, rows, ...ptyChannels(h) }),
  ptyRerunJob: (jobId, cols, rows, h) =>
    invoke("pty_rerun_job", { jobId, cols, rows, ...ptyChannels(h) }),
  ptyWrite: (id, data) => invoke("pty_write", { id, data }),
  ptyResize: (id, cols, rows) => invoke("pty_resize", { id, cols, rows }),
  ptyClose: (id) => invoke("pty_close", { id }),
  openFolder: (projectDir) => invoke("open_folder", { projectDir }),
  listBoxes: () => invoke("list_boxes"),
  checkBoxUpdates: () => invoke("check_box_updates"),
  pruneDryRun: (keepUsed) => invoke("prune_dry_run", { keepUsed }),
  runBoxOp: (op) => invoke("run_box_op", { op }),
  popularBoxes: () => invoke("popular_boxes"),
  searchBoxes: (query, provider) => invoke("search_boxes", { query, provider: provider || null }),
  getSettings: () => invoke("get_settings"),
  saveSettings: (settings) => invoke("save_settings", { settings }),
  pickFolder: () => invoke("pick_folder"),
  addBookmark: (dir) => invoke("add_bookmark", { dir }),
  removeBookmark: (dir) => invoke("remove_bookmark", { dir }),
  readVagrantfile: (projectDir) => invoke("read_vagrantfile", { projectDir }),
  saveVagrantfile: (projectDir, base, content) =>
    invoke("save_vagrantfile", { projectDir, base, content }),
  validateVagrantfile: (projectDir) => invoke("validate_vagrantfile", { projectDir }),
  exportSettings: () => invoke("export_settings"),
  importSettings: () => invoke("import_settings"),
  redetectVagrant: () => invoke("redetect_vagrant"),
  mcpClients: () => invoke("mcp_clients"),
  mcpConnect: (kind) => invoke("mcp_connect", { kind }),
  mcpDisconnect: (kind) => invoke("mcp_disconnect", { kind }),
  mcpGrants: () => invoke("mcp_grants"),
  mcpRevokeGrant: (project, machine) => invoke("mcp_revoke_grant", { project, machine }),
  listAudit: (limit) => invoke("list_audit", { limit: limit ?? null }),
  onAuditChanged(cb) {
    const un = listen("audit-changed", () => cb());
    return () => {
      void un.then((f) => f());
    };
  },
  getApproval: (id) => invoke("get_approval", { id }),
  answerApproval: (id, allow, session) =>
    invoke("answer_approval", { id, allow, session: session ?? null }),
  pendingAppUpdate: () => invoke("pending_app_update"),
  checkAppUpdate: () => invoke("check_app_update"),
  installAppUpdate(onProgress) {
    const channel = new Channel<UpdateProgress>();
    channel.onmessage = onProgress;
    return invoke("install_app_update", { onProgress: channel });
  },
  onAppUpdateAvailable(cb) {
    const un = listen<AppUpdate>("update-available", (e) => cb(e.payload));
    return () => {
      void un.then((f) => f());
    };
  },
  onBoxesChanged(cb) {
    const un = listen("boxes-changed", () => cb());
    return () => {
      void un.then((f) => f());
    };
  },
};
