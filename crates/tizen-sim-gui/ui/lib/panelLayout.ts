// Where the Log/Network/Diagnostics panel sits next to the TV screen, and
// how much room it takes: what the developer last chose, remembered in the
// webview's localStorage, or a default for their monitor (#76).
import { useState } from "react";

/** Which side of the TV screen the panel is docked on. */
export type Dock = "bottom" | "right";

export type PanelLayout = {
  dock: Dock;
  /** The panel's share of the room it and the TV screen split, 0 to 1. */
  fraction: number;
};

/** The panel's default share: the TV screen keeps most of the room. */
export const DEFAULT_FRACTION = 0.3;

/** Monitors at least this wide for their height (21:9 is 2.37, 32:9 is 3.56) dock it on the right. */
const ULTRAWIDE_RATIO = 2.3;

/** The least the panel and the TV screen keep, in CSS px: the tabs and a few rows, and a screen still worth looking at. */
const MIN_PANEL_PX = 120;
const MIN_SCREEN_PX = 160;
/** The shares a size is kept within, whatever the room's size. */
export const MIN_FRACTION = 0.1;
export const MAX_FRACTION = 0.9;
/** How far one arrow key press moves the splitter. */
const KEY_STEP = 0.02;

const STORAGE_KEY = "tizen-sim.panel";

/** What's remembered: only what the developer chose themselves. */
type Saved = { dock?: Dock; sizes?: Partial<Record<Dock, number>> };

/** The webview's localStorage, or one that remembers nothing where even reaching it throws. */
export function browserStorage(): Storage {
  try {
    return window.localStorage;
  } catch {
    return {
      length: 0,
      clear: () => {},
      getItem: () => null,
      key: () => null,
      removeItem: () => {},
      setItem: () => {},
    };
  }
}

/** The dock for a monitor of this size, before the developer picks one. */
export function defaultDock(screen: { width: number; height: number }): Dock {
  return screen.height > 0 && screen.width / screen.height >= ULTRAWIDE_RATIO ? "right" : "bottom";
}

/**
 * The panel's share of `total` px of room, kept so neither it nor the TV
 * screen goes below its minimum; the screen's wins when both can't fit.
 * With `total` unknown (0), only the share's own bounds apply.
 */
export function clampFraction(fraction: number, total: number): number {
  let low = MIN_FRACTION;
  let high = MAX_FRACTION;
  if (total > 0) {
    low = Math.max(low, MIN_PANEL_PX / total);
    high = Math.min(high, 1 - MIN_SCREEN_PX / total);
  }
  const clamped = Math.min(Math.max(fraction, low), high);
  return Math.round(clamped * 10_000) / 10_000;
}

/** The share after `steps` arrow key presses (positive grows the panel). */
export function stepFraction(fraction: number, steps: number, total: number): number {
  return clampFraction(fraction + steps * KEY_STEP, total);
}

function read(storage: Storage): Saved {
  let raw: string | null;
  try {
    raw = storage.getItem(STORAGE_KEY);
  } catch {
    return {};
  }
  if (!raw) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
  const value = parsed as Record<string, unknown>;
  const saved: Saved = {};
  if (value.dock === "bottom" || value.dock === "right") saved.dock = value.dock;
  if (typeof value.sizes === "object" && value.sizes !== null) {
    const sizes = value.sizes as Record<string, unknown>;
    for (const dock of ["bottom", "right"] as const) {
      const size = sizes[dock];
      if (typeof size === "number" && size >= MIN_FRACTION && size <= MAX_FRACTION) {
        saved.sizes = { ...saved.sizes, [dock]: size };
      }
    }
  }
  return saved;
}

/** The layout to start with: what was remembered, else the monitor's default. */
export function loadLayout(storage: Storage, screen: { width: number; height: number }): PanelLayout {
  const saved = read(storage);
  const dock = saved.dock ?? defaultDock(screen);
  return { dock, fraction: rememberedSize(storage, dock) };
}

/** The panel's remembered size on `dock`, else the default. */
export function rememberedSize(storage: Storage, dock: Dock): number {
  return read(storage).sizes?.[dock] ?? DEFAULT_FRACTION;
}

/** Remembers the dock the developer picked. */
export function saveDock(storage: Storage, dock: Dock) {
  write(storage, { ...read(storage), dock });
}

/** Remembers the size the developer gave the panel on `dock`. */
export function saveSize(storage: Storage, dock: Dock, fraction: number) {
  const saved = read(storage);
  write(storage, { ...saved, sizes: { ...saved.sizes, [dock]: fraction } });
}

/** Forgets the panel's size on `dock`, so it takes the default again. */
export function forgetSize(storage: Storage, dock: Dock) {
  const saved = read(storage);
  const sizes = { ...saved.sizes };
  delete sizes[dock];
  write(storage, { ...saved, sizes });
}

/** Storage that can't be written (blocked, full) just doesn't remember. */
function write(storage: Storage, saved: Saved) {
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify(saved));
  } catch {
    // The layout just isn't remembered.
  }
}

/**
 * The panel's layout as App state, remembered as the developer changes it.
 * The state never depends on the storage working: a dock or size that
 * can't be remembered still applies for this run.
 */
export function usePanelLayout() {
  const [layout, setLayout] = useState(() => loadLayout(browserStorage(), window.screen));
  return {
    layout,
    /** Moves the panel to `dock`, at the size it last had there. */
    dock(dock: Dock) {
      const storage = browserStorage();
      saveDock(storage, dock);
      setLayout({ dock, fraction: rememberedSize(storage, dock) });
    },
    /** Shows the panel at `fraction` while it's being resized. */
    preview(fraction: number) {
      setLayout((l) => ({ ...l, fraction }));
    },
    /** Sets the panel's size and remembers it. */
    resize(fraction: number) {
      saveSize(browserStorage(), layout.dock, fraction);
      setLayout({ ...layout, fraction });
    },
    /** Restores the panel's default size on its dock. */
    reset() {
      forgetSize(browserStorage(), layout.dock);
      setLayout({ ...layout, fraction: DEFAULT_FRACTION });
    },
  };
}
