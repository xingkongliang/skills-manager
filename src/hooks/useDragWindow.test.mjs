import assert from "node:assert/strict";
import { test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { useDragWindow } from "./useDragWindow.ts";

test("double-clicking the drag bar toggles maximization while a single click starts dragging", () => {
  globalThis.window = globalThis;
  const commands = [];
  mockIPC((command) => commands.push(command));
  window.__TAURI_INTERNALS__.metadata = { currentWindow: { label: "main" } };

  let onMouseDown;
  function Probe() {
    onMouseDown = useDragWindow();
    return null;
  }
  renderToStaticMarkup(createElement(Probe));

  onMouseDown({ buttons: 1, detail: 1 });
  onMouseDown({ buttons: 1, detail: 2 });
  assert.deepEqual(commands, [
    "plugin:window|start_dragging",
    "plugin:window|toggle_maximize",
  ]);
  clearMocks();
});
