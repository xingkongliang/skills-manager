// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi, type Mock } from "vitest";
import { AgentBuddySwitchDialog } from "./AgentBuddySwitchDialog";
import type { ManagedSkill } from "../lib/tauri";
import type { MarketSkill } from "../byte/views/installSkills/frontend";

const mocks = vi.hoisted(() => ({
  fetch: vi.fn(),
  open: vi.fn(),
  cancel: vi.fn(),
  t: (key: string) => key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));
vi.mock("../byte/views/installSkills/frontend", () => ({ fetchSkills: mocks.fetch }));
vi.mock("../lib/tauri", () => ({ cancelInstall: mocks.cancel }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: mocks.open }));
const skill = { id: "legacy", name: "demo", source_type: "local" } as ManagedSkill;
const candidate = (repository: string): MarketSkill => ({
  id: repository,
  identifier: "skills:team/demo",
  repository,
  name: "demo",
  description: "",
  version: "2.0",
  downloads: 0,
  download_total: 0,
  download_biweekly: 0,
  download_monthly: 0,
  disabled: false,
  no_permission: false,
  url: "https://skills.bytedance.net/skill/skills:team/demo",
});
let container: HTMLDivElement;
let root: Root;
let close: Mock<() => void>;
let apply: Mock<(target: MarketSkill) => Promise<void>>;
beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  close = vi.fn();
  apply = vi.fn().mockResolvedValue(undefined);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});
async function render() {
  await act(async () =>
    root.render(<AgentBuddySwitchDialog skill={skill} onClose={close} onSwitch={apply} />),
  );
}
function switchButton() {
  return [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "mySkills.agentBuddy.switch",
  )!;
}

it("preselects a unique exact name but waits for an explicit switch", async () => {
  const target = candidate("internal");
  mocks.fetch.mockResolvedValue({ skills: [target], page: 1, page_size: 100, total: 1 });
  await render();
  expect(container.querySelector<HTMLInputElement>('input[type="radio"]')!.checked).toBe(true);
  expect(apply).not.toHaveBeenCalled();
  await act(async () => switchButton().click());
  expect(apply).toHaveBeenCalledWith(target);
  expect(close).toHaveBeenCalledOnce();
});

it("checks every page and requires selection when another same name is found", async () => {
  mocks.fetch
    .mockResolvedValueOnce({ skills: [candidate("internal")], page: 1, page_size: 1, total: 2 })
    .mockResolvedValueOnce({ skills: [candidate("public")], page: 2, page_size: 1, total: 2 });
  await render();
  expect(mocks.fetch).toHaveBeenCalledTimes(2);
  expect(container.querySelectorAll('input[type="radio"]:checked')).toHaveLength(0);
  expect(switchButton().disabled).toBe(true);
  expect(apply).not.toHaveBeenCalled();
});

it("leaves the original skill alone when no match exists or switching fails", async () => {
  mocks.fetch.mockResolvedValue({ skills: [], page: 1, page_size: 100, total: 0 });
  await render();
  expect(container.textContent).toContain("mySkills.agentBuddy.noMatch");
  expect(switchButton().disabled).toBe(true);
  expect(apply).not.toHaveBeenCalled();
  mocks.fetch.mockResolvedValue({
    skills: [candidate("internal")],
    page: 1,
    page_size: 100,
    total: 1,
  });
  await act(async () =>
    container
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  apply.mockRejectedValue(new Error("download failed"));
  await act(async () => switchButton().click());
  expect(container.querySelector('[role="alert"]')!.textContent).toContain("download failed");
  expect(close).not.toHaveBeenCalled();
});

it("cancels an in-flight conversion without closing or committing it", async () => {
  mocks.fetch.mockResolvedValue({
    skills: [candidate("internal")],
    page: 1,
    page_size: 100,
    total: 1,
  });
  let rejectApply: (error: Error) => void = () => {};
  apply.mockImplementation(
    () =>
      new Promise<void>((_, reject) => {
        rejectApply = reject;
      }),
  );
  mocks.cancel.mockImplementation(async () => {
    rejectApply(new Error("cancelled"));
    return true;
  });
  await render();
  await act(async () => switchButton().click());
  const cancel = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === "common.cancel",
  )!;
  await act(async () => cancel.click());
  expect(mocks.cancel).toHaveBeenCalledWith("update:legacy");
  expect(container.querySelector('[role="alert"]')!.textContent).toContain("cancelled");
  expect(close).not.toHaveBeenCalled();
});
