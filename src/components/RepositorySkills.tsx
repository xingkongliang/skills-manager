import { useEffect, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, FolderGit2, GitBranch, Loader2, MinusSquare, Square, SquareCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { getErrorMessage } from "../lib/error";
import { buildRepositorySkillGroups, prepareRepositorySkills, repositoryAvailableCount, repositoryDeploymentState, type RepositorySkillGroup } from "../lib/repositoryGroups";
import { normalizeSkillSearch, skillSearchMatches } from "../lib/skillSearch";
import { UNTAGGED_FILTER } from "../lib/skillTags";
import { listRepositories, type ManagedSkill, type SkillRepository } from "../lib/tauri";

export interface RepositoryAgent {
  key: string;
  display_name: string;
}

export interface RepositoryDeploymentProps {
  existsInWorkspace: (skill: ManagedSkill, agentKey: string) => boolean;
  /** Uses the existing protected batch planner, including for one skill. */
  onApply: (skills: ManagedSkill[], agentKey: string, enabled: boolean) => Promise<void>;
  onComplete: () => Promise<void>;
  confirmRemove?: (pairs: { skill: ManagedSkill; agentKey: string }[]) => Promise<boolean>;
  disabled?: boolean;
}

export function RepositoryViewSwitch({ grouped, onChange }: { grouped: boolean; onChange: (grouped: boolean) => void }) {
  const { t } = useTranslation();
  return (
    <div className="app-segmented app-toolbar-segmented shrink-0">
      {[false, true].map((value) => (
        <button key={String(value)} type="button" aria-pressed={grouped === value}
          onClick={() => onChange(value)}
          className={cn("app-segmented-button inline-flex items-center gap-1.5", grouped === value && "app-segmented-button-active")}>
          {value && <FolderGit2 className="h-3.5 w-3.5" />}
          {t(value ? "repositoryGroups.byRepository" : "repositoryGroups.flat")}
        </button>
      ))}
    </div>
  );
}

export function RepositoryTargetPicker({ agents, agentKey, onChange, scopeLabel, sharedAgents = [] }: {
  agents: RepositoryAgent[];
  agentKey: string;
  onChange: (key: string) => void;
  scopeLabel: (agent: RepositoryAgent) => string;
  sharedAgents?: RepositoryAgent[];
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-1">
      <label className="flex flex-wrap items-center gap-2 text-[12px] text-muted">
      {t("repositoryGroups.target")}
      <select value={agentKey} onChange={(event) => onChange(event.target.value)} disabled={agents.length === 0}
        className="app-input max-w-full text-[12px]">
        {agents.length === 0 && <option value="">{t("repositoryGroups.noTargets")}</option>}
        {agents.map((agent) => <option key={agent.key} value={agent.key}>{scopeLabel(agent)}</option>)}
      </select>
      </label>
      {sharedAgents.length > 0 && <p className="text-[12px] text-muted">{t("repositoryGroups.sharedHint", { agents: sharedAgents.map((agent) => agent.display_name).join(", ") })}</p>}
    </div>
  );
}

export function RepositoryGroupLabel({ group }: { group: RepositorySkillGroup }) {
  const { t } = useTranslation();
  return (
    <>
      <FolderGit2 className="h-4 w-4 shrink-0 text-muted" />
      <span className="min-w-0 truncate text-[13px] font-semibold text-secondary">{group.label}</span>
      <span className="inline-flex min-w-0 items-center gap-1 text-[11px] font-normal text-muted" title={group.branch ?? t("repositoryGroups.defaultRef")}>
        <GitBranch className="h-3 w-3 shrink-0" />
        <span className="truncate">{group.branch ?? t("repositoryGroups.defaultRef")}</span>
      </span>
      <span className="app-badge shrink-0">{group.count}</span>
    </>
  );
}

/** The state is actual deployment, independently of preset membership or intent. */
export function RepositorySkillActions({ skills, agentKey, compact = false, availableCount, onEnableAll, upstreamUnscanned = false, ...deployment }: RepositoryDeploymentProps & {
  skills: ManagedSkill[];
  agentKey: string;
  compact?: boolean;
  availableCount?: number;
  onEnableAll?: () => Promise<number>;
  upstreamUnscanned?: boolean;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const actual = repositoryDeploymentState(skills, agentKey, deployment.existsInWorkspace);
  const total = Math.max(actual.total, availableCount ?? 0);
  const status = { ...actual, total, state: actual.enabled === 0 ? "off" : actual.enabled === total ? "on" : "partial" };
  const StateIcon = status.state === "on" ? SquareCheck : status.state === "partial" ? MinusSquare : Square;
  const apply = async (enabled: boolean) => {
    if (busy || deployment.disabled || !agentKey) return;
    const changing = skills.filter((skill) => deployment.existsInWorkspace(skill, agentKey) !== enabled);
    if (changing.length === 0 && !(enabled && onEnableAll)) return;
    setBusy(true);
    try {
      if (!enabled && deployment.confirmRemove && !(await deployment.confirmRemove(changing.map((skill) => ({ skill, agentKey }))))) return;
      let count = changing.length;
      if (enabled && onEnableAll) {
        count = await onEnableAll();
      } else {
        await deployment.onApply(changing, agentKey, enabled);
      }
      toast.success(t(enabled ? "repositoryGroups.enabledToast" : "repositoryGroups.disabledToast", { count }));
    } catch (error) {
      toast.error(getErrorMessage(error, t("repositoryGroups.failedToast")));
    } finally {
      // A batch can stop after a filesystem error, so always re-read the facts.
      try { await deployment.onComplete(); }
      catch (error) { toast.error(getErrorMessage(error, t("common.error"))); }
      finally { setBusy(false); }
    }
  };
  return (
    <div className="flex flex-wrap items-center gap-2" aria-busy={busy}>
      <span role="status" className={cn("inline-flex items-center gap-1 text-[12px]", status.state === "on" ? "text-accent-light" : "text-muted")}>
        {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <StateIcon className="h-3.5 w-3.5" />}
        {t(upstreamUnscanned ? "repositoryGroups.unscannedCount" : "repositoryGroups.enabledCount",
          { enabled: status.enabled, total: status.total })}
      </span>
      {(status.enabled < status.total || upstreamUnscanned) && (
        <button type="button" onClick={() => void apply(true)} disabled={busy || deployment.disabled || !agentKey}
          className="app-button-secondary px-2 py-1 text-[12px] disabled:opacity-50">
          {t(upstreamUnscanned ? "repositoryGroups.discoverAndEnable"
            : compact ? "repositoryGroups.enable" : "repositoryGroups.enableAll")}
        </button>
      )}
      {status.enabled > 0 && (
        <button type="button" onClick={() => void apply(false)} disabled={busy || deployment.disabled || !agentKey}
          className="app-button-secondary px-2 py-1 text-[12px] disabled:opacity-50">
          {t(compact ? "repositoryGroups.disable" : "repositoryGroups.disableAll")}
        </button>
      )}
    </div>
  );
}

/** Includes undeployed library members, so disabling a repository never hides it. */
export function RepositorySkills({ skills, agents, scopeLabel, search = "", tagFilters = new Set<string>(), filterMode = "all", sharedAgents, onOpenSkill, ...deployment }: RepositoryDeploymentProps & {
  skills: ManagedSkill[];
  agents: RepositoryAgent[];
  scopeLabel: (agent: RepositoryAgent) => string;
  search?: string;
  tagFilters?: Set<string>;
  filterMode?: "all" | "enabled" | "disabled";
  sharedAgents?: (agentKey: string) => RepositoryAgent[];
  onOpenSkill?: (skill: ManagedSkill, agentKey: string) => void;
}) {
  const { t } = useTranslation();
  const [selectedAgent, setSelectedAgent] = useState("");
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const [repositories, setRepositories] = useState<SkillRepository[]>([]);
  useEffect(() => {
    let cancelled = false;
    const reload = () => { void listRepositories().then((value) => { if (!cancelled) setRepositories(value); }).catch(() => {}); };
    reload();
    window.addEventListener("repository-catalog-changed", reload);
    return () => { cancelled = true; window.removeEventListener("repository-catalog-changed", reload); };
  }, [skills]);
  const agentKey = agents.some((agent) => agent.key === selectedAgent) ? selectedAgent : agents[0]?.key ?? "";
  const groups = useMemo(() => buildRepositorySkillGroups(skills, repositories), [skills, repositories]);
  const query = normalizeSkillSearch(search);
  return (
    <div className="space-y-3 pb-4">
      <RepositoryTargetPicker agents={agents} agentKey={agentKey} onChange={setSelectedAgent} scopeLabel={scopeLabel} sharedAgents={sharedAgents?.(agentKey)} />
      {groups.map((group) => {
        const groupMatches = skillSearchMatches(query, [group.label, group.branch]);
        const visible = group.skills.filter((skill) =>
          (groupMatches || skillSearchMatches(query, [skill.name, skill.description, skill.source_subpath]))
          && (tagFilters.size === 0 || skill.tags.some((tag) => tagFilters.has(tag)) || (skill.tags.length === 0 && tagFilters.has(UNTAGGED_FILTER)))
          && (filterMode === "all" || deployment.existsInWorkspace(skill, agentKey) === (filterMode === "enabled"))
        );
        if (visible.length === 0) return null;
        const expanded = query !== "" || (open[group.key] ?? false);
        return (
          <section key={group.key} className="app-panel overflow-hidden">
            <div className="flex flex-wrap items-center justify-between gap-2 px-3 py-2.5">
              <button type="button" aria-expanded={expanded} onClick={() => setOpen((previous) => ({ ...previous, [group.key]: !expanded }))}
                disabled={query !== ""} className="flex min-w-0 flex-1 items-center gap-2 text-left">
                {expanded ? <ChevronDown className="h-3.5 w-3.5 shrink-0 text-muted" /> : <ChevronRight className="h-3.5 w-3.5 shrink-0 text-muted" />}
                <RepositoryGroupLabel group={group} />
              </button>
              <RepositorySkillActions {...deployment} skills={group.skills} agentKey={agentKey}
                availableCount={repositoryAvailableCount(group)}
                upstreamUnscanned={!!group.repository && !group.repository.snapshot_revision}
                onEnableAll={group.repository ? async () => {
                  const members = await prepareRepositorySkills(group, !group.repository?.snapshot_revision);
                  const missing = members.filter((skill) => !deployment.existsInWorkspace(skill, agentKey));
                  if (missing.length > 0) await deployment.onApply(missing, agentKey, true);
                  return missing.length;
                } : undefined} />
            </div>
            <p className="px-3 pb-2 text-[11px] text-faint">{t("repositoryGroups.fullScope", { total: group.count })}{visible.length !== group.count && ` ${t("repositoryGroups.shownCount", { shown: visible.length, total: group.count })}`}</p>
            {expanded && (
              <div className="border-t border-border-subtle">
                {visible.map((skill) => (
                  <div key={skill.id} className="flex flex-wrap items-center justify-between gap-2 border-b border-border-faint px-4 py-2.5 last:border-b-0">
                    <button type="button" onClick={() => onOpenSkill?.(skill, agentKey)} disabled={!onOpenSkill}
                      className="min-w-0 flex-1 text-left disabled:cursor-default">
                      <span className="block truncate text-[13px] font-medium text-secondary">{skill.name}</span>
                      <span className="block truncate text-[12px] text-muted">{skill.description || skill.source_subpath}</span>
                    </button>
                    <RepositorySkillActions {...deployment} skills={[skill]} agentKey={agentKey} compact />
                  </div>
                ))}
              </div>
            )}
          </section>
        );
      })}
    </div>
  );
}
