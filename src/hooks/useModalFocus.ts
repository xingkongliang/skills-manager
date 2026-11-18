import { useLayoutEffect, useRef } from "react";

const openDialogs: HTMLElement[] = [];

function tabStops(dialog: HTMLElement) {
  return Array.from(
    dialog.querySelectorAll<HTMLElement>(
      "button, a[href], input, select, textarea, [contenteditable='true'], [tabindex]",
    ),
  ).filter(
    (element) =>
      element.tabIndex >= 0 &&
      !element.matches(":disabled") &&
      element.getClientRects().length > 0 &&
      getComputedStyle(element).visibility === "visible",
  );
}

/** Focus and keyboard behavior for custom dialogs on desktop WebViews. */
export function useModalFocus(open: boolean, onEscape?: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  const escape = useRef(onEscape);

  useLayoutEffect(() => {
    escape.current = onEscape;
  }, [onEscape]);

  useLayoutEffect(() => {
    const dialog = ref.current;

    if (!open || !dialog) return;
    const previous = document.activeElement;
    openDialogs.push(dialog);
    const controls = tabStops(dialog);
    (
      controls.find((element) => element.hasAttribute("data-modal-focus")) ??
      controls[0] ??
      dialog
    ).focus();

    // Listen after the dialog's controls, so editors and nested widgets can handle keys first.
    const onKeyDown = (event: KeyboardEvent) => {
      if (openDialogs[openDialogs.length - 1] !== dialog || event.defaultPrevented) return;
      const target = event.target;
      const inside = target instanceof Node && dialog.contains(target);

      // Other dialogs, such as the command palette, can still own their own keys.
      if (!inside && target instanceof Element && target.closest("[role='dialog']")) return;

      if (inside) event.stopPropagation();

      if (event.isComposing || event.keyCode === 229) return;

      // WebKitGTK can report Shift+Tab as an unidentified key.
      const isTab = event.key === "Tab" || (event.key === "Unidentified" && event.code === "Tab");

      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        escape.current?.();
      } else if (isTab) {
        const items = tabStops(dialog);
        const first = items[0];
        const last = items[items.length - 1];
        const active = document.activeElement;

        if (!first || !inside || (event.shiftKey && (active === first || active === dialog))) {
          event.preventDefault();
          (event.shiftKey ? (last ?? dialog) : (first ?? dialog)).focus();
        } else if (!event.shiftKey && active === last) {
          event.preventDefault();
          first.focus();
        }
      }
    };

    document.addEventListener("keydown", onKeyDown);

    return () => {
      document.removeEventListener("keydown", onKeyDown);
      const wasTop = openDialogs[openDialogs.length - 1] === dialog;
      openDialogs.splice(openDialogs.indexOf(dialog), 1);

      if (wasTop && previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, [open]);

  return ref;
}
