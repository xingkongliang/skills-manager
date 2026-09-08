import { useEffect, useMemo, useState } from "react";
import { Square, SquareCheck, X, Share2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import { AgentIcon } from "./AgentIcon";
import type { ManagedSkill, ToolInfo } from "../lib/tauri";

interface Props {
  open: boolean;
  skills: ManagedSkill[];
  tools: ToolInfo[];
  onClose: () => void;
  /** Syncs every selected skill that is not already on those agents. */
  onApply: (agentKeys: string[]) => Promise<void>;
}

/**
 * Adds a batch of skills to one or more agents. Add-only on purpose: a tri-state
 * control where one click could either install or remove would make a bulk action
 * ambiguous. Removing stays a per-skill action on the card's agent dots.
 */
export function BatchSyncAgentDialog({ open, skills, tools, onClose, onApply }: Props) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (open) setSelected(new Set());
  }, [open]);

  useEffect(() => {
    if (!open || loading) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, loading, onClose]);

  const rows = useMemo(() => {
    return tools
      .filter((tool) => tool.installed && tool.enabled)
      .map((tool) => {
        const synced = skills.filter((skill) =>
          skill.targets.some((target) => target.tool === tool.key)
        ).length;
        return {
          key: tool.key,
          displayName: tool.display_name,
          synced,
          missing: skills.length - synced,
        };
      });
  }, [tools, skills]);

  const pendingCount = useMemo(
    () => rows.filter((row) => selected.has(row.key)).reduce((sum, row) => sum + row.missing, 0),
    [rows, selected]
  );

  if (!open) return null;

  const toggle = (key: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleApply = async () => {
    if (selected.size === 0) return;
    setLoading(true);
    try {
      await onApply(Array.from(selected));
      onClose();
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative w-full max-w-[440px] rounded-xl border border-border bg-surface p-5 shadow-2xl">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="flex items-center gap-2 text-[13px] font-semibold text-primary">
            <Share2 className="h-4 w-4 text-accent-light" />
            {t("mySkills.batchSyncDialog.title", { count: skills.length })}
          </h2>
          <button
            onClick={onClose}
            className="rounded p-1 text-muted outline-none transition-colors hover:text-secondary"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <p className="mb-3 text-[12px] text-muted">
          {t("mySkills.batchSyncDialog.description")}
        </p>

        {rows.length === 0 ? (
          <p className="text-[12px] text-faint">{t("mySkills.batchSyncDialog.noAgents")}</p>
        ) : (
          <div className="grid gap-1.5 md:grid-cols-2">
            {rows.map((row) => {
              const checked = selected.has(row.key);
              const allSynced = row.missing === 0;
              return (
                <button
                  key={row.key}
                  type="button"
                  onClick={() => toggle(row.key)}
                  disabled={allSynced}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md border px-2 py-1.5 text-left text-[12px] transition-colors",
                    checked ? "border-border bg-surface" : "border-border-subtle bg-bg-secondary",
                    allSynced ? "opacity-55" : "hover:bg-surface-hover"
                  )}
                  title={allSynced ? t("mySkills.batchSyncDialog.allSynced") : undefined}
                >
                  <span className="shrink-0">
                    {checked
                      ? <SquareCheck className="h-3.5 w-3.5 text-accent" />
                      : <Square className="h-3.5 w-3.5 text-faint" />}
                  </span>
                  <AgentIcon
                    agentKey={row.key}
                    displayName={row.displayName}
                    className="h-5 w-5 rounded-[4px]"
                  />
                  <span className="min-w-0 flex-1 truncate font-medium text-secondary">
                    {row.displayName}
                  </span>
                  <span className="shrink-0 tabular-nums text-[11px] text-muted">
                    {t("mySkills.syncSummary", { synced: row.synced, total: skills.length })}
                  </span>
                </button>
              );
            })}
          </div>
        )}

        <div className="flex justify-end gap-2 pt-5">
          <button
            onClick={onClose}
            className="rounded-lg px-3 py-1.5 text-[13px] font-medium text-tertiary outline-none transition-colors hover:bg-surface-hover hover:text-secondary"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={handleApply}
            disabled={loading || pendingCount === 0}
            className="rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white outline-none transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
          >
            {loading
              ? t("common.loading")
              : t("mySkills.batchSyncDialog.apply", { count: pendingCount })}
          </button>
        </div>
      </div>
    </div>
  );
}
