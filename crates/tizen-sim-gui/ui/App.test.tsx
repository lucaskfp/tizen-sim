// GUI smoke tests: the real App against a stand-in for the GUI's Rust side that
// records commands and emits Session events as the Rust side would.
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { readFileSync } from "node:fs";
import path from "node:path";
import { onTestFinished, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import { App } from "./App";

type Invocation = { cmd: string; args: Record<string, unknown> };

/** Installs the stand-in Rust side; returns the commands the GUI invokes. */
function rustSide(answers: Record<string, unknown> = {}): Invocation[] {
  const invoked: Invocation[] = [];
  mockIPC(
    (cmd, args) => {
      invoked.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
      if (cmd === "profiles") {
        return [
          { version: "9.0", milestone: 120, latest: false },
          { version: "10.0", milestone: 130, latest: true },
        ];
      }
      return answers[cmd];
    },
    { shouldMockEvents: true },
  );
  return invoked;
}

/** Sends events of Session `session`, as the Rust side does. */
async function sessionEvents(session: number, ...events: unknown[]) {
  for (const event of events) {
    await act(() => emit("session-event", { Session: { session, event } }));
  }
}

async function sessionStarted(session: number, source: string, adopted = false) {
  await act(() => emit("session-event", { SessionStarted: { session, source, adopted } }));
}

async function screenEvent(session: number, embedded: boolean) {
  await act(() => emit("session-event", { Screen: { session, embedded } }));
}

test("a crashed Engine shows on the screen, and Restart starts the App again", async () => {
  const invoked = rustSide({ "plugin:dialog|open": "C:/apps/hello", embeds_screen: true });
  render(<App />);
  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));
  await sessionStarted(1, "C:/apps/hello");
  await sessionEvents(1, { Lifecycle: { AppLoaded: { url: "http://127.0.0.1:1/index.html" } } });
  await screenEvent(1, true);

  await sessionEvents(1, { Lifecycle: { Exited: { reason: "EngineCrashed" } } });

  const tv = screen.getByRole("region", { name: "TV screen" });
  expect(tv).toHaveTextContent("The Engine crashed");
  expect(screen.getByText("Crashed")).toBeInTheDocument();
  await userEvent.click(within(tv).getByRole("button", { name: /restart/i }));
  expect(invoked).toContainEqual({ cmd: "restart", args: {} });
});

test("a Session started from the CLI can be adopted", async () => {
  const invoked = rustSide({
    running_sessions: [
      { id: "ab12cd", app_id: "Fixture006.RemoteKeys", profile: "10.0", source: "C:/apps/remote-keys" },
    ],
  });
  render(<App />);

  await userEvent.click(screen.getByRole("button", { name: /running sessions/i }));
  const list = await screen.findByRole("list", { name: "Running Sessions" });
  expect(list).toHaveTextContent("Fixture006.RemoteKeys");
  expect(list).toHaveTextContent("Tizen 10.0");
  await userEvent.click(within(list).getByRole("button", { name: /adopt/i }));
  await sessionStarted(1, "C:/apps/remote-keys", true);
  await sessionEvents(
    1,
    { Lifecycle: { Starting: { app_id: "Fixture006.RemoteKeys", profile: "10.0" } } },
    { Lifecycle: { AppLoaded: { url: "http://127.0.0.1:1/index.html" } } },
  );

  expect(invoked).toContainEqual({ cmd: "adopt", args: { id: "ab12cd" } });
  expect(screen.queryByRole("list", { name: "Running Sessions" })).not.toBeInTheDocument();
  expect(screen.getByText("Started from the CLI")).toBeInTheDocument();
  // Its owner chose the profile; the GUI can't restart it on another.
  expect(screen.getByRole("combobox", { name: "Version Profile" })).toBeDisabled();
  expect(screen.getByRole("button", { name: /reload/i })).toBeEnabled();
});

test("with no Session running elsewhere there is nothing to adopt", async () => {
  rustSide({ running_sessions: [] });
  render(<App />);

  await userEvent.click(screen.getByRole("button", { name: /running sessions/i }));

  expect(await screen.findByText(/no Session is running/i)).toBeInTheDocument();
});

test("the screen area tells the Rust side where the Engine window goes", async () => {
  const invoked = rustSide({ embeds_screen: true });
  render(<App />);

  await screen.findByRole("region", { name: "TV screen" });

  await waitFor(() => expect(invoked.some((i) => i.cmd === "set_screen_area" && i.args.area !== null)).toBe(true));
  const report = invoked.find((i) => i.cmd === "set_screen_area");
  expect(report?.args.area).toEqual({ x: 0, y: 0, width: 0, height: 0 });
});

test("where the Engine keeps its own window, the screen area says so", async () => {
  rustSide({ "plugin:dialog|open": "C:/apps/hello", embeds_screen: false });
  render(<App />);
  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));
  await sessionStarted(1, "C:/apps/hello");
  await sessionEvents(1, { Lifecycle: { AppLoaded: { url: "http://127.0.0.1:1/index.html" } } });
  await screenEvent(1, false);

  expect(screen.getByRole("region", { name: "TV screen" })).toHaveTextContent(/its own Engine window/);
});

test("the dev server form opens inline, so it never goes under the TV screen", async () => {
  const invoked = rustSide({ embeds_screen: true });
  render(<App />);

  await userEvent.click(screen.getByRole("button", { name: /dev server/i }));
  await userEvent.type(screen.getByLabelText("config.xml"), "C:/apps/dev/config.xml");
  await userEvent.click(screen.getByRole("button", { name: "Run" }));

  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(invoked).toContainEqual({
    cmd: "open_app",
    args: { source: { kind: "devServer", url: "http://localhost:5173/", config: "C:/apps/dev/config.xml" } },
  });
});

test("opening a Project Folder starts a Session and shows its console output", async () => {
  const invoked = rustSide({ "plugin:dialog|open": "C:/apps/hello" });
  render(<App />);

  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));
  await sessionStarted(1, "C:/apps/hello");
  await sessionEvents(
    1,
    { Lifecycle: { Starting: { app_id: "Fixture001.Hello", profile: "10.0" } } },
    { Console: { level: "log", text: "hello from fixture" } },
  );

  expect(invoked).toContainEqual({
    cmd: "open_app",
    args: { source: { kind: "path", path: "C:/apps/hello" } },
  });
  expect(screen.getByText("hello from fixture")).toBeInTheDocument();
  expect(screen.getByText("Fixture001.Hello")).toBeInTheDocument();
});

test("the on-screen remote sends Remote Keys, and dropped keys show in the log", async () => {
  const invoked = rustSide({ "plugin:dialog|open": "C:/apps/keys" });
  render(<App />);
  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));
  await sessionStarted(1, "C:/apps/keys");
  await sessionEvents(1, { Lifecycle: { AppLoaded: { url: "http://127.0.0.1:1/index.html" } } });

  await userEvent.click(screen.getByRole("button", { name: "Red" }));
  await sessionEvents(1, {
    RemoteKey: {
      key: "ColorF0Red",
      outcome: { Dropped: { reason: "ColorF0Red is not registered (call tizen.tvinputdevice.registerKey)" } },
    },
  });

  expect(invoked).toContainEqual({ cmd: "press_key", args: { key: "ColorF0Red" } });
  expect(
    screen.getByText("ColorF0Red dropped: ColorF0Red is not registered (call tizen.tvinputdevice.registerKey)"),
  ).toBeInTheDocument();
});

test("the TV's popup asking for a required gamepad is answered with the remote", async () => {
  const invoked = rustSide({ "plugin:dialog|open": "C:/apps/gamepad", embeds_screen: true });
  render(<App />);
  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));
  await sessionStarted(1, "C:/apps/gamepad");
  await sessionEvents(1, { Lifecycle: { GamepadRequired: { models: ["dualshock4", "usbgamepad"] } } });
  await screenEvent(1, true);

  expect(screen.getByText("Waiting for a gamepad")).toBeInTheDocument();
  expect(
    screen.getByText(
      "The App requires a dualshock4 or usbgamepad gamepad and none is connected: the TV asks before launching it.",
    ),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /reload/i })).toBeDisabled();
  await userEvent.click(screen.getByRole("button", { name: "ArrowRight" }));
  expect(invoked).toContainEqual({ cmd: "press_key", args: { key: "ArrowRight" } });

  await sessionEvents(1, { Lifecycle: { Exited: { reason: "LaunchCanceled" } } });
  expect(screen.getByText("The App was not launched.")).toBeInTheDocument();
});

test("the network panel lists requests with their Access Policy verdicts", async () => {
  rustSide({ "plugin:dialog|open": "C:/apps/network" });
  render(<App />);
  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));
  await sessionStarted(1, "C:/apps/network");
  await sessionEvents(
    1,
    {
      Network: {
        method: "GET",
        url: "http://allowed.test/data.json",
        status: 200,
        duration: { secs: 0, nanos: 12_000_000 },
        verdict: "Allowed",
      },
    },
    {
      Network: {
        method: "POST",
        url: "http://blocked.test/api",
        status: null,
        duration: { secs: 0, nanos: 1_000_000 },
        verdict: { Blocked: { missing_access: '<access origin="http://blocked.test"/>' } },
      },
    },
  );

  await userEvent.click(screen.getByRole("tab", { name: /network \(2\)/i }));

  const rows = screen.getAllByRole("row").slice(1);
  expect(rows[0]).toHaveTextContent(/Allowed.*GET.*http:\/\/allowed\.test\/data\.json.*200.*12 ms/);
  expect(rows[1]).toHaveTextContent(/Blocked.*POST.*http:\/\/blocked\.test\/api/);
  expect(rows[1]).toHaveTextContent('needs <access origin="http://blocked.test"/>');
});

test("switching Version Profile restarts the Session on it", async () => {
  const invoked = rustSide({ "plugin:dialog|open": "C:/apps/hello" });
  render(<App />);
  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));
  await sessionStarted(1, "C:/apps/hello");
  await sessionEvents(
    1,
    { Lifecycle: { Starting: { app_id: "Fixture001.Hello", profile: "10.0" } } },
    { Lifecycle: { AppLoaded: { url: "http://127.0.0.1:1/index.html" } } },
  );

  await userEvent.selectOptions(screen.getByRole("combobox", { name: "Version Profile" }), "9.0");
  await sessionEvents(1, { Lifecycle: { Exited: { reason: "Stopped" } } });
  await sessionStarted(2, "C:/apps/hello");
  // Until the new Session says which profile it runs on, the switcher keeps
  // the App's last one rather than going blank.
  expect(screen.getByRole("combobox", { name: "Version Profile" })).toHaveValue("10.0");
  await sessionEvents(2, { Lifecycle: { Starting: { app_id: "Fixture001.Hello", profile: "9.0" } } });

  expect(invoked).toContainEqual({ cmd: "switch_profile", args: { profile: "9.0" } });
  expect(screen.getByRole("combobox", { name: "Version Profile" })).toHaveValue("9.0");
  expect(screen.getByText("Starting Fixture001.Hello on Tizen 9.0")).toBeInTheDocument();
  // The log keeps the previous Session's lines to compare with.
  expect(screen.getByText("Starting Fixture001.Hello on Tizen 10.0")).toBeInTheDocument();
});

test("the Diagnostics panel lists what would behave differently on a TV", async () => {
  rustSide({ "plugin:dialog|open": "C:/apps/tizen-apis" });
  render(<App />);
  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));
  await sessionStarted(1, "C:/apps/tizen-apis");
  await sessionEvents(1, {
    Diagnostic: {
      code: "privilege-not-declared",
      message: "tizen.application.getAppsInfo needs http://tizen.org/privilege/application.info",
    },
  });

  await userEvent.click(screen.getByRole("tab", { name: /diagnostics \(1\)/i }));

  const list = screen.getByRole("list", { name: "Diagnostics" });
  expect(list).toHaveTextContent("privilege-not-declared");
  expect(list).toHaveTextContent("tizen.application.getAppsInfo needs http://tizen.org/privilege/application.info");
});

test("a failed command is shown", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "plugin:dialog|open") return "C:/apps/hello";
      if (cmd === "open_app") throw "invalid dev server URL";
      return [];
    },
    { shouldMockEvents: true },
  );
  render(<App />);

  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));

  expect(await screen.findByRole("alert")).toHaveTextContent("invalid dev server URL");
});

/** The version `tizen-sim --version` reports: the CLI crate's, which release-please bumps. */
function cliVersion(): string {
  const manifest = readFileSync(path.resolve(__dirname, "../../tizen-sim-cli/Cargo.toml"), "utf8");
  const version = /^version = "(.+)"$/m.exec(manifest)?.[1];
  if (!version) throw new Error("no version in the CLI's Cargo.toml");
  return version;
}

test("the installed version is shown, the one the CLI of the same release reports", async () => {
  rustSide({ "plugin:updater|check": null });
  render(<App />);

  expect(screen.getByTitle("Installed version")).toHaveTextContent(`tizen-sim ${cliVersion()}`);
});

/** What the updater reports when the latest release is newer than the GUI. */
const NEWER_RELEASE = { rid: 7, currentVersion: "0.2.0", version: "0.3.0", rawJson: {} };

test("a newer release is offered, and taking it installs it and restarts the GUI", async () => {
  const invoked = rustSide({ "plugin:updater|check": NEWER_RELEASE });
  render(<App />);

  await userEvent.click(await screen.findByRole("button", { name: /update to 0\.3\.0/i }));

  await waitFor(() => expect(invoked.map((i) => i.cmd)).toContain("plugin:process|restart"));
  const install = invoked.find((i) => i.cmd === "plugin:updater|download_and_install");
  expect(install?.args).toMatchObject({ rid: 7 });
});

test("with a newer release offered, both the installed and the offered version show", async () => {
  rustSide({ "plugin:updater|check": NEWER_RELEASE });
  render(<App />);

  expect(await screen.findByRole("button", { name: /update to 0\.3\.0/i })).toBeVisible();
  expect(screen.getByTitle("Installed version")).toHaveTextContent(`tizen-sim ${cliVersion()}`);
});

test("without a newer release, or when the check fails, no update is offered", async () => {
  rustSide({ "plugin:updater|check": null });
  const { unmount } = render(<App />);
  await waitFor(() => expect(screen.getByText("No App open")).toBeInTheDocument());
  expect(screen.queryByRole("button", { name: /update to/i })).not.toBeInTheDocument();
  unmount();

  mockIPC(
    (cmd) => {
      if (cmd === "plugin:updater|check") throw "could not fetch a valid release JSON";
      return [];
    },
    { shouldMockEvents: true },
  );
  render(<App />);
  await waitFor(() => expect(screen.getByText("No App open")).toBeInTheDocument());
  // Offline, the GUI works as before: a failed check isn't a problem to show.
  expect(screen.queryByRole("button", { name: /update to/i })).not.toBeInTheDocument();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("a failed update is shown", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "plugin:updater|check") return NEWER_RELEASE;
      if (cmd === "plugin:updater|download_and_install") throw "the signature doesn't match";
      return [];
    },
    { shouldMockEvents: true },
  );
  render(<App />);

  await userEvent.click(await screen.findByRole("button", { name: /update to 0\.3\.0/i }));

  expect(await screen.findByRole("alert")).toHaveTextContent("the signature doesn't match");
});

/** Has the webview report a monitor of this size until the test ends. */
function monitor(width: number, height: number) {
  for (const [name, value] of Object.entries({ width, height })) {
    Object.defineProperty(window.screen, name, { value, configurable: true });
  }
  onTestFinished(() => {
    delete (window.screen as unknown as Record<string, unknown>).width;
    delete (window.screen as unknown as Record<string, unknown>).height;
  });
}

const splitter = () => screen.getByRole("separator", { name: /resize the panel/i });

test("the panel docks at the bottom, a button moves it to the right, and the choice is remembered", async () => {
  monitor(3840, 2160);
  rustSide();
  const { unmount } = render(<App />);
  expect(splitter()).toHaveAttribute("aria-orientation", "horizontal");
  expect(splitter()).toHaveAttribute("aria-valuenow", "30");

  await userEvent.click(screen.getByRole("button", { name: "Dock the panel on the right" }));

  expect(splitter()).toHaveAttribute("aria-orientation", "vertical");
  unmount();
  rustSide();
  render(<App />);
  expect(splitter()).toHaveAttribute("aria-orientation", "vertical");
  await userEvent.click(screen.getByRole("button", { name: "Dock the panel at the bottom" }));
  expect(splitter()).toHaveAttribute("aria-orientation", "horizontal");
});

test("on an ultrawide monitor the panel starts on the right, unless the developer chose otherwise", async () => {
  monitor(3440, 1440);
  rustSide();
  const { unmount } = render(<App />);
  expect(splitter()).toHaveAttribute("aria-orientation", "vertical");
  expect(splitter()).toHaveAttribute("aria-valuenow", "30");

  await userEvent.click(screen.getByRole("button", { name: "Dock the panel at the bottom" }));
  unmount();
  rustSide();
  render(<App />);

  expect(splitter()).toHaveAttribute("aria-orientation", "horizontal");
});

test("the arrow keys resize the panel, a double click restores its size, and the size is remembered", async () => {
  rustSide();
  const { unmount } = render(<App />);

  splitter().focus();
  // Docked at the bottom, up moves the splitter up: a taller panel.
  await userEvent.keyboard("{ArrowUp}{ArrowUp}");
  expect(splitter()).toHaveAttribute("aria-valuenow", "34");
  unmount();
  rustSide();
  render(<App />);
  expect(splitter()).toHaveAttribute("aria-valuenow", "34");

  await userEvent.dblClick(splitter());
  expect(splitter()).toHaveAttribute("aria-valuenow", "30");

  // Docked on the right, left moves it left: a wider panel.
  await userEvent.click(screen.getByRole("button", { name: "Dock the panel on the right" }));
  splitter().focus();
  await userEvent.keyboard("{ArrowLeft}");
  expect(splitter()).toHaveAttribute("aria-valuenow", "32");
  await userEvent.keyboard("{ArrowRight}{ArrowRight}");
  expect(splitter()).toHaveAttribute("aria-valuenow", "28");
});

test("docked on the right, the screen area still tells the Rust side where the Engine window goes", async () => {
  monitor(3440, 1440);
  const invoked = rustSide({ embeds_screen: true });
  render(<App />);

  await screen.findByRole("region", { name: "TV screen" });
  expect(splitter()).toHaveAttribute("aria-orientation", "vertical");

  await waitFor(() => expect(invoked.some((i) => i.cmd === "set_screen_area" && i.args.area !== null)).toBe(true));
});

test("a click on the splitter without dragging leaves the panel's size alone", async () => {
  rustSide();
  render(<App />);

  await userEvent.click(splitter());

  expect(splitter()).toHaveAttribute("aria-valuenow", "30");
  expect(localStorage.getItem("tizen-sim.panel")).toBeNull();
});

test("the splitter highlights after the pointer rests on it, and at once while dragged", () => {
  rustSide();
  render(<App />);
  // Only the splitter's delay is faked. The test cleanup and userEvent both await a
  // setTimeout, and hang while it's faked, so the pointer events are dispatched directly.
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    const wait = (ms: number) => act(() => vi.advanceTimersByTime(ms));

    // Passing over it leaves it unchanged.
    fireEvent.pointerEnter(splitter());
    wait(100);
    fireEvent.pointerLeave(splitter());
    wait(1000);
    expect(splitter()).not.toHaveAttribute("data-highlighted");

    // Resting on it highlights it once the delay is over.
    fireEvent.pointerEnter(splitter());
    expect(splitter()).not.toHaveAttribute("data-highlighted");
    wait(1000);
    expect(splitter()).toHaveAttribute("data-highlighted");
    fireEvent.pointerLeave(splitter());
    expect(splitter()).not.toHaveAttribute("data-highlighted");

    // Pressing it to drag highlights it with no delay, until released.
    fireEvent.pointerEnter(splitter());
    fireEvent.pointerDown(splitter(), { button: 0, pointerId: 1 });
    expect(splitter()).toHaveAttribute("data-highlighted");
    fireEvent.pointerUp(splitter(), { button: 0, pointerId: 1 });
    fireEvent.pointerLeave(splitter());
    expect(splitter()).not.toHaveAttribute("data-highlighted");
  } finally {
    // Here, not in onTestFinished: that runs after the cleanup, which would hang.
    vi.useRealTimers();
  }
});

test("the splitter shows a grip at its centre, for both docks", async () => {
  rustSide();
  render(<App />);
  expect(within(splitter()).getByTestId("splitter-grip")).toHaveAttribute("data-orientation", "horizontal");

  await userEvent.click(screen.getByRole("button", { name: "Dock the panel on the right" }));

  expect(within(splitter()).getByTestId("splitter-grip")).toHaveAttribute("data-orientation", "vertical");
});

test("where the layout can't be remembered, the dock button still moves the panel", async () => {
  rustSide();
  const blocked = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new DOMException("blocked", "SecurityError");
  });
  onTestFinished(() => blocked.mockRestore());
  render(<App />);

  await userEvent.click(screen.getByRole("button", { name: "Dock the panel on the right" }));

  expect(splitter()).toHaveAttribute("aria-orientation", "vertical");
});

/**
 * Lays out the session panel's header `width` pixels wide, its tabs needing 260 and the dock
 * button 32, until the test ends. jsdom has no layout; the returned function resizes the header
 * and tells the GUI, as a ResizeObserver would.
 */
function panelHeader(width: number) {
  const observers = new Set<() => void>();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      private readonly notify: () => void;
      constructor(callback: () => void) {
        this.notify = callback;
      }
      observe() {
        observers.add(this.notify);
      }
      unobserve() {}
      disconnect() {
        observers.delete(this.notify);
      }
    },
  );
  const clientWidth = vi
    .spyOn(HTMLElement.prototype, "clientWidth", "get")
    .mockImplementation(function (this: HTMLElement) {
      return this.dataset.slot === "session-panel-header" ? width : 0;
    });
  const offsetWidth = vi
    .spyOn(HTMLElement.prototype, "offsetWidth", "get")
    .mockImplementation(function (this: HTMLElement) {
      if (this.dataset.slot === "tabs-list") return 260;
      return this.tagName === "BUTTON" ? 32 : 0;
    });
  onTestFinished(() => {
    clientWidth.mockRestore();
    offsetWidth.mockRestore();
    vi.unstubAllGlobals();
  });
  return async (to: number) => {
    width = to;
    await act(async () => observers.forEach((notify) => notify()));
  };
}

test("a narrow panel shows its tabs as a dropdown, and widening it brings the tabs back", async () => {
  monitor(3440, 1440);
  const resize = panelHeader(200);
  rustSide({ "plugin:dialog|open": "C:/apps/tizen-apis" });
  render(<App />);
  expect(splitter()).toHaveAttribute("aria-orientation", "vertical");
  await userEvent.click(screen.getByRole("button", { name: /open folder/i }));
  await sessionStarted(1, "C:/apps/tizen-apis");
  await sessionEvents(1, { Diagnostic: { code: "privilege-not-declared", message: "needs a privilege" } });

  expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
  const picker = screen.getByRole("combobox", { name: "Panel" });
  expect(picker).toHaveDisplayValue("Log");
  expect(screen.getByRole("button", { name: /dock the panel/i })).toBeInTheDocument();
  await userEvent.selectOptions(picker, "Diagnostics (1)");
  expect(screen.getByRole("tabpanel", { name: "Diagnostics (1)" })).toHaveTextContent("privilege-not-declared");

  await resize(600);

  expect(screen.queryByRole("combobox", { name: "Panel" })).not.toBeInTheDocument();
  expect(screen.getByRole("tab", { name: "Diagnostics (1)" })).toHaveAttribute("aria-selected", "true");
});
