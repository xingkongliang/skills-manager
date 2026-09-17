import { useCallback, useRef, type MouseEventHandler } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * Drag on the first press; apply the title bar action on the second release.
 * Moving the pointer during the second press cancels the double-click action.
 */
export function useDragWindow() {
  const doubleClickPosition = useRef<{ x: number; y: number } | null>(null);
  const onMouseDown = useCallback<MouseEventHandler>((e) => {
    doubleClickPosition.current = null;
    if (e.button !== 0) return;
    // Suppress WebKit text selection on every press, including double clicks.
    e.preventDefault();
    if (e.detail === 1) {
      void getCurrentWindow().startDragging().catch(console.error);
    } else if (e.detail === 2) {
      doubleClickPosition.current = { x: e.clientX, y: e.clientY };
    }
  }, []);

  const onMouseUp = useCallback<MouseEventHandler>((e) => {
    const position = doubleClickPosition.current;
    doubleClickPosition.current = null;
    if (e.button === 0 && e.detail === 2 && position
      && position.x === e.clientX && position.y === e.clientY) {
      void invoke("titlebar_double_click").catch(console.error);
    }
  }, []);

  const onMouseMove = useCallback<MouseEventHandler>((e) => {
    const position = doubleClickPosition.current;
    if (position && (position.x !== e.clientX || position.y !== e.clientY)) {
      doubleClickPosition.current = null;
    }
  }, []);

  const onMouseLeave = useCallback<MouseEventHandler>(() => {
    doubleClickPosition.current = null;
  }, []);

  return { onMouseDown, onMouseUp, onMouseMove, onMouseLeave };
}
