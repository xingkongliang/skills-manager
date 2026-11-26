import { useEffect, useState } from "react";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { BookOpen, ClipboardCopy, GitBranch, Loader2, Plus, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";
import { toast } from "sonner";
import { useApp } from "../context/AppContext";
import { getErrorMessage } from "../lib/error";
import * as api from "../lib/tauri";

const CATALOG_CHANGED = "repository-catalog-changed";
const CONTROL = "app-button-secondary inline-flex items-center gap-1.5";

function webUrl(value: string | null | undefined): string | undefined {
  if (!value) return undefined;
  try {
    const url = new URL(value);
    if (url.protocol === "https:" || url.protocol === "http:") return url.href;
  } catch {
    return undefined;
  }
}

function repositoryKind(repository: api.SkillRepository) {
  const identity = repository.identity.split("#ref=")[0].toLowerCase();
  if (identity === "repo:github.com/dietrichgebert/ponytail") return "ponytail";
  if (identity === "repo:github.com/fission-ai/openspec") return "openspec";
  return "generic";
}

function documentationUrl(repository: api.SkillRepository) {
  if (repository.documentation_url) return webUrl(repository.documentation_url);
  switch (repositoryKind(repository)) {
    case "ponytail":
      return "https://github.com/DietrichGebert/ponytail/blob/main/INSTALL.md";
    case "openspec":
      return "https://openspec.dev/docs/installation";
    default:
      return webUrl(repository.clone_url);
  }
}

function RepositoryIntegration({
  repository,
  canEnableSkills,
}: {
  repository: api.SkillRepository;
  canEnableSkills: boolean;
}) {
  const { t } = useTranslation();
  const { tools, projects, refreshManagedSkills } = useApp();
  const [agent, setAgent] = useState("");
  const [projectId, setProjectId] = useState("");
  const [busy, setBusy] = useState(false);
  const [observation, setObservation] = useState<api.OpenSpecObservation>();
  const [ponytail, setPonytail] = useState<api.PonytailObservation>();
  const [copied, setCopied] = useState(false);
  const kind = repositoryKind(repository);
  const project = projects.find((item) => item.id === projectId && item.workspace_type === "project");
  const tool = tools.find((item) => item.key === agent);
  const docs = documentationUrl(repository);

  const copyTask = async () => {
    if (!tool) return;
    const source = repository.source_ref;
    const version = repository.snapshot_revision || repository.branch || t("repositoryCatalog.upstreamDefault");
    const scope = project
      ? t("repositoryCatalog.taskProject", { path: project.path, interpolation: { escapeValue: false } })
      : t("repositoryCatalog.taskGlobal");
    const task = [
      t("repositoryCatalog.taskRequest", {
        source, version, target: tool.display_name, scope, interpolation: { escapeValue: false },
      }),
      docs ? t("repositoryCatalog.taskDocs", { url: docs, interpolation: { escapeValue: false } }) : "",
      t(`repositoryCatalog.taskHint.${kind}`),
      t("repositoryCatalog.taskVerify"),
    ].filter(Boolean).join("\n\n");
    try {
      await writeText(task);
      setCopied(true);
      toast.success(t("repositoryCatalog.taskCopied"));
    } catch (error) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const verify = async () => {
    if (!tool || busy) return;
    setBusy(true);
    setObservation(undefined);
    setPonytail(undefined);
    try {
      if (kind === "openspec") {
        setObservation(await api.observeOpenSpec(tool.key, project?.id ?? null));
      } else if (kind === "ponytail") {
        setPonytail(await api.observePonytail(tool.key, project?.id ?? null));
      }
    } catch (error) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setBusy(false);
    }
  };

  const enableSkills = async () => {
    if (!tool || busy) return;
    setBusy(true);
    try {
      const result = await api.importRepositorySkills(repository.id);
      if (project) {
        await api.batchDeploySkillsToProject(result.repository.skill_ids, project.id, [tool.key]);
      } else {
        await api.batchSyncSkillsToTools(result.repository.skill_ids, [tool.key]);
      }
      toast.success(t("repositoryGroups.enabledToast", { count: result.repository.skill_ids.length }));
    } catch (error) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      await Promise.allSettled([refreshManagedSkills()]);
      window.dispatchEvent(new Event(CATALOG_CHANGED));
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3 border-t border-border-subtle pt-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h4 className="text-[13px] font-medium text-primary">{t("repositoryCatalog.integration")}</h4>
        {docs && (
          <button type="button" className={CONTROL} onClick={() => {
            void openUrl(docs).catch((error) => toast.error(getErrorMessage(error, t("common.error"))));
          }}>
            <BookOpen className="h-3.5 w-3.5" />
            {t("repositoryCatalog.openDocs")}
          </button>
        )}
      </div>
      <p className="text-[13px] text-muted">{t(`repositoryCatalog.integrationHint.${kind}`)}</p>
      <div className="flex flex-wrap items-end gap-2">
        <label className="grid gap-1 text-[12px] text-muted">
          {t("repositoryGroups.target")}
          <select className="app-input" value={agent} disabled={busy} onChange={(event) => {
            setAgent(event.target.value);
            setObservation(undefined);
            setPonytail(undefined);
            setCopied(false);
          }}>
            <option value="">{t("repositoryCatalog.selectAgent")}</option>
            {tools.map((item) => <option key={item.key} value={item.key}>{item.display_name}</option>)}
          </select>
        </label>
        <label className="grid gap-1 text-[12px] text-muted">
          {t("repositoryCatalog.scope")}
          <select className="app-input" value={projectId} disabled={busy} onChange={(event) => {
            setProjectId(event.target.value);
            setObservation(undefined);
            setPonytail(undefined);
            setCopied(false);
          }}>
            <option value="">{t("repositoryCatalog.global")}</option>
            {projects.filter((item) => item.workspace_type === "project").map((item) => (
              <option key={item.id} value={item.id}>{item.name} · {item.path}</option>
            ))}
          </select>
        </label>
        {canEnableSkills && (
          <button type="button" className={CONTROL} disabled={!tool || !tool.enabled || busy}
            onClick={() => void enableSkills()}>
            {t("repositoryGroups.enableAll")}
          </button>
        )}
        <button type="button" className={CONTROL} onClick={() => void copyTask()}
          disabled={!tool || (kind === "openspec" && !project)}>
          <ClipboardCopy className="h-3.5 w-3.5" />
          {t("repositoryCatalog.copyTask")}
        </button>
        {kind !== "generic" && (
          <button type="button" className={CONTROL} disabled={!tool || busy} onClick={() => void verify()}>
            {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {t("repositoryCatalog.checkLocally")}
          </button>
        )}
      </div>
      {copied && <p role="status" className="text-[12px] text-muted">{t("repositoryCatalog.awaitingVerification")}</p>}
      {observation && (
        <ul className="list-disc space-y-1 pl-5 text-[12px] text-tertiary">
          <li>{observation.cli_path
            ? t("repositoryCatalog.cliFound", { path: observation.cli_path })
            : t("repositoryCatalog.cliMissing")}</li>
          {project && <li>{t(observation.project_config_found
            ? "repositoryCatalog.configFound" : "repositoryCatalog.configMissing")}</li>}
          {project && <li>{t(!observation.workflow_check_supported
            ? "repositoryCatalog.workflowUnknown"
            : observation.workflow_found ? "repositoryCatalog.workflowFound" : "repositoryCatalog.workflowMissing")}</li>}
        </ul>
      )}
      {ponytail && (
        <div role="status" className="space-y-1 text-[12px] text-tertiary">
          <p>{t(ponytail.plugin_files_found !== null
            ? ponytail.plugin_files_found ? "repositoryCatalog.pluginFound" : "repositoryCatalog.pluginMissing"
            : ponytail.cursor_hook_entries_found !== null
              ? ponytail.cursor_hook_entries_found ? "repositoryCatalog.hooksFound" : "repositoryCatalog.hooksMissing"
              : "repositoryCatalog.ponytailUnknown")}</p>
          {agent === "codex" && ponytail.plugin_files_found && <p>{t("repositoryCatalog.hooksReview")}</p>}
        </div>
      )}
    </div>
  );
}

export function RepositoryCatalog() {
  const { t } = useTranslation();
  const [searchParams, setSearchParams] = useSearchParams();
  const { refreshManagedSkills } = useApp();
  const [repositories, setRepositories] = useState<api.SkillRepository[]>([]);
  const [error, setError] = useState<string>();
  const [source, setSource] = useState(() => searchParams.get("source") || "");
  const [branch, setBranch] = useState("");
  const [docs, setDocs] = useState("");
  const [busy, setBusy] = useState<string>();
  const [expanded, setExpanded] = useState<string>();
  const [selection, setSelection] = useState<string[]>([]);

  useEffect(() => {
    let disposed = false;
    const load = () => {
      void api.listRepositories().then((items) => {
        if (!disposed) {
          setRepositories(items);
          setError(undefined);
        }
      }).catch((reason) => {
        if (!disposed) setError(getErrorMessage(reason, t("common.error")));
      });
    };
    load();
    window.addEventListener(CATALOG_CHANGED, load);
    return () => {
      disposed = true;
      window.removeEventListener(CATALOG_CHANGED, load);
    };
  }, [t]);

  const run = async (key: string, action: () => Promise<void>) => {
    if (busy) return;
    setBusy(key);
    try {
      await action();
    } catch (reason) {
      toast.error(getErrorMessage(reason, t("common.error")));
    } finally {
      window.dispatchEvent(new Event(CATALOG_CHANGED));
      setBusy(undefined);
    }
  };

  const save = () => void run("save", async () => {
    const repository = await api.saveRepository(source.trim(), docs.trim(), branch.trim());
    setSource("");
    setBranch("");
    setDocs("");
    setExpanded(repository.id);
    if (searchParams.has("source")) {
      const params = new URLSearchParams(searchParams);
      params.delete("source");
      setSearchParams(params, { replace: true });
    }
  });

  return (
    <section className="app-panel space-y-3 p-4">
      <h3 className="text-[14px] font-semibold text-primary">{t("repositoryCatalog.title")}</h3>
      <form className="flex flex-wrap items-end gap-2" onSubmit={(event) => {
        event.preventDefault();
        if (source.trim()) save();
      }}>
        <label className="grid min-w-48 flex-1 gap-1 text-[12px] text-muted">
          {t("install.repoUrl")}
          <input className="app-input" value={source} onChange={(event) => setSource(event.target.value)}
            placeholder="https://github.com/owner/repo" required disabled={!!busy} />
        </label>
        <label className="grid gap-1 text-[12px] text-muted">
          {t("repositoryCatalog.branch")}
          <input className="app-input" value={branch} onChange={(event) => setBranch(event.target.value)}
            disabled={!!busy} />
        </label>
        <label className="grid min-w-44 flex-1 gap-1 text-[12px] text-muted">
          {t("repositoryCatalog.docs")}
          <input className="app-input" value={docs} onChange={(event) => setDocs(event.target.value)}
            type="url" disabled={!!busy} />
        </label>
        <button type="submit" className={CONTROL} disabled={!source.trim() || !!busy}>
          {busy === "save" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Plus className="h-3.5 w-3.5" />}
          {t("repositoryCatalog.save")}
        </button>
      </form>
      {error && <p role="alert" className="text-[13px] text-red-600 dark:text-red-400">{error}</p>}
      {repositories.map((repository) => {
        const open = expanded === repository.id;
        const pending = repository.members.filter((member) => member.upstream_present && !member.installed_skill_ids.length);
        const selectedPaths = pending.filter((member) => selection.includes(member.rel_path)).map((member) => member.rel_path);
        const retired = repository.members.filter((member) => member.upstream_present === false && member.installed_skill_ids.length);
        return (
          <div className="rounded-lg border border-border p-3" key={repository.id}>
            <button type="button" aria-expanded={open} className="flex w-full items-center gap-2 text-left"
              onClick={() => {
                setExpanded(open ? undefined : repository.id);
                setSelection([]);
              }}>
              <GitBranch className="h-4 w-4 shrink-0 text-muted" />
              <span className="min-w-0 grow truncate text-[13px] font-medium text-primary">{repository.name}</span>
              <span className="shrink-0 text-[12px] text-muted">
                {repository.branch || t("repositoryGroups.defaultRef")}
                {repository.snapshot_revision && ` · ${repository.snapshot_revision.slice(0, 8)}`}
              </span>
              {busy === repository.id && <Loader2 className="h-3.5 w-3.5 animate-spin text-muted" />}
            </button>
            {open && (
              <div className="mt-3 space-y-3">
                <p className="break-all text-[12px] text-muted">{repository.clone_url}</p>
                <div className="flex flex-wrap gap-2">
                  <button type="button" className={CONTROL} disabled={!!busy} onClick={() => void run(repository.id, async () => {
                    await api.scanRepository(repository.id);
                  })}>
                    <RefreshCw className="h-3.5 w-3.5" />
                    {t("repositoryCatalog.scan")}
                  </button>
                  <button type="button" className={CONTROL} disabled={!!busy} onClick={() => void run(repository.id, async () => {
                    await api.removeRepository(repository.id);
                  })}>
                    {t("repositoryCatalog.remove")}
                  </button>
                </div>
                {repository.scan_error && <p role="alert" className="text-[12px] text-red-600 dark:text-red-400">{repository.scan_error}</p>}
                {repository.last_scanned_at && !repository.members.length
                  ? <p className="text-[12px] text-muted">{t("repositoryCatalog.noSkills")}</p> : null}
                {pending.length > 0 && (
                  <details className="text-[12px] text-muted">
                    <summary className="cursor-pointer">{t("repositoryCatalog.pending")} · {pending.length}</summary>
                    <div className="my-2 max-h-48 space-y-1 overflow-y-auto">
                      {pending.map((member) => (
                        <label className="flex items-baseline gap-2" key={member.rel_path}>
                          <input type="checkbox" checked={selection.includes(member.rel_path)} disabled={!!busy}
                            onChange={(event) => {
                              const checked = event.target.checked;
                              setSelection((previous) => checked
                                ? [...previous, member.rel_path]
                                : previous.filter((path) => path !== member.rel_path));
                            }} />
                          <span>{member.name} <span className="text-faint">{member.rel_path}</span></span>
                        </label>
                      ))}
                    </div>
                    <button type="button" className={CONTROL} disabled={!!busy} onClick={() => void run(repository.id, async () => {
                      try {
                        await api.importRepositorySkills(repository.id, selectedPaths.length ? selectedPaths : undefined);
                        setSelection([]);
                      } finally {
                        await refreshManagedSkills();
                      }
                    })}>
                      {t(selectedPaths.length ? "repositoryCatalog.importSelected" : "repositoryCatalog.importMissing",
                        { count: selectedPaths.length || pending.length })}
                    </button>
                  </details>
                )}
                {retired.length > 0 && <p className="text-[12px] text-muted">
                  {t("repositoryCatalog.retired")} · {retired.map((member) => member.name).join(", ")}
                </p>}
                <RepositoryIntegration repository={repository}
                  canEnableSkills={!repository.skill_ids.length && pending.length > 0} />
              </div>
            )}
          </div>
        );
      })}
    </section>
  );
}
