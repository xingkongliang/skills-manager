import { readerLanguages, type ReaderLanguage } from "../lib/readerLanguage";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { clearTranslationCache } from "../lib/tauri";
import { useReaderPreferences } from "../hooks/useReaderPreferences";
import { TranslationProviderSettings } from "./TranslationProviderSettings";

export function ReaderSettings() {
  const { t } = useTranslation();
  const preferences = useReaderPreferences();
  const [clearing, setClearing] = useState(false);
  const clear = async () => {
    setClearing(true);
    try { await clearTranslationCache(); window.dispatchEvent(new Event("skill-reader-cache-cleared")); toast.success(t("reader.cacheCleared")); }
    catch { toast.error(t("reader.cacheClearFailed")); }
    finally { setClearing(false); }
  };
  return <section>
    <h2 className="app-section-title mb-3">{t("reader.settingsTitle")}</h2>
    <div className="app-panel divide-y divide-border-faint overflow-hidden">
      <TranslationProviderSettings />
      <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
        <div><h3 className="text-[14px] font-semibold text-primary">{t("reader.targetLanguage")}</h3><p className="mt-1 text-[12px] text-muted">{t("reader.translationPrivacy")}</p></div>
        <select aria-label={t("reader.targetLanguage")} value={preferences.target} onChange={(e) => preferences.setTarget(e.target.value as ReaderLanguage)} className="rounded-md border border-border-subtle bg-surface px-3 py-2 text-[13px] text-secondary">{Object.entries(readerLanguages).map(([code, label]) => <option key={code} value={code}>{label}</option>)}</select>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
        <div><h3 className="text-[14px] font-semibold text-primary">{t("reader.cacheTitle")}</h3><p className="mt-1 text-[12px] text-muted">{t("reader.cacheDescription")}</p></div>
        <button type="button" onClick={() => void clear()} disabled={clearing} className="rounded-lg border border-border-subtle px-3 py-2 text-[13px] text-secondary transition-colors hover:bg-surface-hover disabled:opacity-40">{clearing ? t("common.loading") : t("reader.clearCache")}</button>
      </div>
    </div>
  </section>;
}
