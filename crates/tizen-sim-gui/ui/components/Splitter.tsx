// The splitter between the TV screen and the Log/Network/Diagnostics panel
// (#76): dragged with the pointer or moved with the arrow keys, it sets the
// panel's share of the area they split. A double click restores the default.
import { useEffect, useRef, type RefObject } from "react";
import { clampFraction, MAX_FRACTION, MIN_FRACTION, stepFraction, type Dock } from "@/lib/panelLayout";
import { cn } from "@/lib/utils";

/** A drag in progress: its pointer, the share it started from, and the share it's at. */
type Drag = { pointer: number; from: number; at: number | null };

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
  useEffect(() => () => cancelAnimationFrame(frame.current), []);

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

  return (
    <div
      role="separator"
      aria-label="Resize the panel"
      aria-controls={controls}
      // The separator's orientation is its own: a bottom panel's is a horizontal line.
      aria-orientation={dock === "bottom" ? "horizontal" : "vertical"}
      aria-valuenow={Math.round(fraction * 100)}
      aria-valuemin={MIN_FRACTION * 100}
      aria-valuemax={MAX_FRACTION * 100}
      tabIndex={0}
      title="Drag to resize; double-click to restore"
      className={cn(
        "relative shrink-0 touch-none bg-border outline-none transition-colors",
        "hover:bg-primary/40 focus-visible:bg-primary/60 active:bg-primary/60",
        // A 1 px line with a wider hit area.
        dock === "bottom"
          ? "h-px cursor-row-resize after:absolute after:inset-x-0 after:-inset-y-1"
          : "w-px cursor-col-resize after:absolute after:inset-y-0 after:-inset-x-1",
      )}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        drag.current = { pointer: e.pointerId, from: fraction, at: null };
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
    />
  );
}
