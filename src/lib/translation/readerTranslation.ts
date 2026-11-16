import type { TranslationCacheEntry, TranslationCacheResult, TranslationResponse } from "../tauri";
import { readerRequestId } from "../skillReader";
import { restoreProtectedText, splitTranslationText, type TranslationBlock } from "../markdown/translationPlan";

export type BlockResult = { status: "queued" | "translating" | "cache_hit" | "success" | "error"; text?: string; error?: string };
export interface TranslationBackend {
  readCache(keys: string[]): Promise<TranslationCacheResult>;
  writeCache(entries: TranslationCacheEntry[], epoch: number): Promise<boolean>;
  translate(sessionId: string, requestId: string, texts: string[], target: string): Promise<TranslationResponse>;
  cancel(requestId: string): Promise<void>;
}
interface Options {
  blocks: TranslationBlock[]; sessionId: string; target: string; signal: AbortSignal;
  backend: TranslationBackend; onResult(id: string, result: BlockResult): void;
  priority?: (id: string) => boolean; onMemoryCache?: () => void;
  cacheNamespace?: string;
}

async function cacheKey(block: TranslationBlock, target: string, namespace?: string) {
  const identity = JSON.stringify([namespace ?? "google", "batchexecute-v1", "auto", target, "normalization-v1", block.tokens, block.request]);
  const hash = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(identity));
  return Array.from(new Uint8Array(hash), (b) => b.toString(16).padStart(2, "0")).join("");
}

export function translationErrorCode(error: unknown) {
  const message = typeof error === "object" && error !== null && "message" in error ? String(error.message) : "";
  return /^reader\.[a-zA-Z]+$/.test(message) ? message : "reader.translationNetwork";
}

function wait(milliseconds: number, signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    if (signal.aborted) { resolve(); return; }
    const finish = () => { clearTimeout(timer); signal.removeEventListener("abort", finish); resolve(); };
    const timer = setTimeout(finish, milliseconds);
    signal.addEventListener("abort", finish, { once: true });
  });
}

async function requestWithRetry(texts: string[], options: Options): Promise<TranslationResponse> {
  for (let attempt = 0; ; attempt++) {
    if (options.signal.aborted) throw new Error("reader.cancelled");
    const id = readerRequestId();
    const cancel = () => { void options.backend.cancel(id).catch(() => {}); };
    options.signal.addEventListener("abort", cancel, { once: true });
    let response: TranslationResponse;
    try { response = await options.backend.translate(options.sessionId, id, texts, options.target); }
    catch (error) { response = { results: [], error: translationErrorCode(error), retry_after_ms: null }; }
    finally { options.signal.removeEventListener("abort", cancel); }
    if (options.signal.aborted) throw new Error("reader.cancelled");
    if (!response.error) return response;
    if (attempt >= 2 || !["reader.translationNetwork", "reader.translationRateLimited"].includes(response.error)) return response;
    await wait(response.retry_after_ms ?? (attempt === 0 ? 1000 : 3000) + Math.random() * 200, options.signal);
  }
}

export async function runReaderTranslation(options: Options) {
  const { blocks, signal, backend, target, onResult } = options;
  const publish = (ids: string[], result: BlockResult) => { if (!signal.aborted) ids.forEach((id) => onResult(id, result)); };
  const keyed = await Promise.all(blocks.filter((b) => !b.skipped && b.request).map(async (block) => ({ block, key: await cacheKey(block, target, options.cacheNamespace) })));
  if (signal.aborted) return;
  const groups = new Map<string, { block: TranslationBlock; ids: string[]; pieces: string[]; results: (string | null)[]; failed: boolean }>();
  for (const { block, key } of keyed) {
    const existing = groups.get(key);
    if (existing) existing.ids.push(block.id);
    else { const pieces = splitTranslationText(block.request); groups.set(key, { block, ids: [block.id], pieces, results: new Array(pieces.length).fill(null), failed: false }); }
  }
  let epoch: number | null = null;
  const keys = [...groups.keys()];
  for (let i = 0; i < keys.length; i += 500) {
    try {
      const cache = await backend.readCache(keys.slice(i, i + 500));
      if (signal.aborted) return;
      epoch = cache.epoch;
      if (!cache.persistent) options.onMemoryCache?.();
      for (const entry of cache.entries) {
        const group = groups.get(entry.cache_key);
        if (group && entry.text) { publish(group.ids, { status: "cache_hit", text: entry.text }); groups.delete(entry.cache_key); }
      }
    } catch { options.onMemoryCache?.(); }
  }
  const pending = [...groups].flatMap(([key, group]) => {
    publish(group.ids, { status: "queued" });
    return group.pieces.map((text, index) => ({ key, group, text, index }));
  });
  while (pending.length && !signal.aborted) {
    if (options.priority) pending.sort((a, b) => Number(b.group.ids.some(options.priority!)) - Number(a.group.ids.some(options.priority!)));
    let length = 0; let count = 0;
    while (count < pending.length && count < 20 && length + pending[count].text.length <= 3500) { length += pending[count].text.length; count++; }
    const batch = pending.splice(0, count);
    for (const task of batch) publish(task.group.ids, { status: "translating" });
    let response: TranslationResponse;
    try { response = await requestWithRetry(batch.map((b) => b.text), options); }
    catch { if (signal.aborted) return; response = { results: [], error: "reader.translationNetwork", retry_after_ms: null }; }
    if (signal.aborted) return;
    if (response.error) {
      const seen = new Set<string>();
      for (const task of [...batch, ...pending]) if (!seen.has(task.key)) { seen.add(task.key); publish(task.group.ids, { status: "error", error: response.error }); }
      return;
    }
    const completed = new Set<string>();
    for (let i = 0; i < batch.length; i++) {
      const task = batch[i];
      const result = response.results[i]?.text;
      if (!result?.trim()) { task.group.failed = true; publish(task.group.ids, { status: "error", error: "reader.translationPartialFailure" }); }
      else task.group.results[task.index] = result;
      if (!task.group.failed && task.group.results.every((r) => r !== null)) completed.add(task.key);
    }
    for (let i = pending.length - 1; i >= 0; i--) if (pending[i].group.failed) pending.splice(i, 1);
    const entries: TranslationCacheEntry[] = [];
    for (const key of completed) {
      const group = groups.get(key)!;
      try {
        const text = restoreProtectedText(group.results.join(" "), group.block.tokens);
        publish(group.ids, { status: "success", text });
        entries.push({ cache_key: key, text, target_language: target });
      } catch { publish(group.ids, { status: "error", error: "reader.protectionFailed" }); }
    }
    if (entries.length && epoch !== null && !signal.aborted) { try { await backend.writeCache(entries, epoch); } catch { options.onMemoryCache?.(); } }
  }
}
