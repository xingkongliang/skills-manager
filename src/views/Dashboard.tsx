import { useState, useEffect } from "react";
import { Link, useNavigate } from "react-router-dom";
import {
  Bot,
  CheckCircle2,
  ChevronRight,
  Download,
  Layers,
  Package,
  Plus,
  Puzzle,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useApp } from "../context/AppContext";
import * as api from "../lib/tauri";
import type { AgentConfigDto, ManagedPlugin, ManagedSkill, PackRecord } from "../lib/tauri";
import { getScenarioIconOption } from "../lib/scenarioIcons";
import { TokensSavedWidget } from "../components/TokensSavedWidget";

export function Dashboard() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { activeScenario, managedSkills, openSkillDetailById } = useApp();

  const [agentConfigs, setAgentConfigs] = useState<AgentConfigDto[]>([]);
  const [packs, setPacks] = useState<PackRecord[]>([]);
  const [plugins, setPlugins] = useState<ManagedPlugin[]>([]);
  const [scenarioSkills, setScenarioSkills] = useState<ManagedSkill[]>([]);

  useEffect(() => {
    api.getAllAgentConfigs().then(setAgentConfigs).catch(() => {});
    api.getAllPacks().then(setPacks).catch(() => {});
    api.getManagedPlugins().then(setPlugins).catch(() => {});
  }, []);

  useEffect(() => {
    if (activeScenario) {
      api.getSkillsForScenario(activeScenario.id).then(setScenarioSkills).catch(() => {});
    }
  }, [activeScenario]);

  const managedAgents = agentConfigs.filter((a) => a.managed);
  const totalSkills = managedSkills.length;
  const activePacks = packs.length;
  const activePlugins = plugins.length;
  const totalAgents = agentConfigs.length;

  const stats = [
    {
      title: t("dashboard.scenarioSkills"),
      value: String(totalSkills),
      icon: Layers,
      color: "text-accent-light",
      bg: "bg-accent-bg",
    },
    {
      title: "Active Packs",
      value: String(activePacks),
      icon: Package,
      color: "text-violet-400",
      bg: "bg-violet-500/[0.08]",
    },
    {
      title: "Plugins",
      value: String(activePlugins),
      icon: Puzzle,
      color: "text-sky-400",
      bg: "bg-sky-500/[0.08]",
    },
    {
      title: t("dashboard.supportedAgents"),
      value: String(totalAgents),
      icon: Bot,
      color: "text-amber-400",
      bg: "bg-amber-500/[0.08]",
    },
  ];

  return (
    <div className="app-page app-page-narrow">
      <div className="app-page-header">
        <h1 className="app-page-title">{t("dashboard.greeting")}</h1>
        <p className="app-page-subtitle text-tertiary">
          {managedAgents.length > 0
            ? `${managedAgents.length} agent${managedAgents.length !== 1 ? "s" : ""} managed · ${totalSkills} skills in library`
            : t("dashboard.skillsEnabled", { count: totalSkills })}
        </p>
      </div>

      {/* Quick Stats */}
      <div className="grid grid-cols-4 gap-3">
        {stats.map((stat, i) => {
          const Icon = stat.icon;
          return (
            <div
              key={i}
              className="app-panel flex items-center justify-between px-3.5 py-3.5 transition-colors hover:border-border"
            >
              <div>
                <p className="app-section-title mb-1 text-[11px]">{stat.title}</p>
                <h3 className="text-xl font-semibold text-primary leading-none">{stat.value}</h3>
              </div>
              <div className={`p-2 rounded-md ${stat.bg} ${stat.color} border border-border-subtle`}>
                <Icon className="w-4 h-4" />
              </div>
            </div>
          );
        })}
      </div>

      {/* Tokens Saved */}
      {/* TODO: once PackRecord exposes is_essential + skill_count, compute:
          essentialSkillCount = sum(skill_count) for active-scenario packs where is_essential
          nonEssentialPackCount = count of active-scenario packs where !is_essential
          Also replace "hybrid" hard-coding with the real active disclosure mode. */}
      <TokensSavedWidget
        currentMode="hybrid"
        essentialSkillCount={null}
        nonEssentialPackCount={null}
      />

      {/* Agents Overview */}
      <div>
        <div className="flex items-center justify-between mb-2.5">
          <h2 className="app-section-title">Agents Overview</h2>
          <Link
            to="/matrix"
            className="text-[12px] text-accent-light hover:underline flex items-center gap-0.5"
          >
            Matrix view <ChevronRight className="w-3 h-3" />
          </Link>
        </div>

        {agentConfigs.length === 0 ? (
          <div className="app-panel px-4 py-6 text-center text-muted text-sm">
            No agents detected. Check Settings to configure agent paths.
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-3">
            {agentConfigs.map((agent) => {
              const scenarioIcon = agent.scenario_id
                ? getScenarioIconOption({ id: agent.scenario_id, name: agent.scenario_name ?? "", icon: null, description: null, sort_order: 0, skill_count: 0, created_at: 0, updated_at: 0 })
                : null;
              const ScenarioIcon = scenarioIcon?.icon ?? null;

              return (
                <Link
                  key={agent.tool_key}
                  to={`/agent/${agent.tool_key}`}
                  className="app-panel px-4 py-3.5 hover:border-border transition-colors cursor-pointer group no-underline"
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="flex items-center gap-2.5 min-w-0">
                      {/* Status dot */}
                      <span
                        className={`mt-0.5 h-2 w-2 rounded-full shrink-0 ${
                          agent.managed ? "bg-emerald-400" : "bg-border"
                        }`}
                        title={agent.managed ? "Managed" : "Not managed"}
                      />
                      <div className="min-w-0">
                        <p className="text-[13px] font-medium text-secondary truncate group-hover:text-primary transition-colors">
                          {agent.display_name}
                        </p>
                        {agent.scenario_name ? (
                          <div className="flex items-center gap-1 mt-0.5">
                            {ScenarioIcon && (
                              <ScenarioIcon className={`w-3 h-3 shrink-0 ${scenarioIcon?.colorClass ?? "text-muted"}`} />
                            )}
                            <p className="text-[11px] text-muted truncate">{agent.scenario_name}</p>
                          </div>
                        ) : (
                          <p className="text-[11px] text-faint mt-0.5">No scenario assigned</p>
                        )}
                      </div>
                    </div>

                    <ChevronRight className="w-3.5 h-3.5 text-faint shrink-0 mt-0.5 group-hover:text-muted transition-colors" />
                  </div>

                  <div className="flex items-center gap-3 mt-2.5 pt-2.5 border-t border-border-subtle">
                    <span
                      className="flex items-center gap-1 text-[11px] text-muted"
                      title="Effective skill count"
                    >
                      <Layers className="w-3 h-3" />
                      {agent.effective_skill_count} skills
                    </span>
                    {agent.extra_pack_count > 0 && (
                      <span
                        className="flex items-center gap-1 text-[11px] text-violet-400"
                        title="Extra packs"
                      >
                        <Package className="w-3 h-3" />
                        +{agent.extra_pack_count} pack{agent.extra_pack_count !== 1 ? "s" : ""}
                      </span>
                    )}
                    {!agent.installed && (
                      <span className="text-[11px] text-faint ml-auto">not installed</span>
                    )}
                  </div>
                </Link>
              );
            })}
          </div>
        )}
      </div>

      {/* Actions */}
      <div className="flex gap-3">
        <button
          onClick={() => navigate("/install?tab=local")}
          className="app-button-primary flex-1"
        >
          <Download className="w-4 h-4" />
          {t("dashboard.scanImport")}
        </button>
        <button
          onClick={() => navigate("/install")}
          className="app-button-secondary flex-1"
        >
          <Plus className="w-4 h-4 text-tertiary" />
          {t("dashboard.installNew")}
        </button>
      </div>

      {/* Recent Activity */}
      {scenarioSkills.length > 0 && (
        <div>
          <h2 className="app-section-title mb-2.5">{t("dashboard.recentActivity")}</h2>
          <div className="app-panel overflow-hidden divide-y divide-border-subtle">
            {scenarioSkills.slice(0, 5).map((skill) => (
              <div
                key={skill.id}
                role="button"
                tabIndex={0}
                onClick={() => {
                  openSkillDetailById(skill.id);
                  navigate("/my-skills");
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    openSkillDetailById(skill.id);
                    navigate("/my-skills");
                  }
                }}
                className="flex items-center justify-between px-3.5 py-2.5 hover:bg-surface-hover transition-colors cursor-pointer"
              >
                <div className="flex items-center gap-2.5">
                  <div className="w-6 h-6 rounded-[4px] flex items-center justify-center text-[13px] font-semibold bg-accent-bg text-accent-light shrink-0">
                    {skill.name.charAt(0).toUpperCase()}
                  </div>
                  <div>
                    <h4 className="text-[13px] text-secondary font-medium flex items-center gap-1.5">
                      {skill.name}
                      <span className="text-[9px] px-1.5 py-px rounded bg-surface-hover text-muted border border-border font-normal">
                        {skill.source_type}
                      </span>
                    </h4>
                    <p className="text-[13px] text-muted mt-px">
                      {skill.targets.length > 0
                        ? `${t("dashboard.synced")} → ${skill.targets.map((tgt) => tgt.tool).join(", ")}`
                        : "Not synced"}
                    </p>
                  </div>
                </div>
                <CheckCircle2 className={`w-4 h-4 shrink-0 ${skill.targets.length > 0 ? "text-emerald-400" : "text-border"}`} />
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
