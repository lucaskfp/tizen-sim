// What the GUI shows, folded from the Session events the Rust side emits.
// The types mirror the core's serde output (externally tagged enums).

export type GuiEvent =
  | { SessionStarted: { session: number; source: string; adopted: boolean } }
  | { Session: { session: number; event: SessionEvent } }
  | { Screen: { session: number; embedded: boolean } };

export type SessionEvent =
  | { Lifecycle: Lifecycle }
  | { Console: { level: string; text: string } }
  | { EngineDownload: { received: number; total: number | null } }
  | { Diagnostic: Diagnostic }
  | { RemoteKey: { key: string; outcome: KeyOutcome } }
  | { HotReload: HotReload }
  | { Network: NetworkRequest };

export type Lifecycle =
  | { Starting: { app_id: string; profile: string } }
  | { EngineReady: { engine_version: string; debugger_url: string; devtools_url: string } }
  | { GamepadRequired: { models: string[] } }
  | { AppLoaded: { url: string } }
  | "Reloaded"
  | "Hidden"
  | "Shown"
  | { Failed: { error: string } }
  | { Exited: { reason: ExitReason } };

export type ExitReason =
  | "Stopped"
  | "EngineClosed"
  | "EngineCrashed"
  | "FailedToStart"
  | "AppExited"
  | "TerminatedByRemote"
  | "LaunchCanceled";

export type KeyOutcome = "Delivered" | { Dropped: { reason: string } };

export type HotReload =
  | { FilesChanged: { paths: string[] } }
  | "StylesSwapped"
  | "Reloaded"
  | { Restarted: { app_id: string } }
  | { Failed: { error: string } };

export type Diagnostic = { code: string; message: string };

export type NetworkRequest = {
  method: string;
  url: string;
  status: number | null;
  duration: { secs: number; nanos: number };
  verdict: "Allowed" | { Blocked: { missing_access: string } };
};

export type LogLevel = "debug" | "info" | "warning" | "error";

export type LogLine = {
  id: number;
  /** Where the line comes from: the App's console, a Remote Key, or the simulator. */
  source: "console" | "key" | "session";
  level: LogLevel;
  text: string;
};

export type Status =
  | { kind: "idle" }
  | { kind: "starting"; download?: { received: number; total: number | null } }
  /** The TV's popup asks for a required gamepad before launching the App. */
  | { kind: "asking" }
  | { kind: "running" }
  | { kind: "hidden" }
  | { kind: "failed"; error: string }
  | { kind: "crashed" }
  | { kind: "exited"; reason: ExitReason };

export type SessionView = {
  /** The Session shown; events of other Sessions are ignored. */
  session: number | null;
  source: string | null;
  /** Started from the CLI and adopted, rather than started by the GUI. */
  adopted: boolean;
  /** Whether the Engine window is on the GUI's screen (ADR-0006). */
  embedded: boolean;
  appId: string | null;
  profile: string | null;
  status: Status;
  log: LogLine[];
  network: (NetworkRequest & { id: number })[];
  diagnostics: (Diagnostic & { id: number })[];
  nextId: number;
};

export const initialView: SessionView = {
  session: null,
  source: null,
  adopted: false,
  embedded: false,
  appId: null,
  profile: null,
  status: { kind: "idle" },
  log: [],
  network: [],
  diagnostics: [],
  nextId: 0,
};

/** Lines kept in the log; the oldest go first. */
const LOG_LIMIT = 5000;

export function reduce(view: SessionView, event: GuiEvent): SessionView {
  if ("SessionStarted" in event) {
    const { session, source, adopted } = event.SessionStarted;
    // A new Session starts with fresh panels; the log keeps going, so a
    // profile switch can be compared with what came before.
    return withLog(
      {
        ...initialView,
        session,
        source,
        adopted,
        // The same App restarting, e.g. on another profile, shows its last
        // profile until the new Session says which it runs on.
        profile: source === view.source ? view.profile : null,
        status: { kind: "starting" },
        log: view.log,
        nextId: view.nextId,
      },
      "session",
      "info",
      adopted ? `Adopting ${source}, started from the CLI` : `Opening ${source}`,
    );
  }
  if ("Screen" in event) {
    const { session, embedded } = event.Screen;
    return session === view.session ? { ...view, embedded } : view;
  }
  const { session, event: inner } = event.Session;
  if (session !== view.session) return view;

  if ("Console" in inner) {
    return withLog(view, "console", consoleLevel(inner.Console.level), inner.Console.text);
  }
  if ("EngineDownload" in inner) {
    return { ...view, status: { kind: "starting", download: inner.EngineDownload } };
  }
  if ("Diagnostic" in inner) {
    const { code, message } = inner.Diagnostic;
    const next = withLog(view, "session", "warning", `[${code}] ${message}`);
    return {
      ...next,
      diagnostics: [...next.diagnostics, { code, message, id: next.nextId }],
      nextId: next.nextId + 1,
    };
  }
  if ("RemoteKey" in inner) {
    const { key, outcome } = inner.RemoteKey;
    return outcome === "Delivered"
      ? withLog(view, "key", "debug", `${key} delivered`)
      : withLog(view, "key", "warning", `${key} dropped: ${outcome.Dropped.reason}`);
  }
  if ("HotReload" in inner) {
    return withLog(view, "session", ...hotReloadLine(inner.HotReload));
  }
  if ("Network" in inner) {
    return {
      ...view,
      network: [...view.network, { ...inner.Network, id: view.nextId }],
      nextId: view.nextId + 1,
    };
  }
  return lifecycle(view, inner.Lifecycle);
}

function lifecycle(view: SessionView, event: Lifecycle): SessionView {
  if (event === "Reloaded") return withLog(view, "session", "info", "App reloaded.");
  if (event === "Hidden") {
    return withLog({ ...view, status: { kind: "hidden" } }, "session", "info", "The App is hidden (in the background).");
  }
  if (event === "Shown") {
    return withLog({ ...view, status: { kind: "running" } }, "session", "info", "The App is back in the foreground.");
  }
  if ("Starting" in event) {
    const { app_id, profile } = event.Starting;
    return withLog({ ...view, appId: app_id, profile }, "session", "info", `Starting ${app_id} on Tizen ${profile}`);
  }
  if ("EngineReady" in event) {
    return withLog(
      { ...view, status: { kind: "starting" } },
      "session",
      "info",
      `Engine ready: ${event.EngineReady.engine_version}`,
    );
  }
  if ("GamepadRequired" in event) {
    const models = event.GamepadRequired.models.join(" or ");
    return withLog(
      { ...view, status: { kind: "asking" } },
      "session",
      "info",
      `The App requires a ${models} gamepad and none is connected: the TV asks before launching it.`,
    );
  }
  if ("AppLoaded" in event) {
    return withLog({ ...view, status: { kind: "running" } }, "session", "info", `App loaded: ${event.AppLoaded.url}`);
  }
  if ("Failed" in event) {
    const { error } = event.Failed;
    return withLog({ ...view, status: { kind: "failed", error } }, "session", "error", error);
  }
  const { reason } = event.Exited;
  // A failed start already said why.
  const status: Status =
    view.status.kind === "failed"
      ? view.status
      : reason === "EngineCrashed"
        ? { kind: "crashed" }
        : { kind: "exited", reason };
  // A Session that ended before saying which profile it runs on, e.g. one the
  // App's required_version refused, ran on none: the App's last profile kept
  // at SessionStarted would leave the switcher unable to pick it again.
  const profile = view.appId === null ? null : view.profile;
  return reason === "FailedToStart"
    ? { ...view, status, profile, embedded: false }
    : withLog(
        { ...view, status, profile, embedded: false },
        "session",
        reason === "EngineCrashed" ? "error" : "info",
        exitText[reason],
      );
}

const exitText: Record<ExitReason, string> = {
  Stopped: "Stopped.",
  EngineClosed: "The Engine window was closed.",
  EngineCrashed: "The Engine crashed or was killed.",
  FailedToStart: "The Session failed to start.",
  AppExited: "The App exited.",
  TerminatedByRemote: "Exit was pressed or Back held: the App was terminated, as on a TV.",
  LaunchCanceled: "The App was not launched.",
};

function hotReloadLine(event: HotReload): [LogLevel, string] {
  if (event === "StylesSwapped") return ["info", "Stylesheets swapped in place."];
  if (event === "Reloaded") return ["info", "App reloaded."];
  if ("FilesChanged" in event) return ["info", `Changed: ${event.FilesChanged.paths.join(", ")}`];
  if ("Restarted" in event) return ["info", `config.xml changed: restarted ${event.Restarted.app_id}.`];
  return ["error", `Hot reload failed: ${event.Failed.error}`];
}

/** The console method (`log`, `warn`, ...) or the Engine's own level. */
function consoleLevel(level: string): LogLevel {
  switch (level) {
    case "error":
    case "assert":
      return "error";
    case "warning":
    case "warn":
      return "warning";
    case "debug":
    case "verbose":
      return "debug";
    default:
      return "info";
  }
}

function withLog(view: SessionView, source: LogLine["source"], level: LogLevel, text: string): SessionView {
  const log = [...view.log, { id: view.nextId, source, level, text }];
  return {
    ...view,
    log: log.length > LOG_LIMIT ? log.slice(log.length - LOG_LIMIT) : log,
    nextId: view.nextId + 1,
  };
}
