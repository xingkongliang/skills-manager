import { X } from "lucide-react";
import { type ReactNode, useId } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { useModalFocus } from "../hooks/useModalFocus";

const IS_MACOS = navigator.userAgent.includes("Mac");

interface DetailSheetProps {
  open: boolean;
  title: ReactNode;
  description?: ReactNode;
  meta?: ReactNode;
  onClose: () => void;
  children: ReactNode;
}

export function DetailSheet({
  open,
  title,
  description,
  meta,
  onClose,
  children,
}: DetailSheetProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const sheetRef = useModalFocus(open, onClose);

  if (!open) return null;

  return createPortal(
    <div className="fixed bottom-0 left-[220px] right-0 top-[28px] isolate z-40">
      <div
        className={
          IS_MACOS
            ? "absolute inset-0 z-0 bg-black/65"
            : "absolute inset-0 z-0 bg-black/60 backdrop-blur-sm"
        }
        onClick={onClose}
      />
      <div
        ref={sheetRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className="absolute inset-y-0 right-0 z-10 flex min-h-0 w-[min(760px,100%)] flex-col overflow-hidden border-l border-border-subtle bg-bg-secondary shadow-2xl"
      >
        <div className="flex shrink-0 items-center justify-end gap-2 border-b border-border-subtle px-4 py-2">
          <kbd className="rounded border border-border-subtle px-1.5 py-0.5 font-sans text-[11px] text-faint">
            Esc
          </kbd>
          <button
            type="button"
            aria-label={t("common.close")}
            onClick={onClose}
            className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[13px] font-medium text-secondary outline-none transition-colors hover:bg-surface-hover hover:text-primary focus-visible:ring-2 focus-visible:ring-accent"
          >
            <X className="h-4 w-4" />
            {t("common.close")}
          </button>
        </div>
        <div className="scrollbar-hide min-h-0 flex-1 overflow-y-auto px-6 pb-6 pt-5">
          <h2
            id={titleId}
            className="mb-3 min-w-0 text-[28px] font-semibold leading-tight tracking-tight text-primary"
          >
            <span className="block">{title}</span>
          </h2>
          {description ? (
            <div className="text-[15px] leading-7 text-secondary">{description}</div>
          ) : null}
          {meta ? <div className="mt-4">{meta}</div> : null}
          <div className="mt-5">{children}</div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
