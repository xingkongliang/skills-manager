import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ChevronRight, Download, HardDrive, RefreshCw, Server, Trash2, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { AddHostDialog } from "../components/AddHostDialog";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { useApp } from "../context/AppContext";
import { AgentIcon } from "../components/AgentIcon";
import * as api from "../lib/tauri";
import type { Host, RemoteWorkspaceSkill } from "../lib/tauri";
import { cn } from "../utils";

export function Hosts() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { hostId } = useParams();
  const { hosts, presets, refreshHosts, refreshManagedSkills } = useApp();
  const [addOpen, setAddOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Host | null>(null);
  const [loadingHostId, setLoadingHostId] = useState<string | null>(null);
  const [skillsLoadingAgent, setSkillsLoadingAgent] = useState<string | null>(null);
  const [skillsByAgent, setSkillsByAgent] = useState<Record<string, RemoteWorkspaceSkill[]>>({});
  const [remoteActionKey, setRemoteActionKey] = useState<string | null>(null);
  const [presetByAgent, setPresetByAgent] = useState<Record<string, string>>({});
  const [remoteRemoveTarget, setRemoteRemoveTarget] = useState<{
    agentType: string;
    agentName: string;
    skill: RemoteWorkspaceSkill;
  } | null>(null);
  const [remoteOverwriteTarget, setRemoteOverwriteTarget] = useState<{
    agentType: string;
    agentName: string;
    skill: RemoteWorkspaceSkill;
  } | null>(null);
  const [presetApplyTarget, setPresetApplyTarget] = useState<{
    agentType: string;
    agentName: string;
    presetId: string;
    presetName: string;
  } | null>(null);

  useEffect(() => {
    if (!hosts.length) return;
    if (!hostId) {
      navigate(`/hosts/${hosts[0].id}`, { replace: true });
      return;
    }
    if (!hosts.some((host) => host.id === hostId)) {
      navigate(`/hosts/${hosts[0].id}`, { replace: true });
    }
  }, [hostId, hosts, navigate]);

  useEffect(() => {
    setSkillsByAgent({});
  }, [hostId]);

  const selectedHost = useMemo(
    () => hosts.find((host) => host.id === hostId) ?? hosts[0] ?? null,
    [hostId, hosts]
  );

  const handleRefreshHost = async (target: Host) => {
    setLoadingHostId(target.id);
    try {
      await api.refreshHost(target.id);
      await refreshHosts();
      toast.success(t("hosts.refreshed"));
    } catch (error) {
      console.error("Failed to refresh host:", error);
      toast.error(t("hosts.refreshFailed"));
    } finally {
      setLoadingHostId(null);
    }
  };

  const handleDeleteHost = async () => {
    if (!deleteTarget) return;
    await api.deleteHost(deleteTarget.id);
    await refreshHosts();
    setDeleteTarget(null);
    toast.success(t("hosts.deleted"));
  };

  const handleHostAdded = async (host: Host) => {
    await refreshHosts();
    navigate(`/hosts/${host.id}`);
  };

  const handleLoadSkills = async (agentType: string) => {
    if (!selectedHost || selectedHost.id === "local") return;
    const cacheKey = `${selectedHost.id}:${agentType}`;
    setSkillsLoadingAgent(cacheKey);
    try {
      const skills = await api.listRemoteWorkspaceSkills(selectedHost.id, agentType);
      setSkillsByAgent((prev) => ({ ...prev, [cacheKey]: skills }));
    } catch (error) {
      console.error("Failed to load host skills:", error);
      toast.error(t("hosts.loadSkillsFailed"));
    } finally {
      setSkillsLoadingAgent(null);
    }
  };

  const reloadAgentSkills = async (agentType: string) => {
    if (!selectedHost || selectedHost.id === "local") return;
    const cacheKey = `${selectedHost.id}:${agentType}`;
    const skills = await api.listRemoteWorkspaceSkills(selectedHost.id, agentType);
    setSkillsByAgent((prev) => ({ ...prev, [cacheKey]: skills }));
  };

  const handleInstallRemoteSkill = async (agentType: string, skill: RemoteWorkspaceSkill) => {
    if (!selectedHost || !skill.library_skill_id) return;
    const actionKey = `${selectedHost.id}:${agentType}:${skill.key}:install`;
    setRemoteActionKey(actionKey);
    try {
      await api.installSkillToRemoteHost(selectedHost.id, agentType, skill.library_skill_id);
      await reloadAgentSkills(agentType);
      toast.success(t("hosts.remoteSkillSynced"));
    } catch (error) {
      console.error("Failed to sync remote skill:", error);
      toast.error(t("hosts.remoteSkillSyncFailed"));
    } finally {
      setRemoteActionKey(null);
    }
  };

  const confirmOverwriteRemoteSkill = async () => {
    if (!remoteOverwriteTarget) return;
    await handleInstallRemoteSkill(remoteOverwriteTarget.agentType, remoteOverwriteTarget.skill);
    setRemoteOverwriteTarget(null);
  };

  const handleRemoveRemoteSkill = async (agentType: string, skill: RemoteWorkspaceSkill) => {
    if (!selectedHost) return;
    const actionKey = `${selectedHost.id}:${agentType}:${skill.key}:remove`;
    setRemoteActionKey(actionKey);
    try {
      await api.removeSkillFromRemoteHost(selectedHost.id, agentType, skill.relative_path);
      await reloadAgentSkills(agentType);
      toast.success(t("hosts.remoteSkillRemoved"));
    } catch (error) {
      console.error("Failed to remove remote skill:", error);
      toast.error(t("hosts.remoteSkillRemoveFailed"));
    } finally {
      setRemoteActionKey(null);
    }
  };

  const confirmRemoveRemoteSkill = async () => {
    if (!remoteRemoveTarget) return;
    await handleRemoveRemoteSkill(remoteRemoveTarget.agentType, remoteRemoveTarget.skill);
    setRemoteRemoveTarget(null);
  };

  const handleAdoptRemoteSkill = async (agentType: string, skill: RemoteWorkspaceSkill) => {
    if (!selectedHost) return;
    const actionKey = `${selectedHost.id}:${agentType}:${skill.key}:adopt`;
    setRemoteActionKey(actionKey);
    try {
      await api.adoptRemoteSkillToLibrary(selectedHost.id, agentType, skill.relative_path);
      await Promise.all([refreshManagedSkills(), reloadAgentSkills(agentType)]);
      toast.success(t("hosts.remoteSkillAdopted"));
    } catch (error) {
      console.error("Failed to adopt remote skill:", error);
      toast.error(t("hosts.remoteSkillAdoptFailed"));
    } finally {
      setRemoteActionKey(null);
    }
  };

  const handleApplyPresetRemote = async (agentType: string, presetIdOverride?: string) => {
    if (!selectedHost) return;
    const cacheKey = `${selectedHost.id}:${agentType}`;
    const presetId = presetIdOverride || presetByAgent[cacheKey] || presets[0]?.id;
    if (!presetId) return;
    setRemoteActionKey(`${cacheKey}:preset`);
    try {
      const result = await api.applyPresetToRemoteHost(selectedHost.id, agentType, presetId);
      await Promise.all([refreshHosts(), reloadAgentSkills(agentType)]);
      toast.success(t("hosts.remotePresetApplied", { changed: result.changed, skipped: result.skipped }));
    } catch (error) {
      console.error("Failed to apply preset to remote host:", error);
      toast.error(t("hosts.remotePresetApplyFailed"));
    } finally {
      setRemoteActionKey(null);
    }
  };

  const confirmApplyPresetRemote = async () => {
    if (!presetApplyTarget) return;
    await handleApplyPresetRemote(presetApplyTarget.agentType, presetApplyTarget.presetId);
    setPresetApplyTarget(null);
  };

  return (
    <div className="app-page">
      <div className="app-page-header">
        <div>
          <h1 className="app-page-title">{t("hosts.title")}</h1>
          <p className="app-page-subtitle text-tertiary">{t("hosts.subtitle")}</p>
        </div>
        <button onClick={() => setAddOpen(true)} className="app-button-primary">
          <Server className="w-4 h-4" />
          {t("hosts.addHost")}
        </button>
      </div>

      <div className="grid grid-cols-[320px_minmax(0,1fr)] gap-4 items-start">
        <div className="app-panel p-2.5 space-y-1.5">
          {hosts.map((host) => {
            const active = selectedHost?.id === host.id;
            return (
              <Link
                key={host.id}
                to={`/hosts/${host.id}`}
                className={cn(
                  "block rounded-xl border px-3 py-3 transition-all",
                  active
                    ? "border-accent-border bg-accent-bg/40"
                    : "border-border-subtle hover:border-border hover:bg-surface-hover"
                )}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex items-center gap-2 text-primary font-medium">
                      {host.id === "local" ? <HardDrive className="w-4 h-4 text-accent-light" /> : <Server className="w-4 h-4 text-accent-light" />}
                      <span className="truncate">{host.name}</span>
                    </div>
                    <div className="mt-1 text-[13px] text-muted truncate">{host.connection_label}</div>
                  </div>
                  <ChevronRight className={cn("w-4 h-4 mt-0.5 shrink-0", active ? "text-accent-light" : "text-muted")} />
                </div>
                <div className="mt-2 flex items-center justify-between text-[13px] text-tertiary">
                  <span>{host.host_type}</span>
                  <span className={cn(host.status.startsWith("connected") ? "text-emerald-400" : host.status.startsWith("offline") ? "text-amber-400" : "text-muted")}>
                    {host.status}
                  </span>
                </div>
                <div className="mt-2 text-[13px] text-muted">
                  {t("hosts.summaryLine", { agents: host.agent_count, skills: host.skill_count })}
                </div>
              </Link>
            );
          })}
        </div>

        <div className="space-y-4">
          {selectedHost ? (
            <>
              <div className="app-panel p-4">
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <div className="flex items-center gap-2 mb-1">
                      <h2 className="text-lg font-semibold text-primary">{selectedHost.name}</h2>
                      <span className="rounded-full border border-border-subtle bg-background px-2 py-0.5 text-[12px] text-tertiary">
                        {selectedHost.host_type}
                      </span>
                    </div>
                    <div className="text-[13px] text-muted">{selectedHost.connection_label}</div>
                    <div className="mt-2 text-[13px] text-tertiary">
                      {selectedHost.user ? `${t("hosts.user")}: ${selectedHost.user} · ` : ""}
                      {t("hosts.status")}: {selectedHost.status}
                    </div>
                  </div>

                  <div className="flex items-center gap-2">
                    <button
                      onClick={() => handleRefreshHost(selectedHost)}
                      disabled={loadingHostId === selectedHost.id}
                      className="app-button-secondary"
                    >
                      <RefreshCw className={cn("w-4 h-4", loadingHostId === selectedHost.id && "animate-spin")} />
                      {t("hosts.refresh")}
                    </button>
                    {selectedHost.id !== "local" ? (
                      <button
                        onClick={() => setDeleteTarget(selectedHost)}
                        className="px-3 py-1.5 rounded-lg bg-red-600/90 hover:bg-red-500 text-white text-[13px] font-medium transition-colors border border-red-500/50 outline-none"
                      >
                        <span className="inline-flex items-center gap-1.5">
                          <Trash2 className="w-4 h-4" />
                          {t("hosts.delete")}
                        </span>
                      </button>
                    ) : null}
                  </div>
                </div>
              </div>

              <div className="app-panel p-4">
                <div className="flex items-center justify-between mb-3">
                  <h3 className="app-section-title">{t("hosts.agents")}</h3>
                  <span className="text-[13px] text-muted">{t("hosts.summaryLine", { agents: selectedHost.agent_count, skills: selectedHost.skill_count })}</span>
                </div>
                {selectedHost.agents.length === 0 ? (
                  <div className="text-[13px] text-muted">{t("hosts.noAgents")}</div>
                ) : (
                  <div className="space-y-3">
                    {selectedHost.agents.map((agent) => {
                      const cacheKey = `${selectedHost.id}:${agent.agent_type}`;
                      const skills = skillsByAgent[cacheKey];
                      const selectedPresetId = presetByAgent[cacheKey] || presets[0]?.id || "";
                      return (
                        <div key={agent.agent_type} className="rounded-xl border border-border-subtle bg-background px-3 py-3">
                          <div className="flex items-start justify-between gap-3">
                            <div className="min-w-0">
                              <div className="flex items-center gap-2 text-primary font-medium">
                                <AgentIcon agentKey={agent.agent_type} displayName={agent.display_name} className="h-4 w-4 shrink-0" />
                                <span>{agent.display_name}</span>
                              </div>
                              <div className="mt-1 text-[13px] text-muted break-all">{agent.skill_path}</div>
                              <div className="mt-2 text-[13px] text-tertiary">
                                {t("hosts.agentSummary", { count: agent.skill_count, status: agent.status })}
                              </div>
                            </div>
                            {selectedHost.id !== "local" ? (
                              <div className="flex shrink-0 items-center gap-2">
                                {presets.length > 0 ? (
                                  <>
                                    <select
                                      value={selectedPresetId}
                                      onChange={(event) =>
                                        setPresetByAgent((prev) => ({ ...prev, [cacheKey]: event.target.value }))
                                      }
                                      className="h-8 rounded-lg border border-border-subtle bg-surface px-2 text-[13px] text-secondary outline-none"
                                    >
                                      {presets.map((preset) => (
                                        <option key={preset.id} value={preset.id}>
                                          {preset.name}
                                        </option>
                                      ))}
                                    </select>
                                    <button
                                      onClick={() =>
                                        setPresetApplyTarget({
                                          agentType: agent.agent_type,
                                          agentName: agent.display_name,
                                          presetId: selectedPresetId,
                                          presetName:
                                            presets.find((preset) => preset.id === selectedPresetId)?.name ||
                                            selectedPresetId,
                                        })
                                      }
                                      disabled={remoteActionKey === `${cacheKey}:preset`}
                                      className="app-button-secondary"
                                    >
                                      <Upload className={cn("w-4 h-4", remoteActionKey === `${cacheKey}:preset` && "animate-pulse")} />
                                      {t("hosts.applyPreset")}
                                    </button>
                                  </>
                                ) : null}
                                <button
                                  onClick={() => handleLoadSkills(agent.agent_type)}
                                  disabled={skillsLoadingAgent === cacheKey}
                                  className="app-button-secondary"
                                >
                                  <RefreshCw className={cn("w-4 h-4", skillsLoadingAgent === cacheKey && "animate-spin")} />
                                  {t("hosts.loadSkills")}
                                </button>
                              </div>
                            ) : null}
                          </div>

                          {skills ? (
                            <div className="mt-3 rounded-lg border border-border-subtle overflow-hidden">
                              {skills.length === 0 ? (
                                <div className="px-3 py-2 text-[13px] text-muted bg-surface">{t("hosts.noRemoteSkills")}</div>
                              ) : (
                                <div className="divide-y divide-border-subtle bg-surface">
                                  {skills.map((skill) => (
                                    <div key={skill.key} className="flex items-start justify-between gap-3 px-3 py-2.5">
                                      <div className="min-w-0">
                                        <div className="flex flex-wrap items-center gap-2">
                                          <span className="text-[13px] font-medium text-primary">{skill.name}</span>
                                          <span
                                            className={cn(
                                              "rounded-full border px-2 py-0.5 text-[11px]",
                                              skill.status === "synced" && "border-emerald-500/30 text-emerald-400",
                                              skill.status === "conflict" && "border-amber-500/30 text-amber-400",
                                              skill.status === "missing" && "border-blue-500/30 text-blue-400",
                                              skill.status === "remote_only" && "border-purple-500/30 text-purple-300"
                                            )}
                                          >
                                            {skill.status === "synced"
                                              ? t("hosts.remoteStatusSynced")
                                              : skill.status === "conflict"
                                                ? t("hosts.remoteStatusConflict")
                                                : skill.status === "missing"
                                                  ? t("hosts.remoteStatusMissing")
                                                  : t("hosts.remoteStatusRemoteOnly")}
                                          </span>
                                        </div>
                                        <div className="text-[12px] text-muted mt-0.5 break-all">
                                          {skill.remote_path || skill.relative_path}
                                        </div>
                                        {skill.library_version ? (
                                          <div className="mt-1 text-[12px] text-tertiary">
                                            {t("hosts.libraryVersion", { version: skill.library_version.slice(0, 12) })}
                                          </div>
                                        ) : null}
                                      </div>
                                      <div className="flex shrink-0 items-center gap-2">
                                        {skill.library_skill_id && skill.status !== "synced" ? (
                                          <button
                                            onClick={() =>
                                              skill.status === "conflict"
                                                ? setRemoteOverwriteTarget({
                                                    agentType: agent.agent_type,
                                                    agentName: agent.display_name,
                                                    skill,
                                                  })
                                                : handleInstallRemoteSkill(agent.agent_type, skill)
                                            }
                                            disabled={remoteActionKey === `${selectedHost.id}:${agent.agent_type}:${skill.key}:install`}
                                            className="app-button-secondary"
                                          >
                                            <Download className="w-4 h-4" />
                                            {skill.status === "missing" ? t("hosts.installRemote") : t("hosts.overwriteRemote")}
                                          </button>
                                        ) : null}
                                        {skill.status === "remote_only" ? (
                                          <button
                                            onClick={() => handleAdoptRemoteSkill(agent.agent_type, skill)}
                                            disabled={remoteActionKey === `${selectedHost.id}:${agent.agent_type}:${skill.key}:adopt`}
                                            className="app-button-secondary"
                                          >
                                            <Upload className="w-4 h-4" />
                                            {t("hosts.adoptRemote")}
                                          </button>
                                        ) : null}
                                        {skill.remote_path ? (
                                          <button
                                            onClick={() =>
                                              setRemoteRemoveTarget({
                                                agentType: agent.agent_type,
                                                agentName: agent.display_name,
                                                skill,
                                              })
                                            }
                                            disabled={remoteActionKey === `${selectedHost.id}:${agent.agent_type}:${skill.key}:remove`}
                                            className="rounded-lg border border-red-500/40 px-3 py-1.5 text-[13px] font-medium text-red-300 hover:bg-red-500/10 disabled:opacity-50"
                                          >
                                            {t("hosts.removeRemote")}
                                          </button>
                                        ) : null}
                                      </div>
                                    </div>
                                  ))}
                                </div>
                              )}
                            </div>
                          ) : null}
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </>
          ) : null}
        </div>
      </div>

      <AddHostDialog open={addOpen} onClose={() => setAddOpen(false)} onAdded={handleHostAdded} />
      <ConfirmDialog
        open={!!deleteTarget}
        title={t("hosts.deleteConfirmTitle")}
        message={t("hosts.deleteConfirm", { name: deleteTarget?.name || "" })}
        confirmLabel={t("hosts.delete")}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeleteHost}
      />
      <ConfirmDialog
        open={!!remoteRemoveTarget}
        title={t("hosts.remoteRemoveConfirmTitle")}
        message={t("hosts.remoteRemoveConfirm", {
          host: selectedHost?.name || "",
          agent: remoteRemoveTarget?.agentName || "",
          path: remoteRemoveTarget?.skill.remote_path || remoteRemoveTarget?.skill.relative_path || "",
        })}
        confirmLabel={t("hosts.removeRemote")}
        onClose={() => setRemoteRemoveTarget(null)}
        onConfirm={confirmRemoveRemoteSkill}
      />
      <ConfirmDialog
        open={!!remoteOverwriteTarget}
        title={t("hosts.remoteOverwriteConfirmTitle")}
        message={t("hosts.remoteOverwriteConfirm", {
          host: selectedHost?.name || "",
          agent: remoteOverwriteTarget?.agentName || "",
          path: remoteOverwriteTarget?.skill.remote_path || remoteOverwriteTarget?.skill.relative_path || "",
        })}
        confirmLabel={t("hosts.overwriteRemote")}
        onClose={() => setRemoteOverwriteTarget(null)}
        onConfirm={confirmOverwriteRemoteSkill}
      />
      <ConfirmDialog
        open={!!presetApplyTarget}
        title={t("hosts.remotePresetConfirmTitle")}
        message={t("hosts.remotePresetConfirm", {
          host: selectedHost?.name || "",
          agent: presetApplyTarget?.agentName || "",
          preset: presetApplyTarget?.presetName || "",
        })}
        confirmLabel={t("hosts.applyPreset")}
        onClose={() => setPresetApplyTarget(null)}
        onConfirm={confirmApplyPresetRemote}
      />
    </div>
  );
}
