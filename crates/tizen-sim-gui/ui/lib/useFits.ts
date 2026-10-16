// Whether a row's items fit on it, for layouts that change shape rather than wrap.
import { type RefObject, useLayoutEffect, useState } from "react";

/**
 * Whether `items` fit side by side on one row of `row`'s content box, with its column gap between
 * them, measured on every resize of the row or the items: what they need changes with their text,
 * so no fixed breakpoint would do. Each item keeps its natural width, in or out of the flow.
 * Without a ResizeObserver, assumes they fit.
 */
export function useFits(row: RefObject<HTMLElement | null>, items: RefObject<HTMLElement | null>[]): boolean {
  const [fits, setFits] = useState(true);

  useLayoutEffect(() => {
    const el = row.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const measure = () => {
      const style = getComputedStyle(el);
      const px = (value: string) => parseFloat(value) || 0;
      const available = el.clientWidth - px(style.paddingLeft) - px(style.paddingRight);
      const shown = items.map((item) => item.current).filter((item) => item !== null);
      const needed =
        shown.reduce((sum, item) => sum + item.offsetWidth, 0) + px(style.columnGap) * (shown.length - 1);
      setFits(needed <= available);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    for (const item of items) if (item.current) observer.observe(item.current);
    return () => observer.disconnect();
    // The refs are stable; what they point at is read on each resize.
  }, []);

  return fits;
}
