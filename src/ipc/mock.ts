// In-memory backend for `npm run dev` in a plain browser and for tests. Simulates jobs that
// stream output and change machine state, and a terminal that echoes input.
import type { Api, PtyHandlers } from "./api";
import type { Action } from "./generated/Action";
import type { AppUpdate } from "./generated/AppUpdate";
import type { AuditEntry } from "./generated/AuditEntry";
import type { BoxInfo } from "./generated/BoxInfo";
import type { PopularBox } from "./generated/PopularBox";
import type { JobEvent } from "./generated/JobEvent";
import type { JobInfo } from "./generated/JobInfo";
import type { LogLine } from "./generated/LogLine";
import type { Machine } from "./generated/Machine";
import type { Project } from "./generated/Project";
import type { Settings } from "./generated/Settings";

const LONG: Record<string, string> = {
  running: "The VM is running. To stop this VM, you can run `vagrant halt` to\nshut it down.",
  poweroff: "The VM is powered off. To restart the VM, simply run `vagrant up`",
  saved: "The VM is in a suspended state. To resume, run `vagrant resume`.",
  not_created:
    "The environment has not yet been created. Run `vagrant up` to\ncreate the environment.",
};

function machine(name: string, provider: string, state: string): Machine {
  return {
    name,
    provider,
    state,
    state_short: state.replace("_", " "),
    state_long: LONG[state] ?? "",
    source: "live",
    index_id: state === "not_created" ? null : `${name}0000`,
  };
}

function seed(): Project[] {
  const now = Date.now();
  return [
    {
      env: { dir: "/Users/dev/work/api", vagrantfile: null, dotfile_path: null },
      machines: [
        machine("web", "vmware_desktop", "running"),
        machine("db", "vmware_desktop", "poweroff"),
        machine("cache", "vmware_desktop", "not_created"),
      ],
      checked_at: now - 120_000,
      error: null,
      bookmarked: false,
    },
    {
      env: { dir: "/Users/dev/clients/shop", vagrantfile: null, dotfile_path: null },
      machines: [machine("default", "virtualbox", "saved")],
      checked_at: now - 30_000,
      error: null,
      bookmarked: false,
    },
    {
      env: { dir: "/Users/dev/old/legacy", vagrantfile: null, dotfile_path: null },
      machines: [{ ...machine("default", "virtualbox", "running"), source: "index" }],
      checked_at: now - 600_000,
      error:
        "There is a syntax error in the following Vagrantfile.\n/Users/dev/old/legacy/Vagrantfile:12: syntax error, unexpected end-of-input",
      bookmarked: false,
    },
  ];
}

function seedBoxes(): BoxInfo[] {
  const box = (
    name: string,
    provider: string,
    version: string,
    architecture: string | null,
    size: number,
    usedBy: [string, string][] = [],
  ): BoxInfo => ({
    name,
    provider,
    version,
    architecture,
    info: {},
    size_bytes: size,
    used_by: usedBy.map(([machine, project]) => ({ machine, project })),
  });
  return [
    box("bento/ubuntu-24.04", "vmware_desktop", "202508.03.0", "arm64", 1_830_000_000, [
      ["web", "/Users/dev/work/api"],
      ["db", "/Users/dev/work/api"],
    ]),
    box("bento/ubuntu-24.04", "vmware_desktop", "202404.23.0", "arm64", 1_710_000_000),
    box("hashicorp/bionic64", "virtualbox", "1.0.282", "amd64", 512_000_000, [
      ["default", "/Users/dev/clients/shop"],
    ]),
    box("local/base", "virtualbox", "0", null, 2_400_000_000),
  ];
}

export function defaultSettings(): Settings {
  return {
    version: 1,
    vagrant_path: null,
    extra_path: [],
    env: { VAGRANT_CHECKPOINT_DISABLE: "1" },
    status: { concurrency: 2, interval_secs: null },
    jobs: { global_cap: 3 },
    app: {
      keep_in_tray: true,
      start_hidden: false,
      menu_bar_only: false,
      launch_at_login: false,
      notify_on_finish: true,
      check_updates_on_launch: true,
    },
    bookmarks: [],
    mcp: {
      enabled: false,
      access: "read",
      paused: false,
      headless_destructive: false,
      ssh_exec: false,
      default_access: "all_projects",
      port: 47816,
      clients: [],
      audit_max_bytes: 5_000_000,
    },
    projects: {
      "/Users/dev/work/api": {
        alias: "API stack",
        group: "Work",
        color: "blue",
        pinned: true,
        hidden: false,
        env: {},
        up: { provider: "vmware_desktop", provision: null },
        agent_access: "inherit",
        machines: {},
      },
    },
  };
}

const VAGRANTFILE = `Vagrant.configure("2") do |config|
  config.vm.box = "bento/ubuntu-24.04"

  config.vm.define "web" do |web|
    web.vm.network "forwarded_port", guest: 80, host: 8080
  end

  config.vm.define "db"
end
`;

const BENTO = ["vmware_desktop", "virtualbox", "parallels", "utm"];
const MOCK_UPDATE: AppUpdate = {
  version: "0.2.0",
  current_version: "0.1.0",
  notes:
    "### Added\n\n- Snapshots: take, restore and delete from the machine menu.\n\n### Fixed\n\n- Status refresh no longer stalls when a provider is slow.",
  date: "2026-11-02T09:00:00Z",
};

const MOCK_POPULAR: PopularBox[] = [
  {
    name: "bento/ubuntu-24.04",
    label: "Ubuntu 24.04 LTS",
    providers: BENTO,
    architectures: ["arm64", "amd64"],
  },
  {
    name: "bento/debian-12",
    label: "Debian 12",
    providers: BENTO,
    architectures: ["arm64", "amd64"],
  },
  {
    name: "ubuntu/jammy64",
    label: "Ubuntu 22.04 (Canonical)",
    providers: ["virtualbox"],
    architectures: [],
  },
  {
    name: "laravel/homestead",
    label: "Laravel Homestead",
    providers: ["virtualbox", "parallels"],
    architectures: ["arm64", "amd64"],
  },
];

const AFTER: Partial<Record<Action, string>> = {
  up: "running",
  resume: "running",
  reload: "running",
  halt: "poweroff",
  suspend: "saved",
  destroy: "not_created",
};

export function createMockApi(opts: { stepMs?: number; vagrantMissing?: boolean } = {}): Api {
  const step = opts.stepMs ?? 300;
  let projects = opts.vagrantMissing ? [] : seed();
  let vagrantMissing = opts.vagrantMissing ?? false;
  let boxes = seedBoxes();
  let settings = defaultSettings();
  let pendingUpdate: AppUpdate | null = null;
  const files = new Map<string, string>();
  const tokens = new Map<string, string>();
  const audit: AuditEntry[] = [
    {
      time: Date.now() - 90_000,
      transport: "http",
      client: "Claude Code",
      tool: "machine_up",
      arguments: { project: "/Users/dev/work/api", machine: "db" },
      decision: "allowed",
      job_id: 1,
      result: "ok",
    },
    {
      time: Date.now() - 60_000,
      transport: "http",
      client: "Claude Code",
      tool: "machine_destroy",
      arguments: { project: "/Users/dev/work/api", machine: "cache" },
      decision: "rejected",
      job_id: null,
      result: "The user didn't approve this call.",
    },
    {
      time: Date.now() - 30_000,
      transport: "stdio",
      client: "stdio:cursor",
      tool: "list_machines",
      arguments: {},
      decision: "allowed",
      job_id: null,
      result: "ok",
    },
  ];
  const newToken = () =>
    Array.from({ length: 64 }, () => "0123456789abcdef"[Math.floor(Math.random() * 16)]).join("");
  const boxListeners = new Set<() => void>();
  const jobs = new Map<number, JobInfo>();
  const logs = new Map<number, LogLine[]>();
  const jobListeners = new Set<(e: JobEvent) => void>();
  const changeListeners = new Set<() => void>();
  const timers = new Map<number, ReturnType<typeof setTimeout>[]>();
  let nextJob = 1;
  let nextPty = 1;
  const ptys = new Map<number, PtyHandlers>();

  const emit = (e: JobEvent) => jobListeners.forEach((l) => l(e));
  const update = (job: JobInfo) => {
    jobs.set(job.id, job);
    emit({ type: "updated", job: { ...job } });
  };
  const line = (id: number, text: string, stream: "stdout" | "stderr" = "stdout") => {
    const l = logs.get(id) ?? [];
    const entry: LogLine = { seq: l.length, stream, text };
    l.push(entry);
    logs.set(id, l);
    emit({ type: "output", id, line: entry });
  };
  const find = (dir: string) => {
    const p = projects.find((p) => p.env.dir === dir);
    if (!p) throw new Error("project folder is not known");
    return p;
  };

  return {
    appInfo: async () => ({
      version: "0.1.0",
      vagrant: vagrantMissing
        ? null
        : {
            path: "/usr/local/bin/vagrant",
            version: "2.4.9",
            features: { box_architecture: true },
          },
      vagrant_error: vagrantMissing
        ? "vagrant was not found on PATH; install Vagrant or set its path in Settings"
        : null,
      detecting: false,
      index_path: "/Users/dev/.vagrant.d/data/machine-index/index",
      host_architecture: "arm64",
    }),
    listMachines: async () => structuredClone(projects),
    refreshStatus: async (dir) => {
      const now = Date.now();
      projects = projects.map((p) => (!dir || p.env.dir === dir ? { ...p, checked_at: now } : p));
      return structuredClone(projects);
    },
    runAction: async (dir, name, action) => {
      const project = find(dir);
      const targets = project.machines.filter((m) => name === null || m.name === name);
      if (name !== null && targets.length === 0) throw new Error(`no machine named "${name}"`);
      const id = nextJob++;
      const argv = ["vagrant", action, ...(name ? [name] : []), "--no-tty"];
      const job: JobInfo = {
        id,
        label: `${action} ${name ?? "(all machines)"}`,
        origin: { kind: "ui" },
        scope: name ? { scope: "machine", project: dir, name } : { scope: "project", project: dir },
        argv,
        cwd: dir,
        state: "running",
        exit_code: null,
        error: null,
        queued_at: Date.now(),
        started_at: Date.now(),
        finished_at: null,
      };
      update(job);
      const script = [
        `Bringing machine '${name ?? "default"}' ${action} with '${targets[0]?.provider ?? "vmware_desktop"}' provider...`,
        `==> ${name ?? "default"}: Checking if box 'bento/ubuntu-24.04' version '202508.03.0' is up to date...`,
        `==> ${name ?? "default"}: Running ${action}...`,
        `==> ${name ?? "default"}: Done.`,
      ];
      const t = script.map((text, i) => setTimeout(() => line(id, text), step * (i + 1)));
      t.push(
        setTimeout(
          () => {
            const next = AFTER[action];
            if (next) {
              for (const m of targets) {
                Object.assign(m, machine(m.name, m.provider, next));
              }
            }
            project.checked_at = Date.now();
            const done = jobs.get(id);
            if (done)
              update({ ...done, state: "succeeded", exit_code: 0, finished_at: Date.now() });
            changeListeners.forEach((l) => l());
          },
          step * (script.length + 1),
        ),
      );
      timers.set(id, t);
      return id;
    },
    cancelJob: async (id, force) => {
      const job = jobs.get(id);
      if (!job || !["running", "cancelling", "queued"].includes(job.state)) {
        throw new Error(`job ${id} already finished`);
      }
      timers.get(id)?.forEach(clearTimeout);
      if (!force) line(id, "==> Waiting for cleanup before exiting...");
      update({ ...job, state: "cancelled", exit_code: 130, finished_at: Date.now() });
    },
    listJobs: async () => [...jobs.values()].reverse().map((j) => ({ ...j })),
    jobLog: async (id, tail) => {
      const l = logs.get(id) ?? [];
      return tail === undefined ? [...l] : l.slice(-tail);
    },
    subscribeJobs(onEvent) {
      jobListeners.add(onEvent);
      return () => jobListeners.delete(onEvent);
    },
    onMachinesChanged(cb) {
      changeListeners.add(cb);
      return () => changeListeners.delete(cb);
    },
    onTrayRequest: () => () => {},
    async sshOpen(dir, name, _cols, _rows, h) {
      const m = find(dir).machines.find((m) => m.name === name);
      if (!m) throw new Error(`no machine named "${name}"`);
      const id = nextPty++;
      ptys.set(id, h);
      const enc = new TextEncoder();
      setTimeout(() => {
        h.onData(enc.encode(`Welcome to Ubuntu 24.04 LTS (mock)\r\nvagrant@${name}:~$ `));
      }, 50);
      return id;
    },
    async ptyRerunJob(_jobId, _cols, _rows, h) {
      const id = nextPty++;
      ptys.set(id, h);
      h.onData(new TextEncoder().encode("[sudo] password for dev: "));
      return id;
    },
    async ptyWrite(id, data) {
      const h = ptys.get(id);
      if (!h) throw new Error(`no terminal ${id}`);
      const enc = new TextEncoder();
      if (data === "\r") h.onData(enc.encode("\r\nvagrant@mock:~$ "));
      else if (data === "\u0004") {
        h.onData(enc.encode("\r\nlogout\r\n"));
        h.onExit(0);
        ptys.delete(id);
      } else h.onData(enc.encode(data));
    },
    ptyResize: async () => {},
    async ptyClose(id) {
      ptys.delete(id);
    },
    openFolder: async () => {},
    listBoxes: async () => structuredClone(boxes),
    checkBoxUpdates: async () => ({
      updates: [
        {
          name: "bento/ubuntu-24.04",
          provider: "vmware_desktop",
          status: { status: "outdated", current: "202508.03.0", latest: "202510.01.0" },
        },
        {
          name: "hashicorp/bionic64",
          provider: "virtualbox",
          status: { status: "up_to_date", version: "1.0.282" },
        },
        { name: "local/base", provider: "virtualbox", status: { status: "no_metadata" } },
      ],
      raw: "* 'bento/ubuntu-24.04' for 'vmware_desktop' is outdated! Current: 202508.03.0. Latest: 202510.01.0\n* 'hashicorp/bionic64' for 'virtualbox' (v1.0.282) is up to date\n* 'local/base' for 'virtualbox' wasn't added from a catalog, no version information",
    }),
    pruneDryRun: async () =>
      "The following boxes will be kept...\nbento/ubuntu-24.04 (vmware_desktop, 202508.03.0)\n\nChecking for older boxes...\n\nWould remove bento/ubuntu-24.04 vmware_desktop 202404.23.0",
    async runBoxOp(op) {
      const id = nextJob++;
      const label =
        op.op === "add"
          ? `box add ${op.name ?? op.source}`
          : op.op === "prune"
            ? "box prune"
            : `box ${op.op} ${op.name}`;
      update({
        id,
        label,
        origin: { kind: "ui" },
        scope: { scope: "boxes" },
        argv: ["vagrant", "box", op.op],
        cwd: null,
        state: "running",
        exit_code: null,
        error: null,
        queued_at: Date.now(),
        started_at: Date.now(),
        finished_at: null,
      });
      setTimeout(() => line(id, `==> box: ${label}...`), step);
      setTimeout(() => {
        if (op.op === "remove") {
          boxes = boxes.filter(
            (b) => !(b.name === op.name && (op.version === null || b.version === op.version)),
          );
        } else if (op.op === "prune") {
          boxes = boxes.filter((b) => b.version !== "202404.23.0");
        }
        line(id, "==> box: Done.");
        const done = jobs.get(id);
        if (done) update({ ...done, state: "succeeded", exit_code: 0, finished_at: Date.now() });
        boxListeners.forEach((l) => l());
      }, step * 2);
      return id;
    },
    getSettings: async () => structuredClone(settings),
    async saveSettings(next) {
      if (next.status.concurrency < 1 || next.status.concurrency > 8) {
        throw new Error("invalid status concurrency: must be 1 to 8");
      }
      settings = structuredClone(next);
      changeListeners.forEach((l) => l());
      return structuredClone(settings);
    },
    pickFolder: async () => "/Users/dev/side/new-project",
    async addBookmark(dir) {
      if (!settings.bookmarks.includes(dir)) settings.bookmarks.push(dir);
      if (!projects.some((p) => p.env.dir === dir)) {
        projects.push({
          env: { dir, vagrantfile: null, dotfile_path: null },
          machines: [machine("default", "vmware_desktop", "not_created")],
          checked_at: Date.now(),
          error: null,
          bookmarked: true,
        });
      }
      changeListeners.forEach((l) => l());
      return structuredClone(projects);
    },
    async removeBookmark(dir) {
      settings.bookmarks = settings.bookmarks.filter((b) => b !== dir);
      setTimeout(() => changeListeners.forEach((l) => l()));
      projects = projects.filter(
        (p) =>
          !(
            p.bookmarked &&
            p.env.dir === dir &&
            p.machines.every((m) => m.state === "not_created")
          ),
      );
      return structuredClone(projects);
    },
    readVagrantfile: async (dir) => ({
      path: `${dir}/Vagrantfile`,
      content: files.get(dir) ?? VAGRANTFILE,
    }),
    async saveVagrantfile(dir, base, content) {
      if ((files.get(dir) ?? VAGRANTFILE) !== base) {
        throw new Error(
          "invalid Vagrantfile: changed on disk since you opened it; reload to see the new version",
        );
      }
      files.set(dir, content);
      const ok = !content.includes("syntax error");
      return {
        backup: `/Users/dev/Library/Application Support/io.github.oops-apps.Boxdeck/backups/x/Vagrantfile-${Date.now()}.rb`,
        validation: ok
          ? { ok, output: "Vagrantfile validated successfully." }
          : { ok, output: "There is a syntax error in the following Vagrantfile." },
      };
    },
    validateVagrantfile: async () => ({ ok: true, output: "Vagrantfile validated successfully." }),
    exportSettings: async () => "/Users/dev/boxdeck-settings.json",
    importSettings: async () => structuredClone(settings),
    redetectVagrant: async () => {
      vagrantMissing = false;
      projects = seed();
      changeListeners.forEach((l) => l());
    },
    mcpStatus: async () => ({
      running: settings.mcp.enabled,
      port: settings.mcp.enabled ? settings.mcp.port : null,
      error: null,
      url: settings.mcp.enabled ? `http://127.0.0.1:${settings.mcp.port}/mcp` : null,
      stdio_path: "/Applications/Boxdeck.app/Contents/MacOS/boxdeck-mcp",
    }),
    async mcpAddClient(name) {
      const token = newToken();
      tokens.set(name, token);
      settings.mcp.clients.push({ name, token: "", created_at: Date.now() });
      return token;
    },
    async mcpRotateClient(name) {
      const token = newToken();
      tokens.set(name, token);
      return token;
    },
    async mcpRemoveClient(name) {
      tokens.delete(name);
      settings.mcp.clients = settings.mcp.clients.filter((c) => c.name !== name);
    },
    mcpClientToken: async (name) => tokens.get(name) ?? "",
    listAudit: async () => [...audit].reverse(),
    onAuditChanged: () => () => {},
    getApproval: async (id) => ({
      id,
      client: "Claude Code",
      tool: "machine_destroy",
      project: "/Users/dev/work/api",
      machine: "cache",
      arguments: JSON.stringify({ project: "/Users/dev/work/api", machine: "cache" }, null, 2),
      expires_at: Date.now() + 120_000,
    }),
    answerApproval: async () => {},
    pendingAppUpdate: async () => pendingUpdate,
    async checkAppUpdate() {
      await new Promise((r) => setTimeout(r, step));
      pendingUpdate = MOCK_UPDATE;
      return pendingUpdate;
    },
    async installAppUpdate(onProgress) {
      const total = 12_000_000;
      onProgress({ kind: "started", total });
      for (let done = 0; done < total; done += 1_500_000) {
        await new Promise((r) => setTimeout(r, step / 2));
        onProgress({ kind: "chunk", bytes: 1_500_000 });
      }
      onProgress({ kind: "installing" });
    },
    onAppUpdateAvailable: () => () => {},
    popularBoxes: async () => MOCK_POPULAR,
    async searchBoxes(query, provider) {
      const all = [
        {
          name: "bento/ubuntu-24.04",
          version: "202510.26.0",
          downloads: 130558,
          providers: ["utm", "virtualbox", "vmware_desktop", "parallels"],
          architectures: ["arm64", "amd64"],
        },
        {
          name: "bento/ubuntu-22.04",
          version: "202510.26.0",
          downloads: 167527,
          providers: ["parallels", "utm", "virtualbox", "vmware_desktop"],
          architectures: ["arm64", "amd64"],
        },
        {
          name: "bento/ubuntu-20.04",
          version: "202407.23.0",
          downloads: 258977,
          providers: ["vmware_desktop", "virtualbox"],
          architectures: ["amd64"],
        },
        {
          name: "gusztavvargadr/ubuntu-desktop",
          version: "2607.1.0",
          downloads: 46432,
          providers: ["virtualbox", "vmware_desktop"],
          architectures: ["arm64"],
        },
      ];
      const q = query.toLowerCase();
      return {
        boxes: all.filter(
          (b) => b.name.includes(q) && (!provider || b.providers.includes(provider)),
        ),
        error: null,
        popular: MOCK_POPULAR,
        host_architecture: "arm64",
      };
    },
    onBoxesChanged(cb) {
      boxListeners.add(cb);
      return () => boxListeners.delete(cb);
    },
  };
}
