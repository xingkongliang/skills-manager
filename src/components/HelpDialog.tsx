import { BookOpen, FolderTree, Layers3, Map, RefreshCw, Settings2, Sparkles, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useApp } from "../context/AppContext";

const GUIDE_ICONS = [Map, Layers3, BookOpen, Sparkles, FolderTree, RefreshCw, Settings2];

export function HelpDialog() {
  const { t } = useTranslation();
  const { helpOpen, closeHelp } = useApp();

  if (!helpOpen) return null;

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 px-6 backdrop-blur-sm">
      <div className="absolute inset-0" onClick={closeHelp} />
      <div className="relative w-full max-w-[640px] overflow-hidden rounded-[28px] border border-border bg-bg-secondary shadow-[0_40px_90px_rgba(0,0,0,0.45)]">
        <div className="border-b border-border-subtle bg-[radial-gradient(circle_at_top_left,rgba(245,158,11,0.18),transparent_45%),radial-gradient(circle_at_top_right,rgba(16,185,129,0.16),transparent_40%)] px-6 py-5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <p className="text-[13px] font-semibold uppercase tracking-[0.18em] text-faint">
                {t("help.eyebrow")}
              </p>
              <h2 className="mt-2 text-[20px] font-semibold text-primary">{t("help.title")}</h2>
              <p className="mt-1 text-[13px] text-muted">{t("help.description")}</p>
            </div>
            <button
              type="button"
              onClick={closeHelp}
              className="rounded-xl border border-border bg-background p-2 text-muted transition hover:border-border-subtle hover:text-secondary"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        <div className="max-h-[min(72vh,720px)] space-y-3 overflow-y-auto px-5 py-5">
          {(["workflows", "scenarios", "install", "sync", "projects", "backup", "settings"] as const).map((key, index) => {
            const Icon = GUIDE_ICONS[index];
            return (
              <div
                key={key}
                className="flex items-start gap-3 rounded-2xl border border-border-subtle bg-surface px-4 py-3"
              >
                <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-background text-accent">
                  <Icon className="h-4 w-4" />
                </div>
                <div>
                  <h3 className="text-[13px] font-semibold text-secondary">{t(`help.items.${key}.title`)}</h3>
                  <p className="mt-1 text-[13px] leading-5 text-muted">{t(`help.items.${key}.description`)}</p>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
