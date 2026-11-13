import { useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from "react";
import { ArrowLeft, ArrowRight, ExternalLink, FolderOpen, Languages, Loader2, PanelLeft, RefreshCw, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import * as api from "../lib/tauri";
import { classifyReaderLink, imageBlobUrl, ReaderHistory, readerErrorKey } from "../lib/skillReader";
import { buildTranslationPlan } from "../lib/markdown/translationPlan";
import { useSkillBrowser } from "../hooks/useSkillBrowser";
import { useBilingualReading } from "../hooks/useBilingualReading";
import { useReaderPreferences } from "../hooks/useReaderPreferences";
import { SkillFileTree } from "./SkillFileTree";
import { SkillReaderMarkdown } from "./SkillReaderMarkdown";
import { cn } from "../utils";

export type ReaderMode = "original" | "bilingual";
interface Props { target: api.SkillBrowserTarget; active?: boolean; description?: string; initialMode?: ReaderMode }
const buttonClass = "inline-flex items-center gap-1.5 rounded-md px-2 py-1.5 text-[12px] text-secondary transition-colors hover:bg-surface-hover disabled:opacity-40";

export function SkillFileBrowser({ target, ...props }: Props) {
  return <SessionBrowser key={JSON.stringify(target)} target={target} {...props} />;
}

function SessionBrowser({ target, ...props }: Props) {
  const { t } = useTranslation();
  const browser = useSkillBrowser(target);
  const preferences = useReaderPreferences();
  const [mode, setMode] = useState<ReaderMode>(() => props.initialMode ?? (preferences.mode === "bilingual" ? "bilingual" : "original"));
  const [history] = useState(() => new ReaderHistory());
  return <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
    {browser.error != null && <div role="alert" className="mb-3 flex items-center justify-between gap-3 rounded-lg border border-border-subtle p-3 text-[13px] text-secondary">
      <span>{t(readerErrorKey(browser.error))}</span><button type="button" onClick={browser.reopen} className={buttonClass}>{t("reader.retry")}</button>
    </div>}
    {browser.session ? <BrowserContent key={browser.session.session_id} session={browser.session} history={history} mode={mode} onModeChange={(next) => { setMode(next); preferences.setMode(next); }} active={props.active ?? true} description={props.description} reopening={browser.opening} reopen={browser.reopen} /> : browser.opening ? <div className="flex items-center justify-center gap-2 py-12 text-[13px] text-muted"><Loader2 className="h-4 w-4 animate-spin" />{t("common.loading")}</div> : null}
  </div>;
}

function PreviewImage({ file }: { file: api.SkillFilePreview }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    const blob = imageBlobUrl(file.base64!, file.mime);
    // Blob creation and disposal are tied to this preview, never to the skill file.
    queueMicrotask(() => setUrl(blob));
    return () => URL.revokeObjectURL(blob);
  }, [file]);
  return url ? <img src={url} alt={file.relative_path} className="mx-auto max-h-[640px] max-w-full object-contain" /> : null;
}

function MarkdownReader({ file, sessionId, active, description, mode, onModeChange, onNavigate, rootRef }: {
  file: api.SkillFilePreview; sessionId: string; active: boolean; description?: string; onNavigate(href: string): void; rootRef: RefObject<HTMLDivElement | null>; mode: ReaderMode; onModeChange(mode: ReaderMode): void;
}) {
  const { t } = useTranslation();
  const preferences = useReaderPreferences();
  const plan = useMemo(() => buildTranslationPlan(file.content ?? ""), [file.content]);
  const bilingual = active && mode === "bilingual";
  const reading = useBilingualReading(sessionId, `${sessionId}:${file.relative_path}:${file.content_hash}`, plan, description, bilingual, preferences.target);
  useEffect(() => {
    const root = rootRef.current;
    if (!bilingual || !root || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) { const id = entry.target.getAttribute("data-reader-block"); if (id) { if (entry.isIntersecting) reading.visible.current.add(id); else reading.visible.current.delete(id); } }
    }, { root, rootMargin: "300px" });
    root.querySelectorAll("[data-reader-block]").forEach((node) => observer.observe(node));
    return () => observer.disconnect();
  }, [bilingual, plan, rootRef, reading.visible]);
  return <>
    <div className="sticky top-0 z-10 -mx-4 mb-4 border-b border-border-subtle bg-bg-secondary px-4 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <Languages className="h-3.5 w-3.5 text-muted" />
        {(["original", "bilingual"] as const).map((option) => <button key={option} type="button" onClick={() => onModeChange(option)} aria-pressed={mode === option} className={cn(buttonClass, mode === option && "bg-accent/10 text-accent-light")}>{t(`reader.${option}`)}</button>)}
        {mode === "bilingual" && <>
          <select aria-label={t("reader.targetLanguage")} value={preferences.target} onChange={(e) => preferences.setTarget(e.target.value as "zh-CN" | "zh-TW")} className="rounded-md border border-border-subtle bg-surface px-2 py-1 text-[12px] text-secondary">
            <option value="zh-CN">{t("reader.simplified")}</option><option value="zh-TW">{t("reader.traditional")}</option>
          </select>
          <span className="text-[12px] text-muted" aria-live="polite">{t("reader.progress", { done: reading.complete, total: reading.total })}</span>
          {reading.failed > 0 && <button type="button" onClick={reading.retry} title={t(reading.error ?? "reader.translationPartialFailure")} className={buttonClass}>{t("reader.retryFailed", { count: reading.failed })}</button>}
          {reading.paused && <button type="button" onClick={reading.retry} className={buttonClass}>{t("reader.resume")}</button>}
          {reading.hasMore && <button type="button" onClick={reading.more} className={buttonClass}>{t("reader.translateMore")}</button>}
        </>}
      </div>
      {mode === "bilingual" && <p className="mt-1 text-[11px] leading-5 text-muted">{t("reader.translationPrivacy")}{reading.memory ? ` ${t("reader.memoryCache")}` : ""}</p>}
    </div>
    {bilingual && reading.descriptionTranslation && <div className="mb-4 border-l-2 border-accent-border pl-3 text-[13px] leading-6 text-primary">{reading.descriptionTranslation}</div>}
    <SkillReaderMarkdown content={file.content ?? ""} plan={plan} translations={bilingual ? reading.translations : undefined} onNavigate={onNavigate} sessionId={sessionId} relativePath={file.relative_path} />
  </>;
}

function BrowserContent({ session, history, active, description, mode, onModeChange, reopening, reopen }: { session: api.SkillBrowserSession; history: ReaderHistory; active: boolean; description?: string; mode: ReaderMode; onModeChange(mode: ReaderMode): void; reopening: boolean; reopen(): void }) {
  const { t } = useTranslation();
  const preferences = useReaderPreferences();
  const [location, setLocation] = useState(() => history.initial(session.default_path));
  const [preview, setPreview] = useState<{ path: string; file?: api.SkillFilePreview; error?: unknown }>({ path: "" });
  const [refresh, setRefresh] = useState(0);
  const [treeOpen, setTreeOpen] = useState(false);
  const [revealDirectory, setRevealDirectory] = useState("");
  const [narrow, setNarrow] = useState(() => window.innerWidth - 270 < 900);
  const container = useRef<HTMLDivElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  const path = location?.path ?? "";
  const file = preview.path === path ? preview.file : undefined;
  const error = preview.path === path ? preview.error : undefined;
  const navigation = useRef(0);
  const lastLocation = useRef<typeof location>(null);
  const firstRead = useRef(true);
  useEffect(() => {
    if (!container.current || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => setNarrow(entry.contentRect.width < 900));
    observer.observe(container.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!path) return;
    let live = true;
    api.readSkillFile(session.session_id, path).then((next) => { if (live) { firstRead.current = false; setPreview({ path, file: next }); } }).catch((e: unknown) => {
      if (!live) return;
      const restoring = firstRead.current; firstRead.current = false;
      const missing = typeof e === "object" && e !== null && "kind" in e && e.kind === "not_found";
      if (restoring && session.revision && missing && session.default_path && path !== session.default_path) {
        setLocation(history.replaceCurrent(session.default_path)); toast.info(t("reader.fileFallback"));
      } else setPreview({ path, error: e });
    });
    return () => { live = false; };
  }, [path, session.session_id, session.revision, session.default_path, refresh, history, t]);
  useEffect(() => {
    if (session.revision) return;
    let disposed = false; let unlisten: (() => void) | undefined; let timer: ReturnType<typeof setTimeout>;
    void listen("app-files-changed", () => {
      clearTimeout(timer);
      timer = setTimeout(() => { history.saveScroll(scroll.current?.scrollTop ?? 0); setRefresh((n) => n + 1); }, 300);
    }).then((stop) => { if (disposed) stop(); else unlisten = stop; }).catch(() => {});
    return () => { disposed = true; clearTimeout(timer); unlisten?.(); };
  }, [session.revision, history]);
  useLayoutEffect(() => {
    if (!file || !scroll.current) return;
    const root = scroll.current;
    root.scrollTop = history.current?.scrollTop ?? 0;
    if (location?.fragment && lastLocation.current !== location) {
      const heading = Array.from(root.querySelectorAll<HTMLElement>("[id]")).find((node) => node.id === location.fragment);
      if (heading) root.scrollTop += heading.getBoundingClientRect().top - root.getBoundingClientRect().top - 76;
    }
    lastLocation.current = location;
  }, [file, location, history]);
  useEffect(() => () => { navigation.current++; }, []);

  const select = (next: string, fragment: string | null = null) => {
    navigation.current++;
    history.saveScroll(scroll.current?.scrollTop ?? 0);
    setLocation(history.push(next, fragment));
    setTreeOpen(false);
  };
  const move = (delta: number) => {
    navigation.current++;
    history.saveScroll(scroll.current?.scrollTop ?? 0);
    const next = history.move(delta); if (next) setLocation(next);
  };
  const report = (e: unknown) => toast.error(t(readerErrorKey(e)));
  const navigate = async (href: string) => {
    const generation = ++navigation.current;
    const type = classifyReaderLink(href);
    if (type === "blocked") { toast.error(t("reader.invalidPath")); return; }
    if (type === "external") { try { await openUrl(href); } catch (e) { report(e); } return; }
    try {
      const link = await api.resolveSkillLink(session.session_id, path, href);
      if (navigation.current !== generation) return;
      if (link.directory) { setRevealDirectory(link.relative_path); if (narrow) setTreeOpen(true); else preferences.setCollapsed(false); }
      else select(link.relative_path, link.fragment);
    } catch (e) { if (navigation.current === generation) report(e); }
  };
  const refreshContent = () => {
    history.saveScroll(scroll.current?.scrollTop ?? 0);
    // Reopening also renews an expired local session; history lives above it.
    reopen();
  };
  const showTree = narrow ? treeOpen : !preferences.collapsed;
  return <div ref={container} className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-bg-secondary">
    <div className="flex min-w-0 shrink-0 flex-wrap items-center gap-1 border-b border-border-subtle px-2 py-2">
      <button type="button" onClick={() => narrow ? setTreeOpen(!treeOpen) : preferences.setCollapsed(!preferences.collapsed)} className={buttonClass} aria-label={t("reader.files")} aria-expanded={showTree}><PanelLeft className="h-4 w-4" />{t("reader.files")}</button>
      <button type="button" onClick={() => move(-1)} disabled={!history.canBack} className={buttonClass} aria-label={t("reader.back")}><ArrowLeft className="h-3.5 w-3.5" /></button>
      <button type="button" onClick={() => move(1)} disabled={!history.canForward} className={buttonClass} aria-label={t("reader.forward")}><ArrowRight className="h-3.5 w-3.5" /></button>
      <span title={path} className="min-w-0 flex-1 truncate px-2 font-mono text-[12px] text-secondary">{path || t("reader.selectFile")}</span>
      {session.revision && <span title={session.revision} className="text-[11px] text-muted">{session.source_label} · {session.revision.slice(0, 7)}</span>}
      <button type="button" className={buttonClass} disabled={reopening} onClick={refreshContent} aria-label={t("reader.refresh")}><RefreshCw className={cn("h-3.5 w-3.5", reopening && "animate-spin")} /></button>
      <button type="button" className={buttonClass} onClick={() => void api.revealSkillFile(session.session_id, path).catch(report)} aria-label={t("reader.reveal")}><FolderOpen className="h-3.5 w-3.5" /></button>
    </div>
    <div className="relative flex min-h-0 min-w-0 flex-1">
      {narrow && treeOpen && <button type="button" className="absolute inset-0 z-20 bg-black/30" onClick={() => setTreeOpen(false)} aria-label={t("reader.closeFiles")} />}
      {showTree && <aside className={cn("w-[240px] shrink-0 overflow-x-hidden overflow-y-auto overscroll-contain border-r border-border-subtle bg-bg-secondary px-2 py-2", narrow && "absolute inset-y-0 left-0 z-30 shadow-card-hover")}>
        {narrow && <button type="button" onClick={() => setTreeOpen(false)} className={buttonClass} aria-label={t("reader.closeFiles")}><X className="h-3.5 w-3.5" />{t("reader.files")}</button>}
        <SkillFileTree key={refresh} sessionId={session.session_id} selected={path} revealDirectory={revealDirectory} onSelect={(next) => select(next)} />
      </aside>}
      <div ref={scroll} role="region" aria-label={t("reader.content")} tabIndex={0} className="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden overscroll-contain px-4 pb-5 pt-1">
        {!path ? <p className="py-12 text-center text-[13px] text-muted">{t("reader.selectFile")}</p> : error ? <div role="alert" className="py-12 text-center text-[13px] text-muted">{t(readerErrorKey(error))}</div> : !file ? <div className="flex justify-center py-12"><Loader2 className="h-4 w-4 animate-spin text-muted" /></div> : file.kind === "markdown" ? <MarkdownReader key={`${path}:${file.content_hash}`} file={file} sessionId={session.session_id} active={active} description={description} mode={mode} onModeChange={onModeChange} onNavigate={(href) => void navigate(href)} rootRef={scroll} /> : file.kind === "text" ? <pre className="my-4 max-w-full overflow-x-auto rounded-xl bg-background p-4 font-mono text-[13px] leading-6 text-secondary"><code>{file.content}</code></pre> : file.kind === "image" ? <div className="py-4"><PreviewImage file={file} /></div> : <div className="space-y-3 py-12 text-center text-[13px] text-secondary">
          <p>{t(`reader.unsupported.${file.reason ?? "format"}`)}</p><p className="text-[12px] text-muted">{(file.size_bytes / 1024).toFixed(1)} KiB</p>
          <button type="button" className={buttonClass} onClick={() => void api.openSkillFileExternal(session.session_id, path).catch(report)}><ExternalLink className="h-3.5 w-3.5" />{t("reader.openExternal")}</button>
        </div>}
      </div>
    </div>
  </div>;
}
