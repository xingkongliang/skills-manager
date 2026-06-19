import { useState, useEffect } from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

interface Props {
  open: boolean;
  currentName: string;
  onClose: () => void;
  onRename: (newName: string) => Promise<void>;
}

export function TagRenameDialog({ open, currentName, onClose, onRename }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState(currentName);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (open) setName(currentName);
  }, [open, currentName]);

  if (!open) return null;

  const canSave = name.trim().length > 0 && name.trim() !== currentName;

  const handleRename = async () => {
    if (!canSave) return;
    setLoading(true);
    try {
      await onRename(name.trim());
      onClose();
    } catch {
      // The parent surfaces the error (toast) and re-throws; swallow it here so
      // there's no unhandled rejection, and leave the dialog open to retry.
    } finally {
      setLoading(false);
    }
  };

  const inputClass =
    "w-full bg-background border border-border-subtle rounded-[4px] px-3 py-2 text-[13px] text-secondary focus:outline-none focus:border-border transition-all placeholder-faint";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 z-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative z-10 bg-surface border border-border rounded-xl w-full max-w-[400px] p-5 shadow-2xl">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-[13px] font-semibold text-primary">{t("mySkills.tags.renameTag")}</h2>
          <button
            onClick={onClose}
            className="text-muted hover:text-secondary p-1 rounded transition-colors outline-none"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="space-y-3">
          <div>
            <label className="block text-[13px] font-medium text-tertiary mb-1">
              {t("mySkills.tags.tagName")}
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              className={inputClass}
              autoFocus
              onKeyDown={(e) => e.key === "Enter" && handleRename()}
            />
          </div>
          <div className="flex justify-end gap-2 pt-1">
            <button
              onClick={onClose}
              className="px-3 py-1.5 rounded-[4px] text-[13px] font-medium text-tertiary hover:text-secondary hover:bg-surface-hover transition-colors outline-none"
            >
              {t("common.cancel")}
            </button>
            <button
              onClick={handleRename}
              disabled={!canSave || loading}
              className="px-3 py-1.5 rounded-[4px] bg-accent-dark hover:bg-accent text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-accent-border outline-none"
            >
              {loading ? t("common.loading") : t("common.save")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
