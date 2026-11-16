import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight, FileText, Folder, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { listSkillDirectory, type SkillDirectoryPage, type SkillFileEntry } from "../lib/tauri";
import { readerErrorKey } from "../lib/skillReader";
import { cn } from "../utils";

interface Props { sessionId: string; selected: string; onSelect: (path: string) => void; revealDirectory?: string }

function Branch({ sessionId, selected, onSelect, revealDirectory, path = "", depth = 0 }: Props & { path?: string; depth?: number }) {
  const { t } = useTranslation();
  const [page, setPage] = useState<SkillDirectoryPage | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let live = true;
    listSkillDirectory(sessionId, path).then((next) => { if (live) { setPage(next); setError(null); } }).catch((e: unknown) => { if (live) setError(e); });
    return () => { live = false; };
  }, [sessionId, path, retry]);
  const more = async () => {
    if (page?.next_cursor == null || loadingMore) return;
    setLoadingMore(true);
    try { const next = await listSkillDirectory(sessionId, path, page.next_cursor); setPage((previous) => ({ ...next, entries: [...(previous?.entries ?? []), ...next.entries] })); }
    catch (e) { setError(e); }
    finally { setLoadingMore(false); }
  };
  return <>
    {error ? <button type="button" className="px-3 py-2 text-left text-[12px] text-muted" onClick={() => setRetry((n) => n + 1)}>{t(readerErrorKey(error))} · {t("reader.retry")}</button> : !page ? <Loader2 className="m-3 h-3.5 w-3.5 animate-spin text-muted" /> : null}
    {page?.entries.map((entry) => <TreeEntry key={entry.relative_path} {...{ sessionId, selected, onSelect, revealDirectory, entry, depth }} />)}
    {page?.next_cursor != null && <button type="button" className="px-3 py-2 text-[12px] text-accent-light" onClick={() => void more()} disabled={loadingMore}>{t("reader.loadMore")}</button>}
    {page?.truncated && <p className="p-3 text-[12px] text-muted">{t("reader.directoryLimit")}</p>}
    {page && !page.entries.length && !error && <p className="px-3 py-2 text-[12px] text-muted">{t("reader.emptyDirectory")}</p>}
  </>;
}

function TreeEntry({ entry, depth, ...props }: Props & { entry: SkillFileEntry; depth: number }) {
  const [open, setOpen] = useState(false);
  const [closedFor, setClosedFor] = useState<string | null>(null);
  const directory = entry.kind === "directory";
  const navigation = `${props.selected}|${props.revealDirectory ?? ""}`;
  const expanded = closedFor !== navigation && (open || props.selected.startsWith(`${entry.relative_path}/`) || props.revealDirectory === entry.relative_path || props.revealDirectory?.startsWith(`${entry.relative_path}/`));
  return <div>
    <button type="button" title={entry.relative_path} aria-expanded={directory ? Boolean(expanded) : undefined} aria-current={!directory && props.selected === entry.relative_path ? "page" : undefined} disabled={!entry.accessible}
      onClick={() => { if (directory) { setOpen(!expanded); setClosedFor(expanded ? navigation : null); } else props.onSelect(entry.relative_path); }}
      className={cn("flex w-full min-w-0 items-center gap-1.5 rounded-md py-1.5 pr-2 text-left text-[12px] transition-colors disabled:opacity-40", props.selected === entry.relative_path ? "bg-accent/10 text-accent-light" : "text-secondary hover:bg-surface-hover")}
      style={{ paddingLeft: 8 + depth * 14 }}>
      {directory ? expanded ? <ChevronDown className="h-3 w-3 shrink-0" /> : <ChevronRight className="h-3 w-3 shrink-0" /> : <span className="w-3 shrink-0" />}
      {directory ? <Folder className="h-3.5 w-3.5 shrink-0 text-muted" /> : <FileText className="h-3.5 w-3.5 shrink-0 text-muted" />}
      <span className="truncate">{entry.name}</span>
    </button>
    {directory && expanded && <Branch {...props} path={entry.relative_path} depth={depth + 1} />}
  </div>;
}

export function SkillFileTree(props: Props) { return <nav><Branch {...props} /></nav>; }
