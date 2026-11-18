import { X } from "lucide-react";
import { useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { useModalFocus } from "../hooks/useModalFocus";

interface Props {
  open: boolean;
  currentName: string;
  onClose: () => void;
  onRename: (newName: string) => Promise<void>;
}

export function TagRenameDialog({ open, ...props }: Props) {
  if (!open) return null;

  return <TagRenameDialogContent {...props} />;
}

function TagRenameDialogContent({ currentName, onClose, onRename }: Omit<Props, "open">) {
  const { t } = useTranslation();
  const titleId = useId();
  const nameId = useId();
  const dialogRef = useModalFocus(true, onClose);
  const submitting = useRef(false);
  const [name, setName] = useState(currentName);
  const [loading, setLoading] = useState(false);

  const canSave = name.trim().length > 0 && name.trim() !== currentName;

  const handleRename = async () => {
    if (!canSave || submitting.current) return;
    submitting.current = true;
    setLoading(true);

    try {
      await onRename(name.trim());
      onClose();
    } catch {
      // The parent surfaces the error (toast) and re-throws; swallow it here so
      // there's no unhandled rejection, and leave the dialog open to retry.
    } finally {
      submitting.current = false;
      setLoading(false);
    }
  };

  const inputClass =
    "w-full bg-background border border-border-subtle rounded-lg px-3 py-2 text-[13px] text-secondary focus:outline-none focus:border-border transition-all placeholder-faint";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 z-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className="relative z-10 w-full max-w-[400px] rounded-xl border border-border bg-surface p-5 shadow-2xl"
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 id={titleId} className="text-[13px] font-semibold text-primary">
            {t("mySkills.tags.renameTag")}
          </h2>
          <button
            onClick={onClose}
            aria-label={t("common.close")}
            className="rounded p-1 text-muted outline-none transition-colors hover:text-secondary"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <form
          className="space-y-3"
          onSubmit={(event) => {
            event.preventDefault();
            void handleRename();
          }}
        >
          <div>
            <label htmlFor={nameId} className="mb-1 block text-[13px] font-medium text-tertiary">
              {t("mySkills.tags.tagName")}
            </label>
            <input
              id={nameId}
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              className={inputClass}
              data-modal-focus
              required
            />
          </div>
          <div className="flex justify-end gap-2 pt-1">
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg px-3 py-1.5 text-[13px] font-medium text-tertiary outline-none transition-colors hover:bg-surface-hover hover:text-secondary"
            >
              {t("common.cancel")}
            </button>
            <button
              type="submit"
              disabled={!canSave || loading}
              className="rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white outline-none transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
            >
              {loading ? t("common.loading") : t("common.save")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
