// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RepositorySkillActions, type RepositoryDeploymentProps } from "./RepositorySkills";
import type { ManagedSkill } from "../lib/tauri";

const messages = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string, options?: object) => `${key}${options ? JSON.stringify(options) : ""}` }) }));
vi.mock("sonner", () => ({ toast: messages }));

const members = ["a", "b", "c"].map((id): ManagedSkill => ({
  id, name: id, description: null, source_type: "git", source_ref: "owner/repo",
  source_ref_resolved: null, source_subpath: id, source_branch: null, source_revision: null,
  remote_revision: null, update_status: "up_to_date", last_checked_at: null, last_check_error: null,
  central_path: `/fixture/${id}`, enabled: true, created_at: 0, updated_at: 0, status: "ok",
  targets: [], preset_ids: [], tags: [],
}));

let container: HTMLDivElement;
let root: Root;
let props: RepositoryDeploymentProps;
beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  props = {
    existsInWorkspace: (skill, agent) => skill.id === "a" && agent === "codex",
    onApply: vi.fn().mockResolvedValue(undefined), onComplete: vi.fn().mockResolvedValue(undefined),
  };
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});
const button = (suffix: string) => Array.from(container.querySelectorAll("button")).find((item) => item.textContent === `repositoryGroups.${suffix}`)!;
const render = async () => act(async () => root.render(<RepositorySkillActions {...props} skills={members} agentKey="codex" />));

describe("repository batch controls", () => {
  it("shows separate enable and disable actions for partial state and submits the missing subset in one batch", async () => {
    await render();
    expect(container.querySelector('[role="status"]')?.textContent).toContain('{"enabled":1,"total":3}');
    expect(button("disableAll")).toBeDefined();
    await act(async () => button("enableAll").click());
    expect(props.onApply).toHaveBeenCalledExactlyOnceWith([members[1], members[2]], "codex", true);
    expect(props.onComplete).toHaveBeenCalledOnce();
  });

  it("disables only actual copies for the selected Agent without changing other agents", async () => {
    await render();
    await act(async () => button("disableAll").click());
    expect(props.onApply).toHaveBeenCalledExactlyOnceWith([members[0]], "codex", false);
  });

  it("refreshes actual state and surfaces a protected batch refusal", async () => {
    vi.mocked(props.onApply).mockRejectedValue(new Error("Target is externally managed"));
    await render();
    await act(async () => button("enableAll").click());
    expect(messages.error).toHaveBeenCalledWith("Target is externally managed");
    expect(messages.success).not.toHaveBeenCalled();
    expect(props.onComplete).toHaveBeenCalledOnce();
  });

  it("supports confirmed upstream members before any skills are imported", async () => {
    const onEnableAll = vi.fn().mockResolvedValue(undefined);
    await act(async () => root.render(<RepositorySkillActions {...props} skills={[]} availableCount={4} agentKey="codex" onEnableAll={onEnableAll} />));
    expect(container.querySelector('[role="status"]')?.textContent).toContain('{"enabled":0,"total":4}');
    await act(async () => button("enableAll").click());
    expect(onEnableAll).toHaveBeenCalledOnce();
    expect(props.onApply).not.toHaveBeenCalled();
  });

  it("does not apply a batch after removal is cancelled", async () => {
    props.confirmRemove = vi.fn().mockResolvedValue(false);
    await render();
    await act(async () => button("disableAll").click());
    expect(props.onApply).not.toHaveBeenCalled();
  });
});
