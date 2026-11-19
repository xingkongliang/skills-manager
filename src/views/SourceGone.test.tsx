// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ManagedSkill, MoveCandidate, UpdateSkillResult } from "../lib/tauri";
import type { ScanItem } from "../lib/sourceRepair";
import { SourceGone } from "./SourceGone";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts ? `${key}${JSON.stringify(opts)}` : key,
  }),
}));

vi.mock("react-router-dom", () => ({ useNavigate: () => vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), info: vi.fn(), error: vi.fn() },
}));

const api = vi.hoisted(() => ({
  getManagedSkills: vi.fn(),
  updateSkill: vi.fn(),
  repointSkillSource: vi.fn(),
  scanGitSource: vi.fn(),
  detachSkillSource: vi.fn(),
  deleteManagedSkills: vi.fn(),
}));
vi.mock("../lib/tauri", () => api);

const { toast } = await import("sonner");

const CLONE = "https://github.com/acme/skills.git";

function skill(overrides: Partial<ManagedSkill> = {}): ManagedSkill {
  return {
    id: "b",
    name: "b",
    description: null,
    source_type: "git",
    source_ref: "https://github.com/acme/skills",
    source_ref_resolved: CLONE,
    source_subpath: "skills/b",
    source_branch: null,
    source_revision: "a".repeat(40),
    remote_revision: "a".repeat(40),
    update_status: "source_gone",
    last_checked_at: 1,
    last_check_error: null,
    central_path: "/central/skills/b",
    enabled: true,
    created_at: 1,
    updated_at: 1,
    status: "ok",
    targets: [],
    preset_ids: [],
    tags: [],
    ...overrides,
  };
}

function scanItem(overrides: Partial<ScanItem> = {}): ScanItem {
  return {
    skill_id: "b",
    placement: "at",
    candidates: [],
    local_changed: false,
    installed_revision_present: true,
    ...overrides,
  };
}

function updateResult(overrides: Partial<UpdateSkillResult> = {}): UpdateSkillResult {
  return {
    skill: skill(),
    content_changed: true,
    pending_removals: [],
    removal_approval: null,
    ...overrides,
  };
}

let container: HTMLDivElement;
let root: Root;

async function render(skills: ManagedSkill[], items: ScanItem[] = [scanItem()]) {
  api.getManagedSkills.mockResolvedValue(skills);
  api.scanGitSource.mockResolvedValue({
    clone_url: CLONE,
    branch: null,
    revision: "a".repeat(40),
    error: null,
    items,
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root.render(<SourceGone />);
  });
  // A scan is what puts a row in the "at" group, which is where the update
  // action lives; the page never scans on its own.
  await click("mySkills.repair.scan");
}

function button(label: string): HTMLButtonElement {
  const match = [...container.querySelectorAll("button")].find((candidate) =>
    candidate.textContent?.startsWith(label)
  );
  if (!match) throw new Error(`no button starting with ${label}`);
  return match;
}

async function click(label: string) {
  await act(async () => {
    button(label).dispatchEvent(new MouseEvent("click", { bubbles: true }));
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("SourceGone actions", () => {
  it("re-points against the branch the Skill is tracked on", async () => {
    const candidate: MoveCandidate = {
      subpath: "other/b",
      evidence: "content_hash",
    };
    api.repointSkillSource.mockResolvedValue({
      skill_id: "b",
      subpath: "other/b",
      content_changed: false,
    });

    await render([skill()], [scanItem({ placement: "moved", candidates: [candidate] })]);
    await click("mySkills.repair.repoint");

    expect(api.repointSkillSource).toHaveBeenCalledWith(
      "b",
      CLONE,
      "other/b",
      null,
      true
    );
  });

  it("keeps a tracked non-default branch when re-pointing", async () => {
    const candidate: MoveCandidate = {
      subpath: "other/b",
      evidence: "content_hash",
    };
    api.repointSkillSource.mockResolvedValue({
      skill_id: "b",
      subpath: "other/b",
      content_changed: false,
    });

    await render(
      [skill({ source_branch: "dev" })],
      [scanItem({ placement: "moved", candidates: [candidate] })]
    );
    await click("mySkills.repair.repoint");

    expect(api.repointSkillSource).toHaveBeenCalledWith(
      "b",
      CLONE,
      "other/b",
      "dev",
      true
    );
  });

  it("asks before removing files, and claims nothing when it holds", async () => {
    api.updateSkill.mockResolvedValue(
      updateResult({
        content_changed: false,
        pending_removals: [
          { path: "old.md", location: "library" },
          { path: "gone.md", location: "target" },
        ],
        removal_approval: "approve-me",
      })
    );

    await render([skill()]);
    await click("mySkills.repair.update");

    expect(api.updateSkill).toHaveBeenCalledWith("b", undefined);
    expect(toast.success).not.toHaveBeenCalled();
    expect(container.textContent).toContain("mySkills.updateActions.removalTitle");
    expect(container.textContent).toContain("old.md");
    expect(container.textContent).toContain("gone.md");
  });

  it("runs the held update with the approval the dialog carries", async () => {
    api.updateSkill
      .mockResolvedValueOnce(
        updateResult({
          content_changed: false,
          pending_removals: [{ path: "old.md", location: "library" }],
          removal_approval: "approve-me",
        })
      )
      .mockResolvedValueOnce(updateResult({ content_changed: true }));

    await render([skill()]);
    await click("mySkills.repair.update");
    await click("mySkills.updateActions.removalConfirm");

    expect(api.updateSkill).toHaveBeenLastCalledWith("b", "approve-me");
    expect(toast.success).toHaveBeenCalledWith(
      expect.stringContaining("mySkills.repair.updated")
    );
  });

  it("says already up to date when the update changed nothing", async () => {
    api.updateSkill.mockResolvedValue(updateResult({ content_changed: false }));

    await render([skill()]);
    await click("mySkills.repair.update");

    expect(toast.success).not.toHaveBeenCalled();
    expect(toast.info).toHaveBeenCalledWith("mySkills.updateActions.alreadyUpToDate");
  });
});
