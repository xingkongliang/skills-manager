# 01: Count the selected skills that are not on screen

**What to build:** A single source of truth for "how many of the selected skills
are hidden right now": given the selected ids and the skill rows actually
rendered, it returns how many selected skills have no row. `npm test` proves it
for every way a selection can end up hidden.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] `countHiddenSelected(selectedIds, renderedIds)` exists and is pure
- [x] Returns 0 for an empty selection, and 0 when every selected skill is rendered
- [x] Covers select-all over a collapsed Source
- [x] Covers a Source collapsed after its skills were picked
- [x] Covers grouping off, and search forcing every Source open (both hide nothing)
- [x] Covers selected skills that were deleted meanwhile
- [x] `npm test` green
- [x] Row construction and the count are tested as one composition: a collapsed
      Source hides its skills, whichever way the selection was made
