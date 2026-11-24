import { beforeEach, describe, expect, it, vi } from "vitest";
import { warnDuplicateDeployments } from "./duplicateDeployments";

const mocks = vi.hoisted(() => ({
  warning: vi.fn(),
  findDuplicateDeployments: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: { warning: mocks.warning } }));
vi.mock("./tauri", () => ({ findDuplicateDeployments: mocks.findDuplicateDeployments }));
vi.mock("../i18n", () => ({
  default: {
    language: "en",
    t: (key: string, opts?: Record<string, unknown>) => `${key}${JSON.stringify(opts)}`,
  },
}));

beforeEach(() => vi.clearAllMocks());

describe("warnDuplicateDeployments", () => {
  it("names the agents and skills that are now seen twice", async () => {
    mocks.findDuplicateDeployments.mockResolvedValue([
      {
        skill_id: "s1",
        skill_name: "my-skill",
        agent: "codex",
        agent_name: "Codex",
        dirs: ["/Users/me/.codex/skills", "/Users/me/.agents/skills"],
      },
    ]);
    await warnDuplicateDeployments(["s1"], ["codex"]);
    expect(mocks.findDuplicateDeployments).toHaveBeenCalledWith(["s1"], ["codex"]);
    expect(mocks.warning).toHaveBeenCalledTimes(1);
    const [message, options] = mocks.warning.mock.calls[0];
    expect(message).toContain('"agents":"Codex"');
    expect(message).toContain('"skills":"my-skill"');
    expect(options.description).toBe("~/.codex/skills · ~/.agents/skills");
  });

  it("stays quiet when nothing is duplicated or the check fails", async () => {
    mocks.findDuplicateDeployments.mockResolvedValueOnce([]);
    await warnDuplicateDeployments(["s1"], ["codex"]);
    vi.spyOn(console, "warn").mockImplementation(() => {});
    mocks.findDuplicateDeployments.mockRejectedValueOnce(new Error("boom"));
    await warnDuplicateDeployments(["s1"], ["codex"]);
    await warnDuplicateDeployments([], ["codex"]);
    expect(mocks.warning).not.toHaveBeenCalled();
    expect(mocks.findDuplicateDeployments).toHaveBeenCalledTimes(2);
  });
});
