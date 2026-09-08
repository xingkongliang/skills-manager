import { useCallback, useEffect, useRef, useState } from "react";
import { MoreHorizontal } from "lucide-react";
import { cn } from "../utils";

export interface CardAction {
  key: string;
  label: string;
  icon: React.ReactNode;
  onSelect: () => void;
  danger?: boolean;
  disabled?: boolean;
}

interface Props {
  actions: CardAction[];
  label: string;
  className?: string;
  /** Lets the host lift the whole card above its siblings while the menu is open. */
  onOpenChange?: (open: boolean) => void;
}

/** Overflow "…" menu for low-frequency card actions (see UI spec in CLAUDE.md). */
export function CardActionMenu({ actions, label, className, onOpenChange }: Props) {
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const openChangeRef = useRef(onOpenChange);

  useEffect(() => {
    openChangeRef.current = onOpenChange;
  }, [onOpenChange]);

  const setOpenState = useCallback((next: boolean) => {
    setOpen(next);
    openChangeRef.current?.(next);
  }, []);

  useEffect(() => {
    if (!open) return;
    const handlePointer = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpenState(false);
      }
    };
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // The open menu consumes this Escape: it must not also reach the window
      // listeners behind it (multi-select mode would exit and drop the selection).
      e.stopPropagation();
      setOpenState(false);
    };
    document.addEventListener("mousedown", handlePointer);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("mousedown", handlePointer);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [open, setOpenState]);

  // Report the collapsed state if this menu unmounts while open.
  useEffect(() => () => openChangeRef.current?.(false), []);

  if (actions.length === 0) return null;

  return (
    <div ref={containerRef} className={cn("relative shrink-0", className)}>
      <button
        type="button"
        title={label}
        aria-label={label}
        onClick={(e) => {
          e.stopPropagation();
          setOpenState(!open);
        }}
        className={cn(
          "flex h-5 w-5 items-center justify-center rounded-md text-muted outline-none transition-colors hover:bg-surface-hover hover:text-secondary",
          open && "bg-surface-hover text-secondary"
        )}
      >
        <MoreHorizontal className="h-3.5 w-3.5" />
      </button>
      {open && (
        <div
          className="absolute right-0 top-full z-30 mt-1 min-w-[156px] rounded-lg border border-border bg-surface p-1 shadow-lg"
          onClick={(e) => e.stopPropagation()}
        >
          {actions.map((action) => (
            <button
              key={action.key}
              type="button"
              disabled={action.disabled}
              onClick={(e) => {
                e.stopPropagation();
                setOpenState(false);
                action.onSelect();
              }}
              className={cn(
                "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] outline-none transition-colors disabled:cursor-not-allowed disabled:opacity-40",
                action.danger
                  ? "text-danger hover:bg-danger-bg"
                  : "text-secondary hover:bg-surface-hover"
              )}
            >
              <span className="flex h-3.5 w-3.5 shrink-0 items-center justify-center opacity-70">
                {action.icon}
              </span>
              {action.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
