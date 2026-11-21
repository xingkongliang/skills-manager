import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { IS_MACOS } from "../lib/platform";
import { cn } from "../utils";

interface DetailSheetProps {
  open: boolean;
  title: ReactNode;
  /** Clamped to two lines, with a toggle when it overflows. */
  description?: string | null;
  /** One row under the description (tags, status pills). */
  meta?: ReactNode;
  /** Header controls, left of the close button. */
  actions?: ReactNode;
  /** Drawn in a 36px identity block left of the title; the lines below align with the title. */
  icon?: ReactNode;
  /** One short line under the title. */
  subtitle?: ReactNode;
  /** Right column; omitted means a single column. */
  aside?: ReactNode;
  onClose: () => void;
  /** Main column. */
  children: ReactNode;
}

export function DetailSheet({
  open,
  title,
  description,
  meta,
  actions,
  icon,
  subtitle,
  aside,
  onClose,
  children,
}: DetailSheetProps) {
  const sheetRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || e.isComposing) return;
      const sheet = sheetRef.current;
      if (!sheet) return;
      const rect = sheet.getBoundingClientRect();
      // Leave Escape to any dialog covering the detail sheet.
      const topElement = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
      if (!topElement || !sheet.contains(topElement)) return;
      e.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open) return null;

  // Reserve space for the macOS drag bar. The header stays put because the
  // sheet is a flex column with the columns scrolling inside it: no sticky,
  // transform or backdrop-filter here (Monterey WKWebView blanks on those).
  return createPortal(
    <div
      ref={sheetRef}
      className={cn("fixed right-0 bottom-0 left-[220px] z-40 isolate", IS_MACOS ? "top-[28px]" : "top-0")}
    >
      <div
        className={
          IS_MACOS
            ? "absolute inset-0 z-0 bg-black/65"
            : "absolute inset-0 z-0 bg-black/60 backdrop-blur-sm"
        }
        onClick={onClose}
      />
      {/* Dark mode drops to the page background, as the library grid does:
          bg-secondary sits too close to surface there for the panels to read. */}
      <div className="absolute inset-0 z-10 flex min-h-0 flex-col overflow-hidden border-l border-border-subtle bg-bg-secondary dark:bg-background">
        <header className="shrink-0 border-b border-border-subtle bg-surface px-6 pt-4 pb-3.5">
          <div className="flex min-w-0 items-center gap-3">
            {icon ? (
              <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-border-subtle bg-bg-secondary text-muted">
                {icon}
              </span>
            ) : null}
            <div className="min-w-0 flex-1">
              <h2 className="app-page-title truncate">{title}</h2>
              {subtitle ? <div className="truncate text-[12px] text-muted">{subtitle}</div> : null}
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              {actions}
              <button
                type="button"
                onClick={onClose}
                className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted transition-colors outline-none hover:bg-surface-hover hover:text-secondary"
              >
                <X className="h-4 w-4" />
              </button>
            </div>
          </div>
          {/* With an identity block, the description and meta start under the
              title text (36px block + 12px gap), not under the block. */}
          <div className={icon ? "pl-12" : undefined}>
            {description ? <SheetDescription text={description} /> : null}
            {meta ? <div className="mt-2.5">{meta}</div> : null}
          </div>
        </header>
        <div className="flex min-h-0 flex-1">
          <div className="min-w-0 flex-1 overflow-y-auto px-6 pt-4 pb-6 scrollbar-hide">
            <div className="mx-auto w-full max-w-[760px]">{children}</div>
          </div>
          {aside ? (
            <aside className="w-[300px] shrink-0 overflow-y-auto border-l border-border-subtle bg-surface scrollbar-hide">
              {aside}
            </aside>
          ) : null}
        </div>
      </div>
    </div>,
    document.body
  );
}

/**
 * The header is the only place the full description shows (the document
 * body strips its frontmatter), so a clamped one must be expandable.
 */
function SheetDescription({ text }: { text: string }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLParagraphElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);

  useLayoutEffect(() => {
    if (expanded) return;
    const measure = () => {
      const el = ref.current;
      if (el) setOverflows(el.scrollHeight > el.clientHeight + 1);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [text, expanded]);

  return (
    <div className="mt-1 flex min-w-0 items-end gap-2">
      <p
        ref={ref}
        // Expanded, a very long description scrolls inside the header so the
        // columns and the Collapse button stay on screen.
        className={cn(
          "min-w-0 flex-1 text-[13px] leading-5 text-secondary",
          expanded ? "max-h-[30vh] overflow-y-auto" : "line-clamp-2"
        )}
      >
        {text}
      </p>
      {(overflows || expanded) && (
        <button
          type="button"
          onClick={() => setExpanded((prev) => !prev)}
          className="shrink-0 text-[12px] text-muted transition-colors hover:text-secondary"
        >
          {expanded ? t("common.collapse") : t("common.expand")}
        </button>
      )}
    </div>
  );
}
