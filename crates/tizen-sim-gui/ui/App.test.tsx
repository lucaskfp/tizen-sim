// GUI smoke tests: the real App against a stand-in for the GUI's Rust side that
// records commands and emits Session events as the Rust side would.
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { act, render, screen, waitFor, within } from "@testing-library/react";
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
