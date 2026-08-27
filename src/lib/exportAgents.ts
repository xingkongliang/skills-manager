import type { ProjectAgentTarget } from "./tauri";

const PROJECT_EXPORT_AGENT_PRIORITY = ["claude_code", "codex", "cursor", "gemini_cli", "github_copilot"];

// Keys of project agents that can actually receive skills right now: both
// installed on disk and enabled by the user. Used everywhere export targets
// are derived so disabled/uninstalled agents never get project-local skills.
export function enabledInstalledAgentKeys(targets: ProjectAgentTarget[]): string[] {
  return targets.filter((target) => target.installed && target.enabled).map((target) => target.key);
}

export function getDefaultExportAgents(targets: ProjectAgentTarget[], savedValue?: string | null) {
  const enabledKeys = enabledInstalledAgentKeys(targets);
  const availableKeys = new Set(enabledKeys);
  if (savedValue) {
    try {
      const parsed = JSON.parse(savedValue);
      if (Array.isArray(parsed)) {
        const filtered = parsed.filter((item): item is string => typeof item === "string" && availableKeys.has(item));
        if (filtered.length > 0) {
          return Array.from(new Set(filtered));
        }
      }
    } catch {
      // Ignore invalid persisted settings and fall back to built-in defaults.
    }
  }

  // Priority agents first, then every other enabled agent in its detected
  // order. All enabled agents are included: preset export must reach each
  // one the user has installed and enabled (issue #400 — non-priority
  // agents like "pi" were silently dropped when any priority agent was on).
  const prioritized = PROJECT_EXPORT_AGENT_PRIORITY.filter((key) => availableKeys.has(key));
  const rest = enabledKeys.filter((key) => !prioritized.includes(key));
  return Array.from(new Set([...prioritized, ...rest]));
}
