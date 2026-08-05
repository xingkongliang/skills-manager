import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { getSettings } from "../lib/tauri";
import zh from "./zh.json";
import zhTW from "./zh-TW.json";
import en from "./en.json";
import ko from "./ko.json";

const LANGUAGE_STORAGE_KEY = "language";
const SUPPORTED_LANGUAGES = ["zh", "zh-TW", "en", "ko"] as const;
type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

function isSupportedLanguage(lang: string | null): lang is SupportedLanguage {
  return SUPPORTED_LANGUAGES.includes(lang as SupportedLanguage);
}

function getStoredLanguage(): SupportedLanguage | null {
  const stored = localStorage.getItem(LANGUAGE_STORAGE_KEY);
  return isSupportedLanguage(stored) ? stored : null;
}

export const i18nReady = (async () => {
  const storedLanguage = getStoredLanguage();
  const savedLanguage = await getSettings("language").catch(() => null);
  const lng = isSupportedLanguage(savedLanguage)
    ? savedLanguage
    : storedLanguage || "zh";

  localStorage.setItem(LANGUAGE_STORAGE_KEY, lng);

  await i18n.use(initReactI18next).init({
    resources: {
      zh: { translation: zh },
      "zh-TW": { translation: zhTW },
      en: { translation: en },
      ko: { translation: ko },
    },
    lng,
    // Korean falls back to English before Chinese: a key added to en.json but not
    // yet translated should surface in a language a Korean reader can act on.
    fallbackLng: { ko: ["en", "zh"], default: ["zh"] },
    interpolation: { escapeValue: false },
  });
})();

export default i18n;
