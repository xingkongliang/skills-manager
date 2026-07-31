import type { ProjectAgentTarget } from "./tauri";

export type ProjectAgentScopeStatus =
  | "idle"
  | "loading"
  | "ready"
  | "error";

export interface ProjectAgentScopeState {
  projectId: string | null;
  status: ProjectAgentScopeStatus;
  targets: ProjectAgentTarget[];
  selectedExportAgents: string[];
}

/**
 * 只暴露与当前路由项目完全匹配的 Agent 状态。
 *
 * React 路由可能先 render Project B，再由 effect 发起 B 的 targets 请求；
 * 这段纯函数确保该时间窗内 Project A 的已加载 targets 和默认选择不可见。
 */
export function getActiveProjectAgentScope(
  state: ProjectAgentScopeState,
  currentProjectId: string | undefined
) {
  if (!currentProjectId || state.projectId !== currentProjectId) {
    return null;
  }
  return state;
}

export function getReadyProjectAgentScope(
  state: ProjectAgentScopeState,
  currentProjectId: string | undefined
) {
  const activeScope = getActiveProjectAgentScope(state, currentProjectId);
  return activeScope?.status === "ready" ? activeScope : null;
}
