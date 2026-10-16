// The splitter between the TV screen and the Log/Network/Diagnostics panel
// (#76): dragged with the pointer or moved with the arrow keys, it sets the
// panel's share of the area they split. A double click restores the default.
// At rest it shows a grip; it highlights once the pointer rests on it, at once
// while dragged or focus-visible (#132).
import { useEffect, useRef, useState, type RefObject } from "react";
import { clampFraction, MAX_FRACTION, MIN_FRACTION, stepFraction, type Dock } from "@/lib/panelLayout";
import { cn } from "@/lib/utils";

/** A drag in progress: its pointer, the share it started from, and the share it's at. */
type Drag = { pointer: number; from: number; at: number | null };

/** How long the pointer rests on the splitter before it highlights, in ms: one passing over leaves it alone. */
const HOVER_DELAY_MS = 300;

export function Splitter({
  dock,
  fraction,
  splitArea,
  controls,
  onPreview,
  onResize,
  onReset,
}: {
  dock: Dock;
  fraction: number;
  /** The element the TV screen and the panel split, measured to turn pointer positions into shares. */
  splitArea: RefObject<HTMLElement | null>;
  /** The panel's id. */
  controls: string;
  /** The share while a drag moves it: at most once per frame. */
  onPreview: (fraction: number) => void;
  /** The share it ended at, to remember. */
  onResize: (fraction: number) => void;
  onReset: () => void;
}) {
  const drag = useRef<Drag | null>(null);
  const frame = useRef(0);
  const hoverTimer = useRef(0);
  const [hovered, setHovered] = useState(false);
  const [dragging, setDragging] = useState(false);
  useEffect(
    () => () => {
      cancelAnimationFrame(frame.current);
      clearTimeout(hoverTimer.current);
    },
    [],
  );

  /** The split area's size along the splitter's axis, in CSS px; 0 before layout. */
  const total = () => {
    const rect = splitArea.current?.getBoundingClientRect();
    if (!rect) return 0;
    return dock === "bottom" ? rect.height : rect.width;
  };

  /** The panel's share with the splitter at this pointer position: the panel is below or right of it. */
  const shareAt = (clientX: number, clientY: number) => {
    const rect = splitArea.current?.getBoundingClientRect();
    if (!rect || rect.width === 0 || rect.height === 0) return null;
    const share = dock === "bottom" ? (rect.bottom - clientY) / rect.height : (rect.right - clientX) / rect.width;
    return clampFraction(share, total());
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const current = drag.current;
    if (current?.pointer !== e.pointerId) return;
    const next = shareAt(e.clientX, e.clientY);
    if (next === null) return;
    current.at = next;
    // Every move would re-lay out the page and move the embedded Engine
    // window (TvScreen reports its area); once per frame keeps up.
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => onPreview(next));
  };

  /** Ends the drag: remembered where it was released, undone where it was cancelled. */
  const endDrag = (e: React.PointerEvent, released: boolean) => {
    const current = drag.current;
    if (current?.pointer !== e.pointerId) return;
    drag.current = null;
    setDragging(false);
    cancelAnimationFrame(frame.current);
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    // A click that didn't move changes nothing.
    if (current.at === null) return;
    if (released) onResize(current.at);
    else onPreview(current.from);
  };

  /** Arrow keys toward the panel's side shrink it; away from it grow it. */
  const onKeyDown = (e: React.KeyboardEvent) => {
    const grow = dock === "bottom" ? "ArrowUp" : "ArrowLeft";
    const shrink = dock === "bottom" ? "ArrowDown" : "ArrowRight";
    if (e.key !== grow && e.key !== shrink) return;
    e.preventDefault();
    onResize(stepFraction(fraction, e.key === grow ? 1 : -1, total()));
  };

  const orientation = dock === "bottom" ? "horizontal" : "vertical";
  const horizontal = orientation === "horizontal";
  return (
    <div
      role="separator"
      aria-label="Resize the panel"
      aria-controls={controls}
      // The separator's orientation is its own: a bottom panel's is a horizontal line.
      aria-orientation={orientation}
      aria-valuenow={Math.round(fraction * 100)}
      aria-valuemin={MIN_FRACTION * 100}
      aria-valuemax={MAX_FRACTION * 100}
      tabIndex={0}
      title="Drag to resize; double-click to restore"
      data-highlighted={hovered || dragging || undefined}
      className={cn(
        // Above its neighbours: the highlight and the grip spill past the 1 px line.
        "group/splitter relative z-10 shrink-0 touch-none bg-border outline-none",
        // A 1 px line with a wider hit area.
        horizontal
          ? "h-px cursor-row-resize after:absolute after:inset-x-0 after:-inset-y-1"
          : "w-px cursor-col-resize after:absolute after:inset-y-0 after:-inset-x-1",
      )}
      onPointerEnter={() => {
        clearTimeout(hoverTimer.current);
        hoverTimer.current = window.setTimeout(() => setHovered(true), HOVER_DELAY_MS);
      }}
      onPointerLeave={() => {
        clearTimeout(hoverTimer.current);
        setHovered(false);
      }}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        drag.current = { pointer: e.pointerId, from: fraction, at: null };
        setDragging(true);
        // Keeps the moves coming when the pointer leaves the thin line.
        e.currentTarget.setPointerCapture?.(e.pointerId);
      }}
      onPointerMove={onPointerMove}
      onPointerUp={(e) => endDrag(e, true)}
      onPointerCancel={(e) => endDrag(e, false)}
      // Capture lost without a pointerup (another window took it): as if cancelled.
      onLostPointerCapture={(e) => endDrag(e, false)}
      onKeyDown={onKeyDown}
      onDoubleClick={onReset}
    >
      {/* The highlight: thicker than the line and drawn over it, so the layout doesn't move. */}
      <span
        aria-hidden
        className={cn(
          "pointer-events-none absolute bg-primary opacity-0",
          "group-data-[highlighted]/splitter:opacity-100 group-focus-visible/splitter:opacity-100",
          horizontal ? "inset-x-0 top-1/2 h-1 -translate-y-1/2" : "inset-y-0 left-1/2 w-1 -translate-x-1/2",
        )}
      />
      {/* The grip: dots along the line at its centre, muted so they don't draw the eye. */}
      <span
        aria-hidden
        data-testid="splitter-grip"
        data-orientation={orientation}
        className={cn(
          "pointer-events-none absolute top-1/2 left-1/2 flex -translate-x-1/2 -translate-y-1/2 gap-0.5",
          horizontal ? "flex-row" : "flex-col",
        )}
      >
        {[0, 1, 2].map((dot) => (
          <span key={dot} className="size-0.75 rounded-full bg-muted-foreground/70" />
        ))}
      </span>
    </div>
  );
}
