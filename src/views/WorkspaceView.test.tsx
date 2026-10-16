// @vitest-environment jsdom
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PresetBar } from "../components/PresetBar";
import type { ManagedSkill, Preset, ProjectSkill } from "../lib/tauri";
import { WorkspaceView } from "./WorkspaceView";
import { CODING_WORKSPACE_CONFIG } from "./workspaceConfigs";

const mocks = vi.hoisted(() => ({
  localSkills: [] as ProjectSkill[],
  managedSkills: [] as ManagedSkill[],
  refreshManagedSkills: vi.fn(),
  refreshTools: vi.fn(),
  deleteGlobalLocalSkill: vi.fn(),
  importGlobalLocalSkillToCenter: vi.fn(),
  unsyncSkillFromTool: vi.fn(),
  t: (key: string, values?: Record<string, unknown>) =>
    values ? `${key} ${Object.values(values).join(" ")}` : key,
}));

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }));
vi.mock("../context/AppContext", () => ({
  useApp: () => ({
    tools: [{
      key: "deepseek_harness", display_name: "DSH", installed: true, enabled: true,
      skills_dir: "/fixture/dsh/skills", category: "coding",
    }],
    managedSkills: mocks.managedSkills,
    presets: [],
    refreshManagedSkills: mocks.refreshManagedSkills,
    refreshTools: mocks.refreshTools,
  }),
}));
vi.mock("../lib/tauri", () => ({
  getGlobalLocalSkills: () => Promise.resolve(mocks.localSkills),
  deleteGlobalLocalSkill: mocks.deleteGlobalLocalSkill,
  importGlobalLocalSkillToCenter: mocks.importGlobalLocalSkillToCenter,
  unsyncSkillFromTool: mocks.unsyncSkillFromTool,
}));
vi.mock("../components/AddSkillsSheet", () => ({ AddSkillsSheet: () => null }));
vi.mock("../components/DetailSheet", () => ({ DetailSheet: () => null }));

const preset: Preset = {
  id: "frontend", name: "Frontend", description: null, icon: null,
  sort_order: 0, skill_count: 2, created_at: 0, updated_at: 0,
};

function librarySkill(id: string, deployed = false): ManagedSkill {
  return {
    id, name: id, description: null, source_type: "skillssh",
    source_ref: `vercel-labs/agent-skills/${id}`, source_ref_resolved: null,
    source_subpath: null, source_branch: null, source_revision: null, remote_revision: null,
    update_status: "ok", last_checked_at: null, last_check_error: null,
    central_path: `/fixture/library/${id}`, enabled: true, created_at: 0, updated_at: 0,
    status: "ok", preset_ids: [preset.id], tags: ["frontend"],
    targets: deployed ? [{
      id: `${id}-target`, skill_id: id, tool: "deepseek_harness",
      target_path: `/fixture/dsh/skills/${id}`, mode: "symlink", status: "ok", synced_at: 0,
    }] : [],
  };
}

function localSkill(status: ProjectSkill["sync_status"] = "in_sync"): ProjectSkill {
  return {
    name: "web-design-guidelines", dir_name: "web-design-guidelines",
    relative_path: "web-design-guidelines", description: null,
    path: "/fixture/dsh/skills/web-design-guidelines", files: ["SKILL.md"], enabled: true,
    agent: "deepseek_harness", agent_display_name: "DSH", tags: ["frontend"],
    in_center: true, sync_status: status, center_skill_id: "web-design-guidelines",
  };
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  mocks.localSkills = [localSkill()];
  mocks.managedSkills = [librarySkill("web-design-guidelines")];
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render(element: ReactNode) {
  await act(async () => root.render(element));
}

async function click(button: HTMLButtonElement | null) {
  expect(button).not.toBeNull();
  await act(async () => button!.click());
}

function buttonWithText(text: string) {
  return [...container.querySelectorAll("button")].find((button) => button.textContent === text) ?? null;
}

async function renderWorkspace() {
  await render(
    <MemoryRouter initialEntries={["/global-workspace/deepseek_harness"]}>
      <Routes>
        <Route path="/global-workspace/:agentKey" element={<WorkspaceView config={CODING_WORKSPACE_CONFIG} />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("external skills matching a library entry", () => {
  it.each(["grid", "list"])("keeps confirmed deletion available in %s view", async (view) => {
    localStorage.setItem("skills-manager.skillViewMode", view);
    await renderWorkspace();
    expect(container.textContent).toContain("globalWorkspace.localSkills.status.unmanaged");
    expect(container.textContent).toContain("globalWorkspace.localSkills.summary 1 0 0");
    expect(container.querySelector('[title="globalWorkspace.localSkills.upload"]')).not.toBeNull();
    await click(container.querySelector('[title="globalWorkspace.localSkills.deleteLocal"]'));
    expect(mocks.deleteGlobalLocalSkill).not.toHaveBeenCalled();
    expect(container.textContent).toContain("globalWorkspace.localSkills.deleteLocalConfirmMessage");
    await click(buttonWithText("common.delete"));
    expect(mocks.deleteGlobalLocalSkill).toHaveBeenCalledExactlyOnceWith("deepseek_harness", "web-design-guidelines");
    expect(mocks.unsyncSkillFromTool).not.toHaveBeenCalled();
  });

  it("offers explicit adoption through the existing confirmation", async () => {
    await renderWorkspace();
    await click(container.querySelector('[title="globalWorkspace.localSkills.upload"]'));
    expect(mocks.importGlobalLocalSkillToCenter).not.toHaveBeenCalled();
    await click(buttonWithText("globalWorkspace.localSkills.upload"));
    expect(mocks.importGlobalLocalSkillToCenter).toHaveBeenCalledExactlyOnceWith("deepseek_harness", "web-design-guidelines");
  });

  it.each(["in_sync", "project_newer", "center_newer", "diverged"] as const)("includes unmanaged %s skills in batch deletion", async (status) => {
    mocks.localSkills = [localSkill(status)];
    await renderWorkspace();
    await click(buttonWithText("globalWorkspace.selectMode"));
    await click(buttonWithText("globalWorkspace.selectAll"));
    await click(buttonWithText("globalWorkspace.localSkills.deleteLocalSelected 1"));
    expect(mocks.deleteGlobalLocalSkill).not.toHaveBeenCalled();
    await click(buttonWithText("common.delete"));
    expect(mocks.deleteGlobalLocalSkill).toHaveBeenCalledExactlyOnceWith("deepseek_harness", "web-design-guidelines");
  });

  it.each(["in_sync", "diverged"] as const)("continues to unsync managed %s skills instead of deleting their files", async (status) => {
    mocks.localSkills = [localSkill(status)];
    mocks.managedSkills = [librarySkill("web-design-guidelines", true)];
    await renderWorkspace();
    expect(container.textContent).not.toContain("globalWorkspace.localSkills.status.unmanaged");
    expect(container.querySelector('[title="globalWorkspace.localSkills.deleteLocal"]')).toBeNull();
    await click(container.querySelector('[title="globalWorkspace.localSkills.removeManaged"]'));
    expect(mocks.unsyncSkillFromTool).toHaveBeenCalledExactlyOnceWith("web-design-guidelines", "deepseek_harness");
    expect(mocks.deleteGlobalLocalSkill).not.toHaveBeenCalled();
  });

  it("keeps managed files out of a mixed batch deletion", async () => {
    const managed = { ...localSkill(), name: "frontend-design", relative_path: "frontend-design", center_skill_id: "frontend-design" };
    mocks.localSkills.push(managed);
    mocks.managedSkills.push(librarySkill("frontend-design", true));
    await renderWorkspace();
    await click(buttonWithText("globalWorkspace.selectMode"));
    await click(buttonWithText("globalWorkspace.selectAll"));
    await click(buttonWithText("globalWorkspace.localSkills.deleteLocalSelected 1"));
    await click(buttonWithText("common.delete"));
    expect(mocks.deleteGlobalLocalSkill).toHaveBeenCalledExactlyOnceWith("deepseek_harness", "web-design-guidelines");
    expect(mocks.unsyncSkillFromTool).not.toHaveBeenCalled();
  });
});

describe("partially applied presets", () => {
  const onAddSkill = vi.fn();
  const onRemoveSkill = vi.fn();
  const onComplete = vi.fn();

  async function renderPreset(agentKeys = ["deepseek_harness"], statusMode: "agent-pair" | "logical-skill" = "agent-pair") {
    await render(<PresetBar
      presets={[preset]}
      managedSkills={[librarySkill("frontend-design", true), librarySkill("web-design-guidelines")]}
      agentKeys={agentKeys}
      existsInWorkspace={(skill, agent) => skill.targets.some((target) => target.tool === agent)}
      onAddSkill={onAddSkill}
      onRemoveSkill={onRemoveSkill}
      onComplete={onComplete}
      statusMode={statusMode}
    />);
  }

  it.each(["agent-pair", "logical-skill"] as const)("can remove installed pairs without retrying a conflicting add in %s mode", async (mode) => {
    await renderPreset(["deepseek_harness", "codex"], mode);
    await click(container.querySelector('[aria-label="presetActions.removePreset Frontend"]'));
    expect(onAddSkill).not.toHaveBeenCalled();
    expect(onRemoveSkill).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ id: "frontend-design" }), "deepseek_harness");
    expect(onComplete).toHaveBeenCalledOnce();
  });

  it("preserves the main chip's add-missing behavior", async () => {
    await renderPreset();
    await click(container.querySelector('[title="Frontend"]'));
    expect(onAddSkill).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ id: "web-design-guidelines" }), "deepseek_harness");
    expect(onRemoveSkill).not.toHaveBeenCalled();
  });

  it("can cancel after an add fails without repeating the failed add", async () => {
    onAddSkill.mockRejectedValueOnce(new Error("Target is not managed by Skills Manager"));
    await renderPreset();
    await click(container.querySelector('[title="Frontend"]'));
    await click(container.querySelector('[aria-label="presetActions.removePreset Frontend"]'));
    expect(onAddSkill).toHaveBeenCalledOnce();
    expect(onRemoveSkill).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ id: "frontend-design" }), "deepseek_harness");
  });
});
