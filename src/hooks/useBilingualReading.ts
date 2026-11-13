import { useEffect, useMemo, useRef, useState } from "react";
import * as api from "../lib/tauri";
import { runReaderTranslation, translationErrorCode, type BlockResult, type TranslationBackend } from "../lib/translation/readerTranslation";
import { buildTranslationPlan, type TranslationPlan } from "../lib/markdown/translationPlan";

const backend: TranslationBackend = { readCache: api.getTranslationCache, writeCache: api.putTranslationCache, translate: api.translateSkillBlocks, cancel: api.cancelSkillTranslation };

export function useBilingualReading(sessionId: string, identity: string, plan: TranslationPlan, description: string | undefined, enabled: boolean, target: string) {
  const [attempt, setAttempt] = useState(0);
  const [pages, setPages] = useState(1);
  const [state, setState] = useState<{ identity: string; results: Record<string, BlockResult>; memory: boolean; paused?: boolean }>({ identity: "", results: {}, memory: false });
  const visible = useRef(new Set<string>());
  const allBlocks = useMemo(() => [
    ...buildTranslationPlan(description ?? "").blocks.map((b) => ({ ...b, id: `description:${b.id}` })),
    ...plan.blocks,
  ], [plan, description]);
  const blocks = useMemo(() => {
    let count = 0; let characters = 0;
    return allBlocks.filter((b) => {
      if (b.skipped) return false;
      if (count >= 500 * pages || characters + b.request.length > 50000 * pages) return false;
      count++; characters += b.request.length;
      return true;
    });
  }, [allBlocks, pages]);
  const stateKey = `${identity}:${target}:${attempt}:${pages}`;
  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    const clear = () => { controller.abort(); setState({ identity: stateKey, results: {}, memory: false, paused: true }); };
    const changed = () => { controller.abort(); setAttempt((n) => n + 1); };
    window.addEventListener("skill-reader-cache-cleared", clear);
    window.addEventListener("skill-reader-translation-settings-changed", changed);
    void api.getTranslationSettings().then((settings) => {
      if (controller.signal.aborted) return;
      return runReaderTranslation({ blocks, sessionId, target, signal: controller.signal,
        cacheNamespace: settings.cache_namespace,
        backend: { ...backend, translate: (session, request, texts, language) => api.translateSkillBlocks(session, request, texts, language, settings.cache_namespace) },
        priority: (id) => visible.current.has(id),
        onMemoryCache: () => { if (!controller.signal.aborted) setState((s) => ({ identity: stateKey, results: s.identity === stateKey ? s.results : {}, memory: true })); },
        onResult: (id, result) => { if (!controller.signal.aborted) setState((s) => ({ identity: stateKey, memory: s.identity === stateKey && s.memory, results: { ...(s.identity === stateKey ? s.results : {}), [id]: result } })); },
      });
    }).catch((error: unknown) => {
      if (!controller.signal.aborted) setState({ identity: stateKey, results: Object.fromEntries(blocks.map((b) => [b.id, { status: "error" as const, error: translationErrorCode(error) }])), memory: false });
    });
    return () => { controller.abort(); window.removeEventListener("skill-reader-cache-cleared", clear); window.removeEventListener("skill-reader-translation-settings-changed", changed); };
  }, [blocks, enabled, sessionId, stateKey, target]);
  const results = enabled && state.identity === stateKey ? state.results : {};
  const translations = Object.fromEntries(Object.entries(results).filter(([, result]) => result.text).map(([id, result]) => [id, result.text!]));
  const complete = Object.values(results).filter((r) => r.text).length;
  const failed = Object.values(results).filter((r) => r.status === "error");
  return { translations, complete, total: blocks.length, failed: failed.length, error: failed[0]?.error, memory: state.memory, paused: state.identity === stateKey && state.paused,
    descriptionTranslation: Object.entries(translations).filter(([id]) => id.startsWith("description:")).map(([, text]) => text).join("\n"),
    hasMore: allBlocks.filter((b) => !b.skipped).length > blocks.length,
    retry: () => setAttempt((n) => n + 1), more: () => setPages((n) => n + 1), visible };
}
