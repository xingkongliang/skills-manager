import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { getSettings } from "../lib/tauri";
import zh from "./zh.json";
import en from "./en.json";

const LANGUAGE_STORAGE_KEY = "language";

function getStoredLanguage() {
  const stored = localStorage.getItem(LANGUAGE_STORAGE_KEY);
  if (stored === "zh" || stored === "en") {
    return stored;
  }
  return null;
}

export const i18nReady = (async () => {
  const storedLanguage = getStoredLanguage();
  const savedLanguage = await getSettings("language").catch(() => null);
  const lng = savedLanguage === "zh" || savedLanguage === "en"
    ? savedLanguage
    : storedLanguage || "zh";

  localStorage.setItem(LANGUAGE_STORAGE_KEY, lng);

  await i18n.use(initReactI18next).init({
    resources: {
      zh: { translation: zh },
      en: { translation: en },
    },
    lng,
    fallbackLng: "zh",
    interpolation: { escapeValue: false },
  });
})();

export default i18n;
