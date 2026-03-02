import { useState, type CSSProperties } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import {
  Hammer,
  LayoutDashboard,
  Layers,
  Download,
  Settings,
  Plus,
  Pencil,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { CreateScenarioDialog } from "./CreateScenarioDialog";
import { RenameScenarioDialog } from "./RenameScenarioDialog";
import { ConfirmDialog } from "./ConfirmDialog";
import * as api from "../lib/tauri";
import { getScenarioIconOption } from "../lib/scenarioIcons";

export function Sidebar() {
  const { t } = useTranslation();
  const location = useLocation();
  const navigate = useNavigate();
  const { scenarios, activeScenario, switchScenario, refreshScenarios, refreshManagedSkills } = useApp();
  const [showCreate, setShowCreate] = useState(false);
  const [renameTarget, setRenameTarget] = useState<{ id: string; name: string; icon?: string | null } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null);

  const NAV_ITEMS = [
    { name: t("sidebar.dashboard"), path: "/", icon: LayoutDashboard },
    { name: t("sidebar.mySkills"), path: "/my-skills", icon: Layers },
    { name: t("sidebar.installSkills"), path: "/install", icon: Download },
  ];

  const handleSwitchScenario = async (id: string) => {
    await switchScenario(id);
    const s = scenarios.find((s) => s.id === id);
    if (location.pathname === "/settings") {
      navigate("/my-skills");
    }
    if (s) toast.success(t("scenario.switched", { name: s.name }));
  };

  const handleCreateScenario = async (name: string, description?: string, icon?: string) => {
    await api.createScenario(name, description, icon);
    await Promise.all([refreshScenarios(), refreshManagedSkills()]);
    if (location.pathname === "/settings") {
      navigate("/my-skills");
    }
    toast.success(t("scenario.created"));
  };

  const handleRenameScenario = async (newName: string, icon?: string) => {
    if (!renameTarget) return;
    const scenario = scenarios.find((s) => s.id === renameTarget.id);
    if (!scenario) return;
    await api.updateScenario(
      renameTarget.id,
      newName,
      scenario.description || undefined,
      icon || scenario.icon || undefined
    );
    await refreshScenarios();
    toast.success(t("scenario.renamed"));
  };

  const handleDeleteScenario = async () => {
    if (!deleteTarget) return;
    await api.deleteScenario(deleteTarget.id);
    await Promise.all([refreshScenarios(), refreshManagedSkills()]);
    if (location.pathname === "/settings") {
      navigate("/my-skills");
    }
    toast.success(t("scenario.deleted"));
  };

  const handleRenameClick = (
    event: React.MouseEvent,
    scenario: { id: string; name: string; icon?: string | null }
  ) => {
    event.preventDefault();
    event.stopPropagation();
    setRenameTarget(scenario);
  };

  const handleDeleteClick = (event: React.MouseEvent, scenario: { id: string; name: string }) => {
    event.preventDefault();
    event.stopPropagation();
    setDeleteTarget(scenario);
  };

  return (
    <>
      <div className="w-[220px] flex-shrink-0 bg-bg-secondary border-r border-border-subtle h-full flex flex-col select-none relative z-10">
        {/* Traffic-light safe zone — blank drag region */}
        <div
          className="h-[38px] shrink-0"
          style={{ WebkitAppRegion: "drag" } as CSSProperties}
        />
        {/* App logo — sits below macOS window controls */}
        <div
          className="flex items-center px-3 gap-2.5 pb-2 shrink-0"
          style={{ WebkitAppRegion: "drag" } as CSSProperties}
        >
          <div
            className="w-[20px] h-[20px] rounded-[5px] bg-gradient-to-br from-emerald-500 to-teal-600 flex items-center justify-center shrink-0 shadow-[0_0_10px_rgba(16,185,129,0.25)]"
            style={{ WebkitAppRegion: "no-drag" } as CSSProperties}
          >
            <Hammer className="w-3 h-3 text-white" />
          </div>
          <span
            className="text-[14px] font-semibold text-secondary tracking-tight truncate leading-[20px]"
            style={{ WebkitAppRegion: "no-drag" } as CSSProperties}
          >
            {t("app.name")}
          </span>
        </div>

        {/* Nav */}
        <div className="px-2.5 space-y-0.5">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            const isActive = location.pathname === item.path;
            return (
              <Link
                key={item.path}
                to={item.path}
                className={cn(
                  "flex items-center gap-2.5 px-2.5 py-[7px] rounded-[5px] text-sm font-medium transition-colors outline-none",
                  isActive
                    ? "bg-surface-active text-primary"
                    : "text-tertiary hover:text-secondary hover:bg-surface-hover"
                )}
              >
                <Icon className={cn("w-4 h-4 shrink-0", isActive ? "text-accent" : "text-muted")} />
                {item.name}
              </Link>
            );
          })}
        </div>

        {/* Divider */}
        <div className="mx-3 mt-3.5 mb-2.5 border-t border-border-subtle" />

        {/* Scenarios */}
        <div className="px-2.5 flex-1 overflow-y-auto scrollbar-hide min-h-0">
          <div className="text-[11px] font-semibold text-faint mb-1.5 px-2.5 tracking-[0.1em] uppercase">
            {t("sidebar.scenarios")}
          </div>
          <div className="space-y-0.5">
            {scenarios.map((scenario) => {
              const isActive = activeScenario?.id === scenario.id;
              const scenarioIcon = getScenarioIconOption(scenario);
              const ScenarioIcon = scenarioIcon.icon;
              return (
                <div
                  key={scenario.id}
                  className={cn(
                    "group flex items-center gap-0.5 rounded-[5px] transition-colors",
                    isActive ? "bg-surface-active" : "hover:bg-surface-hover"
                  )}
                >
                  <button
                    onClick={() => handleSwitchScenario(scenario.id)}
                    className={cn(
                      "flex min-w-0 flex-1 items-center gap-2 px-2.5 py-[7px] text-left text-sm outline-none",
                      isActive ? "font-medium text-primary" : "text-tertiary group-hover:text-secondary"
                    )}
                  >
                    <span
                      className={cn(
                        "flex h-[20px] w-[20px] shrink-0 items-center justify-center rounded border",
                        isActive
                          ? `${scenarioIcon.activeClass} ${scenarioIcon.colorClass}`
                          : "border-border bg-surface text-muted group-hover:border-border group-hover:text-tertiary"
                      )}
                    >
                      <ScenarioIcon className="h-3 w-3" />
                    </span>
                    <span className="flex-1 truncate">{scenario.name}</span>
                    {scenario.skill_count > 0 && (
                      <span
                        className={cn(
                          "rounded-full px-1.5 text-[11px] font-medium leading-[18px]",
                          isActive
                            ? "bg-accent-bg text-accent-light"
                            : "bg-surface-hover text-faint group-hover:bg-surface-active"
                        )}
                      >
                        {scenario.skill_count}
                      </span>
                    )}
                  </button>

                  <div className="mr-1.5 flex items-center opacity-0 transition group-hover:opacity-100">
                    <button
                      onClick={(event) => handleRenameClick(event, scenario)}
                      className="rounded p-1 text-faint transition hover:bg-surface-hover hover:text-secondary"
                      title={t("common.rename")}
                    >
                      <Pencil className="h-3 w-3" />
                    </button>
                    <button
                      onClick={(event) => handleDeleteClick(event, scenario)}
                      className="rounded p-1 text-faint transition hover:bg-surface-hover hover:text-red-400"
                      title={t("common.delete")}
                    >
                      <Trash2 className="h-3 w-3" />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>

          <button
            onClick={() => setShowCreate(true)}
            className="flex items-center gap-2 px-2.5 py-[7px] mt-0.5 rounded-[5px] text-[13px] text-faint hover:text-tertiary hover:bg-surface-hover transition-colors w-full outline-none"
          >
            <Plus className="w-3.5 h-3.5" />
            {t("sidebar.newScenario")}
          </button>
        </div>

        {/* Settings */}
        <div className="p-2.5 border-t border-border-subtle shrink-0">
          <Link
            to="/settings"
            className={cn(
              "flex items-center gap-2.5 px-2.5 py-[7px] rounded-[5px] text-sm font-medium transition-colors outline-none",
              location.pathname === "/settings"
                ? "bg-surface-active text-primary"
                : "text-tertiary hover:text-secondary hover:bg-surface-hover"
            )}
          >
            <Settings
              className={cn(
                "w-4 h-4 shrink-0",
                location.pathname === "/settings" ? "text-accent" : "text-muted"
              )}
            />
            {t("sidebar.settings")}
          </Link>
        </div>
      </div>

      <CreateScenarioDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreate={handleCreateScenario}
      />

      <RenameScenarioDialog
        open={renameTarget !== null}
        currentName={renameTarget?.name || ""}
        currentIcon={renameTarget?.icon}
        onClose={() => setRenameTarget(null)}
        onRename={handleRenameScenario}
      />

      <ConfirmDialog
        open={deleteTarget !== null}
        message={t("scenario.deleteConfirm", { name: deleteTarget?.name || "" })}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeleteScenario}
      />
    </>
  );
}
