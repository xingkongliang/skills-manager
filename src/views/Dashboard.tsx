import { useState, useEffect } from "react";
import { useNavigate } from "react-router-dom";
import { Layers, CheckCircle2, Bot, Plus, Download } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useApp } from "../context/AppContext";
import * as api from "../lib/tauri";
import type { ManagedSkill } from "../lib/tauri";
import { getScenarioIconOption } from "../lib/scenarioIcons";

export function Dashboard() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { activeScenario, tools, openSkillDetailById } = useApp();
  const [skills, setSkills] = useState<ManagedSkill[]>([]);

  const installed = tools.filter((t) => t.installed).length;
  const total = tools.length;
  const synced = skills.filter((s) => s.targets.length > 0).length;
  const scenarioIcon = getScenarioIconOption(activeScenario);
  const ScenarioIcon = scenarioIcon.icon;

  useEffect(() => {
    if (activeScenario) {
      api.getSkillsForScenario(activeScenario.id).then(setSkills).catch(() => { });
    }
  }, [activeScenario]);

  return (
    <div className="app-page app-page-narrow">
      <div className="app-page-header">
        <h1 className="app-page-title">{t("dashboard.greeting")}</h1>
        <p className="app-page-subtitle flex items-center gap-2 flex-wrap text-tertiary">
          {t("dashboard.currentScenario")}：
          <span
            className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[13px] font-medium ${scenarioIcon.activeClass} ${scenarioIcon.colorClass}`}
          >
            <ScenarioIcon className="h-3 w-3" />
            {activeScenario?.name || "—"}
          </span>
          <span className="text-faint">·</span>
          <span>{t("dashboard.skillsEnabled", { count: skills.length })}</span>
        </p>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-3 gap-3.5">
        {[
          {
            title: t("dashboard.scenarioSkills"),
            value: String(skills.length),
            icon: Layers,
            color: "text-accent-light",
            bg: "bg-accent-bg",
          },
          {
            title: t("dashboard.synced"),
            value: String(synced),
            icon: CheckCircle2,
            color: "text-emerald-400",
            bg: "bg-emerald-500/[0.08]",
          },
          {
            title: t("dashboard.supportedAgents"),
            value: `${installed}/${total}`,
            icon: Bot,
            color: "text-amber-400",
            bg: "bg-amber-500/[0.08]",
          },
        ].map((stat, i) => {
          const Icon = stat.icon;
          return (
            <div
              key={i}
              className="app-panel flex items-center justify-between px-4 py-4 transition-colors hover:border-border"
            >
              <div>
                <p className="app-section-title mb-1">
                  {stat.title}
                </p>
                <h3 className="text-xl font-semibold text-primary leading-none">{stat.value}</h3>
              </div>
              <div className={`p-2 rounded-md ${stat.bg} ${stat.color} border border-border-subtle`}>
                <Icon className="w-4 h-4" />
              </div>
            </div>
          );
        })}
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

      {/* Recent skills */}
      {skills.length > 0 && (
        <div>
          <h2 className="app-section-title mb-2.5">
            {t("dashboard.recentActivity")}
          </h2>
          <div className="app-panel overflow-hidden divide-y divide-border-subtle">
            {skills.slice(0, 5).map((skill) => (
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
                        ? `${t("dashboard.synced")} → ${skill.targets.map((t) => t.tool).join(", ")}`
                        : "未同步"}
                    </p>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
