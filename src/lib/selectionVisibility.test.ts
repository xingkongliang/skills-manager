import i18next from "i18next";
import { describe, expect, it } from "vitest";
import en from "../i18n/en.json";
import zh from "../i18n/zh.json";
import zhTW from "../i18n/zh-TW.json";
import { countHiddenSelected, hiddenSelectionTexts } from "./selectionVisibility";

const ids = (...values: string[]) => new Set(values);

describe("countHiddenSelected", () => {
  it("hides nothing when there is no selection", () => {
    expect(countHiddenSelected(ids(), [])).toBe(0);
    expect(countHiddenSelected(ids(), ["a", "b"])).toBe(0);
  });

  it("hides nothing when every selected skill is rendered", () => {
    expect(countHiddenSelected(ids("a", "b"), ["a", "b", "c"])).toBe(0);
  });

  it("counts the skills of a collapsed source swept up by select-all", () => {
    // Three skills selected, only the one in the open source is rendered.
    expect(countHiddenSelected(ids("a", "b", "c"), ["a"])).toBe(2);
  });

  it("counts a source collapsed after its skills were picked", () => {
    // Same arithmetic, different cause: the selection was made row by row.
    const picked = ids("b", "c");
    expect(countHiddenSelected(picked, ["a"])).toBe(2);
  });

  it("hides nothing when grouping is off", () => {
    // Ungrouped, every filtered skill has a row.
    expect(countHiddenSelected(ids("a", "b", "c"), ["a", "b", "c"])).toBe(0);
  });

  it("hides nothing while a search forces every source open", () => {
    expect(countHiddenSelected(ids("a", "b"), ["a", "b", "c"])).toBe(0);
  });

  it("treats an id with no row as hidden, leaving pruning to the caller", () => {
    // `useMultiSelect` drops ids whose skill is gone before this runs, so a
    // missing row here can only mean a display reason, which is what we count.
    expect(countHiddenSelected(ids("gone"), [])).toBe(1);
  });

  it("counts each selected skill once even if a row repeats", () => {
    expect(countHiddenSelected(ids("a", "b"), ["a", "a"])).toBe(1);
  });
});

describe("hiddenSelectionTexts", () => {
  // The real strings: what is under test is which keys are used and that the
  // wording names no reason, so echoing keys back would prove nothing.
  async function textsIn(lng: string, selected: number, hidden: number) {
    const i18n = i18next.createInstance();
    await i18n.init({
      lng,
      resources: { en: { translation: en }, zh: { translation: zh }, "zh-TW": { translation: zhTW } },
      interpolation: { escapeValue: false },
    });
    return hiddenSelectionTexts(i18n.t.bind(i18n), selected, hidden);
  }

  it("says nothing extra when every selected skill is on screen", async () => {
    const texts = await textsIn("en", 3, 0);
    expect(texts.toolbarNote).toBeUndefined();
    expect(texts.deleteConfirm).toBe(
      "Delete 3 selected skills permanently? This removes their central library files, preset links, and synced agent targets."
    );
  });

  it("states how many are not shown without claiming why", async () => {
    // A filter can hide a selected skill as well as a collapsed Source.
    const en = await textsIn("en", 2, 2);
    expect(en.toolbarNote).toBe("(2 not shown)");
    expect(en.deleteConfirm).toBe(
      "Delete 2 selected skills permanently, including 2 not shown in the list? This removes their central library files, preset links, and synced agent targets."
    );

    const zh = await textsIn("zh", 5, 3);
    expect(zh.toolbarNote).toBe("（其中 3 个未显示）");
    expect(zh.deleteConfirm).toBe(
      "确定彻底删除选中的 5 个 Skill（其中 3 个未显示）吗？中央仓库文件、Preset 关联和已同步到各 Agent 的目标都会被移除。"
    );

    const zhTW = await textsIn("zh-TW", 5, 3);
    expect(zhTW.toolbarNote).toBe("（其中 3 個未顯示）");
    expect(zhTW.deleteConfirm).toContain("（其中 3 個未顯示）");

    for (const texts of [en, zh, zhTW]) {
      expect(`${texts.toolbarNote} ${texts.deleteConfirm}`).not.toMatch(/collapsed|折叠|收合|摺疊/);
    }
  });
});
