import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";

/** How many rows stay visible before the tail folds away. */
export const FOLDED_ROWS = 2;

/** The height the first `rows` rows of a wrapping flex container need. */
export function heightForRows(rowHeight: number, gap: number, rows: number): number {
  return rowHeight * rows + gap * (rows - 1);
}

export interface RowFold {
  /** Height of the rows that stay visible; 0 until measured. */
  height: number;
  /** Index of the first child that falls below the fold, or null when nothing does. */
  firstFoldedIndex: number | null;
}

const NOTHING_FOLDED: RowFold = { height: 0, firstFoldedIndex: null };

/**
 * Where a wrapping list has to fold, measured from the rendered DOM.
 *
 * The fold point is geometric, not a count: the same seven tags are two rows in
 * the narrow drawer and one row on the full-width page, and the text-size
 * setting moves it again. Per-child geometry (rather than a single overflow
 * test) is what lets the caller keep the visible part interactive and fold only
 * the tail.
 *
 * Two guards keep the measurement from feeding itself. The result is only
 * written when it actually changed, and a measurement only runs when the
 * content or the container's width changed — folding changes the container's
 * height, which fires the ResizeObserver, which would otherwise measure, write
 * the same numbers back, re-render, and do it again.
 */
export function useRowOverflow(contentKey: string) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [fold, setFold] = useState<RowFold>(NOTHING_FOLDED);
  const lastMeasured = useRef({ contentKey: "", width: -1 });

  const measure = useCallback(
    (force = false) => {
      const node = ref.current;
      const children = node ? (Array.from(node.children) as HTMLElement[]) : [];
      if (!node || children.length === 0) {
        setFold((prev) => (prev === NOTHING_FOLDED ? prev : NOTHING_FOLDED));
        return;
      }

      const width = node.getBoundingClientRect().width;
      const last = lastMeasured.current;
      if (!force && last.contentKey === contentKey && Math.abs(last.width - width) < 0.5) {
        return;
      }
      lastMeasured.current = { contentKey, width };

      const rowGap = parseFloat(getComputedStyle(node).rowGap || "0") || 0;
      const height = heightForRows(children[0].offsetHeight, rowGap, FOLDED_ROWS);
      // Children flow in order, so the first one whose centre sits below the
      // fold bounds everything that follows.
      const top = node.getBoundingClientRect().top;
      const found = children.findIndex((child) => {
        const rect = child.getBoundingClientRect();
        return rect.top + rect.height / 2 > top + height;
      });
      const firstFoldedIndex = found === -1 ? null : found;

      setFold((prev) =>
        prev.height === height && prev.firstFoldedIndex === firstFoldedIndex
          ? prev
          : { height, firstFoldedIndex }
      );
    },
    [contentKey]
  );

  // Before the first paint: a fold applied one frame later is a visible jump
  // from five rows to two. The measurement above is guarded, so it cannot
  // cascade — the rule cannot see that far.
  useLayoutEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    measure(true);
  }, [measure]);

  useEffect(() => {
    const node = ref.current;
    if (!node) return;

    if (typeof ResizeObserver === "undefined") {
      const frame = requestAnimationFrame(() => measure());
      return () => cancelAnimationFrame(frame);
    }

    const observer = new ResizeObserver(() => measure());
    observer.observe(node);
    return () => observer.disconnect();
  }, [measure]);

  return { ref, ...fold };
}
