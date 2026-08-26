import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { getSettings } from "../lib/tauri";
import zh from "./zh.json";
import zhTW from "./zh-TW.json";
import en from "./en.json";

const LANGUAGE_STORAGE_KEY = "language";
const SUPPORTED_LANGUAGES = ["zh", "zh-TW", "en"] as const;
type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

function isSupportedLanguage(lang: string | null): lang is SupportedLanguage {
  return SUPPORTED_LANGUAGES.includes(lang as SupportedLanguage);
}

function getStoredLanguage(): SupportedLanguage | null {
  const stored = localStorage.getItem(LANGUAGE_STORAGE_KEY);
  return isSupportedLanguage(stored) ? stored : null;
}

/**
 * First-run language, from the OS locale list. Only reached when neither the
 * saved setting nor localStorage has a value, so an existing user's choice is
 * never overridden.
 */
function detectLanguage(): SupportedLanguage {
  const tags = navigator.languages?.length
    ? navigator.languages
    : [navigator.language];

  for (const tag of tags) {
    const lower = tag.toLowerCase();
    if (lower.startsWith("zh")) {
      // An explicit script wins over the region, so zh-Hans-HK stays Simplified.
      if (lower.includes("hans")) return "zh";
      return /hant|-(tw|hk|mo)\b/.test(lower) ? "zh-TW" : "zh";
    }
    if (lower.startsWith("en")) return "en";
  }

  return "en";
}

export const i18nReady = (async () => {
  const storedLanguage = getStoredLanguage();
  const savedLanguage = await getSettings("language").catch(() => null);
  const lng = isSupportedLanguage(savedLanguage)
    ? savedLanguage
    : storedLanguage || detectLanguage();

  localStorage.setItem(LANGUAGE_STORAGE_KEY, lng);

  await i18n.use(initReactI18next).init({
    resources: {
      zh: { translation: zh },
      "zh-TW": { translation: zhTW },
      en: { translation: en },
    },
    lng,
    // zh-TW is a partial locale, so it still falls back to Simplified
    // Chinese. Everything else falls back to English.
    fallbackLng: { "zh-TW": ["zh"], default: ["en"] },
    interpolation: { escapeValue: false },
  });
})();

export default i18n;
