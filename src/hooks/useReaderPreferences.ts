import { useEffect, useSyncExternalStore } from "react";
import { getSettings, setSettings } from "../lib/tauri";

const EVENT = "skill-reader-preferences";
const KEYS = ["reader_mode", "reader_tree_collapsed", "translation_target_language"] as const;
type Key = typeof KEYS[number];
let loading: Promise<void> | null = null;
function notify() { window.dispatchEvent(new Event(EVENT)); }
function subscribe(callback: () => void) {
  window.addEventListener(EVENT, callback); window.addEventListener("storage", callback);
  return () => { window.removeEventListener(EVENT, callback); window.removeEventListener("storage", callback); };
}
function valid(key: Key, value: string | null): value is string {
  return key === "reader_mode" ? value === "original" || value === "bilingual" : key === "reader_tree_collapsed" ? value === "true" || value === "false" : value === "zh-CN" || value === "zh-TW";
}
function save(key: Key, value: string) {
  localStorage.setItem(key, value); notify();
  // The local mirror also preserves preferences when the app backend is offline.
  void setSettings(key, value).catch(() => {});
}
export function useReaderPreferences() {
  const mode = useSyncExternalStore(subscribe, () => localStorage.getItem("reader_mode") === "bilingual" ? "bilingual" : "original");
  const collapsed = useSyncExternalStore(subscribe, () => localStorage.getItem("reader_tree_collapsed") === "true");
  const target = useSyncExternalStore(subscribe, () => localStorage.getItem("translation_target_language") === "zh-TW" ? "zh-TW" : "zh-CN");
  useEffect(() => {
    loading ??= Promise.all(KEYS.map(async (key) => {
      const before = localStorage.getItem(key);
      try { const value = await getSettings(key); if (valid(key, value) && localStorage.getItem(key) === before) localStorage.setItem(key, value); } catch { /* local mirror */ }
    })).then(notify);
  }, []);
  return { mode, collapsed, target, setMode: (v: "original" | "bilingual") => save("reader_mode", v), setCollapsed: (v: boolean) => save("reader_tree_collapsed", String(v)), setTarget: (v: "zh-CN" | "zh-TW") => save("translation_target_language", v) };
}
