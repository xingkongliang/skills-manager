import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { PresetBar } from "./PresetBar";
import type { ManagedSkill, Preset } from "../lib/tauri";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    error: vi.fn(),
  },
}));

const preset = (id: string, name: string): Preset => ({
  id,
  name,
  description: null,
  icon: null,
  sort_order: 0,
  skill_count: 1,
  created_at: 0,
  updated_at: 0,
});

const skill = (id: string, presetId: string): ManagedSkill => ({
  id,
  name: id,
  description: null,
  source_type: "local",
  source_ref: null,
  source_ref_resolved: null,
  source_subpath: null,
  source_branch: null,
  source_revision: null,
  remote_revision: null,
  update_status: "unknown",
  last_checked_at: null,
  last_check_error: null,
  central_path: `C:\\skills\\${id}`,
  enabled: true,
  created_at: 0,
  updated_at: 0,
  status: "ready",
  targets: [],
  preset_ids: [presetId],
  tags: [],
});

const presets = [
  preset("preset-a", "11-软件工程-核心交付"),
  preset("preset-b", "12-软件工程-架构规划"),
];
const managedSkills = [
  skill("skill-a", "preset-a"),
  skill("skill-b", "preset-b"),
];

describe("PresetBar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("始终换行展示完整的 Preset 名称", () => {
    render(
      <PresetBar
        presets={presets}
        managedSkills={managedSkills}
        agentKeys={["codex"]}
        scopeKey="global:codex"
        existsInWorkspace={() => false}
        onApplyPreset={vi.fn().mockResolvedValue({ applied: 0, skipped: 0, failures: [] })}
        onComplete={vi.fn().mockResolvedValue(undefined)}
      />
    );

    const list = screen.getByTestId("preset-list");
    expect(list).toHaveClass("flex-wrap");
    expect(list).not.toHaveClass("overflow-x-auto");

    const fullLabel = screen.getByText("11-软件工程-核心交付");
    expect(fullLabel).toHaveClass("whitespace-nowrap");
    expect(fullLabel).not.toHaveClass("truncate");
  });

  it("显示没有成员的 Preset，但禁止执行空操作", () => {
    const emptyPreset = preset("preset-empty", "99-未分类");
    render(
      <PresetBar
        presets={[...presets, emptyPreset]}
        managedSkills={managedSkills}
        agentKeys={["codex"]}
        scopeKey="global:codex"
        existsInWorkspace={() => false}
        onApplyPreset={vi.fn().mockResolvedValue({ applied: 0, skipped: 0, failures: [] })}
        onComplete={vi.fn().mockResolvedValue(undefined)}
      />
    );

    expect(screen.getByRole("button", { name: /99-未分类/ })).toBeDisabled();
  });

  it("批量失败时展示前三项明细并把完整报告写入控制台", async () => {
    const failures = [
      { skillId: "skill-1", toolKey: "codex", message: "disk full" },
      { skillId: "skill-2", toolKey: "codex", message: "permission denied" },
      { skillId: "skill-3", toolKey: "codex", message: "source missing" },
      { skillId: "skill-4", toolKey: "codex", message: "database busy" },
    ];
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);

    render(
      <PresetBar
        presets={presets}
        managedSkills={managedSkills}
        agentKeys={["codex"]}
        scopeKey="global:codex"
        existsInWorkspace={() => false}
        onApplyPreset={vi.fn().mockResolvedValue({
          applied: 0,
          skipped: 0,
          failures,
        })}
        onComplete={vi.fn().mockResolvedValue(undefined)}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /11-软件工程-核心交付/ }));

    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith(
        "presetActions.partialFailedToast",
        expect.objectContaining({
          description: expect.stringContaining("skill-1 · codex: disk full"),
        })
      );
    });
    const toastOptions = vi.mocked(toast.error).mock.calls[0]?.[1] as
      | { description?: string }
      | undefined;
    expect(toastOptions?.description).toContain("skill-2 · codex: permission denied");
    expect(toastOptions?.description).toContain("skill-3 · codex: source missing");
    expect(toastOptions?.description).not.toContain("skill-4 · codex: database busy");
    expect(consoleError).toHaveBeenCalledWith(
      "[PresetBar] Preset apply failures",
      failures
    );
  });

  it("连续点击不同 Preset 时按 FIFO 执行，且不禁用未入队项", async () => {
    let resolveFirst!: () => void;
    const first = new Promise<void>((resolve) => {
      resolveFirst = resolve;
    });
    const callOrder: string[] = [];
    const onApplyPreset = vi.fn(async (selected: Preset) => {
      callOrder.push(`start:${selected.id}`);
      if (selected.id === "preset-a") await first;
      callOrder.push(`end:${selected.id}`);
      return { applied: 1, skipped: 0, failures: [] };
    });
    const onComplete = vi.fn().mockResolvedValue(undefined);

    render(
      <PresetBar
        presets={presets}
        managedSkills={managedSkills}
        agentKeys={["codex"]}
        scopeKey="global:codex"
        existsInWorkspace={() => false}
        onApplyPreset={onApplyPreset}
        onComplete={onComplete}
      />
    );

    const firstButton = screen.getByRole("button", { name: /11-软件工程-核心交付/ });
    const secondButton = screen.getByRole("button", { name: /12-软件工程-架构规划/ });
    fireEvent.click(firstButton);

    expect(firstButton).toBeDisabled();
    expect(secondButton).not.toBeDisabled();

    fireEvent.click(secondButton);
    expect(secondButton).toBeDisabled();
    expect(onApplyPreset).toHaveBeenCalledTimes(1);

    resolveFirst();

    await waitFor(() => expect(onApplyPreset).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(onComplete).toHaveBeenCalledTimes(1));
    expect(callOrder).toEqual([
      "start:preset-a",
      "end:preset-a",
      "start:preset-b",
      "end:preset-b",
    ]);
  });

  it("同一个 Preset 在队列中时忽略重复点击", async () => {
    let resolveApply!: () => void;
    const applying = new Promise<void>((resolve) => {
      resolveApply = resolve;
    });
    const onApplyPreset = vi.fn(async () => {
      await applying;
      return { applied: 1, skipped: 0, failures: [] };
    });

    render(
      <PresetBar
        presets={presets}
        managedSkills={managedSkills}
        agentKeys={["codex"]}
        scopeKey="global:codex"
        existsInWorkspace={() => false}
        onApplyPreset={onApplyPreset}
        onComplete={vi.fn().mockResolvedValue(undefined)}
      />
    );

    const button = screen.getByRole("button", { name: /11-软件工程-核心交付/ });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(onApplyPreset).toHaveBeenCalledTimes(1);

    resolveApply();
    await waitFor(() => expect(button).not.toBeDisabled());
    expect(onApplyPreset).toHaveBeenCalledTimes(1);
  });

  it("切换 Agent 后允许同名 Preset 以新作用域入队，并使用新 Agent 的回调", async () => {
    let resolveCodex1!: () => void;
    const codex1Pending = new Promise<void>((resolve) => {
      resolveCodex1 = resolve;
    });
    const applyCodex1 = vi.fn(async () => {
      await codex1Pending;
      return { applied: 1, skipped: 0, failures: [] };
    });
    const applyCodex2 = vi.fn().mockResolvedValue({
      applied: 1,
      skipped: 0,
      failures: [],
    });
    const completeCodex1 = vi.fn().mockResolvedValue(undefined);
    const completeCodex2 = vi.fn().mockResolvedValue(undefined);

    const { rerender } = render(
      <PresetBar
        presets={presets}
        managedSkills={managedSkills}
        agentKeys={["codex"]}
        scopeKey="global:codex"
        existsInWorkspace={() => false}
        onApplyPreset={applyCodex1}
        onComplete={completeCodex1}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: /11-软件工程-核心交付/ }));

    rerender(
      <PresetBar
        presets={presets}
        managedSkills={managedSkills}
        agentKeys={["codex_2"]}
        scopeKey="global:codex_2"
        existsInWorkspace={() => false}
        onApplyPreset={applyCodex2}
        onComplete={completeCodex2}
      />
    );
    const codex2Button = screen.getByRole("button", { name: /11-软件工程-核心交付/ });
    expect(codex2Button).not.toBeDisabled();
    fireEvent.click(codex2Button);

    resolveCodex1();

    await waitFor(() => expect(applyCodex2).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(completeCodex2).toHaveBeenCalledTimes(1));
    expect(applyCodex1).toHaveBeenCalledTimes(1);
    expect(completeCodex1).not.toHaveBeenCalled();
  });

  it("切换 Agent 后即使没有新操作也只刷新当前页面", async () => {
    let resolveCodex1!: () => void;
    const codex1Pending = new Promise<void>((resolve) => {
      resolveCodex1 = resolve;
    });
    const applyCodex1 = vi.fn(async () => {
      await codex1Pending;
      return { applied: 1, skipped: 0, failures: [] };
    });
    const completeCodex1 = vi.fn().mockResolvedValue(undefined);
    const completeCodex2 = vi.fn().mockResolvedValue(undefined);

    const { rerender } = render(
      <PresetBar
        presets={presets}
        managedSkills={managedSkills}
        agentKeys={["codex"]}
        scopeKey="global:codex"
        existsInWorkspace={() => false}
        onApplyPreset={applyCodex1}
        onComplete={completeCodex1}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: /11-软件工程-核心交付/ }));

    rerender(
      <PresetBar
        presets={presets}
        managedSkills={managedSkills}
        agentKeys={["codex_2"]}
        scopeKey="global:codex_2"
        existsInWorkspace={() => false}
        onApplyPreset={vi.fn().mockResolvedValue({ applied: 1, skipped: 0, failures: [] })}
        onComplete={completeCodex2}
      />
    );
    resolveCodex1();

    await waitFor(() => expect(completeCodex2).toHaveBeenCalledTimes(1));
    expect(completeCodex1).not.toHaveBeenCalled();
  });
});
