import { useState } from "react";
import { X, Globe, Loader2, Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { OnlineMatchResult } from "../lib/tauri";

interface Props {
  open: boolean;
  skillName: string;
  matches: OnlineMatchResult[];
  loading: boolean;
  onClose: () => void;
  onSelect: (match: OnlineMatchResult) => Promise<void>;
}

export function OnlineMatchDialog({
  open,
  skillName,
  matches,
  loading,
  onClose,
  onSelect,
}: Props) {
  const { t } = useTranslation();
  const [converting, setConverting] = useState(false);

  if (!open) return null;

  const handleSelect = async (match: OnlineMatchResult) => {
    setConverting(true);
    try {
      await onSelect(match);
    } finally {
      setConverting(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative bg-surface border border-border rounded-xl w-full max-w-md p-5 shadow-2xl">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-[13px] font-semibold text-primary flex items-center gap-2">
            <Globe className="w-4 h-4 text-accent" />
            {t("mySkills.updateActions.selectOnlineVersion", { name: skillName })}
          </h2>
          <button
            onClick={onClose}
            disabled={converting}
            className="text-muted hover:text-secondary p-1 rounded transition-colors outline-none"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {loading ? (
          <div className="flex items-center justify-center gap-2 py-8 text-muted">
            <Loader2 className="w-4 h-4 animate-spin" />
            <span className="text-[13px]">{t("mySkills.updateActions.searchingOnline")}</span>
          </div>
        ) : matches.length === 0 ? (
          <div className="flex items-center justify-center gap-2 py-8 text-muted">
            <Search className="w-4 h-4" />
            <span className="text-[13px]">{t("mySkills.updateActions.noOnlineMatches")}</span>
          </div>
        ) : (
          <div className="max-h-80 overflow-y-auto space-y-2">
            {matches.map((match) => (
              <button
                key={`${match.origin}-${match.skill_id}`}
                onClick={() => handleSelect(match)}
                disabled={converting}
                className="w-full text-left p-3 rounded-lg border border-border-subtle hover:border-accent/50 hover:bg-surface-hover transition-all disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <div className="flex items-center justify-between mb-1">
                  <span className="text-[13px] font-medium text-primary truncate">
                    {match.name}
                  </span>
                  <span className="text-[11px] px-1.5 py-0.5 rounded bg-accent/15 text-accent border border-accent/20 shrink-0 ml-2">
                    {match.origin === "skillsmp"
                      ? t("mySkills.updateActions.originSkillsmp")
                      : t("mySkills.updateActions.originSkillssh")}
                  </span>
                </div>
                <div className="flex items-center gap-3 text-[11px] text-muted">
                  <span>{match.source}</span>
                  <span>{t("mySkills.updateActions.installCount", { count: match.installs })}</span>
                  <span>
                    {t("mySkills.updateActions.matchSimilarity", {
                      percent: Math.round(match.similarity * 100),
                    })}
                  </span>
                </div>
              </button>
            ))}
          </div>
        )}

        <p className="text-[11px] text-muted mt-3">
          {t("mySkills.updateActions.convertWarning", {
            type: "skillssh",
          })}
        </p>

        <div className="flex justify-end mt-4">
          <button
            onClick={onClose}
            disabled={converting}
            className="px-3 py-1.5 rounded-[4px] text-[13px] font-medium text-tertiary hover:text-secondary hover:bg-surface-hover transition-colors outline-none"
          >
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
