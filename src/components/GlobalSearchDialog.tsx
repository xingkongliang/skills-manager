import { useDeferredValue, useEffect, useMemo, useState } from "react";
import { Search, FolderTree, ArrowRight, Layers3 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router-dom";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";

interface SearchResult {
  id: string;
  name: string;
  description: string | null;
  sourceType: string;
  scenarioCount: number;
  enabledInScenario: boolean;
  rank: number;
}

function normalize(value: string | null | undefined) {
  return (value || "").trim().toLowerCase();
}

export function GlobalSearchDialog() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const {
    managedSkills,
    activeScenario,
    globalSearchOpen,
    closeGlobalSearch,
    openSkillDetailById,
  } = useApp();
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const deferredQuery = useDeferredValue(query);

  useEffect(() => {
    if (!globalSearchOpen) {
      setQuery("");
      setActiveIndex(0);
    }
  }, [globalSearchOpen]);

  const results = useMemo<SearchResult[]>(() => {
    const term = normalize(deferredQuery);
    if (!term) return [];

    return managedSkills
      .map((skill) => {
        const name = normalize(skill.name);
        const description = normalize(skill.description);
        const sourceType = normalize(skill.source_type);
        const sourceRef = normalize(skill.source_ref);

        let rank = -1;
        if (name.startsWith(term)) rank = 0;
        else if (name.includes(term)) rank = 1;
        else if (description.includes(term)) rank = 2;
        else if (sourceType.includes(term) || sourceRef.includes(term)) rank = 3;

        if (rank === -1) return null;

        return {
          id: skill.id,
          name: skill.name,
          description: skill.description,
          sourceType: skill.source_type === "skillssh" ? "skills.sh" : skill.source_type,
          scenarioCount: skill.scenario_ids.length,
          enabledInScenario: activeScenario ? skill.scenario_ids.includes(activeScenario.id) : false,
          rank,
        };
      })
      .filter((result): result is SearchResult => result !== null)
      .sort((a, b) => {
        if (a.rank !== b.rank) return a.rank - b.rank;
        return a.name.localeCompare(b.name);
      })
      .slice(0, 20);
  }, [activeScenario, deferredQuery, managedSkills]);

  useEffect(() => {
    setActiveIndex(0);
  }, [deferredQuery]);

  useEffect(() => {
    if (!globalSearchOpen) return;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeGlobalSearch();
        return;
      }

      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActiveIndex((current) => (results.length === 0 ? 0 : (current + 1) % results.length));
        return;
      }

      if (event.key === "ArrowUp") {
        event.preventDefault();
        setActiveIndex((current) =>
          results.length === 0 ? 0 : (current - 1 + results.length) % results.length
        );
        return;
      }

      if (event.key === "Enter" && results[activeIndex]) {
        event.preventDefault();
        const result = results[activeIndex];
        if (location.pathname !== "/my-skills") {
          navigate("/my-skills");
        }
        openSkillDetailById(result.id);
        closeGlobalSearch();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    activeIndex,
    closeGlobalSearch,
    globalSearchOpen,
    location.pathname,
    navigate,
    openSkillDetailById,
    results,
  ]);

  if (!globalSearchOpen) return null;

  return (
    <div className="fixed inset-0 z-[60] flex items-start justify-center bg-black/60 px-6 pt-24 backdrop-blur-sm">
      <div className="absolute inset-0" onClick={closeGlobalSearch} />
      <div className="relative w-full max-w-[680px] overflow-hidden rounded-[24px] border border-border bg-bg-secondary shadow-[0_40px_100px_rgba(0,0,0,0.45)]">
        <div className="border-b border-border-subtle bg-[radial-gradient(circle_at_top,rgba(16,185,129,0.12),transparent_55%)] px-5 py-4">
          <div className="flex items-center gap-3 rounded-2xl border border-border-subtle bg-background/90 px-4 py-3 shadow-[inset_0_1px_0_rgba(255,255,255,0.03)]">
            <Search className="h-4 w-4 text-muted" />
            <input
              autoFocus
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("search.placeholder")}
              className="w-full bg-transparent text-[14px] text-primary outline-none placeholder:text-faint"
            />
            <span className="rounded-full border border-border bg-surface px-2 py-0.5 text-[10px] font-medium text-faint">
              ESC
            </span>
          </div>
        </div>

        <div className="max-h-[420px] overflow-y-auto px-3 py-3">
          {!query.trim() ? (
            <div className="flex flex-col items-center justify-center gap-3 px-6 py-14 text-center">
              <div className="flex h-12 w-12 items-center justify-center rounded-2xl border border-border bg-surface-hover text-accent">
                <Search className="h-5 w-5" />
              </div>
              <div>
                <h3 className="text-[14px] font-semibold text-secondary">{t("search.emptyTitle")}</h3>
                <p className="mt-1 text-[12px] text-muted">{t("search.emptyDescription")}</p>
              </div>
            </div>
          ) : results.length === 0 ? (
            <div className="flex flex-col items-center justify-center gap-3 px-6 py-14 text-center">
              <div className="flex h-12 w-12 items-center justify-center rounded-2xl border border-border bg-surface-hover text-muted">
                <Layers3 className="h-5 w-5" />
              </div>
              <div>
                <h3 className="text-[14px] font-semibold text-secondary">{t("search.noResultsTitle")}</h3>
                <p className="mt-1 text-[12px] text-muted">{t("search.noResultsDescription")}</p>
              </div>
            </div>
          ) : (
            <div className="space-y-2">
              {results.map((result, index) => {
                const isActive = index === activeIndex;

                return (
                  <button
                    key={result.id}
                    type="button"
                    onMouseEnter={() => setActiveIndex(index)}
                    onClick={() => {
                      if (location.pathname !== "/my-skills") {
                        navigate("/my-skills");
                      }
                      openSkillDetailById(result.id);
                      closeGlobalSearch();
                    }}
                    className={cn(
                      "flex w-full items-center gap-3 rounded-2xl border px-4 py-3 text-left transition-all",
                      isActive
                        ? "border-accent-border bg-accent-bg/60 shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]"
                        : "border-transparent bg-surface hover:border-border hover:bg-surface-hover"
                    )}
                  >
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-background text-[12px] font-semibold text-accent-light">
                      {result.name.charAt(0).toUpperCase()}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="truncate text-[13px] font-semibold text-secondary">{result.name}</span>
                        <span className="rounded-full border border-border-subtle bg-background px-2 py-0.5 text-[10px] font-medium text-muted">
                          {result.sourceType}
                        </span>
                      </div>
                      <p className="mt-1 truncate text-[12px] text-muted">{result.description || "—"}</p>
                      <div className="mt-2 flex items-center gap-3 text-[11px] text-faint">
                        <span className="inline-flex items-center gap-1">
                          <FolderTree className="h-3 w-3" />
                          {t("search.scenarioCount", { count: result.scenarioCount })}
                        </span>
                        {result.enabledInScenario ? (
                          <span className="text-emerald-400">{t("search.enabledInCurrent")}</span>
                        ) : null}
                      </div>
                    </div>
                    <ArrowRight className={cn("h-4 w-4 shrink-0", isActive ? "text-accent" : "text-faint")} />
                  </button>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
