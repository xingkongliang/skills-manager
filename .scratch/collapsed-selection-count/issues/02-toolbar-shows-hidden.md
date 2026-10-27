# 02: Show hidden selections in the multi-select toolbar

**What to build:** In My Skills, collapsing a Source that holds selected skills
is disclosed in the toolbar: the selection count gains a note naming how many
of the selected skills are inside collapsed Sources. Nothing is hidden and
nothing is invented — when every selected row is on screen the toolbar reads
exactly as it does today, and the other two views that share the toolbar are
untouched.

**Blocked by:** 01

**Status:** done

- [x] Collapsing a Source with selected skills makes the toolbar name the hidden count
- [x] Selecting a skill and then collapsing its Source produces the same note (not only select-all)
- [x] Grouping off, or a search that opens every Source, leaves the toolbar text unchanged
- [x] Workspace and Project detail multi-select toolbars render unchanged
- [x] New copy exists in both en and zh, and `npm run lint` passes the i18n check
- [x] Component test asserts both the 0-hidden and hidden states
