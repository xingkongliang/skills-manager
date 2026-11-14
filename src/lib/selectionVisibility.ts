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

/**
 * The My Skills texts that disclose hidden selections: the toolbar note and the
 * bulk-delete confirmation. Neither names a reason, because a selected skill
 * can lose its row to a collapsed Source or to a filter changed after it was
 * picked, and the count cannot tell which. The confirmation is one sentence
 * per case, never two joined with a space.
 */
export function hiddenSelectionTexts(
  t: (key: string, options: Record<string, number>) => string,
  selectedCount: number,
  hiddenCount: number
): { toolbarNote: string | undefined; deleteConfirm: string } {
  if (hiddenCount === 0) {
    return {
      toolbarNote: undefined,
      deleteConfirm: t("mySkills.batchDeleteConfirm", { count: selectedCount }),
    };
  }
  return {
    toolbarNote: t("mySkills.notShownCount", { count: hiddenCount }),
    deleteConfirm: t("mySkills.batchDeleteConfirmWithHidden", {
      count: selectedCount,
      hidden: hiddenCount,
    }),
  };
}
