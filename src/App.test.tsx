import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { setApi, type Api } from "./ipc/api";
import { createMockApi } from "./ipc/mock";
import { useUi } from "./state/store";

// xterm needs a real canvas; the dock is covered by the browser preview instead.
vi.mock("./dock/Terminal", () => ({ TerminalView: () => null }));

let mock: Api;

function row(name: string): HTMLElement {
  const li = screen.getByText(name).closest("li");
  if (!li) throw new Error(`no row for ${name}`);
  return li;
}

function renderApp() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <App />
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  mock = createMockApi({ stepMs: 1 });
  setApi(mock);
  useUi.setState({
    view: "machines",
    search: "",
    stateFilter: "all",
    jobs: {},
    tabs: [],
    selected: new Set(),
    settingsTab: "general",
    settingsProject: null,
    updateCheck: "idle",
    dockSize: 0.38,
  });
});

describe("Machines", () => {
  it("groups machines by project with a text state, not just color", async () => {
    renderApp();
    expect(await screen.findByText("~/work/api")).toBeInTheDocument();
    const web = row("web");
    expect(within(web).getByText("running")).toBeInTheDocument();
    expect(screen.getByText("Vagrant couldn't load this project")).toBeInTheDocument();
    expect(screen.getByText("(cached)")).toBeInTheDocument();
  });

  it("filters by state and search", async () => {
    renderApp();
    await screen.findByText("web");
    fireEvent.change(screen.getByLabelText("Filter by state"), {
      target: { value: "not_created" },
    });
    expect(screen.queryByText("web")).not.toBeInTheDocument();
    expect(screen.getByText("cache")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Filter by state"), { target: { value: "all" } });
    fireEvent.change(screen.getByPlaceholderText("Search"), { target: { value: "shop" } });
    expect(screen.queryByText("web")).not.toBeInTheDocument();
    expect(screen.getByText("~/clients/shop")).toBeInTheDocument();
  });

  it("runs an action as a job and shows it in Activity", async () => {
    const run = vi.spyOn(mock, "runAction");
    renderApp();
    await screen.findByText("db");
    const db = row("db");
    fireEvent.click(within(db).getByRole("button", { name: /Up/ }));
    await waitFor(() => expect(run).toHaveBeenCalledWith("/Users/dev/work/api", "db", "up"));
    await act(() => new Promise((r) => setTimeout(r, 50)));
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    const table = screen.getByRole("table");
    expect(within(table).getByText("up db")).toBeInTheDocument();
    expect(within(table).getByText("Succeeded")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "up db" })).toBeInTheDocument();
  });

  it("resizes the log panel from its top edge", async () => {
    renderApp();
    await screen.findByText("db");
    fireEvent.click(within(row("db")).getByRole("button", { name: /Up/ }));
    const handle = await screen.findByRole("separator", { name: "Resize panel" });
    expect(handle).toHaveAttribute("aria-valuenow", "38");
    fireEvent.keyDown(handle, { key: "ArrowUp" });
    fireEvent.keyDown(handle, { key: "ArrowUp" });
    expect(handle).toHaveAttribute("aria-valuenow", "48");
    fireEvent.doubleClick(handle);
    expect(handle).toHaveAttribute("aria-valuenow", "38");
  });

  it("asks before destroying", async () => {
    const run = vi.spyOn(mock, "runAction");
    renderApp();
    await screen.findByText("web");
    const confirm = screen.queryByRole("dialog");
    expect(confirm).not.toBeInTheDocument();
    // Radix menus open on pointer events, so drive the dialog through the row's handler path.
    fireEvent.keyDown(screen.getByRole("button", { name: "More actions for web" }), {
      key: "Enter",
    });
    fireEvent.click(await screen.findByRole("menuitem", { name: "Destroy…" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Destroy web?")).toBeInTheDocument();
    expect(run).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Destroy" }));
    await waitFor(() => expect(run).toHaveBeenCalledWith("/Users/dev/work/api", "web", "destroy"));
  });
});

describe("Boxes", () => {
  it("lists boxes with size and users, and finds updates", async () => {
    useUi.setState({ view: "boxes" });
    renderApp();
    const table = await screen.findByRole("table");
    expect(within(table).getByText("web, db")).toBeInTheDocument();
    expect(within(table).getByText("1.8 GB")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
    expect(await within(table).findByText("202510.01.0 available")).toBeInTheDocument();
    expect(within(table).getByText("older version")).toBeInTheDocument();
    expect(within(table).getByText("Not from a catalog")).toBeInTheDocument();
  });

  it("warns before removing a box that machines use", async () => {
    const run = vi.spyOn(mock, "runBoxOp");
    useUi.setState({ view: "boxes" });
    renderApp();
    await screen.findByRole("table");
    fireEvent.keyDown(
      screen.getByRole("button", { name: "More actions for bento/ubuntu-24.04 202508.03.0" }),
      { key: "Enter" },
    );
    fireEvent.click(await screen.findByRole("menuitem", { name: "Remove this version…" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/In use by web/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove anyway" }));
    await waitFor(() =>
      expect(run).toHaveBeenCalledWith({
        op: "remove",
        name: "bento/ubuntu-24.04",
        version: "202508.03.0",
        provider: "vmware_desktop",
        all_architectures: false,
      }),
    );
  });

  it("needs a name when adding from a URL", async () => {
    useUi.setState({ view: "boxes" });
    renderApp();
    fireEvent.click(await screen.findByRole("button", { name: /Add box/ }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Source"), {
      target: { value: "https://example.com/a.box" },
    });
    expect(within(dialog).getByRole("button", { name: "Add box" })).toBeDisabled();
    fireEvent.change(within(dialog).getByLabelText(/Name/), { target: { value: "me/a" } });
    expect(within(dialog).getByRole("button", { name: "Add box" })).toBeEnabled();
  });
});

describe("Configuration", () => {
  it("shows aliases, groups and pinned projects first", async () => {
    renderApp();
    expect(await screen.findByText("API stack")).toBeInTheDocument();
    expect(screen.getByText("Work")).toBeInTheDocument();
    expect(screen.getByText("Other projects")).toBeInTheDocument();
    const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(headings).toEqual(["Work", "Other projects"]);
  });

  it("adds a project folder from Machines", async () => {
    renderApp();
    await screen.findByText("API stack");
    fireEvent.click(screen.getByRole("button", { name: /Add project/ }));
    expect(await screen.findByText("~/side/new-project")).toBeInTheDocument();
  });

  it("saves settings and reports validation errors", async () => {
    useUi.setState({ view: "settings" });
    renderApp();
    const input = await screen.findByLabelText("Parallel status checks");
    fireEvent.change(input, { target: { value: "0" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("must be 1 to 8");
    fireEvent.change(input, { target: { value: "4" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Settings saved.")).toBeInTheDocument();
    expect((await mock.getSettings()).status.concurrency).toBe(4);
  });

  it("edits project environment variables", async () => {
    useUi.setState({
      view: "settings",
      settingsTab: "projects",
      settingsProject: "/Users/dev/work/api",
    });
    renderApp();
    const addButtons = await screen.findAllByRole("button", { name: /Add variable/ });
    fireEvent.click(addButtons[addButtons.length - 1]);
    const names = screen.getAllByLabelText(/^Name \d+$/);
    fireEvent.change(names[names.length - 1], { target: { value: "VAGRANT_VAGRANTFILE" } });
    const values = screen.getAllByLabelText(/^Value \d+$/);
    fireEvent.change(values[values.length - 1], { target: { value: "Custom.rb" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(async () =>
      expect((await mock.getSettings()).projects["/Users/dev/work/api"].env).toEqual({
        VAGRANT_VAGRANTFILE: "Custom.rb",
      }),
    );
  });
});

describe("Agents", () => {
  it("has an MCP tab in Settings", async () => {
    useUi.setState({ view: "settings" });
    renderApp();
    fireEvent.click(await screen.findByRole("tab", { name: "MCP / Agents" }));
    expect(screen.getByLabelText("Allow agents to connect")).toBeInTheDocument();
  });

  it("adds a client and fills its token into the snippets", async () => {
    useUi.setState({ view: "settings", settingsTab: "mcp" });
    renderApp();
    fireEvent.change(await screen.findByLabelText("New client name"), {
      target: { value: "Claude Code" },
    });
    fireEvent.click(screen.getByRole("button", { name: /Add client/ }));
    const token = await screen.findByText(/^[0-9a-f]{64}$/);
    expect(
      screen.getByText(new RegExp(`claude mcp add .*Bearer ${token.textContent}`)),
    ).toBeInTheDocument();
  });

  it("lists agent calls with their decision", async () => {
    useUi.setState({ view: "activity" });
    renderApp();
    fireEvent.click(await screen.findByRole("tab", { name: "Agent calls" }));
    expect(await screen.findByText("rejected")).toBeInTheDocument();
    expect(screen.getByText("stdio:cursor")).toBeInTheDocument();
  });
});

describe("Onboarding", () => {
  it("guides through a missing Vagrant and looks again", async () => {
    setApi(createMockApi({ stepMs: 1, vagrantMissing: true }));
    renderApp();
    expect(await screen.findByText("Boxdeck can't find Vagrant")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Look again/ }));
    expect(await screen.findByText("API stack")).toBeInTheDocument();
    expect(screen.queryByText("Boxdeck can't find Vagrant")).not.toBeInTheDocument();
  });
});

describe("Adding a box", () => {
  it("preselects the provider in use and requires one for catalog boxes", async () => {
    useUi.setState({ view: "boxes" });
    renderApp();
    fireEvent.click(await screen.findByRole("button", { name: /Add box/ }));
    const dialog = await screen.findByRole("dialog");
    // The mock's machines and boxes mostly use vmware_desktop.
    await waitFor(() =>
      expect(within(dialog).getByLabelText(/Provider/)).toHaveValue("vmware_desktop"),
    );
    fireEvent.change(within(dialog).getByLabelText("Source"), {
      target: { value: "bento/ubuntu-24.04" },
    });
    const add = within(dialog).getByRole("button", { name: "Add box" });
    expect(add).toBeEnabled();
    fireEvent.change(within(dialog).getByLabelText(/Provider/), { target: { value: "" } });
    expect(add).toBeDisabled();
  });
});

describe("Box catalog", () => {
  it("searches the catalog and fills in the picked box", async () => {
    useUi.setState({ view: "boxes" });
    renderApp();
    fireEvent.click(await screen.findByRole("button", { name: /Add box/ }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Popular:")).toBeInTheDocument();
    fireEvent.change(within(dialog).getByLabelText("Source"), { target: { value: "ubuntu" } });
    const results = await within(dialog).findByRole(
      "list",
      { name: "Catalog results" },
      { timeout: 3000 },
    );
    expect(within(results).getByText("no arm64 build")).toBeInTheDocument();
    fireEvent.click(within(results).getByText("bento/ubuntu-24.04"));
    expect(within(dialog).getByLabelText("Source")).toHaveValue("bento/ubuntu-24.04");
  });
});

describe("Popular boxes", () => {
  it("lists Homestead and flags boxes the chosen provider can't run", async () => {
    useUi.setState({ view: "boxes" });
    renderApp();
    fireEvent.click(await screen.findByRole("button", { name: /Add box/ }));
    const dialog = await screen.findByRole("dialog");
    const homestead = await within(dialog).findByRole("button", { name: /Laravel Homestead/ });
    expect(homestead).toHaveTextContent("VirtualBox, Parallels only");
    expect(within(dialog).getByRole("button", { name: /Ubuntu 24.04 LTS/ })).not.toHaveTextContent(
      "only",
    );
    fireEvent.click(homestead);
    expect(within(dialog).getByLabelText("Source")).toHaveValue("laravel/homestead");
  });
});

describe("App updates", () => {
  it("checks, shows the release notes and installs with progress", async () => {
    useUi.setState({ view: "settings", settingsTab: "about" });
    renderApp();
    fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    expect(await screen.findByText("Boxdeck 0.2.0 is available")).toBeInTheDocument();
    expect(screen.getByText(/Snapshots: take, restore/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Install and restart" }));
    expect(await screen.findByRole("progressbar", { name: "Update download" })).toBeInTheDocument();
    expect(await screen.findByText("Restarting Boxdeck…")).toBeInTheDocument();
  });

  it("says when Boxdeck is up to date", async () => {
    mock.checkAppUpdate = async () => null;
    useUi.setState({ view: "settings", settingsTab: "about" });
    renderApp();
    fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));
    expect(await screen.findByText("You're up to date.")).toBeInTheDocument();
  });

  it("announces a release found at launch and waits for running jobs", async () => {
    mock.pendingAppUpdate = async () => ({
      version: "0.2.0",
      current_version: "0.1.0",
      notes: null,
      date: null,
    });
    renderApp();
    fireEvent.click(await screen.findByRole("button", { name: "View update" }));
    await screen.findByText("Boxdeck 0.2.0 is available");
    expect(screen.queryByRole("button", { name: "View update" })).not.toBeInTheDocument();
    act(() =>
      useUi.getState().upsertJob({
        id: 99,
        label: "up web",
        origin: { kind: "ui" },
        scope: { scope: "machine", project: "/Users/dev/work/api", name: "web" },
        argv: ["up", "web"],
        cwd: null,
        state: "running",
        exit_code: null,
        error: null,
        queued_at: Date.now(),
        started_at: Date.now(),
        finished_at: null,
      }),
    );
    expect(screen.getByRole("button", { name: "Install and restart" })).toBeDisabled();
    expect(screen.getByText("Wait for running Vagrant jobs to finish first.")).toBeInTheDocument();
  });
});
