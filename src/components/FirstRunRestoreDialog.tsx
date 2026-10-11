import { useEffect, useState } from "react";
import { CloudDownload, Loader2, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useApp } from "../context/AppContext";
import { mapGitErrorMessage } from "../lib/gitErrors";
import { useSetAsideOffer } from "../hooks/useSetAsideOffer";
import * as api from "../lib/tauri";

const PROMPT_SETTING_KEY = "backup_first_run_prompt";

/**
 * First-launch wizard (backup redesign §3.5): when the library is empty and
 * no backup is connected, ask up front whether to start fresh or restore from
 * an existing backup — the restore entry must not be buried in a toolbar
 * (#193/#140). Shown once; both choices persist the dismissal.
 */
export function FirstRunRestoreDialog() {
  const { t } = useTranslation();
  const { managedSkills, loading: skillsLoading, refreshManagedSkills, refreshPresets } = useApp();
  const [open, setOpen] = useState(false);
  const [checked, setChecked] = useState(false);
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { attempt, announceLocalCopies, dialog: setAsideDialog } = useSetAsideOffer();

  useEffect(() => {
    if (skillsLoading || checked) return;
    setChecked(true);
    if (managedSkills.length > 0) return;
    void (async () => {
      const dismissed = await api.getSettings(PROMPT_SETTING_KEY).catch(() => null);
      if (dismissed) return;
      const savedRemote = (await api.getSettings("git_backup_remote_url").catch(() => null))?.trim();
      if (savedRemote) return;
      const status = await api.gitBackupStatus().catch(() => null);
      if (!status || status.is_repo) return;
      setOpen(true);
    })();
  }, [skillsLoading, checked, managedSkills.length]);

  if (!open) return null;

  const dismiss = async () => {
    if (busy) return;
    setOpen(false);
    await api.setSettings(PROMPT_SETTING_KEY, "fresh").catch(() => {});
  };

  const handleRestore = async () => {
    const trimmed = url.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      // Same sanitize-first flow as the Backup page: embedded credentials go
      // to the OS keychain, only the clean URL is persisted (§3.7).
      const effective = await api.gitBackupSanitizeRemoteUrl(trimmed);
      await api.setSettings("git_backup_remote_url", effective);
      await attempt(async (setAside) => {
        announceLocalCopies(await api.gitBackupClone(effective, setAside));
        await api.setSettings(PROMPT_SETTING_KEY, "restored").catch(() => {});
        // Restore pulls skills AND presets/scenarios from metadata; refresh both
        // so the sidebar preset list isn't empty until a restart (#302).
        await Promise.all([refreshManagedSkills(), refreshPresets()]);
        toast.success(t("firstRun.restoreSuccess"));
        setOpen(false);
      });
    } catch (err) {
      setError(mapGitErrorMessage(err, t));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" />
      <div className="relative z-10 w-full max-w-lg rounded-xl border border-border bg-surface p-5 shadow-2xl">
        <div className="flex items-start gap-3">
          <div className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-border-subtle bg-bg-secondary">
            <CloudDownload className="h-5 w-5 text-muted" />
          </div>
          <div className="min-w-0">
            <h2 className="text-[15px] font-semibold text-primary">{t("firstRun.title")}</h2>
            <p className="mt-1 text-[13px] leading-5 text-muted">{t("firstRun.subtitle")}</p>
          </div>
        </div>

        <div className="mt-4">
          <label className="text-[12px] font-medium text-tertiary">{t("firstRun.urlLabel")}</label>
          <input
            type="text"
            value={url}
            onChange={(event) => {
              setUrl(event.target.value);
              setError(null);
            }}
            placeholder={t("settings.gitRemoteUrlPlaceholder")}
            disabled={busy}
            className="mt-1.5 h-8 w-full rounded-lg border border-border-subtle bg-background px-2.5 font-mono text-[13px] text-secondary outline-none transition-colors focus:border-border disabled:opacity-50"
            autoCapitalize="none"
            autoCorrect="off"
            spellCheck={false}
          />
          {error && (
            <div className="mt-2 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-[12px] leading-5 text-red-600 dark:text-red-300">
              {error}
            </div>
          )}
        </div>

        <div className="mt-5 flex items-center justify-between gap-2">
          <button
            type="button"
            onClick={dismiss}
            disabled={busy}
            className="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[13px] font-medium text-tertiary transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50 outline-none"
          >
            <Sparkles className="h-3.5 w-3.5" />
            {t("firstRun.startFresh")}
          </button>
          <button
            type="button"
            onClick={handleRestore}
            disabled={busy || !url.trim()}
            className="inline-flex items-center gap-1.5 rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50 outline-none"
          >
            {busy ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <CloudDownload className="h-3.5 w-3.5" />}
            {busy ? t("firstRun.restoring") : t("firstRun.restore")}
          </button>
        </div>

        <p className="mt-3 text-[12px] leading-5 text-faint">{t("firstRun.hint")}</p>
      </div>
      {setAsideDialog}
    </div>
  );
}
