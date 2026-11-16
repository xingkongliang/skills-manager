import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { getTranslationSettings, type TranslationSettingsView } from "../lib/tauri";
import { TranslationProviderSettings } from "./TranslationProviderSettings";

export function ReaderTranslationService() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<TranslationSettingsView | null>(null);
  const [failed, setFailed] = useState(false);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    let live = true;
    const load = () => { void getTranslationSettings().then((value) => { if (live) { setSettings(value); setFailed(false); } }).catch(() => { if (live) setFailed(true); }); };
    load();
    window.addEventListener("skill-reader-translation-settings-changed", load);
    return () => { live = false; window.removeEventListener("skill-reader-translation-settings-changed", load); };
  }, []);
  return <div className="border-t border-border-subtle">
    <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className="flex w-full items-center gap-2 px-3 py-2 text-left text-[12px] text-secondary hover:bg-surface-hover">
      <span className="min-w-0 flex-1"><span className="block text-[11px] text-muted">{t("reader.providerTitle")}</span>
        <span className="block truncate" title={settings?.model}>{settings ? `${t(`reader.providers.${settings.provider}`)}${settings.provider === "google" ? "" : ` · ${settings.model || t("reader.defaultModel")}`}` : t(failed ? "reader.providerLoadFailed" : "common.loading")}</span>
      </span><ChevronDown className={`h-3.5 w-3.5 shrink-0 ${open ? "rotate-180" : ""}`} />
    </button>
    {open && <TranslationProviderSettings compact />}
  </div>;
}
