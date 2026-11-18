import type { Page } from "@playwright/test";

import { agentTarget, project, projectSkill, skill } from "../fake-backend/state";
import { expect, test } from "../fixtures";

// F2 + F4: the project page: grid and list, the detail panel, the update
// to/from center buttons following each skill's sync status, and the agents
// last used to add skills, remembered per project.

const targets = [agentTarget("claude_code", "Claude Code"), agentTarget("codex", "Codex")];

const seed = {
  skills: [skill("s1", "deploy")],
  projects: [project("p1", "webapp", 0), project("p2", "backend", 1)],
  projectAgentTargets: { p1: targets, p2: targets },
  projectSkills: {
    p1: [
      projectSkill("deploy", "claude_code", {
        sync_status: "center_newer",
        in_center: true,
        center_skill_id: "s1",
      }),
      projectSkill("format", "claude_code", { sync_status: "in_sync", in_center: true }),
      projectSkill("lint-rules", "claude_code", { sync_status: "project_only" }),
    ],
  },
};

const skillHeading = (page: Page, name: string) =>
  page.getByRole("heading", { name, exact: true, level: 3 });

/** The innermost element holding both the skill's name and its delete button: its card or row. */
const skillItem = (page: Page, name: string) =>
  page
    .locator("div")
    .filter({ has: skillHeading(page, name) })
    .filter({ has: page.getByRole("button", { name: "Delete skill" }) })
    .last();

async function topOf(page: Page, name: string) {
  return (await skillHeading(page, name).boundingBox())!.y;
}

test.beforeEach(async ({ page, backend }) => {
  await backend.seed(seed);
  await page.goto("/project/p1");
  await page.getByRole("button", { name: "Grid view", exact: true }).click();
  await expect(page.getByRole("heading", { name: "webapp", level: 1 })).toBeVisible();
  await expect(page.getByRole("heading", { level: 3 })).toHaveText([
    "deploy",
    "format",
    "lint-rules",
  ]);
});

test("switches between grid and list", async ({ page }) => {
  expect(await topOf(page, "format")).toBe(await topOf(page, "deploy"));

  await page.getByTestId("view-list").click();
  await expect
    .poll(async () => (await topOf(page, "format")) > (await topOf(page, "deploy")))
    .toBe(true);

  await page.getByTestId("view-grid").click();
  await expect
    .poll(async () => (await topOf(page, "format")) === (await topOf(page, "deploy")))
    .toBe(true);
});

test("opens a skill's detail panel with its SKILL.md", async ({ page }) => {
  await skillHeading(page, "format").click();

  await expect(page.getByRole("heading", { name: "format", level: 2 })).toBeVisible();
  await expect(page.getByText("Project copy.")).toBeVisible();
});

test("Add from Library starts in search and keeps focus inside the sheet", async ({ page }) => {
  const trigger = page.getByRole("button", { name: "Add Skill", exact: true });
  await trigger.press("Enter");
  const sheet = page.getByRole("dialog", { name: "Add from Library" });
  await expect(sheet.getByRole("textbox", { name: "Search the library…" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(sheet.getByRole("button", { name: "Close", exact: true })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect
    .poll(() => sheet.evaluate((element) => element.contains(document.activeElement)))
    .toBe(true);
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  await expect(trigger).toBeFocused();
});

test("offers only the update the project status allows", async ({ page }) => {
  await expect(skillItem(page, "lint-rules")).toContainText("Project only");
  await expect(
    skillItem(page, "lint-rules").getByRole("button", { name: "Update Center" }),
  ).toBeVisible();
  await expect(
    skillItem(page, "lint-rules").getByRole("button", { name: "Update Project" }),
  ).toHaveCount(0);

  await expect(skillItem(page, "deploy")).toContainText("Update available");
  await expect(
    skillItem(page, "deploy").getByRole("button", { name: "Update Project" }),
  ).toBeVisible();
  await expect(
    skillItem(page, "deploy").getByRole("button", { name: "Update Center" }),
  ).toHaveCount(0);

  await expect(skillItem(page, "format")).toContainText("Up to date");
  await expect(skillItem(page, "format").getByRole("button", { name: /^Update/ })).toHaveCount(0);
});

test("updating the center brings the skill up to date", async ({ page, backend }) => {
  await skillItem(page, "lint-rules").getByRole("button", { name: "Update Center" }).click();

  await expect(page.getByText('"lint-rules" updated to Skills Center')).toBeVisible();
  expect(await backend.calls("update_project_skill_to_center")).toEqual([
    { projectId: "p1", skillRelativePath: "lint-rules", agent: "claude_code" },
  ]);
  await expect(skillItem(page, "lint-rules")).toContainText("Up to date");
  await expect(skillItem(page, "lint-rules").getByRole("button", { name: /^Update/ })).toHaveCount(
    0,
  );
});

test("updating the project brings the skill up to date", async ({ page, backend }) => {
  await skillItem(page, "deploy").getByRole("button", { name: "Update Project" }).click();

  await expect(page.getByRole("region", { name: "Project update progress" })).toContainText(
    "1 updated",
  );

  const submissions = (await backend.calls("operation_request")).filter(
    (call) => call.request.action === "submit_project_update",
  );

  expect(submissions).toHaveLength(1);
  expect(submissions[0]).toMatchObject({ request: { project_id: "p1", skill_paths: ["deploy"] } });
  await expect(skillItem(page, "deploy")).toContainText("Up to date");
  await expect(skillItem(page, "deploy").getByRole("button", { name: /^Update/ })).toHaveCount(0);
});

test("remembers the agents last used to add skills, per project", async ({ page, backend }) => {
  const agentPill = (name: string) =>
    page.getByRole("button", { name, exact: true }).and(page.locator("[aria-pressed]"));

  // The sheet takes its first selection from the project's agents and its
  // remembered ones, so open it once both have loaded.
  const openSheet = async (projectId: string) => {
    await expect(page.getByRole("button", { name: "Settings", exact: true })).toBeEnabled();
    await expect
      .poll(() => backend.calls("get_settings"))
      .toContainEqual({ key: `project_last_used_export_agents:${projectId}` });
    await page.getByRole("button", { name: "Add Skill", exact: true }).click();
  };

  await openSheet("p1");
  await expect(agentPill("Claude Code")).toHaveAttribute("aria-pressed", "true");
  await expect(agentPill("Codex")).toHaveAttribute("aria-pressed", "true");
  await agentPill("Codex").click();
  await expect(agentPill("Codex")).toHaveAttribute("aria-pressed", "false");
  // toContainEqual: in dev, StrictMode runs the state updater that saves it twice.
  expect(await backend.calls("set_settings")).toContainEqual({
    key: "project_last_used_export_agents:p1",
    value: JSON.stringify(["claude_code"]),
  });

  await page.reload();
  await openSheet("p1");
  await expect(agentPill("Claude Code")).toHaveAttribute("aria-pressed", "true");
  await expect(agentPill("Codex")).toHaveAttribute("aria-pressed", "false");

  await page.goto("/project/p2");
  await openSheet("p2");
  await expect(agentPill("Claude Code")).toHaveAttribute("aria-pressed", "true");
  await expect(agentPill("Codex")).toHaveAttribute("aria-pressed", "true");
});
