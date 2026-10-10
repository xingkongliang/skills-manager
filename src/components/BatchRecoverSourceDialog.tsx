import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, GitBranch, Loader2, Search, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { getErrorMessage } from "../lib/error";
import {
  pickFromCandidate,
  repoLabel,
  searchSeeds,
  toRepoUrl,
  useSkillsshSearch,
} from "../lib/recoverSource";
import * as api from "../lib/tauri";
import type {
  BatchRecoverResult,
  BatchRecoverSourceEntry,
  ManagedSkill,
  RecoverSourcePick,
  SkillsShSkill,
} from "../lib/tauri";
import { SkillSourceDiffViewer } from "./SkillSourceDiffViewer";

interface Props {
  open: boolean;
  /** Only the rows that can actually be recovered — the caller filters. */
  skills: ManagedSkill[];
  /** How many selected rows were filtered out, so the count is not a mystery. */
  skipped: number;
  onClose: () => void;
  onDone: () => Promise<void> | void;
}

interface ManualEntry {
  url: string;
  subpath: string;
}

/**
 * One skill's row: search it, or type the address.
 *
 * Deliberately narrower than the single-skill dialog — only the exact and
 * similar tiers are offered. With a dozen rows open at once, the "other results"
 * tier is mostly noise, and the manual box below it is always available.
 */
function RecoverRow({
  skill,
  active,
  manual,
  onCandidate,
  onManual,
}: {
  skill: ManagedSkill;
  active: RecoverSourcePick | null;
  manual: ManualEntry;
  onCandidate: (hit: SkillsShSkill) => void;
  onManual: (patch: Partial<ManualEntry>) => void;
}) {
  const { t } = useTranslation();
  const [seedOverride, setSeedOverride] = useState<string | null>(null);
  const defaultSeed = searchSeeds(skill)[0] ?? skill.name;
  const seed = seedOverride ?? defaultSeed;
  const { grouped, searching, searchFailed, searched, runSearch } = useSkillsshSearch(seed);
  const near = grouped.filter((group) => group.level !== "other");

  return (
    <div className="rounded-lg border border-border-subtle bg-bg-secondary/40 p-3">
      <div className="mb-2 flex items-baseline justify-between gap-2">
        <span className="truncate text-[13px] font-medium text-primary">{skill.name}</span>
        <span
          className={
            "shrink-0 truncate font-mono text-[11.5px] " +
            (active ? "text-accent-light" : "text-faint")
          }
        >
          {active ? repoLabel(active) : t("mySkills.recover.pickHint")}
        </span>
      </div>

      <div className="flex gap-2">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
          <input
            value={seed}
            onChange={(e) => setSeedOverride(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void runSearch(seed);
            }}
            placeholder={t("mySkills.recover.searchPlaceholder")}
            className="app-input w-full pl-9"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
          />
        </div>
        <button
          onClick={() => void runSearch(seed)}
          disabled={searching}
          className="shrink-0 rounded-lg border border-border-subtle px-3 text-[13px] font-medium text-secondary outline-none transition-colors hover:bg-surface-hover disabled:opacity-50"
        >
          {searching ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : t("mySkills.recover.search")}
        </button>
      </div>

      {searchFailed && (
        <p className="mt-2 text-[12px] text-tertiary">{t("mySkills.recover.searchUnavailable")}</p>
      )}
      {searched && !searchFailed && !searching && near.length === 0 && (
        <p className="mt-2 text-[12px] text-tertiary">{t("mySkills.batchRecover.noMatch")}</p>
      )}

      <div className="mt-2 space-y-1">
        {near.map((group) => (
          <div key={group.level} className="space-y-1">
            {group.level === "similar" && (
              <div className="text-[11px] font-medium uppercase tracking-wide text-faint">
                {t("mySkills.recover.confidence.similar")}
              </div>
            )}
            {group.hits.slice(0, 5).map((hit) => (
              <button
                key={hit.id}
                onClick={() => onCandidate(hit)}
                className={
                  "flex w-full items-center gap-2 rounded-lg border px-2.5 py-1.5 text-left outline-none transition-colors " +
                  (active?.locatorSkillId === hit.skill_id && active.locatorSource === hit.source
                    ? "border-accent/50 bg-accent-bg"
                    : "border-border-subtle hover:bg-surface-hover")
                }
              >
                <span className="min-w-0 flex-1 truncate text-[12.5px] text-primary">{hit.name}</span>
                {group.level === "exact" && (
                  <span className="shrink-0 rounded-full bg-accent-bg px-1.5 py-0.5 text-[10.5px] font-medium text-accent-light">
                    {t("mySkills.recover.confidence.exact")}
                  </span>
                )}
                <span className="shrink-0 font-mono text-[11px] text-faint">{hit.source}</span>
              </button>
            ))}
          </div>
        ))}
      </div>

      <div className="mt-2 space-y-1.5 border-t border-border-subtle pt-2">
        <input
          value={manual.url}
          onChange={(e) => onManual({ url: e.target.value })}
          placeholder={t("mySkills.recover.manualPlaceholder")}
          className="app-input w-full"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
        />
        <input
          value={manual.subpath}
          onChange={(e) => onManual({ subpath: e.target.value })}
          placeholder={t("mySkills.recover.subpathPlaceholder")}
          className="app-input w-full"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
        />
      </div>
    </div>
  );
}

export function BatchRecoverSourceDialog({ open, skills, skipped, onClose, onDone }: Props) {
  const { t } = useTranslation();
  const [picks, setPicks] = useState<Record<string, RecoverSourcePick>>({});
  const [manual, setManual] = useState<Record<string, ManualEntry>>({});
  const [preview, setPreview] = useState<BatchRecoverResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  // Every run describes a different set of revisions and approvals.
  useEffect(() => {
    if (!open) return;
    setPicks({});
    setManual({});
    setPreview(null);
    setProblem(null);
    setBusy(false);
  }, [open]);

  const chosenFor = (skill: ManagedSkill): RecoverSourcePick | null => {
    const entry = manual[skill.id];
    // Typing a URL is itself a choice, so it takes over from a picked
    // candidate rather than being silently ignored next to it.
    if (entry?.url.trim()) {
      const url = toRepoUrl(entry.url);
      return url ? { repoUrl: url, subpath: entry.subpath.trim() || null } : null;
    }
    return picks[skill.id] ?? null;
  };

  const requests: BatchRecoverSourceEntry[] = useMemo(
    () =>
      skills.flatMap((skill) => {
        const chosen = chosenFor(skill);
        if (!chosen) return [];
        return [
          {
            skill_id: skill.id,
            repo_url: chosen.repoUrl,
            locator_source: chosen.locatorSource ?? null,
            locator_skill_id: chosen.locatorSkillId ?? null,
            subpath: chosen.subpath ?? null,
            branch: chosen.branch ?? null,
            approved_removals: null,
          },
        ];
      }),
    // `manual` and `picks` are the inputs; `chosenFor` reads them directly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [skills, picks, manual]
  );

  const missing = skills.filter((skill) => !chosenFor(skill));
  const awaiting = preview?.items.filter((item) => !item.applied && !item.error) ?? [];
  const failedItems = preview?.items.filter((item) => item.error) ?? [];
  const removalTotal =
    preview?.items.reduce((sum, item) => sum + item.pending_removals.length, 0) ?? 0;

  /** Progress arrives per skill; the batch may run for minutes. */
  const withProgress = async (run: () => Promise<BatchRecoverResult>) => {
    const toastId = toast.loading(t("mySkills.batchRecover.preparing"));
    const unlisten = await listen<{ index: number; total: number; name: string }>(
      "batch-recover-progress",
      (event) => {
        const { index, total, name } = event.payload;
        toast.loading(t("mySkills.batchRecover.progress", { index, total, name }), {
          id: toastId,
        });
      }
    );
    try {
      return await run();
    } finally {
      unlisten();
      toast.dismiss(toastId);
    }
  };

  /** The first leg never commits — it reports. */
  const previewBatch = async () => {
    if (requests.length === 0) return;
    setBusy(true);
    setProblem(null);
    try {
      setPreview(
        await withProgress(() => api.batchRecoverSkillSources(requests))
      );
    } catch (error) {
      setProblem(getErrorMessage(error, t("mySkills.recover.failed")));
    } finally {
      setBusy(false);
    }
  };

  /** The second leg carries each skill's own approval back. */
  const confirmBatch = async () => {
    if (!preview) return;
    const tokens = new Map(
      preview.items
        .filter((item) => item.removal_approval)
        .map((item) => [item.skill_id, item.removal_approval as string])
    );
    if (tokens.size === 0) return;
    setBusy(true);
    setProblem(null);
    try {
      const result = await withProgress(() =>
        api.batchRecoverSkillSources(
          // Only the skills that were held. One whose content was already
          // identical commits on the reporting leg and comes back with no
          // token; sending it again would be refused by the local/import guard
          // and reported as a failure for work that actually succeeded.
          requests.filter((request) => tokens.has(request.skill_id)).map((request) => ({
            ...request,
            approved_removals: tokens.get(request.skill_id) ?? null,
          }))
        )
      );
      if (result.failed > 0) {
        // Report what did land, then show the rest again: whatever is still
        // held had its revision move under us, and the new list is what the
        // next confirm has to be judged against.
        toast.error(t("mySkills.batchRecover.partial", { failed: result.failed }));
        setPreview(result);
      } else {
        await onDone();
        // Counted across both legs: the reporting leg already committed the
        // rows whose content was identical, and they are part of this batch's
        // result just as much as the ones this click finished.
        toast.success(
          t("mySkills.batchRecover.applied", {
            count: result.applied + (preview?.applied ?? 0),
          })
        );
        onClose();
      }
    } catch (error) {
      setProblem(getErrorMessage(error, t("mySkills.recover.failed")));
    } finally {
      setBusy(false);
    }
  };

  if (!open || skills.length === 0) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div
        className="absolute inset-0 bg-black/70 backdrop-blur-sm"
        onClick={busy ? undefined : onClose}
      />
      <div className="relative flex max-h-[calc(85vh/var(--app-scale))] w-full max-w-3xl flex-col rounded-xl border border-border bg-surface p-5 shadow-2xl">
        <div className="mb-1 flex items-start justify-between gap-3">
          <h2 className="flex items-center gap-2 text-[13px] font-semibold text-primary">
            <GitBranch className="h-4 w-4 text-accent-light" />
            {t("mySkills.batchRecover.title")}
          </h2>
          <button
            onClick={onClose}
            disabled={busy}
            className="rounded p-1 text-muted outline-none transition-colors hover:text-secondary disabled:opacity-40"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <p className="mb-4 text-[12.5px] leading-relaxed text-tertiary">
          {t("mySkills.batchRecover.subtitle", { count: skills.length })}
          {skipped > 0 && (
            <> {t("mySkills.batchRecover.skippedNotice", { count: skipped })}</>
          )}
        </p>

        {preview ? (
          <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-1">
            <div className="flex items-start gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2.5 text-[12.5px] leading-relaxed text-amber-700 dark:text-amber-300">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{t("mySkills.recover.oneWayWarning")}</span>
            </div>

            {failedItems.length > 0 && (
              <div className="rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2.5">
                <p className="mb-1.5 text-[12.5px] font-medium text-red-600 dark:text-red-300">
                  {t("mySkills.batchRecover.failedLine", { count: failedItems.length })}
                </p>
                <ul className="space-y-1">
                  {failedItems.map((item) => (
                    <li key={item.skill_id} className="text-[12px] text-red-600 dark:text-red-300">
                      <span className="font-medium">{item.name}</span>
                      <span className="text-red-500/80"> — {item.error}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            <p className="text-[12.5px] text-tertiary">
              {t("mySkills.batchRecover.summaryLine", { count: awaiting.length })}
              {removalTotal > 0 && (
                <> {t("mySkills.batchRecover.removalsTotal", { count: removalTotal })}</>
              )}
              {preview.applied > 0 && (
                <> {t("mySkills.batchRecover.alreadyApplied", { count: preview.applied })}</>
              )}
            </p>

            <div className="space-y-2">
              {awaiting.map((item) => (
                <div
                  key={item.skill_id}
                  className="rounded-lg border border-border-subtle bg-bg-secondary px-3 py-2"
                >
                  <div className="mb-1 flex items-baseline justify-between gap-2">
                    <span className="truncate text-[12.5px] font-medium text-primary">
                      {item.name}
                    </span>
                    <span className="shrink-0 font-mono text-[11px] text-faint">
                      {item.revision.slice(0, 8)}
                    </span>
                  </div>
                  {!item.central_copy_exists && (
                    <p className="mb-1 text-[11.5px] text-red-600 dark:text-red-300">
                      {t("mySkills.recover.centralCopyMissing")}
                    </p>
                  )}
                  {item.pending_removals.length === 0 ? (
                    <p className="text-[11.5px] text-tertiary">
                      {t("mySkills.recover.contentDiffers")}
                    </p>
                  ) : (
                    <ul className="max-h-24 space-y-0.5 overflow-y-auto">
                      {item.pending_removals.map((removal) => (
                        <li
                          key={`${removal.location}: ${removal.path}`}
                          className="flex items-baseline gap-2 font-mono text-[11px] text-secondary"
                        >
                          <span className="shrink-0 text-faint">{removal.location}</span>
                          <span className="break-all">{removal.path}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                  {item.diff_entries.length > 0 && (
                    <details className="mt-1.5">
                      <summary className="cursor-pointer text-[11.5px] text-tertiary outline-none hover:text-secondary">
                        {t("mySkills.batchRecover.showDiff")}
                      </summary>
                      <div className="mt-1.5">
                        <SkillSourceDiffViewer entries={item.diff_entries} />
                      </div>
                    </details>
                  )}
                </div>
              ))}
            </div>

            {/* The approval leg re-resolves every source, so it can fail where
                the first leg succeeded. */}
            {problem && <p className="text-[12.5px] text-red-600 dark:text-red-300">{problem}</p>}

            <div className="flex justify-end gap-2">
              <button
                onClick={() => setPreview(null)}
                disabled={busy}
                className="rounded-lg px-3 py-1.5 text-[13px] font-medium text-tertiary outline-none transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
              >
                {t("mySkills.recover.back")}
              </button>
              <button
                onClick={confirmBatch}
                disabled={busy || awaiting.length === 0}
                className="rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white outline-none transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
              >
                {busy ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  t("mySkills.batchRecover.confirmButton", { count: awaiting.length })
                )}
              </button>
            </div>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col gap-3">
            <div className="min-h-0 flex-1 space-y-2 overflow-y-auto pr-1">
              {skills.map((skill) => (
                <RecoverRow
                  key={skill.id}
                  skill={skill}
                  active={chosenFor(skill)}
                  manual={manual[skill.id] ?? { url: "", subpath: "" }}
                  onCandidate={(hit) => {
                    setManual((prev) => {
                      const next = { ...prev };
                      delete next[skill.id];
                      return next;
                    });
                    setPicks((prev) => ({ ...prev, [skill.id]: pickFromCandidate(hit) }));
                  }}
                  onManual={(patch) =>
                    setManual((prev) => {
                      const current = prev[skill.id] ?? { url: "", subpath: "" };
                      return { ...prev, [skill.id]: { ...current, ...patch } };
                    })
                  }
                />
              ))}
            </div>

            {problem && <p className="text-[12.5px] text-red-600 dark:text-red-300">{problem}</p>}

            <div className="flex items-center justify-between gap-2">
              <p className="min-w-0 flex-1 text-[11.5px] text-faint">
                {missing.length > 0
                  ? t("mySkills.batchRecover.missingSources", { count: missing.length })
                  : t("mySkills.batchRecover.readyHint")}
              </p>
              <button
                onClick={() => void previewBatch()}
                disabled={busy || missing.length > 0}
                className="shrink-0 rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white outline-none transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
              >
                {busy ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  t("mySkills.batchRecover.previewButton", { count: requests.length })
                )}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}