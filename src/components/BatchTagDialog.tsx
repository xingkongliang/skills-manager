import { useEffect, useMemo, useRef, useState } from "react";
import { X, Plus, Tag } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";

interface TaggableSkill {
  tags: string[];
}

interface Props {
  open: boolean;
  skills: TaggableSkill[];
  allTags: string[];
  /** Scope warning, e.g. when edits from a project page reach the central library. */
  note?: string;
  onClose: () => void;
  onApply: (adds: string[], removes: string[]) => Promise<void>;
}

export function BatchTagDialog({ open, skills, allTags, note, onClose, onApply }: Props) {
  const { t } = useTranslation();
  const [adds, setAdds] = useState<string[]>([]);
  const [removes, setRemoves] = useState<string[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setAdds([]);
      setRemoves([]);
      setInput("");
    }
  }, [open]);

  // Escape closes the dialog; the tag input handles its own Escape first.
  useEffect(() => {
    if (!open || loading) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (e.target === inputRef.current) return;
      onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, loading, onClose]);

  const tagCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const skill of skills) {
      for (const tag of skill.tags) {
        counts.set(tag, (counts.get(tag) || 0) + 1);
      }
    }
    return Array.from(counts.entries()).sort((a, b) => b[1] - a[1]);
  }, [skills]);

  const suggestions = useMemo(() => {
    const needle = input.trim().toLowerCase();
    const existing = new Set(tagCounts.map(([t]) => t));
    return allTags.filter((tag) => {
      if (adds.includes(tag)) return false;
      if (existing.has(tag)) return false;
      if (!needle) return true;
      return tag.toLowerCase().includes(needle);
    }).slice(0, 8);
  }, [allTags, adds, input, tagCounts]);

  if (!open) return null;

  const addTag = (value: string) => {
    const trimmed = value.trim();
    if (!trimmed) return;
    if (!adds.includes(trimmed)) setAdds([...adds, trimmed]);
    setInput("");
    inputRef.current?.focus();
  };

  const toggleRemove = (tag: string) => {
    setRemoves((prev) =>
      prev.includes(tag) ? prev.filter((t) => t !== tag) : [...prev, tag]
    );
  };

  const handleApply = async () => {
    if (adds.length === 0 && removes.length === 0) {
      onClose();
      return;
    }
    setLoading(true);
    try {
      await onApply(adds, removes);
      onClose();
    } finally {
      setLoading(false);
    }
  };

  const hasChanges = adds.length > 0 || removes.length > 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative bg-surface border border-border rounded-xl w-full max-w-[440px] p-5 shadow-2xl">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-[13px] font-semibold text-primary flex items-center gap-2">
            <Tag className="w-4 h-4 text-accent-light" />
            {t("mySkills.batchTagDialog.title", { count: skills.length })}
          </h2>
          <button
            onClick={onClose}
            className="text-muted hover:text-secondary p-1 rounded transition-colors outline-none"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {note && (
          <p className="mb-4 rounded-lg bg-surface-hover px-3 py-2 text-[12px] leading-snug text-muted">
            {note}
          </p>
        )}

        <div className="space-y-4">
          <div>
            <label className="block text-[12px] font-medium text-tertiary mb-1.5">
              {t("mySkills.batchTagDialog.currentTags")}
            </label>
            {tagCounts.length === 0 ? (
              <p className="text-[12px] text-faint">{t("mySkills.batchTagDialog.noTags")}</p>
            ) : (
              <div className="flex flex-wrap gap-1.5">
                {tagCounts.map(([tag, count]) => {
                  const marked = removes.includes(tag);
                  return (
                    <button
                      key={tag}
                      onClick={() => toggleRemove(tag)}
                      className={cn(
                        "inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[12px] font-medium transition-colors",
                        marked
                          ? "bg-red-500/15 text-red-500 line-through"
                          : "bg-accent-bg text-accent-light hover:bg-red-500/10 hover:text-red-500"
                      )}
                      title={
                        marked
                          ? t("mySkills.batchTagDialog.undoRemove")
                          : t("mySkills.batchTagDialog.clickToRemove")
                      }
                    >
                      {tag}
                      <span className="text-[10px] opacity-70">
                        {count}/{skills.length}
                      </span>
                      <X className="h-2.5 w-2.5" />
                    </button>
                  );
                })}
              </div>
            )}
          </div>

          <div>
            <label className="block text-[12px] font-medium text-tertiary mb-1.5">
              {t("mySkills.batchTagDialog.toAdd")}
            </label>
            <div className="flex flex-wrap items-center gap-1.5">
              {adds.map((tag) => (
                <span
                  key={tag}
                  className="inline-flex items-center gap-1 rounded-full bg-emerald-500/15 px-2 py-0.5 text-[12px] font-medium text-emerald-600 dark:text-emerald-400"
                >
                  {tag}
                  <button
                    onClick={() => setAdds(adds.filter((a) => a !== tag))}
                    className="hover:text-emerald-700 dark:hover:text-emerald-300"
                  >
                    <X className="h-2.5 w-2.5" />
                  </button>
                </span>
              ))}
              <div className="relative">
                <input
                  ref={inputRef}
                  type="text"
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      addTag(input);
                    } else if (e.key === "Escape") {
                      setInput("");
                    }
                  }}
                  placeholder={t("mySkills.tags.addTag")}
                  className="h-6 w-32 rounded-full border border-border-subtle bg-transparent px-2 text-[12px] text-secondary outline-none focus:border-accent"
                />
                {suggestions.length > 0 && input && (
                  <div className="absolute left-0 top-7 z-10 min-w-[140px] max-w-[220px] rounded-md border border-border-subtle bg-surface p-1 shadow-lg">
                    {suggestions.map((suggestion) => (
                      <button
                        key={suggestion}
                        type="button"
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => addTag(suggestion)}
                        className="w-full truncate rounded px-1.5 py-1 text-left text-[12px] text-secondary hover:bg-surface-hover"
                      >
                        {suggestion}
                      </button>
                    ))}
                  </div>
                )}
              </div>
              <button
                onClick={() => addTag(input)}
                disabled={!input.trim()}
                className="inline-flex items-center rounded-full border border-border-subtle p-0.5 text-muted transition-colors hover:border-accent hover:text-accent-light disabled:opacity-50"
                title={t("mySkills.batchTagDialog.addButton")}
              >
                <Plus className="h-3 w-3" />
              </button>
            </div>
          </div>
        </div>

        <div className="flex justify-end gap-2 pt-5">
          <button
            onClick={onClose}
            className="px-3 py-1.5 rounded-lg text-[13px] font-medium text-tertiary hover:text-secondary hover:bg-surface-hover transition-colors outline-none"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={handleApply}
            disabled={loading || !hasChanges}
            className="px-3 py-1.5 rounded-lg bg-accent-dark hover:bg-accent text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-accent-border outline-none"
          >
            {loading ? t("common.loading") : t("mySkills.batchTagDialog.apply")}
          </button>
        </div>
      </div>
    </div>
  );
}
