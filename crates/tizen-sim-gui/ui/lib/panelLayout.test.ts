// The panel layout's rules (#76): the monitor's default dock, what's
// remembered and how, and the limits on its size.
import {
  DEFAULT_FRACTION,
  clampFraction,
  defaultDock,
  forgetSize,
  loadLayout,
  saveDock,
  saveSize,
  stepFraction,
} from "./panelLayout";

/** A Storage holding `items`, like the webview's localStorage. */
function storage(items: Record<string, string> = {}): Storage {
  const map = new Map(Object.entries(items));
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key) => map.get(key) ?? null,
    key: (i) => [...map.keys()][i] ?? null,
    removeItem: (key) => void map.delete(key),
    setItem: (key, value) => void map.set(key, value),
  };
}

const UHD = { width: 3840, height: 2160 };
const ULTRAWIDE = { width: 3440, height: 1440 };

test("an ultrawide monitor docks the panel on the right, others at the bottom", () => {
  expect(defaultDock(ULTRAWIDE)).toBe("right");
  expect(defaultDock({ width: 5120, height: 1440 })).toBe("right"); // 32:9
  expect(defaultDock({ width: 2560, height: 1080 })).toBe("right"); // 21:9 (2.37)
  expect(defaultDock(UHD)).toBe("bottom");
  expect(defaultDock({ width: 1920, height: 1200 })).toBe("bottom");
  // No monitor size (a test environment): the usual layout.
  expect(defaultDock({ width: 0, height: 0 })).toBe("bottom");
});

test("with nothing remembered, the panel takes the default share of the monitor's dock", () => {
  expect(loadLayout(storage(), UHD)).toEqual({ dock: "bottom", fraction: DEFAULT_FRACTION });
  expect(loadLayout(storage(), ULTRAWIDE)).toEqual({ dock: "right", fraction: DEFAULT_FRACTION });
});

test("a remembered dock wins over the monitor's, and each dock remembers its own size", () => {
  const saved = storage();
  saveSize(saved, "right", 0.45);
  saveDock(saved, "bottom");
  expect(loadLayout(saved, ULTRAWIDE)).toEqual({ dock: "bottom", fraction: DEFAULT_FRACTION });

  saveSize(saved, "bottom", 0.2);
  saveDock(saved, "right");
  expect(loadLayout(saved, UHD)).toEqual({ dock: "right", fraction: 0.45 });
  saveDock(saved, "bottom");
  expect(loadLayout(saved, UHD)).toEqual({ dock: "bottom", fraction: 0.2 });
});

test("a size reset to the default is forgotten", () => {
  const saved = storage();
  saveSize(saved, "bottom", 0.5);
  forgetSize(saved, "bottom");
  expect(loadLayout(saved, ULTRAWIDE)).toEqual({ dock: "right", fraction: DEFAULT_FRACTION });
});

test("what can't be read is ignored", () => {
  for (const junk of ["not json", "[]", '{"dock":"left"}', '{"sizes":{"bottom":7}}', '{"sizes":{"bottom":"x"}}']) {
    expect(loadLayout(storage({ "tizen-sim.panel": junk }), UHD)).toEqual({
      dock: "bottom",
      fraction: DEFAULT_FRACTION,
    });
  }
  const throwing = { ...storage(), getItem: () => { throw new Error("blocked"); }, setItem: () => { throw new Error("blocked"); } };
  expect(loadLayout(throwing, UHD)).toEqual({ dock: "bottom", fraction: DEFAULT_FRACTION });
  expect(() => saveDock(throwing, "right")).not.toThrow();
  expect(() => saveSize(throwing, "right", 0.4)).not.toThrow();
});

test("neither the TV screen nor the panel can be dragged below its minimum size", () => {
  // 1000 px to share: the panel keeps 120 px, the screen 160 px.
  expect(clampFraction(0.05, 1000)).toBe(0.12);
  expect(clampFraction(0.95, 1000)).toBe(0.84);
  expect(clampFraction(0.5, 1000)).toBe(0.5);
  // Too little room for both minimums: the screen's wins.
  expect(clampFraction(0.5, 200)).toBe(0.2);
  // Unknown size (not laid out yet): only the fraction's own bounds.
  expect(clampFraction(0.01, 0)).toBe(0.1);
  expect(clampFraction(0.99, 0)).toBe(0.9);
});

test("each arrow key press moves the splitter a step, within the limits", () => {
  expect(stepFraction(0.3, 1, 1000)).toBeCloseTo(0.32);
  expect(stepFraction(0.3, -1, 1000)).toBeCloseTo(0.28);
  expect(stepFraction(0.83, 1, 1000)).toBe(0.84);
  expect(stepFraction(0.13, -1, 1000)).toBe(0.12);
});
