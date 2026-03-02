import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from "react";
import type { ManagedSkill, Scenario, ToolInfo } from "../lib/tauri";
import * as api from "../lib/tauri";

interface AppState {
  scenarios: Scenario[];
  activeScenario: Scenario | null;
  tools: ToolInfo[];
  managedSkills: ManagedSkill[];
  loading: boolean;
  globalSearchOpen: boolean;
  helpOpen: boolean;
  detailSkillId: string | null;
  refreshScenarios: () => Promise<void>;
  refreshTools: () => Promise<void>;
  refreshManagedSkills: () => Promise<void>;
  switchScenario: (id: string) => Promise<void>;
  openGlobalSearch: () => void;
  closeGlobalSearch: () => void;
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
  const [globalSearchOpen, setGlobalSearchOpen] = useState(false);
  const [helpOpen, setHelpOpen] = useState(false);
  const [detailSkillId, setDetailSkillId] = useState<string | null>(null);

  const refreshScenarios = useCallback(async () => {
    try {
      const [s, active] = await Promise.all([
        api.getScenarios(),
        api.getActiveScenario(),
      ]);
      setScenarios(s);
      setActiveScenario(active);
    } catch (e) {
      console.error("Failed to load scenarios:", e);
    }
  }, []);

  const refreshTools = useCallback(async () => {
    try {
      const t = await api.getToolStatus();
      setTools(t);
    } catch (e) {
      console.error("Failed to load tools:", e);
    }
  }, []);

  const refreshManagedSkills = useCallback(async () => {
    try {
      const skills = await api.getManagedSkills();
      setManagedSkills(skills);
    } catch (e) {
      console.error("Failed to load managed skills:", e);
    }
  }, []);

  const handleSwitchScenario = useCallback(
    async (id: string) => {
      try {
        await api.switchScenario(id);
        await Promise.all([refreshScenarios(), refreshManagedSkills()]);
      } catch (e) {
        console.error("Failed to switch scenario:", e);
      }
    },
    [refreshManagedSkills, refreshScenarios]
  );

  useEffect(() => {
    async function init() {
      setLoading(true);
      await Promise.all([refreshScenarios(), refreshTools(), refreshManagedSkills()]);
      setLoading(false);
    }
    init();
  }, [refreshManagedSkills, refreshScenarios, refreshTools]);

  return (
    <AppContext.Provider
      value={{
        scenarios,
        activeScenario,
        tools,
        managedSkills,
        loading,
        globalSearchOpen,
        helpOpen,
        detailSkillId,
        refreshScenarios,
        refreshTools,
        refreshManagedSkills,
        switchScenario: handleSwitchScenario,
        openGlobalSearch: () => setGlobalSearchOpen(true),
        closeGlobalSearch: () => setGlobalSearchOpen(false),
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
