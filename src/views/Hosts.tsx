import { useEffect, useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ChevronRight, HardDrive, RefreshCw, Server, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { AddHostDialog } from "../components/AddHostDialog";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { useApp } from "../context/AppContext";
import { AgentIcon } from "../components/AgentIcon";
import * as api from "../lib/tauri";
import type { Host, HostSkill } from "../lib/tauri";
import { cn } from "../utils";

export function Hosts() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { hostId } = useParams();
  const { hosts, refreshHosts } = useApp();
  const [addOpen, setAddOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Host | null>(null);
  const [loadingHostId, setLoadingHostId] = useState<string | null>(null);
  const [skillsLoadingAgent, setSkillsLoadingAgent] = useState<string | null>(null);
  const [skillsByAgent, setSkillsByAgent] = useState<Record<string, HostSkill[]>>({});

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

  const handleLoadSkills = async (agentType: string) => {
    if (!selectedHost || selectedHost.id === "local") return;
    const cacheKey = `${selectedHost.id}:${agentType}`;
    setSkillsLoadingAgent(cacheKey);
    try {
      const skills = await api.listHostSkills(selectedHost.id, agentType);
      setSkillsByAgent((prev) => ({ ...prev, [cacheKey]: skills }));
    } catch (error) {
      console.error("Failed to load host skills:", error);
      toast.error(t("hosts.loadSkillsFailed"));
    } finally {
      setSkillsLoadingAgent(null);
    }
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
                              <button
                                onClick={() => handleLoadSkills(agent.agent_type)}
                                disabled={skillsLoadingAgent === cacheKey}
                                className="app-button-secondary shrink-0"
                              >
                                <RefreshCw className={cn("w-4 h-4", skillsLoadingAgent === cacheKey && "animate-spin")} />
                                {t("hosts.loadSkills")}
                              </button>
                            ) : null}
                          </div>

                          {skills ? (
                            <div className="mt-3 rounded-lg border border-border-subtle overflow-hidden">
                              {skills.length === 0 ? (
                                <div className="px-3 py-2 text-[13px] text-muted bg-surface">{t("hosts.noRemoteSkills")}</div>
                              ) : (
                                <div className="divide-y divide-border-subtle bg-surface">
                                  {skills.map((skill) => (
                                    <div key={`${skill.path}`} className="px-3 py-2.5">
                                      <div className="text-[13px] font-medium text-primary">{skill.name}</div>
                                      <div className="text-[12px] text-muted mt-0.5 break-all">{skill.path}</div>
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

      <AddHostDialog open={addOpen} onClose={() => setAddOpen(false)} onAdded={refreshHosts} />
      <ConfirmDialog
        open={!!deleteTarget}
        title={t("hosts.deleteConfirmTitle")}
        message={t("hosts.deleteConfirm", { name: deleteTarget?.name || "" })}
        confirmLabel={t("hosts.delete")}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeleteHost}
      />
    </div>
  );
}
