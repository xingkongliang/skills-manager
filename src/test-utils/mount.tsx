import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";

/**
 * The jsdom mount/unmount dance every pill test needs, in one place. Call
 * `cleanup` from `afterEach`; `mount` creates the tree on first use so a test
 * can re-render with new props by calling it again.
 */
// React only enables act() when the environment says so, and everything that
// mounts through this helper is a jsdom test.
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

export function mount(ui: ReactElement): HTMLDivElement {
  if (!container) {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
  }
  const target = root;
  act(() => target?.render(ui));
  return container;
}

export function cleanup() {
  const target = root;
  const node = container;
  if (target) act(() => target.unmount());
  node?.remove();
  root = null;
  container = null;
}

/** The pill buttons currently on screen. */
export function pills(node: HTMLElement): HTMLButtonElement[] {
  return Array.from(node.querySelectorAll("button"));
}
