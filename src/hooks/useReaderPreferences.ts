import { useEffect, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { readerLanguages, targetLanguage, type ReaderLanguage } from "../lib/readerLanguage";
import { getSettings, setSettings } from "../lib/tauri";

const EVENT = "skill-reader-preferences";
const KEYS = ["reader_mode", "translation_target_language"] as const;
type Key = typeof KEYS[number];
let ready = false;
let loading: Promise<void> | null = null;
function notify() { window.dispatchEvent(new Event(EVENT)); }
function subscribe(callback: () => void) {
  window.addEventListener(EVENT, callback); window.addEventListener("storage", callback);
  return () => { window.removeEventListener(EVENT, callback); window.removeEventListener("storage", callback); };
}
function valid(key: Key, value: string | null): value is string {
  return key === "reader_mode" ? value === "original" || value === "bilingual" : value !== null && value in readerLanguages;
}
function save(key: Key, value: string) {
  localStorage.setItem(key, value); notify();
  // The local mirror also preserves preferences when the app backend is offline.
  void setSettings(key, value).catch(() => {});
}
export function useReaderPreferences() {
  const { i18n } = useTranslation();
  const loaded = useSyncExternalStore(subscribe, () => ready);
  const mode = useSyncExternalStore(subscribe, () => localStorage.getItem("reader_mode") === "bilingual" ? "bilingual" : "original");
  const target = useSyncExternalStore(subscribe, () => valid("translation_target_language", localStorage.getItem("translation_target_language")) ? localStorage.getItem("translation_target_language") as ReaderLanguage : targetLanguage(i18n?.resolvedLanguage ?? i18n?.language ?? navigator.language));
  useEffect(() => {
    loading ??= Promise.all(KEYS.map(async (key) => {
      const before = localStorage.getItem(key);
      try { const value = await getSettings(key); if (valid(key, value) && localStorage.getItem(key) === before) localStorage.setItem(key, value); } catch { /* local mirror */ }
    })).then(() => { ready = true; notify(); });
  }, []);
  return { loaded, mode, target, setMode: (v: "original" | "bilingual") => save("reader_mode", v), setTarget: (v: ReaderLanguage) => save("translation_target_language", v) };
}
