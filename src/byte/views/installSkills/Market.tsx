import { useEffect, useMemo, useRef, useState } from "react";
import {
  Search,
  RefreshCw,
  Loader2,
  DownloadCloud,
  ExternalLink,
  Check,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { useApp } from "../../../context/AppContext";
import { cn } from "../../../utils";
import { getErrorKind, getErrorMessage } from "../../../lib/error";
import * as api from "./frontend";
import type { Board, BrowseRequest, MarketPage, MarketSkill } from "./frontend";
import "./Market.css";

const PAGE_SIZE = 24;
const EMPTY_PAGE: MarketPage = { skills: [], total: 0, page: 1, page_size: PAGE_SIZE };
type Progress = { key: string; name: string; phase: string; cancelling: boolean };

export function ByteDanceMarket() {
  const { t } = useTranslation();
  const { t: bt } = useTranslation(undefined, { keyPrefix: "bytedance" });
  const { managedSkills, refreshManagedSkills, refreshPresets, openSkillDetailById } = useApp();
  const navigate = useNavigate();
  const [request, setRequest] = useState<BrowseRequest>({
    board: "all",
    query: "",
    page: 1,
    pageSize: PAGE_SIZE,
    refresh: false,
  });
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<MarketPage>(EMPTY_PAGE);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [installedNow, setInstalledNow] = useState<Record<string, string>>({});
  const optionsRef = useRef({ mounted: false, installKey: "" });

  useEffect(() => {
    const options = optionsRef.current;
    options.mounted = true;
    let disposed = false;
    const unlisten = listen<{ skill_id: string; phase: string }>(
      "install-progress",
      ({ payload }) => {
        if (!disposed && payload.skill_id === options.installKey) {
          setProgress((previous) => previous && { ...previous, phase: payload.phase });
        }
      },
    );
    return () => {
      disposed = true;
      options.mounted = false;
      void unlisten.then((stop) => stop()).catch(console.warn);
    };
  }, []);

  useEffect(() => {
    const timer = setTimeout(() => {
      setRequest((previous) =>
        previous.query === query.trim()
          ? previous
          : { ...previous, query: query.trim(), page: 1, refresh: false },
      );
    }, 450);
    return () => clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    let disposed = false;
    if (query.trim() !== request.query) return;
    void api
      .fetchSkills(request)
      .then((page) => {
        if (disposed) return;
        setResult(page);
        setError(null);
        setLoading(false);
      })
      .catch((reason: unknown) => {
        if (disposed) return;
        setError(getErrorMessage(reason, t("common.error")));
        setLoading(false);
      });
    return () => {
      disposed = true;
    };
  }, [request, reload, query, t]);

  const installed = useMemo(() => {
    const entries = managedSkills
      .filter((skill) => skill.source_type === "bytedance" && skill.source_ref)
      .map((skill) => [skill.source_ref!, skill.id]);
    return { ...Object.fromEntries(entries), ...installedNow } as Record<string, string>;
  }, [managedSkills, installedNow]);

  const changeRequest = (next: Partial<BrowseRequest>) => {
    setLoading(true);
    setError(null);
    setRequest((previous) => ({ ...previous, refresh: false, ...next }));
  };
  const install = async (skill: MarketSkill) => {
    if (optionsRef.current.installKey) return;
    const key = api.cancelKey(skill);
    optionsRef.current.installKey = key;
    setProgress({ key, name: skill.name, phase: "downloading", cancelling: false });
    try {
      const id = await api.installSkill(skill);
      if (optionsRef.current.mounted) {
        setInstalledNow((previous) => ({ ...previous, [skill.url]: id }));
      }
      await Promise.allSettled([refreshManagedSkills(), refreshPresets()]);
      toast.success(t("install.toast.success", { name: skill.name }), {
        action: {
          label: t("install.toast.view"),
          onClick: () => {
            openSkillDetailById(id);
            navigate("/my-skills");
          },
        },
      });
    } catch (reason: unknown) {
      if (getErrorKind(reason) === "cancelled") toast.info(t("install.toast.cancelled"));
      else toast.error(getErrorMessage(reason, t("common.error")));
    } finally {
      optionsRef.current.installKey = "";
      if (optionsRef.current.mounted) setProgress(null);
    }
  };
  const cancel = async () => {
    if (!progress || progress.cancelling) return;
    setProgress((previous) => previous && { ...previous, cancelling: true });
    try {
      await api.cancelInstall(progress.key);
    } catch (reason: unknown) {
      if (optionsRef.current.mounted)
        setProgress((previous) => previous && { ...previous, cancelling: false });
      toast.error(getErrorMessage(reason, t("common.error")));
    }
  };
  const totalPages = Math.max(1, Math.ceil(result.total / result.page_size));
  const showWeb = (url: string) => {
    void openUrl(url).catch((reason: unknown) =>
      toast.error(getErrorMessage(reason, t("common.error"))),
    );
  };
  const retry = () => {
    changeRequest({ refresh: true });
    setReload((previous) => previous + 1);
  };

  return (
    <div className="app-page gap-4">
      <div className="app-panel p-3.5">
        <div className="bytedance-controls">
          <div className="bytedance-search-row">
            <div className="app-segmented shrink-0 bg-background">
              {(["all", "biweekly", "monthly"] as Board[]).map((board) => (
                <button
                  key={board}
                  type="button"
                  aria-pressed={request.board === board}
                  onClick={() => changeRequest({ board, page: 1 })}
                  className={cn(
                    "app-segmented-button",
                    request.board === board && "app-segmented-button-active",
                  )}
                >
                  {bt(board)}
                </button>
              ))}
            </div>
            <div className="relative flex-1">
              <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <input
                className="app-input w-full bg-background pl-9"
                value={query}
                placeholder={bt("search")}
                aria-label={bt("search")}
                spellCheck={false}
                onChange={(event) => {
                  setQuery(event.target.value);
                  setLoading(true);
                  setError(null);
                }}
              />
            </div>
            <button
              type="button"
              className="app-button-secondary"
              disabled={loading}
              title={bt("refresh")}
              aria-label={bt("refresh")}
              onClick={retry}
            >
              <RefreshCw className="h-3.5 w-3.5" />
            </button>
          </div>
        </div>
      </div>
      {progress && (
        <div className="app-panel bytedance-status" role="status">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          {t("install.toast.installing", { name: progress.name })}
          <button
            className="app-button-secondary"
            type="button"
            disabled={progress.cancelling || progress.phase !== "downloading"}
            onClick={() => void cancel()}
          >
            {progress.cancelling ? bt("cancelling") : t("install.cancel")}
          </button>
        </div>
      )}
      {loading && (
        <div className="bytedance-status" role="status">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("common.loading")}
        </div>
      )}
      {!loading && error && (
        <div className="app-panel bytedance-status" role="alert">
          <span>{error}</span>
          <button className="app-button-secondary" onClick={retry}>
            {t("common.retry")}
          </button>
        </div>
      )}
      {!loading && !error && result.skills.length === 0 && (
        <div className="app-panel bytedance-status">{t("install.noResults.title")}</div>
      )}
      {!loading && !error && result.skills.length > 0 && (
        <div>
          <div className="bytedance-grid">
            {result.skills.map((skill) => {
              const id = installed[skill.url];
              const isInstalling = progress?.key === api.cancelKey(skill);
              return (
                <article key={skill.id} className="app-panel bytedance-card">
                  <div className="bytedance-card-heading">
                    <div className="min-w-0">
                      <h2
                        className="truncate text-[14px] font-semibold text-primary"
                        title={skill.name}
                      >
                        {skill.name}
                      </h2>
                      {skill.version && (
                        <span className="text-[12px] text-muted">v{skill.version}</span>
                      )}
                    </div>
                    <button
                      type="button"
                      className="app-button-secondary"
                      title={bt("web")}
                      aria-label={bt("web")}
                      onClick={() => showWeb(skill.url)}
                    >
                      <ExternalLink className="h-3.5 w-3.5" />
                    </button>
                  </div>
                  <p className="bytedance-description" title={skill.description}>
                    {skill.description}
                  </p>
                  <div className="bytedance-card-footer">
                    <span
                      className="inline-flex items-center gap-1 text-[12px] text-muted"
                      title={bt("downloads")}
                    >
                      <DownloadCloud className="h-3 w-3" />
                      {skill.downloads.toLocaleString()}
                    </span>
                    {id && (
                      <button
                        className="app-button-secondary"
                        type="button"
                        onClick={() => {
                          openSkillDetailById(id);
                          navigate("/my-skills");
                        }}
                      >
                        <Check className="h-3.5 w-3.5" />
                        {t("install.installed")}
                      </button>
                    )}
                    {!id && (
                      <button
                        className="app-button-primary"
                        type="button"
                        disabled={!!progress || skill.disabled || skill.no_permission}
                        onClick={() => void install(skill)}
                      >
                        {isInstalling
                          ? t("install.installing")
                          : skill.disabled || skill.no_permission
                            ? bt("unavailable")
                            : t("install.oneClickInstall")}
                      </button>
                    )}
                  </div>
                </article>
              );
            })}
          </div>
          <div className="bytedance-pagination">
            <button
              className="app-button-secondary"
              disabled={result.page <= 1}
              onClick={() => changeRequest({ page: result.page - 1 })}
            >
              <ChevronLeft className="h-3.5 w-3.5" />
              {t("install.pagination.previous")}
            </button>
            <span>{t("install.pagination.page", { current: result.page, total: totalPages })}</span>
            <button
              className="app-button-secondary"
              disabled={result.page >= totalPages}
              onClick={() => changeRequest({ page: result.page + 1 })}
            >
              {t("install.pagination.next")}
              <ChevronRight className="h-3.5 w-3.5" />
            </button>
          </div>
          <p className="mt-3 text-center text-[12px] text-muted">
            {t("install.pagination.summary", {
              start: (result.page - 1) * result.page_size + 1,
              end: Math.min(result.page * result.page_size, result.total),
              total: result.total,
            })}
          </p>
        </div>
      )}
    </div>
  );
}
