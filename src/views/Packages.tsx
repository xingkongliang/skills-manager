import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Boxes,
  CheckCircle2,
  GitBranch,
  Link2,
  Loader2,
  PackagePlus,
  Play,
  RefreshCw,
  ShieldAlert,
  Trash2,
  X,
  XCircle,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { ConfirmDialog } from "../components/ConfirmDialog";
import { useApp } from "../context/AppContext";
import { getErrorMessage } from "../lib/error";
import * as api from "../lib/tauri";
import type {
  BindingPlan,
  PackageBinding,
  PackageDetails,
  PackageScope,
  SurfacePolicy,
} from "../lib/tauri";
import { cn } from "../utils";

type BusyAction = "load" | "import" | "update" | "binding" | "apply" | "sync" | null;

function compatibilityClass(value: string) {
  if (value === "full") return "border-emerald-500/30 bg-emerald-500/10 text-emerald-400";
  if (value === "partial") return "border-amber-500/30 bg-amber-500/10 text-amber-400";
  return "border-red-500/30 bg-red-500/10 text-red-400";
}

function stateClass(value: string) {
  if (value === "installed") return "text-emerald-400";
  if (value === "partial" || value === "drifted") return "text-amber-400";
  if (value === "failed") return "text-red-400";
  return "text-muted";
}

function bindingSurfaceKind(binding: PackageBinding, surfaces: PackageDetails["surfaces"]) {
  const resolved = surfaces.find((surface) => surface.id === binding.resolved_surface_id);
  if (resolved) return resolved.kind;
  const rank = { native_plugin: 4, host_bundle: 3, portable_skills: 2, setup_script: 1 } as const;
  return surfaces
    .filter((surface) => surface.tool === binding.tool || surface.tool === "*")
    .filter((surface) => binding.surface_policy === "auto" || surface.kind.startsWith(binding.surface_policy))
    .sort((a, b) =>
      Number(b.tool === binding.tool) - Number(a.tool === binding.tool)
      || rank[b.kind] - rank[a.kind]
      || b.priority - a.priority)[0]?.kind;
}

function PlanDialog({
  plan,
  loading,
  onClose,
  onApply,
}: {
  plan: BindingPlan | null;
  loading: boolean;
  onClose: () => void;
  onApply: () => Promise<void>;
}) {
  const { t } = useTranslation();
  if (!plan) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center px-4">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative max-h-[85vh] w-full max-w-2xl overflow-y-auto rounded-xl border border-border bg-surface p-5 shadow-2xl">
        <div className="mb-4 flex items-start justify-between gap-4">
          <div>
            <div className="mb-1 flex items-center gap-2">
              <ShieldAlert className="h-4 w-4 text-amber-400" />
              <h2 className="text-[15px] font-semibold text-primary">{t("packages.planTitle")}</h2>
            </div>
            <p className="text-[13px] text-tertiary">
              {plan.package_name} · {plan.package_revision.slice(0, 10)} · {plan.tool} · {plan.scope}
            </p>
          </div>
          <button onClick={onClose} className="rounded p-1 text-muted hover:text-primary" aria-label={t("common.close")}>
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="mb-4 flex flex-wrap gap-2">
          <span className={cn("rounded-full border px-2.5 py-1 text-[12px] font-medium", compatibilityClass(plan.compatibility))}>
            {t(`packages.compatibility.${plan.compatibility}`)}
          </span>
          <span className="rounded-full border border-border bg-bg-secondary px-2.5 py-1 text-[12px] text-secondary">
            {plan.surface_kind || t("packages.noSurface")}
          </span>
        </div>

        {plan.risk_items.length > 0 && (
          <div className="mb-4 rounded-lg border border-amber-500/25 bg-amber-500/5 p-3">
            <p className="mb-2 text-[12px] font-semibold uppercase tracking-wide text-amber-400">{t("packages.risks")}</p>
            <ul className="space-y-1 text-[13px] text-secondary">
              {plan.risk_items.map((item) => <li key={item}>• {item}</li>)}
            </ul>
          </div>
        )}

        <div className="mb-4 space-y-2">
          <p className="text-[12px] font-semibold uppercase tracking-wide text-muted">{t("packages.operations")}</p>
          {plan.operations.length === 0 ? (
            <p className="rounded-lg border border-border bg-bg-secondary p-3 text-[13px] text-tertiary">{t("packages.noOperations")}</p>
          ) : plan.operations.map((operation, index) => (
            <div key={`${operation.kind}-${index}`} className="rounded-lg border border-border bg-bg-secondary p-3">
              <p className="text-[13px] font-medium text-primary">{operation.description}</p>
              <p className="mt-1 break-all text-[12px] text-muted">{operation.target}</p>
              {operation.command && (
                <code className="mt-2 block overflow-x-auto rounded bg-black/25 px-2 py-1.5 text-[12px] text-tertiary">
                  {JSON.stringify(operation.command)}
                </code>
              )}
            </div>
          ))}
        </div>

        {plan.missing_components.length > 0 && (
          <p className="mb-4 text-[13px] text-amber-400">
            {t("packages.missingComponents", { components: plan.missing_components.join(", ") })}
          </p>
        )}

        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="rounded-lg px-3 py-1.5 text-[13px] text-tertiary hover:bg-surface-hover hover:text-secondary">
            {t("common.cancel")}
          </button>
          <button
            onClick={() => void onApply()}
            disabled={!plan.can_apply || loading}
            className="flex items-center gap-2 rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white hover:bg-accent disabled:cursor-not-allowed disabled:opacity-40"
          >
            {loading ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Play className="h-3.5 w-3.5" />}
            {t("packages.approveApply")}
          </button>
        </div>
      </div>
    </div>
  );
}

export function Packages() {
  const { t } = useTranslation();
  const { tools, projects } = useApp();
  const [items, setItems] = useState<PackageDetails[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [sourceUrl, setSourceUrl] = useState("");
  const [revision, setRevision] = useState("");
  const [busy, setBusy] = useState<BusyAction>("load");
  const [plan, setPlan] = useState<BindingPlan | null>(null);
  const [tool, setTool] = useState("");
  const [scope, setScope] = useState<PackageScope>("user");
  const [projectId, setProjectId] = useState("");
  const [surfacePolicy, setSurfacePolicy] = useState<SurfacePolicy>("auto");
  const [selectedComponents, setSelectedComponents] = useState<Set<string>>(new Set());
  const [removeBindingTarget, setRemoveBindingTarget] = useState<PackageBinding | null>(null);
  const [deletePackageTarget, setDeletePackageTarget] = useState<PackageDetails | null>(null);
  const [manifestProjectId, setManifestProjectId] = useState("");

  const selected = useMemo(
    () => items.find((item) => item.package.id === selectedId) ?? items[0] ?? null,
    [items, selectedId],
  );
  const availableTools = useMemo(
    () => [...tools].sort((a, b) => Number(b.installed) - Number(a.installed) || a.display_name.localeCompare(b.display_name)),
    [tools],
  );
  const skillNames = useMemo(() => {
    const names = selected?.components
      .filter((component) => component.kind === "skill")
      .map((component) => component.name) ?? [];
    return [...new Set(names)].sort();
  }, [selected]);
  const removingSetupBinding = Boolean(
    removeBindingTarget
      && selected
      && bindingSurfaceKind(removeBindingTarget, selected.surfaces) === "setup_script",
  );

  const reload = useCallback(async () => {
    const packages = await api.getPackages();
    setItems(packages);
    setSelectedId((current) => current && packages.some((item) => item.package.id === current)
      ? current
      : packages[0]?.package.id ?? null);
  }, []);

  useEffect(() => {
    reload()
      .catch((error) => toast.error(getErrorMessage(error, t("common.error"))))
      .finally(() => setBusy(null));
  }, [reload, t]);

  useEffect(() => {
    if (!tool && availableTools.length > 0) setTool(availableTools[0].key);
  }, [availableTools, tool]);

  useEffect(() => {
    if (!projectId && projects.length > 0) setProjectId(projects[0].id);
    if (!manifestProjectId && projects.length > 0) setManifestProjectId(projects[0].id);
  }, [manifestProjectId, projectId, projects]);

  useEffect(() => setSelectedComponents(new Set()), [selected?.package.id]);

  const run = async (action: BusyAction, work: () => Promise<void>) => {
    setBusy(action);
    try {
      await work();
    } catch (error) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setBusy(null);
    }
  };

  const handleImport = () => run("import", async () => {
    if (!sourceUrl.trim()) return;
    const imported = await api.importGitPackage(sourceUrl.trim(), revision);
    await reload();
    setSelectedId(imported.package.id);
    setSourceUrl("");
    setRevision("");
    toast.success(t("packages.imported", { name: imported.package.name }));
  });

  const handleCreateBinding = () => run("binding", async () => {
    if (!selected || !tool) return;
    const needsProject = scope === "project_shared" || scope === "project_local";
    if (needsProject && !projectId) {
      toast.error(t("packages.projectRequired"));
      return;
    }
    const nextPlan = await api.createPackageBinding(
      selected.package.id,
      tool,
      scope,
      needsProject ? projectId : null,
      surfacePolicy,
      [...selectedComponents],
    );
    await reload();
    setPlan(nextPlan);
  });

  const handleApply = async () => {
    if (!plan) return;
    setBusy("apply");
    try {
      const result = await api.applyPackageBinding(plan.binding_id, plan.plan_hash);
      toast.success(t("packages.applied", { name: result.plan.package_name }));
      setPlan(null);
      await reload();
    } catch (error) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setBusy(null);
    }
  };

  const handlePreview = (bindingId: string) => run("binding", async () => {
    setPlan(await api.previewPackageBinding(bindingId));
    await reload();
  });

  const toggleComponent = (name: string) => {
    setSelectedComponents((current) => {
      const next = new Set(current);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  if (busy === "load") {
    return <div className="flex min-h-[300px] items-center justify-center text-muted"><Loader2 className="h-5 w-5 animate-spin" /></div>;
  }

  return (
    <>
      <section className="space-y-4">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="flex items-center gap-2 text-lg font-semibold text-primary">
              <Boxes className="h-5 w-5 text-accent" />
              {t("packages.title")}
            </h1>
            <p className="mt-1 text-[13px] text-tertiary">{t("packages.subtitle")}</p>
          </div>
          {projects.length > 0 && (
            <div className="flex items-center gap-2">
              <select value={manifestProjectId} onChange={(event) => setManifestProjectId(event.target.value)} className="rounded-lg border border-border bg-bg-secondary px-2.5 py-1.5 text-[13px] text-secondary outline-none">
                {projects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}
              </select>
              <button
                onClick={() => run("sync", async () => {
                  const plans = await api.syncProjectPackageManifest(manifestProjectId);
                  await reload();
                  if (plans[0]) setPlan(plans[0]);
                  toast.success(t("packages.manifestSynced", { count: plans.length }));
                })}
                disabled={busy !== null || !manifestProjectId}
                className="flex items-center gap-2 rounded-lg border border-border bg-surface px-3 py-1.5 text-[13px] text-secondary hover:bg-surface-hover disabled:opacity-50"
              >
                {busy === "sync" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Link2 className="h-3.5 w-3.5" />}
                {t("packages.syncManifest")}
              </button>
            </div>
          )}
        </div>

        <div className="rounded-xl border border-border bg-surface p-4">
          <div className="grid gap-2 md:grid-cols-[1fr_150px_auto]">
            <input
              value={sourceUrl}
              onChange={(event) => setSourceUrl(event.target.value)}
              onKeyDown={(event) => { if (event.key === "Enter") void handleImport(); }}
              placeholder={t("packages.gitPlaceholder")}
              className="rounded-lg border border-border bg-bg-secondary px-3 py-2 text-[13px] text-primary outline-none focus:border-accent-border"
            />
            <input
              value={revision}
              onChange={(event) => setRevision(event.target.value)}
              placeholder={t("packages.revisionPlaceholder")}
              className="rounded-lg border border-border bg-bg-secondary px-3 py-2 text-[13px] text-primary outline-none focus:border-accent-border"
            />
            <button
              onClick={() => void handleImport()}
              disabled={busy !== null || !sourceUrl.trim()}
              className="flex items-center justify-center gap-2 rounded-lg border border-accent-border bg-accent-dark px-3 py-2 text-[13px] font-medium text-white hover:bg-accent disabled:opacity-50"
            >
              {busy === "import" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <PackagePlus className="h-3.5 w-3.5" />}
              {t("packages.import")}
            </button>
          </div>
        </div>

        {items.length === 0 ? (
          <div className="rounded-xl border border-dashed border-border p-10 text-center">
            <Boxes className="mx-auto mb-3 h-8 w-8 text-faint" />
            <p className="text-[14px] font-medium text-secondary">{t("packages.empty")}</p>
            <p className="mt-1 text-[13px] text-muted">{t("packages.emptyHint")}</p>
          </div>
        ) : (
          <div className="grid min-h-[480px] gap-4 lg:grid-cols-[260px_1fr]">
            <div className="space-y-2">
              {items.map((item) => (
                <button
                  key={item.package.id}
                  onClick={() => setSelectedId(item.package.id)}
                  className={cn(
                    "w-full rounded-xl border p-3 text-left transition-colors",
                    selected?.package.id === item.package.id
                      ? "border-accent-border bg-accent-bg"
                      : "border-border bg-surface hover:bg-surface-hover",
                  )}
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate text-[14px] font-medium text-primary">{item.package.name}</span>
                    <span className="rounded bg-bg-secondary px-1.5 py-0.5 text-[11px] text-muted">{item.bindings.length}</span>
                  </div>
                  <p className="mt-1 truncate text-[12px] text-muted">{item.package.source_url}</p>
                  <p className="mt-2 text-[11px] text-faint">{item.package.resolved_revision.slice(0, 10)}</p>
                </button>
              ))}
            </div>

            {selected && (
              <div className="space-y-4">
                <div className="rounded-xl border border-border bg-surface p-4">
                  <div className="flex items-start justify-between gap-4">
                    <div className="min-w-0">
                      <h2 className="truncate text-[16px] font-semibold text-primary">{selected.package.name}</h2>
                      <p className="mt-1 break-all text-[12px] text-muted">{selected.package.source_url}</p>
                      <div className="mt-3 flex flex-wrap gap-2 text-[12px] text-tertiary">
                        <span className="rounded-full border border-border px-2 py-1">{selected.package.manifest_kind}</span>
                        <span className="rounded-full border border-border px-2 py-1">{selected.components.length} {t("packages.components")}</span>
                        <span className="rounded-full border border-border px-2 py-1">{selected.surfaces.length} {t("packages.surfaces")}</span>
                      </div>
                    </div>
                    <div className="flex gap-1">
                      <button
                        onClick={() => run("update", async () => {
                          await api.updatePackage(selected.package.id);
                          await reload();
                          toast.success(t("packages.updated", { name: selected.package.name }));
                        })}
                        disabled={busy !== null}
                        className="rounded-lg p-2 text-muted hover:bg-surface-hover hover:text-primary disabled:opacity-50"
                        title={t("packages.update")}
                      >
                        <RefreshCw className={cn("h-4 w-4", busy === "update" && "animate-spin")} />
                      </button>
                      <button
                        onClick={() => setDeletePackageTarget(selected)}
                        disabled={busy !== null || selected.bindings.length > 0}
                        className="rounded-lg p-2 text-muted hover:bg-red-500/10 hover:text-red-400 disabled:opacity-30"
                        title={selected.bindings.length > 0 ? t("packages.removeBindingsFirst") : t("common.delete")}
                      >
                        <Trash2 className="h-4 w-4" />
                      </button>
                    </div>
                  </div>
                </div>

                <div className="rounded-xl border border-border bg-surface p-4">
                  <h3 className="mb-3 text-[13px] font-semibold text-primary">{t("packages.hostSurfaces")}</h3>
                  <div className="grid gap-2 sm:grid-cols-2">
                    {selected.surfaces.map((surface) => (
                      <div key={surface.id} className="rounded-lg border border-border bg-bg-secondary p-3">
                        <div className="flex items-center justify-between gap-2">
                          <span className="text-[13px] font-medium text-secondary">{surface.tool === "*" ? t("packages.allTools") : surface.tool}</span>
                          <span className="rounded bg-surface px-1.5 py-0.5 text-[11px] text-muted">{surface.kind}</span>
                        </div>
                        <p className="mt-1 text-[12px] text-faint">{surface.root_path}</p>
                      </div>
                    ))}
                  </div>
                </div>

                <div className="rounded-xl border border-border bg-surface p-4">
                  <h3 className="mb-3 text-[13px] font-semibold text-primary">{t("packages.addBinding")}</h3>
                  <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
                    <select value={tool} onChange={(event) => setTool(event.target.value)} className="rounded-lg border border-border bg-bg-secondary px-2.5 py-2 text-[13px] text-secondary outline-none">
                      {availableTools.map((item) => <option key={item.key} value={item.key}>{item.display_name}{item.installed ? "" : ` (${t("packages.notDetected")})`}</option>)}
                    </select>
                    <select value={scope} onChange={(event) => setScope(event.target.value as PackageScope)} className="rounded-lg border border-border bg-bg-secondary px-2.5 py-2 text-[13px] text-secondary outline-none">
                      <option value="user">user</option>
                      <option value="project_shared">project_shared</option>
                      <option value="project_local">project_local</option>
                    </select>
                    {(scope === "project_shared" || scope === "project_local") ? (
                      <select value={projectId} onChange={(event) => setProjectId(event.target.value)} className="rounded-lg border border-border bg-bg-secondary px-2.5 py-2 text-[13px] text-secondary outline-none">
                        <option value="">{t("packages.selectProject")}</option>
                        {projects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}
                      </select>
                    ) : (
                      <select value={surfacePolicy} onChange={(event) => setSurfacePolicy(event.target.value as SurfacePolicy)} className="rounded-lg border border-border bg-bg-secondary px-2.5 py-2 text-[13px] text-secondary outline-none">
                        <option value="auto">auto</option>
                        <option value="native">native</option>
                        <option value="portable">portable</option>
                        <option value="setup">setup</option>
                      </select>
                    )}
                    {(scope === "project_shared" || scope === "project_local") && (
                      <select value={surfacePolicy} onChange={(event) => setSurfacePolicy(event.target.value as SurfacePolicy)} className="rounded-lg border border-border bg-bg-secondary px-2.5 py-2 text-[13px] text-secondary outline-none">
                        <option value="auto">auto</option>
                        <option value="native">native</option>
                        <option value="portable">portable</option>
                        <option value="setup">setup</option>
                      </select>
                    )}
                  </div>

                  {skillNames.length > 0 && (
                    <div className="mt-3">
                      <p className="mb-2 text-[12px] text-muted">{t("packages.componentHint")}</p>
                      <div className="flex max-h-28 flex-wrap gap-1.5 overflow-y-auto">
                        {skillNames.map((name) => (
                          <button
                            key={name}
                            onClick={() => toggleComponent(name)}
                            className={cn(
                              "rounded-full border px-2 py-1 text-[11px] transition-colors",
                              selectedComponents.has(name)
                                ? "border-accent-border bg-accent-bg text-accent-light"
                                : "border-border bg-bg-secondary text-muted hover:text-secondary",
                            )}
                          >
                            {name}
                          </button>
                        ))}
                      </div>
                    </div>
                  )}

                  <button
                    onClick={() => void handleCreateBinding()}
                    disabled={busy !== null || !tool}
                    className="mt-3 flex items-center gap-2 rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white hover:bg-accent disabled:opacity-50"
                  >
                    {busy === "binding" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <GitBranch className="h-3.5 w-3.5" />}
                    {t("packages.previewBinding")}
                  </button>
                </div>

                <div className="rounded-xl border border-border bg-surface p-4">
                  <h3 className="mb-3 text-[13px] font-semibold text-primary">{t("packages.bindings")}</h3>
                  {selected.bindings.length === 0 ? (
                    <p className="text-[13px] text-muted">{t("packages.noBindings")}</p>
                  ) : (
                    <div className="space-y-2">
                      {selected.bindings.map((binding) => {
                        const project = projects.find((item) => item.id === binding.project_id);
                        return (
                          <div key={binding.id} className="flex items-center gap-3 rounded-lg border border-border bg-bg-secondary p-3">
                            {binding.compatibility === "full" ? <CheckCircle2 className="h-4 w-4 text-emerald-400" /> : binding.compatibility === "partial" ? <AlertTriangle className="h-4 w-4 text-amber-400" /> : <XCircle className="h-4 w-4 text-red-400" />}
                            <div className="min-w-0 flex-1">
                              <div className="flex flex-wrap items-center gap-2">
                                <span className="text-[13px] font-medium text-secondary">{binding.tool}</span>
                                <span className="text-[12px] text-muted">{binding.scope}</span>
                                {project && <span className="text-[12px] text-faint">· {project.name}</span>}
                              </div>
                              <p className={cn("mt-1 text-[12px]", stateClass(binding.state))}>{binding.state}{binding.last_error ? ` · ${binding.last_error}` : ""}</p>
                            </div>
                            <button onClick={() => void handlePreview(binding.id)} className="rounded p-1.5 text-muted hover:bg-surface-hover hover:text-primary" title={t("packages.previewBinding")}>
                              <Play className="h-3.5 w-3.5" />
                            </button>
                            <button onClick={() => setRemoveBindingTarget(binding)} className="rounded p-1.5 text-muted hover:bg-red-500/10 hover:text-red-400" title={t("common.delete")}>
                              <Trash2 className="h-3.5 w-3.5" />
                            </button>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        )}
      </section>

      <PlanDialog plan={plan} loading={busy === "apply"} onClose={() => setPlan(null)} onApply={handleApply} />
      <ConfirmDialog
        open={removeBindingTarget !== null}
        title={t("packages.removeBinding")}
        message={t(removingSetupBinding ? "packages.removeSetupBindingMessage" : "packages.removeBindingMessage")}
        details={removeBindingTarget ? [removeBindingTarget.tool, removeBindingTarget.scope] : []}
        onClose={() => setRemoveBindingTarget(null)}
        onConfirm={async () => {
          if (!removeBindingTarget) return;
          await api.removePackageBinding(removeBindingTarget.id, removingSetupBinding);
          await reload();
          toast.success(t("packages.bindingRemoved"));
        }}
      />
      <ConfirmDialog
        open={deletePackageTarget !== null}
        title={t("packages.deletePackage")}
        message={t("packages.deletePackageMessage")}
        details={deletePackageTarget ? [deletePackageTarget.package.name] : []}
        onClose={() => setDeletePackageTarget(null)}
        onConfirm={async () => {
          if (!deletePackageTarget) return;
          await api.deletePackage(deletePackageTarget.package.id);
          await reload();
          toast.success(t("packages.packageDeleted"));
        }}
      />
    </>
  );
}
