import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { fetchSkills, type MarketSkill } from "../byte/views/installSkills/frontend";
import { cancelInstall, type ManagedSkill } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";

export function AgentBuddySwitchDialog({
  skill,
  onClose,
  onSwitch,
}: {
  skill: ManagedSkill;
  onClose: () => void;
  onSwitch: (target: MarketSkill) => Promise<void>;
}) {
  const { t } = useTranslation();
  const [query, setQuery] = useState(skill.name);
  const [results, setResults] = useState<MarketSkill[]>([]);
  const [selected, setSelected] = useState<MarketSkill | null>(null);
  const [loading, setLoading] = useState(false);
  const [applying, setApplying] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searched, setSearched] = useState(false);
  const request = useRef(0);

  const search = async (value: string) => {
    const id = ++request.current;
    setLoading(true);
    setSelected(null);
    setError(null);
    setResults([]);
    try {
      // Read all result pages before deciding that an exact name is unique.
      const found = new Map<string, MarketSkill>();
      for (let page = 1; ; page++) {
        const response = await fetchSkills({
          board: "all",
          query: value.trim(),
          page,
          pageSize: 100,
          refresh: true,
        });
        if (id !== request.current) return;
        if (response.page !== page || response.page_size < 1) {
          throw new Error(t("common.error"));
        }
        for (const candidate of response.skills) {
          found.set(JSON.stringify([candidate.identifier, candidate.repository]), candidate);
        }
        if (response.skills.length === 0 || response.page * response.page_size >= response.total)
          break;
      }
      const items = [...found.values()];
      setResults(items);
      const exact = items.filter((candidate) => candidate.name.trim() === skill.name.trim());
      if (exact.length === 1 && !exact[0].disabled && !exact[0].no_permission && exact[0].version) {
        setSelected(exact[0]);
      }
      setSearched(true);
    } catch (cause) {
      if (id === request.current) setError(getErrorMessage(cause, t("common.error")));
    } finally {
      if (id === request.current) setLoading(false);
    }
  };

  useEffect(() => {
    void search(skill.name);
    const counter = request;
    return () => {
      counter.current++;
    };
    // A fresh dialog is mounted for every skill.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [skill.id]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !applying) {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [applying, onClose]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div
        className="absolute inset-0 bg-black/70 backdrop-blur-sm"
        onClick={() => {
          if (!applying) onClose();
        }}
      />
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="buddy-switch-title"
        className="relative flex max-h-[calc(85vh/var(--app-scale))] w-full max-w-xl flex-col rounded-xl border border-border bg-surface p-5 shadow-2xl"
      >
        <h2 id="buddy-switch-title" className="text-[17px] font-semibold text-primary">
          {t("mySkills.agentBuddy.switch")}
        </h2>
        <p className="mt-2 text-[13px] text-muted">
          {t("mySkills.agentBuddy.switchDescription", { name: skill.name })}
        </p>
        <form
          className="my-4 flex gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            void search(query);
          }}
        >
          <input
            aria-label={t("bytedance.search")}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            disabled={applying}
            className="min-w-0 flex-1 rounded-lg border border-border-subtle bg-bg-secondary px-3 py-2 text-[13px] text-secondary"
            autoFocus
          />
          <button
            type="submit"
            disabled={loading || applying || !query.trim()}
            className="rounded-lg bg-surface-hover px-3 py-2 text-[13px] text-secondary disabled:opacity-50"
          >
            {t("common.search")}
          </button>
        </form>
        {loading && (
          <p role="status" className="text-[13px] text-muted">
            {t("common.loading")}
          </p>
        )}
        {error && (
          <p role="alert" className="text-[13px] text-red-500">
            {error}
          </p>
        )}
        {searched && !loading && results.length === 0 && !error && (
          <p className="text-[13px] text-muted">{t("mySkills.agentBuddy.noMatch")}</p>
        )}
        <div className="min-h-0 space-y-2 overflow-y-auto">
          {results.map((candidate) => (
            <div
              key={JSON.stringify([candidate.identifier, candidate.repository])}
              className="rounded-lg border border-border-subtle p-3"
            >
              <label className="flex items-start gap-2 text-[13px] text-secondary">
                <input
                  type="radio"
                  name="buddy-target"
                  checked={
                    selected?.identifier === candidate.identifier &&
                    selected?.repository === candidate.repository
                  }
                  disabled={
                    loading ||
                    applying ||
                    candidate.disabled ||
                    candidate.no_permission ||
                    !candidate.version
                  }
                  onChange={() => setSelected(candidate)}
                />
                <span className="min-w-0 break-words">
                  <strong>{candidate.name}</strong> · {candidate.repository} ·{" "}
                  {candidate.version ?? "—"}
                  <span className="mt-1 block text-muted">{candidate.identifier}</span>
                </span>
              </label>
              {(candidate.disabled || candidate.no_permission || !candidate.version) && (
                <p className="mt-1 text-[12px] text-muted">{t("bytedance.unavailable")}</p>
              )}
              <button
                type="button"
                onClick={() => {
                  void openUrl(candidate.url).catch((cause) =>
                    setError(getErrorMessage(cause, t("common.error"))),
                  );
                }}
                className="mt-2 text-[12px] text-accent"
              >
                {t("bytedance.web")}
              </button>
            </div>
          ))}
        </div>
        <div className="mt-4 flex justify-end gap-2">
          <button
            disabled={cancelling}
            onClick={async () => {
              if (!applying) {
                onClose();
                return;
              }
              setCancelling(true);
              try {
                await cancelInstall(`update:${skill.id}`);
              } catch (cause) {
                setError(getErrorMessage(cause, t("common.error")));
                setCancelling(false);
              }
            }}
            className="rounded-lg px-3 py-2 text-[13px] text-muted disabled:opacity-50"
          >
            {cancelling ? t("bytedance.cancelling") : t("common.cancel")}
          </button>
          <button
            disabled={!selected || loading || applying}
            onClick={async () => {
              if (!selected) return;
              setApplying(true);
              setError(null);
              try {
                await onSwitch(selected);
                onClose();
              } catch (cause) {
                setError(getErrorMessage(cause, t("common.error")));
              } finally {
                setApplying(false);
                setCancelling(false);
              }
            }}
            className="rounded-lg bg-accent px-3 py-2 text-[13px] text-white disabled:opacity-50"
          >
            {applying ? t("common.loading") : t("mySkills.agentBuddy.switch")}
          </button>
        </div>
      </div>
    </div>
  );
}
