import { useEffect, useState } from "react";
import { X, AlertTriangle } from "lucide-react";
import { useTranslation } from "react-i18next";

interface Props {
  open: boolean;
  title?: string;
  message: string;
  details?: string[];
  confirmLabel?: string;
  tone?: "danger" | "warning";
  /** Keep the dialog up while the confirmed action runs (closing would not
   *  stop it). Only for actions that always finish, like a clone. */
  lockWhileRunning?: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}

export function ConfirmDialog({
  open,
  title,
  message,
  details,
  confirmLabel,
  tone = "danger",
  lockWhileRunning = false,
  onClose,
  onConfirm,
}: Props) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);

  // Escape closes the dialog, except while the confirmed action is running.
  useEffect(() => {
    if (!open || loading) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, loading, onClose]);

  if (!open) return null;

  const locked = lockWhileRunning && loading;
  const close = () => {
    if (!locked) onClose();
  };

  const handleConfirm = async () => {
    setLoading(true);
    try {
      await onConfirm();
      onClose();
    } finally {
      setLoading(false);
    }
  };

  return (
    // The panel is capped so the footer buttons stay reachable (#430). The cap
    // divides by --app-scale because the text-size setting applies `zoom` to
    // <html> and zoom does not scale vh: a bare 85vh renders at 102% of the
    // viewport on the largest size. Same compensation as html/body in index.css.
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={close} />
      <div className="relative bg-surface border border-border rounded-xl w-full max-w-sm p-5 shadow-2xl flex flex-col max-h-[calc(85vh/var(--app-scale))]">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-[13px] font-semibold text-primary flex items-center gap-2">
            <AlertTriangle className="w-4 h-4 text-amber-400" />
            {title || t("common.confirm")}
          </h2>
          <button onClick={close} disabled={locked} className="text-muted hover:text-secondary p-1 rounded transition-colors outline-none disabled:opacity-50">
            <X className="w-4 h-4" />
          </button>
        </div>

        <p className="text-[13px] text-tertiary mb-5">{message}</p>
        {details && details.length > 0 ? (
          <div className="mb-5 flex flex-wrap gap-2 overflow-y-auto min-h-0">
            {details.map((detail) => (
              <span
                key={detail}
                className="rounded-full border border-border-subtle bg-bg-secondary px-2.5 py-1 text-[13px] text-secondary"
              >
                {detail}
              </span>
            ))}
          </div>
        ) : null}

        <div className="flex justify-end gap-2">
          <button
            onClick={close}
            disabled={locked}
            className="px-3 py-1.5 rounded-lg text-[13px] font-medium text-tertiary hover:text-secondary hover:bg-surface-hover transition-colors outline-none disabled:opacity-50"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={handleConfirm}
            disabled={loading}
            className={
              tone === "warning"
                ? "px-3 py-1.5 rounded-lg bg-accent-dark hover:bg-accent text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-accent-border outline-none"
                : "px-3 py-1.5 rounded-lg bg-red-600/90 hover:bg-red-500 text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-red-500/50 outline-none"
            }
          >
            {loading ? t("common.loading") : confirmLabel || t("common.delete")}
          </button>
        </div>
      </div>
    </div>
  );
}
