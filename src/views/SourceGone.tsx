import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft,
  ExternalLink,
  FolderSearch,
  Loader2,
  RefreshCw,
  Trash2,
  Unlink,
} from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "../components/ConfirmDialog";
import * as api from "../lib/tauri";
import type { ManagedSkill, SourceScanResult } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";
import {
  repairGroups,
  repoUrl,
  REPAIR_GROUP_ORDER,
  type RepairRow,
  type ScanItem,
} from "../lib/sourceRepair";

/**
 * The page for skills their Source no longer holds where they were installed.
 *
 * It exists because the answer is rarely "delete": a Source can move a skill,
 * rewrite its history, or vanish while the copy still works in every agent.
 * The page shows what is known, offers to look closer, and never decides for
 * the user (#655).
 */
export function SourceGone() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [skills, setSkills] = useState<ManagedSkill[]>([]);
  const [scans, setScans] = useState<Record<string, ScanItem[]>>({});
  const [scanErrors, setScanErrors] = useState<Record<string, string>>({});
  const [scanning, setScanning] = useState<Record<string, boolean>>({});
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [overwrite, setOverwrite] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [deleteConfirm, setDeleteConfirm] = useState(false);

  const refresh = useCallback(async () => {
    setSkills(await api.getManagedSkills());
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const groups = useMemo(() => repairGroups(skills, scans), [skills, scans]);
  const rows = useMemo(() => REPAIR_GROUP_ORDER.flatMap((group) => groups[group]), [groups]);
  const sources = useMemo(() => {
    const byKey = new Map<string, { key: string; cloneUrl: string; branch: string | null }>();
    for (const row of rows) {
      if (!row.cloneUrl || byKey.has(row.sourceKey)) continue;
      byKey.set(row.sourceKey, {
        key: row.sourceKey,
        cloneUrl: row.cloneUrl,
        branch: row.branch,
      });
    }
    return [...byKey.values()];
  }, [rows]);

  const scanSource = async (source: { key: string; cloneUrl: string; branch: string | null }) => {
    // One clone per Source per visit: the second click would learn the same
    // thing and cost the same round trip.
    if (scans[source.key]) return;
    setScanning((prev) => ({ ...prev, [source.key]: true }));
    try {
      const result: SourceScanResult = await api.scanGitSource(source.cloneUrl, source.branch);
      setScans((prev) => ({ ...prev, [source.key]: result.items }));
      setScanErrors((prev) => ({ ...prev, [source.key]: result.error ?? "" }));
      const found = result.items.some((item) => item.placement !== "gone");
      if (!result.error && !found) {
        toast.info(t("mySkills.repair.noChange"));
      }
    } catch (error: unknown) {
      setScanErrors((prev) => ({
        ...prev,
        [source.key]: getErrorMessage(error, t("common.error")),
      }));
    } finally {
      setScanning((prev) => ({ ...prev, [source.key]: false }));
    }
  };

  // Deletion is never preselected: it is the one action that cannot be undone.
  const toggle = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const toggleOverwrite = (id: string) =>
    setOverwrite((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    try {
      await work();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      await refresh();
      setBusy(false);
    }
  };

  const deleteSelected = () =>
    run(async () => {
      const ids = [...selected];
      if (ids.length === 0) return;
      const result = await api.deleteManagedSkills(ids);
      if (result.deleted > 0) {
        toast.success(t("mySkills.cleanup.deleted", { count: result.deleted }));
      }
      if (result.failed.length > 0) {
        toast.error(t("mySkills.cleanup.failed", { count: result.failed.length }));
      }
      setSelected(new Set());
    });

  const detach = (row: RepairRow) =>
    run(async () => {
      await api.detachSkillSource(row.id);
      toast.success(t("mySkills.repair.detached", { name: row.name }));
    });

  const update = (row: RepairRow) =>
    run(async () => {
      await api.updateSkill(row.id);
      toast.success(t("mySkills.repair.updated", { name: row.name }));
    });

  const repoint = (row: RepairRow, subpath: string) =>
    run(async () => {
      // `force` is about the library copy, not about the move: an untouched
      // copy may be replaced by the new location's content, an edited one only
      // when the user ticked it.
      const force = !row.localChanged || overwrite.has(row.id);
      await api.repointSkillSource(row.id, row.cloneUrl ?? "", subpath, force);
      toast.success(t("mySkills.repair.repointed", { name: row.name, subpath }));
    });

  return (
    <div className="app-page">
      <div className="app-page-header pr-2 pb-1 flex items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <button
            onClick={() => navigate("/my-skills")}
            className="rounded-md p-1.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
            aria-label={t("mySkills.repair.back")}
          >
            <ArrowLeft className="h-4 w-4" />
          </button>
          <div>
            <h1 className="app-page-title">{t("mySkills.repair.title")}</h1>
            <p className="app-page-subtitle">{t("mySkills.repair.subtitle", { count: rows.length })}</p>
          </div>
        </div>
        <button
          onClick={() => setDeleteConfirm(true)}
          disabled={busy || selected.size === 0}
          className="app-button-secondary text-red-600 disabled:opacity-50 dark:text-red-300"
        >
          <Trash2 className="h-3.5 w-3.5" />
          {t("mySkills.repair.deleteSelected", { count: selected.size })}
        </button>
      </div>

      <ConfirmDialog
        open={deleteConfirm}
        title={t("mySkills.repair.deleteTitle", { count: selected.size })}
        message={t("mySkills.repair.deleteWarning")}
        details={rows
          .filter((row) => selected.has(row.id))
          .map((row) =>
            t("mySkills.cleanup.row", {
              name: row.name,
              subpath: row.subpath || t("mySkills.cleanup.rootSubpath"),
              path: row.centralPath,
              deployments: row.deployments,
              presets: row.presets,
            })
          )}
        confirmLabel={t("mySkills.repair.deleteSelected", { count: selected.size })}
        onClose={() => setDeleteConfirm(false)}
        onConfirm={async () => {
          await deleteSelected();
          setDeleteConfirm(false);
        }}
      />

      {rows.length === 0 ? (
        <p className="mt-6 text-[13px] text-muted">{t("mySkills.repair.empty")}</p>
      ) : (
        <div className="mt-4 space-y-6">
          {sources.map((source) => {
            const sourceRows = rows.filter((row) => row.sourceKey === source.key);
            const error = scanErrors[source.key];
            return (
              <section key={source.key} className="rounded-xl border border-border-subtle">
                <header className="flex flex-wrap items-center justify-between gap-2 border-b border-border-subtle px-4 py-3">
                  <div className="min-w-0">
                    <p className="truncate text-[13px] font-medium text-secondary">{source.cloneUrl}</p>
                    <p className="text-[12px] text-muted">
                      {t("mySkills.repair.sourceMeta", {
                        count: sourceRows.length,
                        branch: source.branch || t("mySkills.repair.defaultBranch"),
                      })}
                    </p>
                  </div>
                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => {
                        const url = repoUrl(source.cloneUrl);
                        if (url) void openUrl(url).catch(() => {});
                      }}
                      className="rounded-md px-2.5 py-1.5 text-[12px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
                    >
                      <ExternalLink className="mr-1 inline h-3.5 w-3.5" />
                      {t("mySkills.repair.openRepo")}
                    </button>
                    <button
                      onClick={() => void scanSource(source)}
                      disabled={scanning[source.key]}
                      className="inline-flex items-center gap-1.5 rounded-md border border-border-subtle px-2.5 py-1.5 text-[12px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                    >
                      {scanning[source.key] ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <FolderSearch className="h-3.5 w-3.5" />
                      )}
                      {t("mySkills.repair.scan")}
                    </button>
                  </div>
                </header>

                {error && (
                  <p className="border-b border-border-subtle bg-red-500/5 px-4 py-2 text-[12px] text-red-600 dark:text-red-300">
                    {t("mySkills.repair.scanFailed", { reason: error })}
                  </p>
                )}

                {REPAIR_GROUP_ORDER.map((group) => {
                  const groupRows = sourceRows.filter((row) => row.group === group);
                  if (groupRows.length === 0) return null;
                  return (
                    <div key={group} className="px-4 py-3">
                      <p className="mb-2 text-[12px] font-semibold uppercase tracking-[0.12em] text-faint">
                        {t(`mySkills.repair.groups.${group}`, { count: groupRows.length })}
                      </p>
                      <ul className="space-y-2">
                        {groupRows.map((row) => (
                          <li
                            key={row.id}
                            className="rounded-lg border border-border-subtle px-3 py-2"
                          >
                            <div className="flex flex-wrap items-start justify-between gap-2">
                              <label className="flex min-w-0 items-start gap-2">
                                <input
                                  type="checkbox"
                                  checked={selected.has(row.id)}
                                  onChange={() => toggle(row.id)}
                                  className="mt-1"
                                />
                                <span className="min-w-0">
                                  <span className="block truncate text-[13px] font-medium text-secondary">
                                    {row.name}
                                  </span>
                                  <span className="block break-all text-[12px] text-muted">
                                    {t("mySkills.repair.rowMeta", {
                                      subpath: row.subpath || t("mySkills.cleanup.rootSubpath"),
                                      path: row.centralPath,
                                      deployments: row.deployments,
                                      presets: row.presets,
                                    })}
                                  </span>
                                  {row.scanned && row.installedRevisionPresent !== null && (
                                    <span className="mt-1 block text-[12px] text-faint">
                                      {row.installedRevisionPresent
                                        ? t("mySkills.repair.revisionKnown")
                                        : t("mySkills.repair.revisionGone")}
                                    </span>
                                  )}
                                </span>
                              </label>

                              <div className="flex flex-wrap items-center gap-1.5">
                                {row.repoUrl && (
                                  <button
                                    onClick={() => void openUrl(row.repoUrl!).catch(() => {})}
                                    className="rounded-full border border-border-subtle px-2 py-0.5 text-[12px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
                                  >
                                    {t("mySkills.repair.openRepo")}
                                  </button>
                                )}
                                {row.pathUrl && (
                                  <button
                                    onClick={() => void openUrl(row.pathUrl!).catch(() => {})}
                                    className="rounded-full border border-border-subtle px-2 py-0.5 text-[12px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
                                  >
                                    {t("mySkills.repair.openPath")}
                                  </button>
                                )}
                                {row.group === "at" && (
                                  <button
                                    onClick={() => void update(row)}
                                    disabled={busy}
                                    className="inline-flex items-center gap-1 rounded-full border border-border-subtle px-2 py-0.5 text-[12px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                                  >
                                    <RefreshCw className="h-3 w-3" />
                                    {t("mySkills.repair.update")}
                                  </button>
                                )}
                                {row.group === "moved" &&
                                  row.candidates.map((candidate) => (
                                    <button
                                      key={candidate.subpath}
                                      onClick={() => void repoint(row, candidate.subpath)}
                                      disabled={busy || (row.localChanged && !overwrite.has(row.id))}
                                      className="rounded-full border border-border-subtle px-2 py-0.5 text-[12px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                                      title={t(`mySkills.repair.evidence.${candidate.evidence}`)}
                                    >
                                      {t("mySkills.repair.repoint", {
                                      subpath:
                                        candidate.subpath ||
                                        t("mySkills.cleanup.rootSubpath"),
                                    })}
                                    </button>
                                  ))}
                                {row.group !== "at" && (
                                  <button
                                    onClick={() => void detach(row)}
                                    disabled={busy}
                                    className="inline-flex items-center gap-1 rounded-full border border-border-subtle px-2 py-0.5 text-[12px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                                  >
                                    <Unlink className="h-3 w-3" />
                                    {t("mySkills.updateActions.detachSource")}
                                  </button>
                                )}
                              </div>
                            </div>
                            {row.localChanged && (
                              <label className="mt-2 flex items-center gap-1.5 text-[12px] text-amber-600 dark:text-amber-400">
                                <input
                                  type="checkbox"
                                  checked={overwrite.has(row.id)}
                                  onChange={() => toggleOverwrite(row.id)}
                                />
                                {t("mySkills.repair.overwrite")}
                              </label>
                            )}
                          </li>
                        ))}
                      </ul>
                    </div>
                  );
                })}
              </section>
            );
          })}
        </div>
      )}
    </div>
  );
}
