import { BrowserRouter, Routes, Route } from "react-router-dom";
import { Toaster } from "sonner";
import { AppProvider } from "./context/AppContext";
import { ThemeProvider, useThemeContext } from "./context/ThemeContext";
import { HelpDialog } from "./components/HelpDialog";
import { CloseActionGuard } from "./components/CloseActionGuard";
import { Layout } from "./components/Layout";
import { Dashboard } from "./views/Dashboard";
import { MySkills } from "./views/MySkills";
import { InstallSkills } from "./views/InstallSkills";
import { MatrixView } from "./views/MatrixView";
import { PluginsView } from "./views/PluginsView";
import { Settings } from "./views/Settings";
import { ProjectDetail } from "./views/ProjectDetail";
import { PacksView } from "./views/PacksView";
import { AgentDetail } from "./views/AgentDetail";

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
              <Route path="/packs" element={<PacksView />} />
              <Route path="/install" element={<InstallSkills />} />
              <Route path="/matrix" element={<MatrixView />} />
              <Route path="/plugins" element={<PluginsView />} />
              <Route path="/project/:id" element={<ProjectDetail />} />
              <Route path="/agent/:toolKey" element={<AgentDetail />} />
              <Route path="/settings" element={<Settings />} />
            </Route>
          </Routes>
          <HelpDialog />
          <CloseActionGuard />
        </BrowserRouter>
        <ThemedToaster />
      </AppProvider>
    </ThemeProvider>
  );
}

export default App;
