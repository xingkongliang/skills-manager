# 04: A Source header stops pretending to toggle while searching

**What to build:** While a search is active every Source is forced open, but the
header still looks and clicks like a toggle and nothing happens. During a search
the header becomes a plain label row (arrow fixed open, no hover, not clickable);
clearing the search brings back folding, still honouring any Source the user had
opened or closed before.

**Blocked by:** None (can start immediately).

**Status:** done

- [x] A non-empty search renders Source headers as non-interactive rows
- [x] Clearing the search restores folding, with earlier explicit open/closed choices intact
- [x] No change to any Source header when no search is active
