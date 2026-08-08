import { BrowserRouter, Routes, Route } from "react-router-dom";
import { Toaster } from "sonner";
import { AppProvider } from "./context/AppContext";
import { ThemeProvider, useThemeContext } from "./context/ThemeContext";
import { HelpDialog } from "./components/HelpDialog";
import { CloseActionGuard } from "./components/CloseActionGuard";
import { FirstRunRestoreDialog } from "./components/FirstRunRestoreDialog";
import { Layout } from "./components/Layout";
import { Dashboard } from "./views/Dashboard";
import { MySkills } from "./views/MySkills";
import { WorkspaceView } from "./views/WorkspaceView";
import { CODING_WORKSPACE_CONFIG, LOBSTER_WORKSPACE_CONFIG } from "./views/workspaceConfigs";
import { InstallSkills } from "./views/InstallSkills";
import { Settings } from "./views/Settings";
import { ProjectDetail } from "./views/ProjectDetail";
import { Backup } from "./views/Backup";
import { Hosts } from "./views/Hosts";

function ThemedToaster() {
  const { resolvedTheme } = useThemeContext();
  return (
    <Toaster
      theme={resolvedTheme}
      position="bottom-right"
      toastOptions={{
        style: {
          background: "var(--color-surface)",
          border: "1px solid var(--color-border)",
          color: "var(--color-text-primary)",
        },
      }}
    />
  );
}

function App() {
  return (
    <ThemeProvider>
      <AppProvider>
        <BrowserRouter>
          <Routes>
            <Route element={<Layout />}>
              <Route path="/" element={<Dashboard />} />
              <Route path="/my-skills" element={<MySkills />} />
              <Route path="/global-workspace" element={<WorkspaceView config={CODING_WORKSPACE_CONFIG} />} />
              <Route path="/global-workspace/:agentKey" element={<WorkspaceView config={CODING_WORKSPACE_CONFIG} />} />
              <Route path="/lobster-workspace" element={<WorkspaceView config={LOBSTER_WORKSPACE_CONFIG} />} />
              <Route path="/lobster-workspace/:agentKey" element={<WorkspaceView config={LOBSTER_WORKSPACE_CONFIG} />} />
              <Route path="/install" element={<InstallSkills />} />
              <Route path="/hosts" element={<Hosts />} />
              <Route path="/hosts/:hostId" element={<Hosts />} />
              <Route path="/backup" element={<Backup />} />
              <Route path="/project/:id" element={<ProjectDetail />} />
              <Route path="/settings" element={<Settings />} />
            </Route>
          </Routes>
          <HelpDialog />
          <CloseActionGuard />
          <FirstRunRestoreDialog />
        </BrowserRouter>
        <ThemedToaster />
      </AppProvider>
    </ThemeProvider>
  );
}

export default App;
