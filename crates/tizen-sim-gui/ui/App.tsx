import { open, save } from "@tauri-apps/plugin-dialog";
import { Camera, FolderOpen, Globe, MonitorPlay, Package, PanelBottom, PanelRight, RotateCw } from "lucide-react";
import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { DevServerForm } from "@/components/DevServerForm";
import { DiagnosticsPanel, LogPanel, NetworkPanel } from "@/components/Panels";
import { Remote } from "@/components/Remote";
import { RunningSessions } from "@/components/RunningSessions";
import { Splitter } from "@/components/Splitter";
import { TvScreen } from "@/components/TvScreen";
import { UpdateButton } from "@/components/Update";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import * as commands from "@/lib/commands";
import { usePanelLayout } from "@/lib/panelLayout";
import { initialView, reduce, type SessionView, type Status } from "@/lib/session";
import { useFits } from "@/lib/useFits";
import { cn } from "@/lib/utils";

/** The panel open under the toolbar, if any. Panels sit beside the TV screen, never over it. */
type Panel = "devServer" | "sessions" | null;

const PANEL_ID = "session-panel";

/** A tab of the Log/Network/Diagnostics panel. */
type SessionPanelTab = "log" | "network" | "diagnostics";

/** The session panel's tabs, each labelled with what it lists. */
const SESSION_PANEL_TABS: { value: SessionPanelTab; label: (view: SessionView) => string }[] = [
  { value: "log", label: () => "Log" },
  { value: "network", label: (view) => `Network (${view.network.length})` },
  { value: "diagnostics", label: (view) => `Diagnostics (${view.diagnostics.length})` },
];

export function App() {
  const [view, dispatch] = useReducer(reduce, initialView);
  const [profiles, setProfiles] = useState<commands.ProfileInfo[]>([]);
  const [embeds, setEmbeds] = useState(false);
  const [panel, setPanel] = useState<Panel>(null);
  /** The last command that failed, and why. */
  const [problem, setProblem] = useState<string | null>(null);
  const showProblem = useCallback((e: unknown) => setProblem(String(e)), []);
  /** Where the Log/Network/Diagnostics panel sits, and its share of the area it splits with the TV screen. */
  const sessionPanel = usePanelLayout();
  const splitArea = useRef<HTMLElement>(null);
  /** The session panel's open tab. */
  const [tab, setTab] = useState<SessionPanelTab>("log");
  const panelHeader = useRef<HTMLDivElement>(null);
  const panelTabs = useRef<HTMLDivElement>(null);
  const dockButton = useRef<HTMLButtonElement>(null);
  const tabsFit = useFits(panelHeader, [panelTabs, dockButton]);

  useEffect(() => {
    const unlisten = commands.onSessionEvent(dispatch);
    commands.profiles().then(setProfiles, showProblem);
    commands.embedsScreen().then((on) => setEmbeds(on === true), showProblem);
    return () => {
      unlisten.then((stop) => stop());
    };
  }, [showProblem]);

  /** Runs a command, showing why if it fails. */
  const run = (action: () => Promise<unknown>) => {
    setProblem(null);
    action().catch(showProblem);
  };

  const toggle = (which: Panel) => setPanel((shown) => (shown === which ? null : which));

  const openPath = (directory: boolean) =>
    run(async () => {
      const path = await open(
        directory ? { directory: true } : { filters: [{ name: "Tizen Package", extensions: ["wgt"] }] },
      );
      if (typeof path === "string") await commands.openApp({ kind: "path", path });
    });

  const takeScreenshot = () =>
    run(async () => {
      const path = await save({
        defaultPath: "screenshot.png",
        filters: [{ name: "PNG image", extensions: ["png"] }],
      });
      if (path) await commands.screenshot(path);
    });

  const dockedBottom = sessionPanel.layout.dock === "bottom";
  const dockLabel = dockedBottom ? "Dock the panel on the right" : "Dock the panel at the bottom";
  const hasSession = view.session !== null;
  const loaded = view.status.kind === "running" || view.status.kind === "hidden";

  return (
    <div className="flex h-full flex-col">
      <header className="flex flex-wrap items-center gap-2 border-b px-3 py-2">
        <Button variant="outline" size="sm" onClick={() => openPath(true)}>
          <FolderOpen /> Open folder…
        </Button>
        <Button variant="outline" size="sm" onClick={() => openPath(false)}>
          <Package /> Open package…
        </Button>
        <Button variant="outline" size="sm" aria-pressed={panel === "devServer"} onClick={() => toggle("devServer")}>
          <Globe /> Dev server…
        </Button>
        <Button variant="outline" size="sm" aria-pressed={panel === "sessions"} onClick={() => toggle("sessions")}>
          <MonitorPlay /> Running Sessions…
        </Button>

        <div className="mx-2 h-6 border-l" />

        <label className="flex items-center gap-2 text-sm">
          Version Profile
          <select
            aria-label="Version Profile"
            className="h-8 rounded-md border bg-background px-2 text-sm disabled:opacity-50"
            // An adopted Session's owner chose its profile.
            disabled={!hasSession || view.adopted}
            value={view.profile ?? ""}
            onChange={(e) => run(() => commands.switchProfile(e.target.value))}
          >
            {view.profile === null && <option value="">—</option>}
            {profiles.map((p) => (
              <option key={p.version} value={p.version}>
                Tizen {p.version} (M{p.milestone}){p.latest ? ", latest" : ""}
              </option>
            ))}
          </select>
        </label>
        <Button variant="outline" size="sm" disabled={!loaded} onClick={() => run(commands.reload)}>
          <RotateCw /> Reload
        </Button>
        <Button variant="outline" size="sm" disabled={!loaded} onClick={takeScreenshot}>
          <Camera /> Screenshot
        </Button>

        <div className="ml-auto flex items-center gap-2 text-sm">
          <UpdateButton onError={showProblem} />
          {view.appId && <span className="font-medium">{view.appId}</span>}
          {view.adopted && <Badge variant="outline">Started from the CLI</Badge>}
          <StatusBadge status={view.status} />
        </div>
      </header>

      {panel === "devServer" && (
        <DevServerForm
          onClose={() => setPanel(null)}
          onOpen={(url, config) => run(() => commands.openApp({ kind: "devServer", url, config }))}
        />
      )}
      {panel === "sessions" && (
        <RunningSessions
          onClose={() => setPanel(null)}
          onError={showProblem}
          onAdopt={(id) => {
            setPanel(null);
            run(() => commands.adopt(id));
          }}
        />
      )}

      {problem && (
        <p role="alert" className="border-b bg-red-50 px-3 py-1 text-sm text-red-700 dark:bg-red-950 dark:text-red-300">
          {problem}
        </p>
      )}

      <div className="flex min-h-0 flex-1">
        <aside className="w-60 shrink-0 overflow-auto border-r p-3">
          <Remote disabled={!loaded} onPress={(key) => run(() => commands.pressKey(key))} />
        </aside>

        <main ref={splitArea} className={cn("flex min-h-0 min-w-0 flex-1", dockedBottom ? "flex-col" : "flex-row")}>
          {/* The TV screen takes what the panel leaves, down to its minimum (MIN_SCREEN_PX in panelLayout.ts). */}
          <div className={cn("flex min-h-0 min-w-0 flex-1 flex-col", dockedBottom ? "min-h-40" : "min-w-40")}>
            <TvScreen view={view} embeds={embeds} onRestart={() => run(commands.restart)} />
          </div>
          <Splitter
            dock={sessionPanel.layout.dock}
            fraction={sessionPanel.layout.fraction}
            splitArea={splitArea}
            controls={PANEL_ID}
            onPreview={sessionPanel.preview}
            onResize={sessionPanel.resize}
            onReset={sessionPanel.reset}
          />
          <Tabs
            id={PANEL_ID}
            value={tab}
            onValueChange={(value) => setTab(value as SessionPanelTab)}
            // Grows no further than its share, but gives up the splitter's pixel when the screen is at its
            // minimum; never below MIN_PANEL_PX (panelLayout.ts).
            className={cn("min-h-0 min-w-0 shrink grow-0 gap-0 overflow-hidden", dockedBottom ? "min-h-30" : "min-w-30")}
            style={{ flexBasis: `${sessionPanel.layout.fraction * 100}%` }}
          >
            <div ref={panelHeader} data-slot="session-panel-header" className="relative flex items-center gap-2 p-2">
              {/* Where the tabs don't fit on one row, a dropdown stands in for them. The tabs stay, hidden, so
                  their width is still measured and they still name the open panel. */}
              <TabsList
                ref={panelTabs}
                aria-hidden={!tabsFit || undefined}
                className={cn("shrink-0", !tabsFit && "invisible absolute")}
              >
                {SESSION_PANEL_TABS.map(({ value, label }) => (
                  <TabsTrigger key={value} value={value}>
                    {label(view)}
                  </TabsTrigger>
                ))}
              </TabsList>
              {!tabsFit && (
                <select
                  aria-label="Panel"
                  aria-controls={PANEL_ID}
                  className="h-8 min-w-0 rounded-md border bg-background px-2 text-sm"
                  value={tab}
                  onChange={(e) => setTab(e.target.value as SessionPanelTab)}
                >
                  {SESSION_PANEL_TABS.map(({ value, label }) => (
                    <option key={value} value={value}>
                      {label(view)}
                    </option>
                  ))}
                </select>
              )}
              <Button
                ref={dockButton}
                variant="ghost"
                size="icon-sm"
                className="ml-auto shrink-0"
                aria-label={dockLabel}
                title={dockLabel}
                onClick={() => sessionPanel.dock(dockedBottom ? "right" : "bottom")}
              >
                {dockedBottom ? <PanelRight /> : <PanelBottom />}
              </Button>
            </div>
            <TabsContent value="log" className="min-h-0 border-t">
              <LogPanel log={view.log} />
            </TabsContent>
            <TabsContent value="network" className="min-h-0 border-t">
              <NetworkPanel requests={view.network} />
            </TabsContent>
            <TabsContent value="diagnostics" className="min-h-0 border-t">
              <DiagnosticsPanel diagnostics={view.diagnostics} />
            </TabsContent>
          </Tabs>
        </main>
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: Status }) {
  switch (status.kind) {
    case "idle":
      return <Badge variant="outline">No App open</Badge>;
    case "starting": {
      const d = status.download;
      if (!d) return <Badge variant="secondary">Starting…</Badge>;
      const mb = (n: number) => Math.round(n / (1024 * 1024));
      return (
        <Badge variant="secondary">
          Downloading Engine{" "}
          {d.total ? `${Math.floor((d.received * 100) / d.total)}%` : `${mb(d.received)} MB`}
        </Badge>
      );
    }
    case "running":
      return <Badge>Running</Badge>;
    case "hidden":
      return <Badge variant="secondary">In the background</Badge>;
    case "failed":
      return <Badge variant="destructive">Failed</Badge>;
    case "crashed":
      return <Badge variant="destructive">Crashed</Badge>;
    case "exited":
      return <Badge variant="outline">Exited</Badge>;
  }
}
