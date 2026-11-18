import type { RemoteHost } from "../../src/lib/tauri";
import { project } from "../fake-backend/state";
import { expect, test } from "../fixtures";

const remote: RemoteHost = {
  id: "dev",
  name: "Build host",
  ssh_target: "dev@build-host",
  cli_path: null,
  created_at: 1,
};

test("arrow navigation keeps focus on the current resource tab in each workspace", async ({
  page,
  backend,
}) => {
  await backend.seed({ projects: [project("repo", "Repository", 0)] });

  for (const url of ["/my-skills", "/project/repo"]) {
    await page.goto(url);
    const tabs = page.getByRole("tablist", { name: "Resource type" });
    await tabs.getByRole("tab", { name: "Skills", exact: true }).press("ArrowRight");
    await expect(tabs.getByRole("tab", { name: "Instructions", selected: true })).toBeFocused();
    await page.keyboard.press("ArrowRight");
    await expect(tabs.getByRole("tab", { name: "MCPs", selected: true })).toBeFocused();
  }
});

test("the remote folder picker returns focus to Browse before the project dialog closes", async ({
  page,
  backend,
}) => {
  await backend.seed({ remoteHosts: [remote] });
  await page.goto("/my-skills");
  await page.getByRole("button", { name: "Local", exact: true }).click();
  await page.getByRole("menuitemradio", { name: /Build host/ }).click();
  const trigger = page.getByRole("button", { name: "Link Project", exact: true });
  await trigger.press("Enter");
  const project = page.getByRole("dialog", { name: "Link Project Workspace" });
  const close = project.getByRole("button", { name: "Close", exact: true });
  await expect(close).toBeFocused();
  await close.dispatchEvent("keydown", { key: "Unidentified", code: "Tab", shiftKey: true });
  await expect(project.getByRole("button", { name: "Select Repository Directory" })).toBeFocused();
  await project.getByRole("button", { name: "Scan", exact: true }).click();
  const browse = project.getByRole("button", { name: "Browse for scan directory" });
  await browse.press("Enter");

  const picker = page.getByRole("dialog", { name: "Choose a folder on Build host" });
  await expect(picker.getByRole("button", { name: "Cancel" }).first()).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect
    .poll(() => picker.evaluate((element) => element.contains(document.activeElement)))
    .toBe(true);
  await page.keyboard.press("Escape");
  await expect(picker).toHaveCount(0);
  await expect(browse).toBeFocused();

  await page.keyboard.press("Escape");
  await expect(project).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

test("project scan ignores composing Enter and submits once while pending", async ({
  page,
  backend,
}) => {
  await page.goto("/my-skills");
  await page.getByRole("button", { name: "Link Project", exact: true }).press("Enter");
  const project = page.getByRole("dialog", { name: "Link Project Workspace" });
  await project.getByRole("button", { name: "Scan", exact: true }).click();
  const root = project.getByRole("textbox", { name: "Root directory to scan" });
  await root.fill("/home/e2e/work");
  await root.dispatchEvent("keydown", { key: "Enter", isComposing: true });
  expect(await backend.calls("scan_projects")).toEqual([]);

  await backend.holdResponse("scan_projects");
  await root.press("Enter");
  await expect(project.getByRole("button", { name: "Scanning..." })).toBeDisabled();
  await root.press("Enter");
  expect(await backend.calls("scan_projects")).toEqual([{ root: "/home/e2e/work" }]);
  await backend.releaseResponse("scan_projects");
  await expect(project.getByText("No projects with agent skills directories found")).toBeVisible();
});
