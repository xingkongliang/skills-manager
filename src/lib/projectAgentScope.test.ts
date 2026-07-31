import { describe, expect, it } from "vitest";
import {
  getReadyProjectAgentScope,
  type ProjectAgentScopeState,
} from "./projectAgentScope";

const readyProjectA: ProjectAgentScopeState = {
  projectId: "project-a",
  status: "ready",
  targets: [
    {
      key: "codex",
      display_name: "Codex",
      enabled: true,
      installed: true,
      is_custom: false,
    },
  ],
  selectedExportAgents: ["codex"],
};

describe("getReadyProjectAgentScope", () => {
  it("A 项目的已加载状态在路由切到 B 后立即不可用", () => {
    expect(getReadyProjectAgentScope(readyProjectA, "project-b")).toBeNull();
  });

  it("当前项目仍在加载 targets 时不能启用 Preset", () => {
    const loadingProjectB: ProjectAgentScopeState = {
      projectId: "project-b",
      status: "loading",
      targets: [],
      selectedExportAgents: [],
    };

    expect(
      getReadyProjectAgentScope(loadingProjectB, "project-b")
    ).toBeNull();
  });

  it("只有当前项目 targets 完整加载后才返回其选择状态", () => {
    expect(getReadyProjectAgentScope(readyProjectA, "project-a")).toEqual(
      readyProjectA
    );
  });
});
