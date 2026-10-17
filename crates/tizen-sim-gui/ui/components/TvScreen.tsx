// The TV screen area. On Windows the Rust side lays the Engine's own window
// over it (ADR-0006), so it reports the space inside its padding whenever
// that changes; the Rust side centers the screen there at 16:9. The black
// screen drawn here in the same place is only seen while no Engine window
// covers it.
import { RotateCw } from "lucide-react";
import { useEffect, useRef } from "react";
import { Button } from "@/components/ui/button";
import * as commands from "@/lib/commands";
import type { SessionView } from "@/lib/session";
import { cn } from "@/lib/utils";

/** The TV screen as drawn while no Engine window covers it. */
const BLACK_SCREEN = "flex-col items-center justify-center gap-3 bg-black p-4 text-center text-sm text-neutral-300";

export function TvScreen({
  view,
  embeds,
  onRestart,
}: {
  view: SessionView;
  /** Whether the Engine window is embedded here on this platform. */
  embeds: boolean;
  onRestart: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!embeds || !el) return;
    let last = "";
    const report = () => {
      const r = el.getBoundingClientRect();
      const area = { x: r.x, y: r.y, width: r.width, height: r.height };
      const key = JSON.stringify(area);
      if (key === last) return;
      last = key;
      commands.setScreenArea(area).catch(() => {});
    };
    report();
    // Its box moves when panels above it open or close, not only on resize.
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(report);
    observer?.observe(el);
    observer?.observe(document.body);
    window.addEventListener("resize", report);
    // A ResizeObserver misses a box that moves without changing size, and
    // the Engine window must never be left over the wrong spot: check now
    // and then too.
    const poll = setInterval(report, 500);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", report);
      clearInterval(poll);
      commands.setScreenArea(null).catch(() => {});
    };
  }, [embeds]);

  const message = <ScreenMessage view={view} embeds={embeds} onRestart={onRestart} />;
  if (!embeds) {
    return (
      <section
        aria-label="TV screen"
        className={cn("flex min-h-0 flex-1", BLACK_SCREEN)}
      >
        {message}
      </section>
    );
  }
  return (
    <section aria-label="TV screen" className="flex min-h-0 flex-1 flex-col bg-muted p-4">
      {/* The space the TV screen fits in; the screen is as large as fits at 16:9. */}
      <div ref={ref} className="flex min-h-0 flex-1 items-center justify-center [container-type:size]">
        <div className={cn("flex aspect-video w-[min(100cqw,100cqh*16/9)]", BLACK_SCREEN)}>
          {message}
        </div>
      </div>
    </section>
  );
}

function ScreenMessage({
  view,
  embeds,
  onRestart,
}: {
  view: SessionView;
  embeds: boolean;
  onRestart: () => void;
}) {
  const restart = (
    <Button variant="secondary" size="sm" onClick={onRestart}>
      <RotateCw /> Restart
    </Button>
  );
  switch (view.status.kind) {
    case "idle":
      return <p>Open an App to run it on the TV screen.</p>;
    case "starting":
      return <p>Starting…</p>;
    case "crashed":
      return (
        <>
          <p className="font-medium text-red-400">The Engine crashed.</p>
          <p>The App stopped with it. Restart runs it again on the same Version Profile.</p>
          {restart}
        </>
      );
    case "failed":
      return <p className="text-red-400">The App could not start: {view.status.error}</p>;
    case "exited":
      return (
        <>
          <p>The App is not running.</p>
          {restart}
        </>
      );
    case "asking":
    case "running":
    case "hidden":
      if (!embeds || !view.embedded) {
        return <p>The TV screen is in its own Engine window.</p>;
      }
      return view.status.kind === "hidden" ? <p>The App is in the background.</p> : null;
  }
}
