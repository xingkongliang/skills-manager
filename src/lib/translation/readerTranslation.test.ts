import { describe, expect, it } from "vitest";
import { runReaderTranslation, type TranslationBackend } from "./readerTranslation";
import { buildTranslationPlan } from "../markdown/translationPlan";

function backend(): TranslationBackend & { calls: string[][]; saved: string[] } {
  const calls: string[][] = []; const saved: string[] = [];
  return { calls, saved,
    readCache: async () => ({ epoch: 1, entries: [], persistent: true }),
    writeCache: async (entries) => { saved.push(...entries.map((e) => e.text)); return true; },
    translate: async (_session, _id, texts) => { calls.push(texts); return { results: texts.map((text) => ({ text: `中文 ${text}` })), error: null, retry_after_ms: null }; },
    cancel: async () => {},
  };
}

describe("reader translation lifecycle", () => {
  it("deduplicates identical blocks and restores protected code before caching", async () => {
    const api = backend(); const translated: Record<string, string> = {};
    const plan = buildTranslationPlan("Read `SKILL.md` first.\n\nRead `SKILL.md` first.");
    await runReaderTranslation({ blocks: plan.blocks, sessionId: "s", target: "zh-CN", signal: new AbortController().signal, backend: api, onResult: (id, result) => { if (result.text) translated[id] = result.text; } });
    expect(api.calls.flat()).toHaveLength(1);
    expect(Object.keys(translated)).toHaveLength(2);
    expect(api.saved).toEqual(["中文 Read SKILL.md first."]);
  });
  it("only caches successful blocks when a batch partially fails", async () => {
    const api = backend();
    api.translate = async () => ({ results: [{ text: "成功" }, { text: null }], error: null, retry_after_ms: null });
    const states: Record<string, string> = {};
    const plan = buildTranslationPlan("First paragraph.\n\nSecond paragraph.");
    await runReaderTranslation({ blocks: plan.blocks, sessionId: "s", target: "zh-CN", signal: new AbortController().signal, backend: api, onResult: (id, r) => { states[id] = r.status; } });
    expect(Object.values(states).sort()).toEqual(["error", "success"]);
    expect(api.saved).toEqual(["成功"]);
  });
  it("never publishes or caches a result after the reader has been cancelled", async () => {
    const api = backend(); const controller = new AbortController(); let results = 0;
    api.translate = async () => { controller.abort(); return { results: [{ text: "late" }], error: null, retry_after_ms: null }; };
    await runReaderTranslation({ blocks: buildTranslationPlan("Translate this.").blocks, sessionId: "s", target: "zh-CN", signal: controller.signal, backend: api, onResult: (_id, r) => { if (r.text) results++; } });
    expect(results).toBe(0);
    expect(api.saved).toEqual([]);
  });
  it("does not send cached content to the provider", async () => {
    const api = backend();
    api.readCache = async (keys) => ({ epoch: 3, entries: [{ cache_key: keys[0], text: "缓存", target_language: "zh-CN" }], persistent: true });
    await runReaderTranslation({ blocks: buildTranslationPlan("Already translated.").blocks, sessionId: "s", target: "zh-CN", signal: new AbortController().signal, backend: api, onResult: () => {} });
    expect(api.calls).toEqual([]);
  });
});
