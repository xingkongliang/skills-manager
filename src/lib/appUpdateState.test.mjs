import assert from "node:assert/strict";
import { test } from "node:test";
import { visibleAppUpdate } from "./appUpdateState.ts";

const available = {
  has_update: true,
  current_version: "1.40.0",
  latest_version: "1.41.0",
  release_url: "https://example.com/release",
};

test("an installed update stays hidden when a stale version check finishes", () => {
  assert.equal(visibleAppUpdate(available, "1.41.0").has_update, false);
});

test("a different newer version remains available", () => {
  assert.equal(visibleAppUpdate(available, "1.40.1").has_update, true);
});
