// GUI smoke tests: the real App against a stand-in for the GUI's Rust side that
// records commands and emits Session events as the Rust side would.
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { act, render, screen } from "@testing-library/react";
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

async function sessionStarted(session: number, source: string) {
  await act(() => emit("session-event", { SessionStarted: { session, source } }));
}

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
