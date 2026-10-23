/**
 * How many selected skills have no row on screen right now.
 *
 * Multi-select acts on the filtered list, not on what is rendered: collapsing a
 * Source is a display choice, so the skills inside it stay selected while their
 * rows disappear. Bulk actions then run on more skills than the user can count
 * on screen, which is why the number has to be disclosed.
 *
 * Derived from the rendered rows rather than from the grouping state, so it
 * covers both ways a selection ends up hidden: select-all sweeping a collapsed
 * Source, and a Source collapsed after its skills were picked.
 */
export function countHiddenSelected(
  selectedIds: ReadonlySet<string>,
  renderedIds: Iterable<string>
): number {
  if (selectedIds.size === 0) return 0;
  // A set rather than a running total: the row list must not be able to count
  // one selected skill twice and report a negative remainder.
  const onScreen = new Set(renderedIds);
  let visible = 0;
  for (const id of selectedIds) {
    if (onScreen.has(id)) visible += 1;
  }
  return selectedIds.size - visible;
}
