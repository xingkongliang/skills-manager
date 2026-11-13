import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import * as api from "../lib/tauri";
import { readerRequestId } from "../lib/skillReader";
import { translationErrorCode } from "../lib/translation/readerTranslation";

export function TranslationProviderSettings({ compact = false }: { compact?: boolean }) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<api.TranslationSettingsView | null>(null);
  const [saved, setSaved] = useState<api.TranslationSettingsView | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const [loadingError, setLoadingError] = useState(false);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ text?: string; error?: string } | null>(null);
  const request = useRef<string | null>(null);
  const mounted = useRef(false);
  const load = () => {
    setLoadingError(false);
    void api.getTranslationSettings().then((value) => { if (mounted.current) { setSettings(value); setSaved(value); } })
      .catch(() => { if (mounted.current) setLoadingError(true); });
  };
  useEffect(() => {
    mounted.current = true;
    void api.getTranslationSettings().then((value) => { if (mounted.current) { setSettings(value); setSaved(value); } })
      .catch(() => { if (mounted.current) setLoadingError(true); });
    return () => { mounted.current = false; if (request.current) void api.cancelSkillTranslation(request.current).catch(() => {}); };
  }, []);
  const dirty = JSON.stringify(settings) !== JSON.stringify(saved) || apiKey !== "" || clearKey;
  const retainsKey = (() => {
    try { return settings?.has_api_key && new URL(settings.api_base_url).origin === new URL(saved?.api_base_url ?? "").origin; }
    catch { return false; }
  })();
  const update = <K extends keyof api.TranslationSettings>(key: K, value: api.TranslationSettings[K]) => {
    setSettings((current) => current ? { ...current, [key]: value } : current);
    setTestResult(null);
  };
  const save = async () => {
    if (!settings) return;
    setSaving(true);
    try {
      const { provider, api_base_url, model, cli_path, timeout_seconds } = settings;
      const value = await api.saveTranslationSettings({ provider, api_base_url, model, cli_path, timeout_seconds }, clearKey ? "" : apiKey || null);
      if (!mounted.current) return;
      setSettings(value); setSaved(value); setApiKey(""); setClearKey(false); setTestResult(null);
      window.dispatchEvent(new Event("skill-reader-translation-settings-changed"));
      toast.success(t("reader.providerSaved"));
    } catch (error) { if (mounted.current) toast.error(t(translationErrorCode(error))); }
    finally { if (mounted.current) setSaving(false); }
  };
  const test = async () => {
    const id = readerRequestId();
    request.current = id; setTesting(true); setTestResult(null);
    try {
      const result = await api.testTranslationSettings(id);
      if (!mounted.current) return;
      const text = result.results[0]?.text;
      setTestResult(result.error || !text ? { error: result.error ?? "reader.translationPartialFailure" } : { text });
    } catch (error) { if (mounted.current) setTestResult({ error: translationErrorCode(error) }); }
    finally { request.current = null; if (mounted.current) setTesting(false); }
  };
  const buttonClass = "rounded-lg border border-border-subtle px-3 py-2 text-[13px] text-secondary transition-colors hover:bg-surface-hover disabled:opacity-40";
  if (!settings) return <div className="px-4 py-3 text-[13px] text-muted">{loadingError ? <button type="button" className={buttonClass} onClick={load}>{t("reader.providerLoadFailed")} · {t("reader.retry")}</button> : t("common.loading")}</div>;
  const cli = settings.provider === "codex" || settings.provider === "claude";
  return <div className="space-y-3 px-4 py-3">
    {!compact && <div><h3 className="text-[14px] font-semibold text-primary">{t("reader.providerTitle")}</h3><p className="mt-1 text-[12px] text-muted">{t("reader.providerDescription")}</p></div>}
    <fieldset disabled={saving || testing} className={`grid gap-3 disabled:opacity-60 ${compact ? "grid-cols-1" : "sm:grid-cols-2"}`}>
      <label className="space-y-1 text-[13px] text-secondary"><span>{t("reader.providerTitle")}</span>
        <select className="app-input w-full" value={settings.provider} onChange={(e) => { update("provider", e.target.value as api.TranslationSettings["provider"]); update("cli_path", ""); }}>
          <option value="google">{t("reader.providers.google")}</option><option value="codex">{t("reader.providers.codex")}</option><option value="claude">{t("reader.providers.claude")}</option><option value="api">{t("reader.providers.api")}</option>
        </select>
      </label>
      {settings.provider !== "google" && <>
        <label className="space-y-1 text-[13px] text-secondary"><span>{t("reader.providerModel")}</span><input className="app-input w-full" value={settings.model} placeholder={cli ? t("reader.cliDefaultModel") : t("reader.apiModelPlaceholder")} onChange={(e) => update("model", e.target.value)} autoComplete="off" spellCheck={false} /></label>
        {cli && <label className="space-y-1 text-[13px] text-secondary col-span-full"><span>{t("reader.cliPath")}</span><input className="app-input w-full" value={settings.cli_path} placeholder={settings.provider === "codex" ? "codex" : "claude"} onChange={(e) => update("cli_path", e.target.value)} autoComplete="off" spellCheck={false} /><span className="block text-[12px] text-muted">{t("reader.cliHelp")}</span></label>}
        {settings.provider === "api" && <>
          <label className="space-y-1 text-[13px] text-secondary col-span-full"><span>{t("reader.apiBaseUrl")}</span><input className="app-input w-full" type="url" value={settings.api_base_url} placeholder="https://api.openai.com/v1" onChange={(e) => update("api_base_url", e.target.value)} autoComplete="off" spellCheck={false} /><span className="block text-[12px] text-muted">{t("reader.apiHelp")}</span></label>
          <label className="space-y-1 text-[13px] text-secondary col-span-full"><span>{t("reader.apiKey")}</span><input className="app-input w-full" type="password" value={apiKey} disabled={clearKey} placeholder={retainsKey ? t("reader.apiKeySaved") : t("reader.apiKeyOptional")} onChange={(e) => setApiKey(e.target.value)} autoComplete="new-password" /><span className="block text-[12px] text-muted">{t("reader.apiKeyHelp")}</span></label>
          {settings.has_api_key && <label className="flex items-center gap-2 text-[12px] text-secondary col-span-full"><input type="checkbox" checked={clearKey} onChange={(e) => setClearKey(e.target.checked)} />{t("reader.clearApiKey")}</label>}
        </>}
        <label className="space-y-1 text-[13px] text-secondary"><span>{t("reader.providerTimeout")}</span><input className="app-input w-full" type="number" min={15} max={600} value={settings.timeout_seconds} onChange={(e) => update("timeout_seconds", Number(e.target.value))} /></label>
      </>}
    </fieldset>
    <div className="flex flex-wrap items-center gap-2">
      <button type="button" onClick={() => void save()} disabled={!dirty || saving || testing} className={buttonClass}>{saving ? t("common.loading") : t("common.save")}</button>
      <button type="button" onClick={() => void test()} disabled={dirty || saving || testing} title={dirty ? t("reader.saveBeforeTest") : undefined} className={buttonClass}>{testing ? t("reader.testingProvider") : t("reader.testProvider")}</button>
      {testing && <button type="button" onClick={() => { if (request.current) void api.cancelSkillTranslation(request.current).catch(() => {}); }} className={buttonClass}>{t("common.cancel")}</button>}
    </div>
    {testResult && <p role="status" className="break-words text-[12px] text-secondary">{testResult.error ? t(testResult.error) : t("reader.providerTestSuccess", { text: testResult.text })}</p>}
  </div>;
}
