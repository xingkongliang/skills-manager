/* eslint-disable react-refresh/only-export-components */
import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ManagedSkill, Project, Scenario, ToolInfo } from "../lib/tauri";
import * as api from "../lib/tauri";
import i18n from "../i18n";
import { toast } from "sonner";

interface AppState {
  scenarios: Scenario[];
  activeScenario: Scenario | null;
  tools: ToolInfo[];
  managedSkills: ManagedSkill[];
  projects: Project[];
  loading: boolean;
  appError: string | null;
  helpOpen: boolean;
  detailSkillId: string | null;
  refreshAppData: () => Promise<void>;
  refreshScenarios: () => Promise<void>;
  refreshTools: () => Promise<void>;
  refreshManagedSkills: () => Promise<void>;
  refreshProjects: () => Promise<void>;
  switchScenario: (id: string) => Promise<void>;
  clearAppError: () => void;
  openHelp: () => void;
  closeHelp: () => void;
  openSkillDetailById: (skillId: string) => void;
  closeSkillDetail: () => void;
}

const AppContext = createContext<AppState | null>(null);

export function AppProvider({ children }: { children: ReactNode }) {
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [activeScenario, setActiveScenario] = useState<Scenario | null>(null);
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [managedSkills, setManagedSkills] = useState<ManagedSkill[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [appError, setAppError] = useState<string | null>(null);
  const [helpOpen, setHelpOpen] = useState(false);
  const [detailSkillId, setDetailSkillId] = useState<string | null>(null);

  const setTranslatedError = useCallback((key: string) => {
    setAppError(i18n.t("common.loadFailed", { item: i18n.t(key) }));
  }, []);

  const refreshScenarios = useCallback(async () => {
    try {
      const [s, active] = await Promise.all([
        api.getScenarios(),
        api.getActiveScenario(),
      ]);
      setScenarios(s);
      setActiveScenario(active);
      setAppError(null);
    } catch (e) {
      console.error("Failed to load scenarios:", e);
      setTranslatedError("common.scenarios");
    }
  }, [setTranslatedError]);

  const refreshTools = useCallback(async () => {
    try {
      const t = await api.getToolStatus();
      setTools(t);
      setAppError(null);
    } catch (e) {
      console.error("Failed to load tools:", e);
      setTranslatedError("common.agents");
    }
  }, [setTranslatedError]);

  const refreshManagedSkills = useCallback(async () => {
    try {
      const skills = await api.getManagedSkills();
      setManagedSkills(skills);
      setAppError(null);
    } catch (e) {
      console.error("Failed to load managed skills:", e);
      setTranslatedError("common.skills");
    }
  }, [setTranslatedError]);

  const refreshProjects = useCallback(async () => {
    try {
      const p = await api.getProjects();
      setProjects(p);
    } catch (e) {
      console.error("Failed to load projects:", e);
    }
  }, []);

  const refreshAppData = useCallback(async () => {
    setLoading(true);
    await Promise.all([refreshScenarios(), refreshTools(), refreshManagedSkills(), refreshProjects()]);
    setLoading(false);
  }, [refreshManagedSkills, refreshProjects, refreshScenarios, refreshTools]);

  const handleSwitchScenario = useCallback(
    async (id: string) => {
      try {
        await api.switchScenario(id);
        await Promise.all([refreshScenarios(), refreshManagedSkills()]);
        setAppError(null);
      } catch (e) {
        console.error("Failed to switch scenario:", e);
        setTranslatedError("common.scenarios");
      }
    },
    [refreshManagedSkills, refreshScenarios, setTranslatedError]
  );

  useEffect(() => {
    async function init() {
      await refreshAppData();
      // Apply saved text size on startup
      const savedSize = await api.getSettings("text_size").catch(() => null);
      if (savedSize) {
        const zoomMap: Record<string, string> = { small: "0.9", default: "1", large: "1.1", xlarge: "1.2" };
        document.documentElement.style.zoom = zoomMap[savedSize] || "1";
      }
    }
    init();
  }, [refreshAppData]);

  useEffect(() => {
    const unlistenPromise = listen<string>("tray-scenario-switched", async () => {
      await Promise.all([refreshScenarios(), refreshManagedSkills()]);
    });

    return () => {
      unlistenPromise
        .then((unlisten) => unlisten())
        .catch((error) => {
          console.error("Failed to unlisten tray-scenario-switched:", error);
        });
    };
  }, [refreshManagedSkills, refreshScenarios]);

  // Auto-check skill updates on startup (non-blocking, silent)
  useEffect(() => {
    if (loading || managedSkills.length === 0) return;
    const hasGitSkills = managedSkills.some(
      (s) => s.source_type === "git" || s.source_type === "skillssh"
    );
    if (!hasGitSkills) return;

    // Delay to avoid slowing down initial render
    const timer = setTimeout(() => {
      api.checkAllSkillUpdates(false)
        .then(async () => {
          const skills = await api.getManagedSkills();
          setManagedSkills(skills);
          const updatable = skills.filter((s) => s.update_status === "update_available");
          if (updatable.length > 0) {
            toast.info(
              i18n.t("mySkills.updateNotification", { count: updatable.length }),
              {
                duration: 8000,
                action: {
                  label: i18n.t("mySkills.viewUpdates"),
                  onClick: () => {
                    setDetailSkillId(updatable[0].id);
                    // Navigate to my-skills page — AppProvider is outside Router,
                    // so use pushState + popstate event for SPA navigation that
                    // preserves React state (window.location.href would discard it).
                    if (!window.location.pathname.endsWith("/my-skills")) {
                      window.history.pushState(null, "", "/my-skills");
                      window.dispatchEvent(new PopStateEvent("popstate"));
                    }
                  },
                },
              }
            );
          }
        })
        .catch(() => {}); // silent failure
    }, 3000);
    return () => clearTimeout(timer);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading]);

  return (
    <AppContext.Provider
      value={{
        scenarios,
        activeScenario,
        tools,
        managedSkills,
        projects,
        loading,
        appError,
        helpOpen,
        detailSkillId,
        refreshAppData,
        refreshScenarios,
        refreshTools,
        refreshManagedSkills,
        refreshProjects,
        switchScenario: handleSwitchScenario,
        clearAppError: () => setAppError(null),
        openHelp: () => setHelpOpen(true),
        closeHelp: () => setHelpOpen(false),
        openSkillDetailById: (skillId: string) => setDetailSkillId(skillId),
        closeSkillDetail: () => setDetailSkillId(null),
      }}
    >
      {children}
    </AppContext.Provider>
  );
}

export function useApp() {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used within AppProvider");
  return ctx;
}
