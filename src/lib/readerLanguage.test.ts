import { describe, expect, it } from "vitest";
import { needsTranslation, targetLanguage } from "./readerLanguage";
import { buildTranslationPlan } from "./markdown/translationPlan";
describe("reader language selection", () => {
  it("maps the app locale to a supported target", () => {
    expect(["zh-Hant", "zh", "en-US", "es-ES", "ko-KR"].map(targetLanguage)).toEqual(["zh-TW", "zh-CN", "en", "es", "ko"]);
  });
  it("compares prose languages and leaves empty or protected content alone", () => {
    expect(needsTranslation("Read the instructions first.", "zh-CN")).toBe(true);
    expect(needsTranslation("请先阅读说明。", "zh-TW")).toBe(false);
    expect(needsTranslation("请先阅读说明。", "en")).toBe(true);
    expect(needsTranslation("⟦S0⟧", "zh-CN")).toBe(false);
    expect(needsTranslation("문서를 먼저 읽으세요", "ko")).toBe(false);
    expect(needsTranslation("Utilizar el archivo para configurar", "es")).toBe(false);
  });
  it("allows non-Latin prose to be translated to English", () => {
    expect(buildTranslationPlan("请阅读说明。", "en").blocks[0].skipped).toBe(false);
    expect(buildTranslationPlan("```sh\necho hello\n```", "zh-CN").blocks).toEqual([]);
  });
});
