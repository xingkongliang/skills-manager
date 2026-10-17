// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PresetBar, type PresetBarProps } from "./PresetBar";
import type { ManagedSkill, Preset } from "../lib/tauri";

const messages = vi.hoisted(() => ({ success: vi.fn(), info: vi.fn(), error: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("sonner", () => ({ toast: messages }));

const preset: Preset = {
  id: "frontend", name: "Frontend", description: null, icon: null,
  sort_order: 0, skill_count: 2, created_at: 0, updated_at: 0,
};
const skills = ["design", "testing"].map((id): ManagedSkill => ({
  id, name: id, description: null, source_type: "local", source_ref: null,
  source_ref_resolved: null, source_subpath: null, source_branch: null,
  source_revision: null, remote_revision: null, update_status: "local_only",
  last_checked_at: null, last_check_error: null, central_path: `/fixture/${id}`,
  enabled: true, created_at: 0, updated_at: 0, status: "ok", targets: [],
  preset_ids: [preset.id], tags: [],
}));

let container: HTMLDivElement;
let root: Root;
let props: PresetBarProps;

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  props = {
    presets: [preset], managedSkills: skills, agentKeys: ["codex"],
    existsInWorkspace: () => false,
    onAddSkill: vi.fn().mockResolvedValue(undefined),
    onRemoveSkill: vi.fn().mockResolvedValue(undefined),
    onComplete: vi.fn().mockResolvedValue(undefined),
  };
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render() {
  await act(async () => root.render(<PresetBar {...props} />));
}

async function clickPreset() {
  const button = container.querySelector<HTMLButtonElement>('[title="Frontend"]');
  expect(button).not.toBeNull();
  await act(async () => button!.click());
}

describe("PresetBar baseline behavior", () => {
  it("adds every missing skill for each requested agent", async () => {
    props.agentKeys = ["codex", "claude_code"];
    await render();
    await clickPreset();
    expect(props.onAddSkill).toHaveBeenCalledTimes(4);
    for (const skill of skills) {
      for (const agent of props.agentKeys) expect(props.onAddSkill).toHaveBeenCalledWith(skill, agent);
    }
    expect(props.onRemoveSkill).not.toHaveBeenCalled();
    expect(props.onComplete).toHaveBeenCalledOnce();
  });

  it("adds only the missing pairs of a partial preset", async () => {
    props.agentKeys = ["codex", "claude_code"];
    props.existsInWorkspace = (skill, agent) => skill.id === "design" && agent === "codex";
    await render();
    expect(container.textContent).toContain("1/4");
    await clickPreset();
    expect(props.onAddSkill).toHaveBeenCalledTimes(3);
    expect(props.onAddSkill).not.toHaveBeenCalledWith(skills[0], "codex");
    expect(props.onRemoveSkill).not.toHaveBeenCalled();
  });

  it("removes each installed pair of a fully active preset", async () => {
    props.agentKeys = ["codex", "claude_code"];
    props.existsInWorkspace = () => true;
    await render();
    await clickPreset();
    expect(props.onRemoveSkill).toHaveBeenCalledTimes(4);
    expect(props.onAddSkill).not.toHaveBeenCalled();
    expect(props.onComplete).toHaveBeenCalledOnce();
  });

  it("counts fully deployed logical skills across multiple agents", async () => {
    props.agentKeys = ["codex", "claude_code"];
    props.statusMode = "logical-skill";
    props.existsInWorkspace = (skill, agent) => skill.id === "design" || agent === "codex";
    await render();
    expect(container.textContent).toContain("1/2");
    await clickPreset();
    expect(props.onAddSkill).toHaveBeenCalledExactlyOnceWith(skills[1], "claude_code");
  });

  it.each(["skills", "agents"])("hides presets with no %s", async (empty) => {
    if (empty === "skills") props.managedSkills = [];
    else props.agentKeys = [];
    await render();
    expect(container.querySelector("button")).toBeNull();
  });

  it("reports a failed addition, continues other pairs and refreshes", async () => {
    vi.mocked(props.onAddSkill).mockRejectedValueOnce(new Error("Target is not managed"));
    await render();
    await clickPreset();
    expect(props.onAddSkill).toHaveBeenCalledTimes(2);
    expect(messages.error).toHaveBeenCalledWith(expect.stringContaining("Target is not managed"));
    expect(props.onComplete).toHaveBeenCalledOnce();
    expect(container.querySelector<HTMLButtonElement>("button")!.disabled).toBe(false);
  });

  it("disables the chip while a write is pending", async () => {
    let finish!: () => void;
    vi.mocked(props.onAddSkill).mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; }));
    await render();
    await clickPreset();
    expect(container.querySelector<HTMLButtonElement>("button")!.disabled).toBe(true);
    await act(async () => finish());
    expect(props.onComplete).toHaveBeenCalledOnce();
    expect(container.querySelector<HTMLButtonElement>("button")!.disabled).toBe(false);
  });
});
