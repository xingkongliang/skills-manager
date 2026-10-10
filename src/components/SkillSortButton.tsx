import { ArrowDownAZ, ArrowUpAZ } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useApp } from "../context/AppContext";
import { cn } from "../utils";

/**
 * Single-button sort control that cycles default → A–Z → Z–A → default.
 * The shared mode lives in AppContext, so the button in any view (Library,
 * workspaces, …) controls the same global preference.
 */
export function SkillSortButton() {
  const { t } = useTranslation();
  const { skillSortMode, setSkillSortMode } = useApp();

  const nextMode =
    skillSortMode === "none" ? "asc" : skillSortMode === "asc" ? "desc" : "none";
  const Icon = skillSortMode === "desc" ? ArrowUpAZ : ArrowDownAZ;
  const labelKey =
    skillSortMode === "none" ? "sortHintDefault"
    : skillSortMode === "asc" ? "sortHintAsc"
    : "sortHintDesc";
  const label = t(`mySkills.${labelKey}`);

  return (
    <button
      type="button"
      onClick={() => setSkillSortMode(nextMode)}
      className={cn(
        "rounded-md p-2 transition-colors outline-none",
        skillSortMode !== "none"
          ? "bg-surface-active text-secondary"
          : "text-muted hover:text-tertiary"
      )}
      title={label}
      aria-label={label}
    >
      <Icon className="h-4 w-4" />
    </button>
  );
}
