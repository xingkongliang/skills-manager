// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { ManagedSkill, SkillToolToggle } from "../lib/tauri";
import * as api from "../lib/tauri";
import { SkillDetailPanel } from "./SkillDetailPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key}${JSON.stringify(opts)}` : key,
    i18n: { language: "en" },
  }),
}));
// The sheet loads SKILL.md on mount; none of that matters here.
vi.mock("../lib/tauri", () => ({
  getSkillDocument: vi.fn(() => new Promise(() => {})),
  getSourceSkillDocument: vi.fn(() => new Promise(() => {})),
  getSkillSourceDiff: vi.fn(() => new Promise(() => {})),
  listSkillFiles: vi.fn(() => new Promise(() => {})),
  readSkillFile: vi.fn(() => new Promise(() => {})),
  revealSkillFolder: vi.fn(() => Promise.resolve()),
}));
// AgentIcon reads custom-agent icons from the app context; loading the real
// module would also start i18n, which reads settings through the tauri mock.
vi.mock("../context/AppContext", () => ({ useOptionalApp: () => null }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn() }));
vi.mock("./SkillSourceDiffViewer", () => ({
  SkillSourceDiffViewer: ({ entries }: { entries: { relative_path: string }[] }) => (
    <div data-testid="diff">{entries.map((e) => e.relative_path).join(",")}</div>
  ),
}));
vi.mock("./SkillMarkdown", () => ({
  SkillMarkdown: ({ content, onRelativeLink }: { content: string; onRelativeLink?: (path: string) => void }) => (
    <div data-testid="markdown">
      {content}
      {onRelativeLink && (
        <button type="button" data-testid="relative-link" onClick={() => onRelativeLink("../SKILL.md")} />
      )}
    </div>
  ),
}));

const skill: ManagedSkill = {
  id: "demo", name: "demo", description: null, source_type: "local", source_ref: null,
  source_ref_resolved: null, source_subpath: null, source_branch: null,
  source_revision: null, remote_revision: null, update_status: "local_only",
  last_checked_at: null, last_check_error: null, central_path: "/fixture/demo",
  enabled: true, created_at: 0, updated_at: 0, status: "ok", targets: [],
  preset_ids: ["p1"], tags: [],
};

const gitSkill: ManagedSkill = {
  ...skill,
  source_type: "git",
  source_ref: "https://github.com/example/skills",
  source_branch: "main",
  source_subpath: "skills/demo",
  source_revision: "9f2c1e7a44b0c3d5e6f708192a3b4c5d6e7f8091",
  remote_revision: "9f2c1e7a44b0c3d5e6f708192a3b4c5d6e7f8091",
  update_status: "up_to_date",
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
  vi.clearAllMocks();
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

async function render(isActivePreset: boolean, target: ManagedSkill = skill) {
  await act(async () =>
    root.render(
      <SkillDetailPanel
        skill={target}
        onClose={() => {}}
        toolToggles={toggles}
        onToggleTool={onToggleTool}
        isActivePreset={isActivePreset}
      />
    )
  );
}

function row(name: string): HTMLElement {
  const element = [...document.body.querySelectorAll<HTMLElement>("[data-state]")]
    .find((el) => el.textContent?.includes(name));
  expect(element, name).toBeDefined();
  return element!;
}

function rowSwitch(name: string): HTMLButtonElement {
  return row(name).querySelector<HTMLButtonElement>("button[role=switch]")!;
}

function tab(label: string): HTMLButtonElement {
  const button = [...document.body.querySelectorAll<HTMLButtonElement>("button[role=tab]")]
    .find((b) => b.textContent === label);
  expect(button, label).toBeDefined();
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

  it("deploys a pending row from its pending pill", async () => {
    await render(true);
    const pill = [...row("claude_code").querySelectorAll("button")]
      .find((b) => b.textContent === "mySkills.agentTogglePendingSync");
    expect(pill).toBeDefined();
    await act(async () => pill!.click());
    expect(onToggleTool).toHaveBeenCalledWith("claude_code", true);
  });

  it("turns a pending row off from its switch", async () => {
    await render(true);
    expect(rowSwitch("claude_code").getAttribute("aria-checked")).toBe("true");
    await act(async () => rowSwitch("claude_code").click());
    expect(onToggleTool).toHaveBeenCalledWith("claude_code", false);
  });

  it("flips intent from the switch on off and deployed rows", async () => {
    await render(true);
    await act(async () => rowSwitch("gemini_cli").click());
    await act(async () => rowSwitch("codex").click());
    expect(onToggleTool.mock.calls).toEqual([["gemini_cli", true], ["codex", false]]);
  });

  it("does not show pending on a preset that is not active", async () => {
    await render(false);
    expect(row("claude_code").dataset.state).toBe("enabled");
    expect(document.body.textContent).not.toContain("mySkills.agentTogglePendingSync");
  });
});

describe("SkillDetailPanel source loading", () => {
  it("does not reach for the source on mount, only when its tab is opened", async () => {
    await render(true, gitSkill);
    expect(api.getSkillDocument).toHaveBeenCalledTimes(1);
    expect(api.getSourceSkillDocument).not.toHaveBeenCalled();
    expect(api.getSkillSourceDiff).not.toHaveBeenCalled();

    await act(async () => tab("mySkills.docTabs.source").click());
    expect(api.getSourceSkillDocument).toHaveBeenCalledTimes(1);
    expect(api.getSkillSourceDiff).not.toHaveBeenCalled();

    await act(async () => tab("mySkills.docTabs.diff").click());
    expect(api.getSkillSourceDiff).toHaveBeenCalledTimes(1);

    // Coming back to a tab does not fetch again.
    await act(async () => tab("mySkills.docTabs.local").click());
    await act(async () => tab("mySkills.docTabs.source").click());
    expect(api.getSourceSkillDocument).toHaveBeenCalledTimes(1);
  });

  // A check that finds a new revision (or an update) changes the version while
  // the first request may still be out; the older response must not win.
  it.each([
    {
      name: "source",
      tab: "mySkills.docTabs.source",
      fetch: () => vi.mocked(api.getSourceSkillDocument),
      bump: { remote_revision: "1b2c3d4e5f60718293a4b5c6d7e8f9012a3b4c5d" },
      payload: (label: string) => ({
        skill_id: "demo", filename: "SKILL.md", content: label, source_label: "example/skills", revision: label,
      }),
      shown: () => document.body.querySelector("[data-testid=markdown]")?.textContent,
    },
    {
      name: "diff",
      tab: "mySkills.docTabs.diff",
      fetch: () => vi.mocked(api.getSkillSourceDiff),
      bump: { remote_revision: "1b2c3d4e5f60718293a4b5c6d7e8f9012a3b4c5d" },
      payload: (label: string) => ({
        skill_id: "demo", source_label: "example/skills", revision: label,
        entries: [{ relative_path: label }],
      }),
      shown: () => document.body.querySelector("[data-testid=diff]")?.textContent,
    },
    {
      name: "files",
      tab: "mySkills.docTabs.files",
      fetch: () => vi.mocked(api.listSkillFiles),
      bump: { updated_at: 1 },
      payload: (label: string) => [{ relative_path: label, size: 1, executable: false }],
      shown: () => document.body.querySelector(".app-panel")?.textContent?.replace(/1 B$/, ""),
    },
  ])("keeps the current $name result when an older response arrives last", async (c) => {
    const pending: Array<(value: unknown) => void> = [];
    const deferred = () => new Promise<never>((resolve) => { pending.push(resolve as (value: unknown) => void); });
    c.fetch().mockImplementationOnce(deferred).mockImplementationOnce(deferred);
    const renderSkill = (target: ManagedSkill) =>
      act(async () =>
        root.render(<SkillDetailPanel skill={target} onClose={() => {}} isActivePreset={false} />)
      );

    await renderSkill(gitSkill);
    await act(async () => tab(c.tab).click());
    await renderSkill({ ...gitSkill, ...c.bump });
    expect(c.fetch()).toHaveBeenCalledTimes(2);

    await act(async () => pending[1](c.payload("new")));
    await act(async () => pending[0](c.payload("old")));
    expect(c.shown()).toBe("new");
  });

  it("lists files only when the Files tab is opened", async () => {
    await render(true, gitSkill);
    expect(api.listSkillFiles).not.toHaveBeenCalled();
    await act(async () => tab("mySkills.docTabs.files").click());
    expect(api.listSkillFiles).toHaveBeenCalledWith("demo");
  });

  it("resolves a link in an opened file against that file's folder", async () => {
    vi.mocked(api.listSkillFiles).mockResolvedValueOnce([
      { relative_path: "SKILL.md", size: 10, executable: false },
      { relative_path: "references/a.md", size: 10, executable: false },
    ]);
    vi.mocked(api.readSkillFile)
      .mockResolvedValueOnce({ relative_path: "references/a.md", content_kind: "text", text: "a" })
      .mockResolvedValueOnce({ relative_path: "SKILL.md", content_kind: "text", text: "root" });
    await render(true, gitSkill);
    await act(async () => tab("mySkills.docTabs.files").click());
    const fileRow = [...document.body.querySelectorAll<HTMLButtonElement>(".app-panel button")]
      .find((b) => b.textContent?.includes("references/a.md"));
    await act(async () => fileRow!.click());
    expect(api.readSkillFile).toHaveBeenLastCalledWith("demo", "references/a.md");

    // `../SKILL.md` written inside references/a.md is the root SKILL.md.
    await act(async () => document.body.querySelector<HTMLButtonElement>("[data-testid=relative-link]")!.click());
    expect(api.readSkillFile).toHaveBeenLastCalledWith("demo", "SKILL.md");
    expect(document.body.textContent).not.toContain("mySkills.detail.fileNotFound");
  });
});
