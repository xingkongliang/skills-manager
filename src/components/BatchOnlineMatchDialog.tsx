import { useState } from "react";
import { X, Globe, Loader2, Search, CheckCircle2, Circle, Zap } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import type { OnlineMatchResult } from "../lib/tauri";

export interface BatchMatchItem {
  skill_id: string;
  skill_name: string;
  matches: OnlineMatchResult[];
  selectedMatch?: OnlineMatchResult;
}

interface Props {
  open: boolean;
  items: BatchMatchItem[];
  loading: boolean;
  converting: boolean;
  onClose: () => void;
  onItemSelect: (skillId: string, match: OnlineMatchResult | undefined) => void;
  onAutoSelect: () => void;
  onConvertAll: () => Promise<void>;
}

export function BatchOnlineMatchDialog({
  open,
  items,
  loading,
  converting,
  onClose,
  onItemSelect,
  onAutoSelect,
  onConvertAll,
}: Props) {
  const { t } = useTranslation();
  const [activeSkillId, setActiveSkillId] = useState<string | null>(null);

  if (!open) return null;

  const activeItem = items.find((i) => i.skill_id === activeSkillId) ?? items[0];
  const selectedCount = items.filter((i) => i.selectedMatch).length;
  const hasSelections = selectedCount > 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative bg-surface border border-border rounded-xl w-full max-w-2xl shadow-2xl flex flex-col max-h-[80vh]">
        {/* Header */}
        <div className="flex items-center justify-between p-5 pb-3 border-b border-border-subtle">
          <h2 className="text-[13px] font-semibold text-primary flex items-center gap-2">
            <Globe className="w-4 h-4 text-accent" />
            {t("mySkills.updateActions.batchConvertTitle", { count: items.length })}
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
          <div className="flex items-center justify-center gap-2 py-12 text-muted">
            <Loader2 className="w-4 h-4 animate-spin" />
            <span className="text-[13px]">{t("mySkills.updateActions.searchingOnline")}</span>
          </div>
        ) : (
          <div className="flex flex-1 min-h-0">
            {/* Left: Skill list */}
            <div className="w-48 border-r border-border-subtle overflow-y-auto shrink-0">
              {items.map((item) => (
                <button
                  key={item.skill_id}
                  onClick={() => setActiveSkillId(item.skill_id)}
                  disabled={converting}
                  className={cn(
                    "w-full text-left px-3 py-2.5 text-[12px] flex items-center gap-2 transition-colors border-b border-border-subtle/50",
                    activeItem?.skill_id === item.skill_id
                      ? "bg-surface-active text-primary"
                      : "text-secondary hover:bg-surface-hover"
                  )}
                >
                  {item.selectedMatch ? (
                    <CheckCircle2 className="w-3.5 h-3.5 text-green-500 shrink-0" />
                  ) : item.matches.length === 0 ? (
                    <Search className="w-3.5 h-3.5 text-muted shrink-0" />
                  ) : (
                    <Circle className="w-3.5 h-3.5 text-muted shrink-0" />
                  )}
                  <span className="truncate">{item.skill_name}</span>
                </button>
              ))}
            </div>

            {/* Right: Match results */}
            <div className="flex-1 overflow-y-auto p-4">
              {activeItem && (
                <>
                  <div className="text-[12px] text-muted mb-3">
                    {activeItem.matches.length > 0
                      ? t("mySkills.updateActions.selectFromMatches")
                      : t("mySkills.updateActions.noOnlineMatches")}
                  </div>
                  <div className="space-y-2">
                    {activeItem.matches.map((match) => {
                      const isSelected =
                        activeItem.selectedMatch?.skill_id === match.skill_id &&
                        activeItem.selectedMatch?.origin === match.origin;
                      return (
                        <button
                          key={`${match.origin}-${match.skill_id}`}
                          onClick={() =>
                            onItemSelect(
                              activeItem.skill_id,
                              isSelected ? undefined : match
                            )
                          }
                          disabled={converting}
                          className={cn(
                            "w-full text-left p-3 rounded-lg border transition-all disabled:opacity-50",
                            isSelected
                              ? "border-accent bg-accent/5"
                              : "border-border-subtle hover:border-accent/50 hover:bg-surface-hover"
                          )}
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
                            <span>
                              {t("mySkills.updateActions.installCount", {
                                count: match.installs,
                              })}
                            </span>
                            <span>
                              {t("mySkills.updateActions.matchSimilarity", {
                                percent: Math.round(match.similarity * 100),
                              })}
                            </span>
                          </div>
                        </button>
                      );
                    })}
                  </div>
                </>
              )}
            </div>
          </div>
        )}

        {/* Footer */}
        {!loading && (
          <div className="flex items-center justify-between p-4 pt-3 border-t border-border-subtle">
            <button
              onClick={onAutoSelect}
              disabled={converting}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-[4px] text-[12px] font-medium text-accent hover:bg-accent/10 transition-colors outline-none disabled:opacity-50"
            >
              <Zap className="w-3.5 h-3.5" />
              {t("mySkills.updateActions.autoSelectBest")}
            </button>
            <div className="flex items-center gap-2">
              <button
                onClick={onClose}
                disabled={converting}
                className="px-3 py-1.5 rounded-[4px] text-[13px] font-medium text-tertiary hover:text-secondary hover:bg-surface-hover transition-colors outline-none"
              >
                {t("common.cancel")}
              </button>
              <button
                onClick={onConvertAll}
                disabled={converting || !hasSelections}
                className="px-3 py-1.5 rounded-[4px] bg-accent-dark hover:bg-accent text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-accent-border outline-none inline-flex items-center gap-1.5"
              >
                {converting && <Loader2 className="w-3.5 h-3.5 animate-spin" />}
                {t("mySkills.updateActions.batchConvert")} ({selectedCount})
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
