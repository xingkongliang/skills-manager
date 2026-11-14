// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { ManagedSkill, SkillToolToggle } from "../lib/tauri";
import { SkillDetailPanel } from "./SkillDetailPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key}${JSON.stringify(opts)}` : key,
  }),
}));
// The sheet loads SKILL.md on mount; none of that matters here.
vi.mock("../lib/tauri", () => ({
  getSkillDocument: vi.fn(() => new Promise(() => {})),
  getSourceSkillDocument: vi.fn(() => new Promise(() => {})),
  getSkillSourceDiff: vi.fn(() => new Promise(() => {})),
}));
vi.mock("./SkillMarkdown", () => ({ SkillMarkdown: () => null }));

const skill: ManagedSkill = {
  id: "demo", name: "demo", description: null, source_type: "local", source_ref: null,
  source_ref_resolved: null, source_subpath: null, source_branch: null,
  source_revision: null, remote_revision: null, update_status: "local_only",
  last_checked_at: null, last_check_error: null, central_path: "/fixture/demo",
  enabled: true, created_at: 0, updated_at: 0, status: "ok", targets: [],
  preset_ids: ["p1"], tags: [],
};

function toggle(tool: string, enabled: boolean, deployed: boolean): SkillToolToggle {
  return { tool, display_name: tool, installed: true, globally_enabled: true, enabled, deployed };
}

// The #384 case: intent and fact disagree in both directions.
const toggles = [
  toggle("codex", true, true),
  toggle("claude_code", true, false),
  toggle("gemini_cli", false, true),
  toggle("opencode", true, false),
];

let container: HTMLDivElement;
let root: Root;
let onToggleTool: Mock<(tool: string, enabled: boolean) => void>;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  onToggleTool = vi.fn<(tool: string, enabled: boolean) => void>();
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render(isActivePreset: boolean) {
  await act(async () =>
    root.render(
      <SkillDetailPanel
        skill={skill}
        onClose={() => {}}
        toolToggles={toggles}
        onToggleTool={onToggleTool}
        isActivePreset={isActivePreset}
      />
    )
  );
}

function row(name: string): HTMLButtonElement {
  const button = [...document.body.querySelectorAll<HTMLButtonElement>("button[data-state]")]
    .find((b) => b.textContent?.includes(name));
  expect(button, name).toBeDefined();
  return button!;
}

describe("SkillDetailPanel agent toggles (#384)", () => {
  it("passes the deployed fact through to the rows and the synced count", async () => {
    await render(true);
    expect(row("codex").dataset.state).toBe("deployed");
    expect(row("claude_code").dataset.state).toBe("pending");
    expect(row("gemini_cli").dataset.state).toBe("off");
    // Counts the green rows: only codex is both enabled and deployed. A count
    // from intent would read 3 (three agents are enabled), and a count from
    // deployment alone would read 2 (gemini_cli has files but is turned off).
    expect(document.body.textContent).toContain('mySkills.syncSummary{"synced":1,"total":4}');
  });

  it("deploys a pending row when clicked", async () => {
    await render(true);
    await act(async () => row("claude_code").click());
    expect(onToggleTool).toHaveBeenCalledWith("claude_code", true);
  });

  it("does not show pending on a preset that is not active", async () => {
    await render(false);
    expect(row("claude_code").dataset.state).toBe("enabled");
    expect(document.body.textContent).not.toContain("mySkills.agentTogglePendingSync");
  });
});
