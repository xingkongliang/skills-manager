import { useEffect, useRef, useState, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import { ExternalLink, Sparkles, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useApp } from "../context/AppContext";
import {
  WHATS_NEW_SEEN_KEY,
  decideWhatsNew,
  loadChangelog,
  type ChangelogEntry,
} from "../lib/changelog";
import * as api from "../lib/tauri";

const RELEASES_URL = "https://github.com/xingkongliang/skills-manager/releases";

// No remark-gfm: its autolink regex uses lookbehind, which old WebKit rejects
// with a SyntaxError that unmounts the app (#613, see SkillMarkdown.tsx). The
// CHANGELOG needs nothing beyond plain Markdown.
function MarkdownLink({ href, children }: { href?: string; children?: ReactNode }) {
  return (
    <a
      href={href}
      onClick={(event) => {
        // Never let the webview navigate away from the app.
        event.preventDefault();
        if (href) openUrl(href).catch(() => {});
      }}
      className="text-accent underline-offset-2 hover:underline"
    >
      {children}
    </a>
  );
}

const inline: Components = {
  a: ({ href, children }) => <MarkdownLink href={href}>{children}</MarkdownLink>,
  code: ({ children }) => (
    <code className="rounded-md bg-surface-hover px-1 py-0.5 font-mono text-[12px] text-primary">
      {children}
    </code>
  ),
  strong: ({ children }) => <strong className="font-semibold text-primary">{children}</strong>,
};

// The overview is written as a one-item list; show it as a plain sentence.
const overviewComponents: Components = {
  ...inline,
  ul: ({ children }) => <div className="space-y-1">{children}</div>,
  li: ({ children }) => <p>{children}</p>,
  p: ({ children }) => <p>{children}</p>,
};

const listComponents: Components = {
  ...inline,
  ul: ({ children }) => <ul className="list-disc space-y-2 pl-4 marker:text-faint">{children}</ul>,
  li: ({ children }) => <li className="pl-0.5">{children}</li>,
  p: ({ children }) => <p className="mt-2 first:mt-0">{children}</p>,
};

interface Props {
  entries: ChangelogEntry[];
  onClose: () => void;
}

export function WhatsNewDialog({ entries, onClose }: Props) {
  const { t } = useTranslation();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    // Height cap divides by --app-scale for the same reason as ConfirmDialog (#430).
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="whats-new-title"
        className="relative z-10 flex w-full max-w-xl flex-col rounded-xl border border-border bg-surface shadow-2xl max-h-[calc(85vh/var(--app-scale))]"
      >
        <div className="flex items-center justify-between px-5 pb-3 pt-5">
          <h2 id="whats-new-title" className="flex items-center gap-2 text-[14px] font-semibold text-primary">
            <Sparkles className="h-4 w-4 text-accent" />
            {t("whatsNew.title")}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={t("whatsNew.close")}
            className="rounded-md p-1 text-muted outline-none transition-colors hover:text-secondary"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <div className="min-h-0 flex-1 space-y-6 overflow-y-auto px-5 pb-5">
          {entries.map((entry) => (
            <section key={entry.version}>
              <div className="flex items-baseline gap-2">
                <span className="text-[14px] font-semibold text-primary tabular-nums">v{entry.version}</span>
                <span className="text-[12px] text-muted tabular-nums">{entry.date}</span>
              </div>
              {entry.overview && (
                <div className="mt-1 text-[13px] leading-6 text-secondary">
                  <ReactMarkdown components={overviewComponents}>{entry.overview}</ReactMarkdown>
                </div>
              )}
              <div className="mt-3 text-[13px] leading-6 text-tertiary">
                <ReactMarkdown components={listComponents}>{entry.userFacing}</ReactMarkdown>
              </div>
            </section>
          ))}
        </div>

        <div className="flex items-center justify-between gap-2 border-t border-border-faint px-5 py-4">
          <button
            type="button"
            onClick={() => { openUrl(RELEASES_URL).catch(() => {}); }}
            className="inline-flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-[13px] font-medium text-tertiary outline-none transition-colors hover:bg-surface-hover hover:text-secondary"
          >
            <ExternalLink className="h-3.5 w-3.5" />
            {t("whatsNew.fullChangelog")}
          </button>
          <button type="button" onClick={onClose} className="app-button-primary px-3 py-1.5">
            {t("whatsNew.gotIt")}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Opens "What's new" once after the app is updated. Waits for the library to
 * load, since an empty library is how a fresh install is told apart from an
 * update from a version without this dialog. The version is recorded as seen
 * only when the dialog closes, so quitting before reading it shows it again.
 * Any failure stays silent: this must never get in the way of starting up.
 */
export function WhatsNewOnUpdate() {
  const { i18n } = useTranslation();
  const { managedSkills, loading } = useApp();
  // Once per launch: `loading` flips back on every library refresh.
  const checkedRef = useRef(false);
  const [shown, setShown] = useState<{ current: string; entries: ChangelogEntry[] } | null>(null);

  useEffect(() => {
    if (loading || checkedRef.current) return;
    checkedRef.current = true;
    const hasLibrary = managedSkills.length > 0;
    void (async () => {
      try {
        const [current, lastSeen] = await Promise.all([
          getVersion(),
          api.getSettings(WHATS_NEW_SEEN_KEY),
        ]);
        const decision = await decideWhatsNew({
          current,
          lastSeen: lastSeen || null,
          hasLibrary,
          loadEntries: () => loadChangelog(i18n.language),
        });
        if (decision.record === "now") {
          await api.setSettings(WHATS_NEW_SEEN_KEY, current);
        } else if (decision.record === "onClose") {
          setShown({ current, entries: decision.entries });
        }
      } catch {
        // Silent by design; the next launch tries again.
      }
    })();
  }, [loading, managedSkills.length, i18n.language]);

  if (!shown) return null;

  return (
    <WhatsNewDialog
      entries={shown.entries}
      onClose={() => {
        setShown(null);
        api.setSettings(WHATS_NEW_SEEN_KEY, shown.current).catch(() => {});
      }}
    />
  );
}
