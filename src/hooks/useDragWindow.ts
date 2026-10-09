import { useCallback, type MouseEventHandler } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Returns a mousedown handler for dragging and double-click maximization.
 */
export function useDragWindow(): MouseEventHandler {
  return useCallback((e) => {
    if (e.buttons === 1) {
      if (e.detail === 1) {
        getCurrentWindow().startDragging();
      } else if (e.detail === 2) {
        getCurrentWindow().toggleMaximize();
      }
    }
  }, []);
}
