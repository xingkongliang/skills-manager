import { useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, GitBranch, Loader2, Search, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { getErrorMessage, getErrorReason } from "../lib/error";
import {
  pickFromCandidate,
  repoLabel,
  searchSeeds,
  toRepoUrl,
  useSkillsshSearch,
} from "../lib/recoverSource";
import * as api from "../lib/tauri";
import type {
  ManagedSkill,
  RecoverSkillSourceResult,
  RecoverSourcePick,
  SkillsShSkill,
} from "../lib/tauri";
import { SkillSourceDiffViewer } from "./SkillSourceDiffViewer";

interface Props {
  open: boolean;
  skill: ManagedSkill | null;
  onClose: () => void;
  onDone: () => Promise<void> | void;
  /**
   * Offered from the empty state, because a skill that was never published has
   * nothing to find and this is the one action that always resolves it. The
   * button beside it already does this; pointing at it in words alone is not
   * enough when the user has just been told there is no match.
   */
  onKeepLocal?: () => Promise<void> | void;
}

export function RecoverSourceDialog({ open, skill, onClose, onDone, onKeepLocal }: Props) {
  const { t } = useTranslation();
  const [seedOverride, setSeedOverride] = useState<string | null>(null);
  const [manualUrl, setManualUrl] = useState("");
  const [manualSubpath, setManualSubpath] = useState("");
  const [pick, setPick] = useState<RecoverSourcePick | null>(null);
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<RecoverSkillSourceResult | null>(null);
  const [problem, setProblem] = useState<{ message: string; subpath: boolean; auth: boolean } | null>(
    null
  );
  const subpathRef = useRef<HTMLInputElement>(null);

  // Derived, not stored: seeding the box from the skill is what makes the
  // first search useful, and keeping it in state would let the auto-search fire
  // once with the previous skill's query before the new one lands.
  const defaultSeed = skill ? searchSeeds(skill)[0] ?? skill.name : "";
  const seed = seedOverride ?? defaultSeed;

  const {
    grouped,
    searching,
    searchFailed,
    searched,
    runSearch,
    reset: resetSearch,
  } = useSkillsshSearch(seed, open && !!skill);

  // A fresh dialog each time: the previous skill's candidates and the approval
  // it was given describe a different row and a different revision. `resetSearch`
  // also orphans an answer still in flight for the skill being left behind.
  useEffect(() => {
    if (!open || !skill) return;
    resetSearch();
    setSeedOverride(null);
    setManualUrl("");
    setManualSubpath("");
    setPick(null);
    setPreview(null);
    setProblem(null);
    setBusy(false);
  }, [open, skill, resetSearch]);

  const chooseCandidate = (hit: SkillsShSkill) => {
    setProblem(null);
    setPreview(null);
    setManualUrl("");
    setPick(pickFromCandidate(hit));
  };

  /** The manual entry, used when nothing has been picked from the list. */
  const manualPick = useMemo<RecoverSourcePick | null>(() => {
    const url = toRepoUrl(manualUrl);
    if (!url) return null;
    return { repoUrl: url, subpath: manualSubpath.trim() || null };
  }, [manualUrl, manualSubpath]);

  // Typing a URL is itself a choice, so it takes over from a picked candidate
  // rather than being silently ignored next to it.
  const chosen = manualUrl.trim() ? manualPick : pick;

  const finish = async (result: RecoverSkillSourceResult) => {
    await onDone();
    toast.success(
      result.content_changed
        ? t("mySkills.recover.reinstalled", { source: result.clone_url })
        : t("mySkills.recover.reinstalledUnchanged")
    );
    onClose();
  };

  /** The first leg never commits — it reports. */
  const previewPick = async () => {
    if (!skill || !chosen) return;
    setBusy(true);
    setProblem(null);
    try {
      const result = await api.recoverSkillSource(skill.id, chosen);
      if (result.applied) {
        await finish(result);
      } else {
        setPreview(result);
      }
    } catch (error) {
      const reason = getErrorReason(error);
      setProblem({
        message: getErrorMessage(error, t("mySkills.recover.failed")),
        subpath: reason === "subpath_required",
        auth: reason === "auth_failed",
      });
      // A repository root that is not a skill is the one failure the user can
      // answer right here, so put the answer where they are already typing.
      if (reason === "subpath_required") subpathRef.current?.focus();
    } finally {
      setBusy(false);
    }
  };

  /** The second leg carries the approval for exactly that revision and list. */
  const confirmPick = async () => {
    if (!skill || !chosen || !preview?.removal_approval) return;
    setBusy(true);
    try {
      const result = await api.recoverSkillSource(
        skill.id,
        chosen,
        preview.removal_approval
      );
      if (result.applied) {
        await finish(result);
      } else {
        // Something changed while the dialog was open. Show the new list rather
        // than applying a decision made against the old one — and drop the
        // previous attempt's error, which described a state that is gone.
        setProblem(null);
        setPreview(result);
      }
    } catch (error) {
      setProblem({
        message: getErrorMessage(error, t("mySkills.recover.failed")),
        subpath: false,
        auth: false,
      });
    } finally {
      setBusy(false);
    }
  };

  if (!open || !skill) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={busy ? undefined : onClose} />
      <div className="relative flex max-h-[calc(85vh/var(--app-scale))] w-full max-w-2xl flex-col rounded-xl border border-border bg-surface p-5 shadow-2xl">
        <div className="mb-1 flex items-start justify-between gap-3">
          <h2 className="flex items-center gap-2 text-[13px] font-semibold text-primary">
            <GitBranch className="h-4 w-4 text-accent-light" />
            {t("mySkills.recover.title")}
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
          {t("mySkills.recover.subtitle", { name: skill.name })}
        </p>

        {preview ? (
          <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-1">
            <div className="rounded-lg border border-border-subtle bg-bg-secondary px-3 py-2 font-mono text-[12px] text-secondary">
              {repoLabel(chosen!)}
              {preview.subpath ? ` / ${preview.subpath}` : ""}
              {" @ "}
              {preview.revision.slice(0, 8)}
            </div>

            {!preview.central_copy_exists && (
              <div className="flex items-start gap-2 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2.5 text-[12.5px] leading-relaxed text-red-600 dark:text-red-300">
                <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
                <span>{t("mySkills.recover.centralCopyMissing")}</span>
              </div>
            )}

            <div className="flex items-start gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2.5 text-[12.5px] leading-relaxed text-amber-700 dark:text-amber-300">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>
                {t("mySkills.recover.oneWayWarning")}
                {preview.duplicate_skill_name && (
                  <> {t("mySkills.recover.duplicateWarning", { name: preview.duplicate_skill_name })}</>
                )}
              </span>
            </div>

            {preview.pending_removals.length === 0 ? (
              <p className="text-[12.5px] text-tertiary">{t("mySkills.recover.contentDiffers")}</p>
            ) : (
              <div>
                <p className="mb-2 text-[12.5px] text-tertiary">
                  {t("mySkills.recover.removalMessage", {
                    name: skill.name,
                    count: preview.pending_removals.length,
                  })}
                </p>
                <ul className="max-h-40 space-y-1 overflow-y-auto rounded-lg border border-border-subtle bg-bg-secondary p-2">
                  {preview.pending_removals.map((removal) => (
                    <li
                      key={`${removal.location}: ${removal.path}`}
                      className="flex items-baseline gap-2 font-mono text-[11.5px] text-secondary"
                    >
                      <span className="shrink-0 text-faint">{removal.location}</span>
                      <span className="break-all">{removal.path}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            <SkillSourceDiffViewer entries={preview.diff_entries} />

            {/* The approval leg runs the whole fetch again, so it can fail
                where the first one succeeded. Without this the user would be
                left looking at an unchanged dialog, with a button that is
                merely re-clickable. */}
            {problem && !problem.subpath && !problem.auth && (
              <p className="text-[12.5px] text-red-600 dark:text-red-300">{problem.message}</p>
            )}

            <div className="flex justify-end gap-2">
              <button
                onClick={() => setPreview(null)}
                disabled={busy}
                className="rounded-lg px-3 py-1.5 text-[13px] font-medium text-tertiary outline-none transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
              >
                {t("mySkills.recover.back")}
              </button>
              <button
                onClick={confirmPick}
                disabled={busy || !preview.removal_approval}
                className="rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white outline-none transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
              >
                {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : t("mySkills.recover.confirmButton")}
              </button>
            </div>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-1">
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
              <p className="text-[12.5px] text-tertiary">{t("mySkills.recover.searchUnavailable")}</p>
            )}
            {searched && !searchFailed && !searching && grouped.length === 0 && (
              <div className="rounded-lg border border-border-subtle bg-bg-secondary px-3 py-2.5">
                <p className="text-[12.5px] leading-relaxed text-tertiary">
                  {t("mySkills.recover.noResultsHint")}
                </p>
                {onKeepLocal && (
                  <button
                    onClick={() => void onKeepLocal()}
                    className="mt-2 rounded-lg border border-border-subtle px-2.5 py-1 text-[12.5px] font-medium text-secondary outline-none transition-colors hover:bg-surface-hover"
                  >
                    {t("mySkills.updateActions.detachSource")}
                  </button>
                )}
              </div>
            )}

            <div className="space-y-3">
              {grouped.map((group) => (
                <div key={group.level}>
                  {group.level !== "exact" && (
                    <div className="mb-1.5 text-[11px] font-medium uppercase tracking-wide text-faint">
                      {t(`mySkills.recover.confidence.${group.level}`)}
                    </div>
                  )}
                  <div className="space-y-1">
                    {group.hits.map((hit) => (
                      <button
                        key={hit.id}
                        onClick={() => chooseCandidate(hit)}
                        className={
                          "flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left outline-none transition-colors " +
                          (chosen?.locatorSkillId === hit.skill_id && chosen.locatorSource === hit.source
                            ? "border-accent/50 bg-accent-bg"
                            : "border-border-subtle hover:bg-surface-hover")
                        }
                      >
                        <span className="min-w-0 flex-1 truncate text-[13px] text-primary">{hit.name}</span>
                        {group.level === "exact" && (
                          <span className="shrink-0 rounded-full bg-accent-bg px-2 py-0.5 text-[11px] font-medium text-accent-light">
                            {t("mySkills.recover.confidence.exact")}
                          </span>
                        )}
                        <span className="shrink-0 font-mono text-[11.5px] text-faint">{hit.source}</span>
                      </button>
                    ))}
                  </div>
                </div>
              ))}
            </div>

            <div className="border-t border-border-subtle pt-3">
              <div className="mb-2 text-[11px] font-medium uppercase tracking-wide text-faint">
                {t("mySkills.recover.manual")}
              </div>
              <div className="space-y-2">
                <input
                  value={manualUrl}
                  onChange={(e) => setManualUrl(e.target.value)}
                  placeholder={t("mySkills.recover.manualPlaceholder")}
                  className="app-input w-full"
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                />
                <input
                  ref={subpathRef}
                  value={manualSubpath}
                  onChange={(e) => setManualSubpath(e.target.value)}
                  placeholder={t("mySkills.recover.subpathPlaceholder")}
                  className="app-input w-full"
                  autoCapitalize="none"
                  autoCorrect="off"
                  spellCheck={false}
                />
                {problem?.subpath && (
                  <p className="text-[12.5px] text-amber-600 dark:text-amber-300">
                    {t("mySkills.recover.subpathRequired")}
                  </p>
                )}
                {problem?.auth && (
                  <p className="text-[12.5px] text-amber-600 dark:text-amber-300">
                    {t("mySkills.recover.authFailedHint")}
                  </p>
                )}
              </div>
            </div>

            {problem && !problem.subpath && !problem.auth && (
              <p className="text-[12.5px] text-red-600 dark:text-red-300">{problem.message}</p>
            )}

            <div className="flex items-center justify-between gap-2">
              <p className="min-w-0 flex-1 truncate text-[11.5px] text-faint">
                {chosen ? repoLabel(chosen) : t("mySkills.recover.pickHint")}
              </p>
              <button
                onClick={() => void previewPick()}
                disabled={busy || !chosen}
                className="shrink-0 rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white outline-none transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
              >
                {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : t("mySkills.recover.continueBtn")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}