import type { Page } from "@playwright/test";

import { preset, skill } from "../fake-backend/state";
import { expect, test } from "../fixtures";

// The library skill detail drawer: closing it, and adding the skill to a preset from it.

const seed = {
  presets: [preset("p1", "Writing", 0)],
  activePresetId: "p1",
  skills: [skill("s1", "alpha", { source_ref: null })],
};

const drawer = (page: Page) => page.getByRole("dialog", { name: "alpha" });

async function openDetail(page: Page) {
  await page.getByRole("heading", { name: "alpha", exact: true, level: 3 }).click();
  await expect(drawer(page)).toBeVisible();
}

test.beforeEach(async ({ page, backend }) => {
  await backend.seed(seed);
  await page.goto("/my-skills");
});

test("detail contains keyboard focus and returns it on close", async ({ page }) => {
  const trigger = page.getByRole("button", { name: "alpha", exact: true });
  await trigger.press("Enter");
  await expect(drawer(page).getByRole("button", { name: "Close", exact: true })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect
    .poll(() => drawer(page).evaluate((element) => element.contains(document.activeElement)))
    .toBe(true);
  await page.keyboard.press("Tab");
  await expect(drawer(page).getByRole("button", { name: "Close", exact: true })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(drawer(page)).toHaveCount(0);
  await expect(trigger).toBeFocused();

  await openDetail(page);
  await drawer(page).getByRole("button", { name: "Close" }).click();
  await expect(drawer(page)).toHaveCount(0);
});

test("adding the skill to a preset from the detail shows its agents", async ({ page, backend }) => {
  await openDetail(page);
  await expect(drawer(page).getByText("Global Workspace")).toHaveCount(0);

  const toggle = drawer(page).getByRole("switch", { name: "Writing" });
  await expect(toggle).not.toBeChecked();
  await toggle.click();

  await expect(toggle).toBeChecked();
  expect(await backend.calls("add_skill_to_preset")).toEqual([{ skillId: "s1", presetId: "p1" }]);
  await expect(drawer(page).getByText("Global Workspace")).toBeVisible();
});
