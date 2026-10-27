import { useMemo, useState } from "react";
import { Download, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import type { NewSkill } from "../lib/tauri";
import { toggleAll, toggleOne, toggleRange } from "../lib/newSkillSelection";

interface Props {
  skills: NewSkill[];
  installing: boolean;
  onInstall: (selected: NewSkill[]) => void;
  /** "Not now" and the close button both come here: the offer is remembered. */
  onDismiss: (dismissed: NewSkill[]) => void;
}

/**
 * The skills a Source gained during an update.
 *
 * Nothing is pre-selected. The round found them by scanning a checkout, which
 * cannot know whether the user wants them, and installing replaces a directory
 * in the library — a decision, not a side effect of pressing update.
 */
export function NewSkillsDialog({ skills, installing, onInstall, onDismiss }: Props) {
  const { t } = useTranslation();
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  // Shift-click ranges need a starting row; it moves with every plain click,
  // exactly as the library's own list behaves.
  const [anchor, setAnchor] = useState<string | null>(null);

  // The key is the backend's: it folds URL spellings and carries the branch.
  const keyOf = (skill: NewSkill) => skill.key;
  const ordered = useMemo(() => skills.map(keyOf), [skills]);
  const selected = skills.filter((skill) => chosen.has(keyOf(skill)));
  const allChosen = ordered.length > 0 && chosen.size === ordered.length;

  if (skills.length === 0) return null;

  const toggle = (skill: NewSkill, shiftKey: boolean) => {
    const key = keyOf(skill);
    if (shiftKey && anchor) {
      setChosen((prev) => toggleRange(prev, ordered, anchor, key));
      return;
    }
    setChosen((prev) => toggleOne(prev, key));
    setAnchor(key);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={() => onDismiss(skills)} />
      <div className="relative flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-border bg-surface p-5 shadow-2xl">
        <div className="mb-1 flex items-center justify-between">
          <h2 className="text-[13px] font-semibold text-primary">
            {t("mySkills.newSkills.title", { count: skills.length })}
          </h2>
          <button
            onClick={() => onDismiss(skills)}
            disabled={installing}
            className="rounded-md p-1 text-muted transition-colors hover:text-secondary disabled:opacity-50"
            aria-label={t("common.close")}
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <div className="mb-4 flex items-center justify-between gap-2">
          <p className="min-w-0 flex-1 text-[12px] text-muted">{t("mySkills.newSkills.hint")}</p>
          <button
            type="button"
            onClick={() => setChosen((prev) => toggleAll(prev, ordered))}
            className="shrink-0 rounded-md px-2 py-0.5 text-[12px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
          >
            {allChosen ? t("mySkills.deselectAll") : t("mySkills.selectAll")}
          </button>
        </div>

        <div className="-mx-1 min-h-0 flex-1 overflow-y-auto px-1">
          {skills.map((skill) => {
            const checked = chosen.has(keyOf(skill));
            return (
              <button
                key={keyOf(skill)}
                type="button"
                onClick={(event) => toggle(skill, event.shiftKey)}
                className={cn(
                  "mb-1 flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors",
                  checked ? "bg-surface-active" : "hover:bg-surface-hover"
                )}
              >
                <span
                  className={cn(
                    "flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-md border",
                    checked ? "border-accent bg-accent text-white" : "border-border"
                  )}
                >
                  {checked && <Download className="h-2.5 w-2.5" />}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[13px] text-secondary">{skill.name}</span>
                  <span className="block truncate text-[11px] text-faint" title={skill.subpath}>
                    {skill.subpath}
                  </span>
                </span>
              </button>
            );
          })}
        </div>

        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={() => onDismiss(skills)}
            disabled={installing}
            className="rounded-lg px-3 py-1.5 text-[13px] font-medium text-tertiary transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
          >
            {t("mySkills.newSkills.notNow")}
          </button>
          <button
            onClick={() => onInstall(selected)}
            disabled={installing || selected.length === 0}
            className="rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t("mySkills.newSkills.install", { count: selected.length })}
          </button>
        </div>
      </div>
    </div>
  );
}
