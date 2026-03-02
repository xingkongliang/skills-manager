import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from "react";
import type { ManagedSkill, Scenario, ToolInfo } from "../lib/tauri";
import * as api from "../lib/tauri";
import i18n from "../i18n";

interface AppState {
  scenarios: Scenario[];
  activeScenario: Scenario | null;
  tools: ToolInfo[];
  managedSkills: ManagedSkill[];
  loading: boolean;
  appError: string | null;
  helpOpen: boolean;
  detailSkillId: string | null;
  refreshAppData: () => Promise<void>;
  refreshScenarios: () => Promise<void>;
  refreshTools: () => Promise<void>;
  refreshManagedSkills: () => Promise<void>;
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

  const refreshAppData = useCallback(async () => {
    setLoading(true);
    await Promise.all([refreshScenarios(), refreshTools(), refreshManagedSkills()]);
    setLoading(false);
  }, [refreshManagedSkills, refreshScenarios, refreshTools]);

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
    }
    init();
  }, [refreshAppData]);

  return (
    <AppContext.Provider
      value={{
        scenarios,
        activeScenario,
        tools,
        managedSkills,
        loading,
        appError,
        helpOpen,
        detailSkillId,
        refreshAppData,
        refreshScenarios,
        refreshTools,
        refreshManagedSkills,
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
