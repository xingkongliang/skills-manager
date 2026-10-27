/**
 * Which of the offered skills a gesture leaves selected.
 *
 * The dialog mirrors the library's own multi-select: a plain click toggles one,
 * a shift-click takes the whole run between the anchor and the row, and the run
 * is deselected only when everything in it was already chosen. Same gesture,
 * same rule — a second convention for the same action would be worse than
 * having none.
 */
export function toggleOne(selected: ReadonlySet<string>, key: string): Set<string> {
  const next = new Set(selected);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  return next;
}

export function toggleRange(
  selected: ReadonlySet<string>,
  ordered: readonly string[],
  anchor: string,
  to: string
): Set<string> {
  const from = ordered.indexOf(anchor);
  const end = ordered.indexOf(to);
  if (from === -1 || end === -1) return toggleOne(selected, to);

  const [lo, hi] = from <= end ? [from, end] : [end, from];
  const run = ordered.slice(lo, hi + 1);
  const everyChosen = run.every((key) => selected.has(key));

  const next = new Set(selected);
  for (const key of run) {
    if (everyChosen) next.delete(key);
    else next.add(key);
  }
  return next;
}

/** The all-or-nothing toggle behind the header button. */
export function toggleAll(selected: ReadonlySet<string>, ordered: readonly string[]): Set<string> {
  const everyChosen = ordered.length > 0 && ordered.every((key) => selected.has(key));
  return everyChosen ? new Set() : new Set(ordered);
}
