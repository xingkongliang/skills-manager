import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  ArrowLeft,
  ArrowUpRight,
  Binary,
  CloudOff,
  Copy,
  FileQuestion,
  FileText,
  FileWarning,
  FileX,
  FolderOpen,
  Github,
  Globe,
  HardDrive,
  RefreshCw,
  RotateCcw,
  SquareTerminal,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText as clipboardWriteText } from "@tauri-apps/plugin-clipboard-manager";
import { cn, compactHomePath } from "../utils";
import { getErrorMessage } from "../lib/error";
import {
  getSkillDocument,
  getSourceSkillDocument,
  getSkillSourceDiff,
  listSkillFiles,
  readSkillFile,
  revealSkillFolder,
  type ManagedSkill,
  type Project,
  type SkillDocument,
  type SkillFile,
  type SkillFileContent,
  type SourceSkillDocument,
  type SkillSourceDiff,
  type SkillToolToggle,
  type ToolInfo,
} from "../lib/tauri";
import { SkillSourceDiffViewer } from "./SkillSourceDiffViewer";
import { DetailSheet } from "./DetailSheet";
import { SkillMarkdown } from "./SkillMarkdown";
import { AgentToggleSection, type AgentToggleItem } from "./AgentToggleSection";
import { SkillProjectsSection } from "./SkillProjectsSection";
import { SyncDots } from "./SyncDots";
import { SegmentedTabs, type SegmentedTab } from "./SegmentedTabs";
import { CardActionMenu, type CardAction } from "./CardActionMenu";
import { SkillTagEditor } from "./SkillTagEditor";
import { resolveRelativePath } from "../lib/relativeLink";
import { DetailEmptyPaper, DetailEmptyState, DetailPaper } from "./DetailPaper";
import { ToggleSwitch } from "./ToggleSwitch";

type ContentTab = "local" | "diff" | "source" | "files";

interface Props {
  skill: ManagedSkill | null;
  onClose: () => void;
  tools?: ToolInfo[];
  toolToggles?: SkillToolToggle[] | null;
  togglingTool?: string | null;
  onToggleTool?: (tool: string, enabled: boolean) => void;
  /** Whether `toolToggles` belong to the active preset, the only one that deploys. */
  isActivePreset: boolean;
  projects?: Project[];
  onProjectsChanged?: () => void;
  /** Name of the preset being viewed; null when none is. */
  viewedPresetName?: string | null;
  /** Whether the skill is in the viewed preset. */
  inViewedPreset?: boolean;
  /** Same as the card's switch: adds the skill to, or removes it from, the viewed preset. */
  onTogglePreset?: () => void;
  /** The card's "…" menu, so the two never drift apart. */
  menuActions?: CardAction[];
  /** Whether the skill has a source to update or reimport from. */
  canRefresh?: boolean;
  refreshLabel?: string;
  updating?: boolean;
  checking?: boolean;
  onRefresh?: () => void;
  onCheckUpdate?: () => void;
  onRelinkSource?: () => void;
  onDetachSource?: () => void;
  allTags?: string[];
  /** Resolves true once the tag is added. */
  onAddTag?: (tag: string) => Promise<boolean>;
  onRemoveTag?: (tag: string) => void;
}

export function SkillDetailPanel(props: Props) {
  if (!props.skill) return null;
  return <SkillDetailPanelContent {...props} skill={props.skill} />;
}

function SkillDetailPanelContent({
  skill,
  onClose,
  tools,
  toolToggles,
  togglingTool,
  onToggleTool,
  isActivePreset,
  projects,
  onProjectsChanged,
  viewedPresetName,
  inViewedPreset = false,
  onTogglePreset,
  menuActions,
  canRefresh,
  refreshLabel,
  updating,
  checking,
  onRefresh,
  onCheckUpdate,
  onRelinkSource,
  onDetachSource,
  allTags,
  onAddTag,
  onRemoveTag,
}: Props & { skill: ManagedSkill }) {
  const { t, i18n } = useTranslation();
  const [requestedTab, setContentTab] = useState<ContentTab>("local");
  const [openFilePath, setOpenFilePath] = useState<string | null>(null);
  const [doc, setDoc] = useState<SkillDocument | null>(null);
  // Each result remembers the version it was fetched for. An update, or a
  // check that finds a new revision, changes the version: stale results stop
  // showing and the open tab fetches again, without remounting the sheet
  // (which would throw both columns back to the top). `null` = failed.
  const [sourceResult, setSourceResult] = useState<{ version: string; doc: SourceSkillDocument | null } | null>(null);
  const [diffResult, setDiffResult] = useState<{ version: string; diff: SkillSourceDiff | null } | null>(null);
  const [filesResult, setFilesResult] = useState<{ version: string; files: SkillFile[] | null } | null>(null);
  const [fileResult, setFileResult] = useState<{
    path: string;
    content: SkillFileContent | null;
  } | null>(null);
  const [loading, setLoading] = useState(true);
  const [tagEditing, setTagEditing] = useState(false);
  const [tagValue, setTagValue] = useState("");
  const localRequestIdRef = useRef(0);
  const sourceRequestedRef = useRef<string | null>(null);
  const diffRequestedRef = useRef<string | null>(null);
  const filesRequestedRef = useRef<string | null>(null);
  const fileRequestIdRef = useRef(0);
  const skillId = skill.id;
  const supportsSourceDiff =
    skill.source_type === "git"
    || skill.source_type === "skillssh"
    || ((skill.source_type === "local" || skill.source_type === "import") && !!skill.source_ref);
  const contentTab: ContentTab =
    !supportsSourceDiff && (requestedTab === "diff" || requestedTab === "source")
      ? "local"
      : requestedTab;
  const localDocVersion = `${skill.id}:${skill.updated_at}`;
  const sourceVersion = [
    skill.id,
    skill.updated_at,
    skill.source_type,
    skill.source_ref ?? "",
    skill.source_ref_resolved ?? "",
    skill.source_revision ?? "",
    skill.remote_revision ?? "",
  ].join(":");

  useEffect(() => {
    localRequestIdRef.current += 1;
    const requestId = localRequestIdRef.current;

    getSkillDocument(skillId)
      .then((nextDoc) => {
        if (requestId === localRequestIdRef.current) {
          setDoc(nextDoc);
        }
      })
      .catch(() => {
        if (requestId === localRequestIdRef.current) {
          setDoc(null);
        }
      })
      .finally(() => {
        if (requestId === localRequestIdRef.current) {
          setLoading(false);
        }
      });
  }, [skillId, localDocVersion]);

  // The source document and the whole-directory diff load only when their tab
  // is opened. For git/skills.sh skills each one resolves the remote and
  // clones, so opening the sheet must not pay for either. A response is kept
  // only while its version is still the latest one requested: a slow clone
  // for an older revision must not land after the newer one and replace it.
  useEffect(() => {
    if (contentTab !== "source" || !supportsSourceDiff) return;
    if (sourceRequestedRef.current === sourceVersion) return;
    sourceRequestedRef.current = sourceVersion;

    getSourceSkillDocument(skillId)
      .then((nextDoc) => {
        if (sourceRequestedRef.current === sourceVersion) setSourceResult({ version: sourceVersion, doc: nextDoc });
      })
      .catch(() => {
        if (sourceRequestedRef.current === sourceVersion) setSourceResult({ version: sourceVersion, doc: null });
      });
  }, [contentTab, supportsSourceDiff, skillId, sourceVersion]);

  useEffect(() => {
    if (contentTab !== "diff" || !supportsSourceDiff) return;
    if (diffRequestedRef.current === sourceVersion) return;
    diffRequestedRef.current = sourceVersion;

    getSkillSourceDiff(skillId)
      .then((diff) => {
        if (diffRequestedRef.current === sourceVersion) setDiffResult({ version: sourceVersion, diff });
      })
      .catch(() => {
        if (diffRequestedRef.current === sourceVersion) setDiffResult({ version: sourceVersion, diff: null });
      });
  }, [contentTab, supportsSourceDiff, skillId, sourceVersion]);

  useEffect(() => {
    if (contentTab !== "files") return;
    if (filesRequestedRef.current === localDocVersion) return;
    filesRequestedRef.current = localDocVersion;

    listSkillFiles(skillId)
      .then((list) => {
        if (filesRequestedRef.current === localDocVersion) setFilesResult({ version: localDocVersion, files: list });
      })
      .catch(() => {
        if (filesRequestedRef.current === localDocVersion) setFilesResult({ version: localDocVersion, files: null });
      });
  }, [contentTab, skillId, localDocVersion]);

  // A stale list keeps showing until the fresh one arrives, so the column
  // does not collapse to a loading line and lose its scroll position.
  const files = filesResult?.files ?? null;
  const filesFailed = filesResult !== null && filesResult.files === null;
  const openFileListed = openFilePath !== null && !!files?.some((f) => f.relative_path === openFilePath);

  useEffect(() => {
    fileRequestIdRef.current += 1;
    const requestId = fileRequestIdRef.current;
    if (contentTab !== "files" || !openFilePath || !openFileListed) return;

    // `content: null` records a failed read.
    readSkillFile(skillId, openFilePath)
      .then((content) => {
        if (requestId === fileRequestIdRef.current) setFileResult({ path: openFilePath, content });
      })
      .catch(() => {
        if (requestId === fileRequestIdRef.current) setFileResult({ path: openFilePath, content: null });
      });
  }, [contentTab, openFilePath, openFileListed, skillId, localDocVersion]);

  // Links resolve against the folder of the document they appear in. One
  // that climbs out of the skill keeps its written form, which no listed
  // file can match, so the Files tab reports it as not found.
  const openRelativeLink = (fromFile: string) => (path: string) => {
    setOpenFilePath(resolveRelativePath(fromFile, path) ?? path);
    setContentTab("files");
  };

  const activeDoc = doc?.skill_id === skill.id ? doc : null;
  const currentSource = sourceResult?.version === sourceVersion ? sourceResult : null;
  const currentDiff = diffResult?.version === sourceVersion ? diffResult : null;
  const activeSourceDoc = currentSource?.doc ?? null;
  const activeSourceDiff = currentDiff?.diff ?? null;
  const sourceFailed = currentSource !== null && currentSource.doc === null;
  const sourceDiffFailed = currentDiff !== null && currentDiff.diff === null;
  const toggleItems: AgentToggleItem[] = (toolToggles ?? []).map((toggle) => ({
    key: toggle.tool,
    displayName: toggle.display_name,
    enabled: toggle.enabled,
    deployed: toggle.deployed,
    isAvailable: toggle.installed && toggle.globally_enabled,
    disabled: !toggle.installed || !toggle.globally_enabled,
    badgeLabel: !toggle.installed
      ? t("mySkills.agentToggleNotInstalled")
      : !toggle.globally_enabled
        ? t("mySkills.agentToggleDisabledGlobally")
        : null,
  }));

  const statusBadge =
    skill.update_status === "source_missing"
      ? t("mySkills.updateStatus.sourceMissing")
      : skill.update_status === "error"
        ? t("mySkills.updateStatus.error")
        : null;
  const showUpdatePill = skill.update_status === "update_available" && !!canRefresh && !!onRefresh;

  const actions = (
    <>
      {statusBadge && (
        <span className="rounded-full bg-danger-bg px-2 py-0.5 text-[11px] font-medium text-danger">
          {statusBadge}
        </span>
      )}
      {showUpdatePill && (
        <button
          type="button"
          onClick={onRefresh}
          disabled={updating}
          className="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-full bg-amber-500/12 px-2.5 text-[12px] font-medium text-amber-600 outline-none transition-colors hover:bg-amber-500/20 disabled:opacity-50 dark:text-amber-400"
          title={refreshLabel}
        >
          <RotateCcw className={cn("h-3 w-3", updating && "animate-spin")} />
          {t("mySkills.updateActions.update")}
        </button>
      )}
      {menuActions && menuActions.length > 0 && (
        <CardActionMenu label={t("mySkills.moreActions")} actions={menuActions} size="md" />
      )}
    </>
  );

  const handleAddTag = async (raw?: string) => {
    if (!onAddTag) return;
    const value = (raw ?? tagValue).trim();
    if (!value || skill.tags.includes(value)) {
      setTagValue("");
      return;
    }
    if (await onAddTag(value)) {
      setTagEditing(false);
      setTagValue("");
    }
  };

  const meta = onAddTag && onRemoveTag ? (
    <SkillTagEditor
      tags={skill.tags}
      allTags={allTags ?? skill.tags}
      editing={tagEditing}
      value={tagValue}
      onValueChange={setTagValue}
      onStartEditing={() => { setTagEditing(true); setTagValue(""); }}
      onStopEditing={() => { setTagEditing(false); setTagValue(""); }}
      onAdd={(tag) => { void handleAddTag(tag); }}
      onRemove={onRemoveTag}
      emptyAddLabel={t("mySkills.tags.addTagShort")}
    />
  ) : skill.tags.length > 0 ? (
    <div className="flex flex-wrap items-center gap-1">
      {skill.tags.map((tag) => (
        <span
          key={tag}
          className="inline-flex items-center rounded-full bg-surface-hover px-2 py-0.5 text-[11px] font-medium text-secondary"
        >
          {tag}
        </span>
      ))}
    </div>
  ) : null;

  const aside = (
    <div className="divide-y divide-border-faint">
      <section className="px-5 py-4">
        <div className="flex min-w-0 items-center justify-between gap-2">
          <h3 className="app-section-title min-w-0 truncate">
            {viewedPresetName ? (
              <>
                {t("mySkills.detail.preset")} · <span className="normal-case tracking-normal">{viewedPresetName}</span>
              </>
            ) : (
              t("mySkills.detail.agents")
            )}
          </h3>
          {viewedPresetName && onTogglePreset && (
            <ToggleSwitch
              checked={inViewedPreset}
              onChange={onTogglePreset}
              title={inViewedPreset ? t("mySkills.enabledButton") : t("mySkills.enable")}
            />
          )}
        </div>
        {/* Rows only while the skill is in the preset: right after it is
            switched off, the toggles still in hand belong to the old state. */}
        {toolToggles && onToggleTool && (inViewedPreset || !viewedPresetName) ? (
          <AgentToggleSection
            className="mt-2"
            items={toggleItems}
            togglingKey={togglingTool}
            onToggle={onToggleTool}
            showPendingSync={isActivePreset}
          />
        ) : tools ? (
          <div className="mt-2">
            <div className="text-[12px] text-muted">{t("mySkills.detail.deployedTo")}</div>
            <SyncDots className="mt-1.5 flex-wrap" skill={skill} tools={tools} includeOrphan />
          </div>
        ) : null}
      </section>

      {projects && projects.length > 0 && (
        <section className="px-5 py-4">
          <SkillProjectsSection skill={skill} projects={projects} onChanged={onProjectsChanged} />
        </section>
      )}

      <section className="px-5 py-4">
        <h3 className="app-section-title">{t("mySkills.detail.source")}</h3>
        <SourceDetails
          skill={skill}
          locale={i18n.language}
          checking={!!checking}
          updating={!!updating}
          onCheckUpdate={onCheckUpdate}
          onRelinkSource={onRelinkSource}
          onDetachSource={onDetachSource}
        />
      </section>
    </div>
  );

  const tabs: SegmentedTab<ContentTab>[] = [
    { key: "local", label: t("mySkills.docTabs.local") },
    ...(supportsSourceDiff
      ? [
          { key: "diff" as const, label: t("mySkills.docTabs.diff") },
          { key: "source" as const, label: t("mySkills.docTabs.source") },
        ]
      : []),
    { key: "files", label: t("mySkills.docTabs.files") },
  ];

  const loadingPaper = <DetailEmptyPaper loading text={t("common.loading")} />;
  const unavailablePaper = <DetailEmptyPaper icon={CloudOff} text={t("mySkills.sourceDiffUnavailable")} />;

  // Documents sit on a white paper; the diff and the file list are panels of
  // their own and stay on the grey background.
  let body: ReactNode;
  if (contentTab === "diff") {
    body = activeSourceDiff
      ? <SkillSourceDiffViewer entries={activeSourceDiff.entries} truncated={activeSourceDiff.truncated} />
      : sourceDiffFailed ? unavailablePaper : loadingPaper;
  } else if (contentTab === "source") {
    body = activeSourceDoc
      ? <DetailPaper><SkillMarkdown content={activeSourceDoc.content} /></DetailPaper>
      : sourceFailed ? unavailablePaper : loadingPaper;
  } else if (contentTab === "files") {
    body = (
      <SkillFilesView
        files={files}
        filesFailed={filesFailed}
        openFilePath={openFilePath}
        openFileListed={openFileListed}
        fileContent={fileResult?.path === openFilePath ? fileResult.content : null}
        fileFailed={fileResult?.path === openFilePath && fileResult.content === null}
        onOpen={setOpenFilePath}
        onRelativeLink={openRelativeLink}
      />
    );
  } else if (loading) {
    body = loadingPaper;
  } else {
    body = activeDoc ? (
      <DetailPaper>
        <SkillMarkdown content={activeDoc.content} onRelativeLink={openRelativeLink(activeDoc.filename)} />
      </DetailPaper>
    ) : (
      <DetailEmptyPaper icon={FileQuestion} text={t("common.documentMissing")} />
    );
  }

  const sourceType = sourceTypeLabel(skill.source_type);
  const sourceShort = shortSourceRef(skill.source_ref);

  return (
    <DetailSheet
      open={true}
      title={skill.name}
      icon={sourceIcon(skill.source_type, "h-[18px] w-[18px]")}
      subtitle={sourceShort ? `${sourceType} · ${sourceShort}` : sourceType}
      description={skill.description}
      meta={meta}
      actions={actions}
      aside={aside}
      onClose={onClose}
    >
      <SegmentedTabs className="mb-4" tabs={tabs} value={contentTab} onChange={setContentTab} />
      {body}
    </DetailSheet>
  );
}

function sourceIcon(type: string, className = "h-3.5 w-3.5") {
  const Icon = type === "git" || type === "skillssh"
    ? Github
    : type === "local" || type === "import"
      ? HardDrive
      : Globe;
  return <Icon className={cn("shrink-0", className)} />;
}

function sourceTypeLabel(type: string) {
  return type === "skillssh" ? "skills.sh" : type;
}

/** `owner/repo` for a web address, a `~`-shortened path otherwise. */
function shortSourceRef(ref: string | null): string | null {
  if (!ref) return null;
  if (/^https?:\/\//i.test(ref)) {
    try {
      const parts = new URL(ref).pathname.split("/").filter(Boolean);
      if (parts.length >= 2) return `${parts[0]}/${parts[1].replace(/\.git$/, "")}`;
      return new URL(ref).host;
    } catch {
      return ref;
    }
  }
  return compactHomePath(ref);
}

function formatRelativeTime(ms: number, locale: string): string {
  const seconds = Math.round((ms - Date.now()) / 1000);
  const abs = Math.abs(seconds);
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  if (abs < 60) return rtf.format(seconds, "second");
  if (abs < 3600) return rtf.format(Math.round(seconds / 60), "minute");
  if (abs < 86400) return rtf.format(Math.round(seconds / 3600), "hour");
  if (abs < 86400 * 30) return rtf.format(Math.round(seconds / 86400), "day");
  if (abs < 86400 * 365) return rtf.format(Math.round(seconds / (86400 * 30)), "month");
  return rtf.format(Math.round(seconds / (86400 * 365)), "year");
}

function SourceDetails({
  skill,
  locale,
  checking,
  updating,
  onCheckUpdate,
  onRelinkSource,
  onDetachSource,
}: {
  skill: ManagedSkill;
  locale: string;
  checking: boolean;
  updating: boolean;
  onCheckUpdate?: () => void;
  onRelinkSource?: () => void;
  onDetachSource?: () => void;
}) {
  const { t } = useTranslation();
  const ref = skill.source_ref;
  const isUrl = !!ref && /^https?:\/\//i.test(ref);
  const revision = skill.source_revision;
  const newRevision =
    skill.update_status === "update_available"
    && skill.remote_revision
    && skill.remote_revision !== revision
      ? skill.remote_revision
      : null;
  const missingLocalSource =
    skill.update_status === "source_missing"
    && (skill.source_type === "local" || skill.source_type === "import");

  const openSource = (url: string) => {
    openUrl(url).catch((error: unknown) => {
      toast.error(getErrorMessage(error, t("common.error")));
    });
  };

  // Label left, value right-aligned and truncated with the full value in its
  // title. Only branch, subpath and revision are code, so only they are mono.
  // The source type is in the header's summary line, so it is not repeated here.
  const rows: { label: string; value: ReactNode; title?: string; mono?: boolean }[] = [];
  if (ref) {
    rows.push({
      label: t("mySkills.detail.location"),
      title: ref,
      value: isUrl ? (
        <button
          type="button"
          onClick={() => openSource(ref)}
          className="inline-flex max-w-full items-center gap-0.5 text-accent-light hover:underline"
        >
          <span className="truncate">{shortSourceRef(ref)}</span>
          <ArrowUpRight className="h-3 w-3 shrink-0" />
        </button>
      ) : (
        compactHomePath(ref)
      ),
    });
  }
  if (skill.source_branch) {
    rows.push({ label: t("mySkills.sourceBranch"), value: skill.source_branch, title: skill.source_branch, mono: true });
  }
  if (skill.source_subpath) {
    rows.push({ label: t("mySkills.sourceSubpath"), value: skill.source_subpath, title: skill.source_subpath, mono: true });
  }
  if (revision) {
    rows.push({
      label: t("mySkills.sourceRevision"),
      title: newRevision ? `${revision} → ${newRevision}` : revision,
      mono: true,
      value: newRevision ? (
        <>
          {revision.slice(0, 7)} →{" "}
          <span className="text-amber-600 dark:text-amber-400">{newRevision.slice(0, 7)}</span>
        </>
      ) : (
        revision.slice(0, 7)
      ),
    });
  }
  const checkedText = skill.last_checked_at
    ? formatRelativeTime(skill.last_checked_at, locale)
    : t("mySkills.detail.neverChecked");
  rows.push({ label: t("mySkills.detail.lastCheckedLabel"), value: checkedText, title: checkedText });

  const copyPath = async () => {
    try {
      await clipboardWriteText(skill.central_path);
      toast.success(t("mySkills.detail.pathCopied"));
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const revealPath = () => {
    revealSkillFolder(skill.id).catch((error: unknown) => {
      toast.error(getErrorMessage(error, t("common.error")));
    });
  };

  const iconButton =
    "flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-muted transition-colors outline-none hover:bg-surface-active hover:text-secondary";

  return (
    <div className="mt-2.5 space-y-3 text-[12px]">
      <dl className="grid grid-cols-[auto_minmax(0,1fr)] items-center gap-x-4 gap-y-1.5">
        {rows.map((row) => (
          <div key={row.label} className="contents">
            <dt className="whitespace-nowrap text-muted">{row.label}</dt>
            <dd
              className={cn("min-w-0 truncate text-right text-secondary", row.mono && "font-mono tabular-nums")}
              title={row.title}
            >
              {row.value}
            </dd>
          </div>
        ))}
      </dl>

      {skill.last_check_error && (
        <p className="line-clamp-3 break-words text-danger" title={skill.last_check_error}>
          {skill.last_check_error}
        </p>
      )}

      {onCheckUpdate && (
        <button
          type="button"
          onClick={onCheckUpdate}
          disabled={checking}
          className="app-button-secondary h-7 w-full gap-1.5 px-2.5 py-0 text-[12px]"
        >
          <RefreshCw className={cn("h-3 w-3", checking && "animate-spin")} />
          {t("mySkills.updateActions.check")}
        </button>
      )}

      {missingLocalSource && (onRelinkSource || onDetachSource) && (
        <div className="flex gap-2">
          {onRelinkSource && (
            <button
              type="button"
              onClick={onRelinkSource}
              disabled={updating}
              className="app-button-secondary h-7 flex-1 px-2.5 py-0 text-[12px]"
            >
              {t("mySkills.updateActions.relink")}
            </button>
          )}
          {onDetachSource && (
            <button
              type="button"
              onClick={onDetachSource}
              disabled={updating}
              className="app-button-secondary h-7 flex-1 px-2.5 py-0 text-[12px]"
            >
              {t("mySkills.updateActions.detachSource")}
            </button>
          )}
        </div>
      )}

      <div className="flex min-w-0 items-center gap-1 rounded-lg bg-bg-secondary py-1.5 pr-1.5 pl-2.5">
        <span className="min-w-0 flex-1 truncate font-mono text-muted" title={skill.central_path}>
          {compactHomePath(skill.central_path)}
        </span>
        <button
          type="button"
          onClick={() => void copyPath()}
          className={iconButton}
          title={t("mySkills.detail.copyPath")}
          aria-label={t("mySkills.detail.copyPath")}
        >
          <Copy className="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          onClick={revealPath}
          className={iconButton}
          title={t("mySkills.detail.revealFolder")}
          aria-label={t("mySkills.detail.revealFolder")}
        >
          <FolderOpen className="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function SkillFilesView({
  files,
  filesFailed,
  openFilePath,
  openFileListed,
  fileContent,
  fileFailed,
  onOpen,
  onRelativeLink,
}: {
  files: SkillFile[] | null;
  filesFailed: boolean;
  openFilePath: string | null;
  openFileListed: boolean;
  fileContent: SkillFileContent | null;
  fileFailed: boolean;
  onOpen: (path: string | null) => void;
  /** Builds the link handler for a document at the given skill-relative path. */
  onRelativeLink: (fromFile: string) => (path: string) => void;
}) {
  const { t } = useTranslation();

  if (filesFailed) return <DetailEmptyPaper icon={CloudOff} text={t("mySkills.detail.filesUnavailable")} />;
  if (!files) return <DetailEmptyPaper loading text={t("common.loading")} />;

  if (openFilePath !== null) {
    let content: ReactNode;
    if (!openFileListed) {
      content = <DetailEmptyState icon={FileX} text={t("mySkills.detail.fileNotFound")} />;
    } else if (fileFailed) {
      content = <DetailEmptyState icon={CloudOff} text={t("mySkills.detail.filesUnavailable")} />;
    } else if (!fileContent) {
      content = <DetailEmptyState loading text={t("common.loading")} />;
    } else if (fileContent.content_kind === "binary") {
      content = <DetailEmptyState icon={Binary} text={t("mySkills.detail.fileBinary")} />;
    } else if (fileContent.content_kind === "too_large") {
      content = <DetailEmptyState icon={FileWarning} text={t("mySkills.detail.fileTooLarge")} />;
    } else if (/\.(md|markdown)$/i.test(openFilePath)) {
      content = <SkillMarkdown content={fileContent.text ?? ""} onRelativeLink={onRelativeLink(openFilePath)} />;
    } else {
      content = (
        <pre className="overflow-x-auto font-mono text-[12px] leading-5 text-secondary">{fileContent.text}</pre>
      );
    }
    return (
      <DetailPaper className="p-0">
        <div className="flex min-w-0 items-center gap-2 border-b border-border-faint px-3 py-2">
          <button
            type="button"
            onClick={() => onOpen(null)}
            className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted transition-colors outline-none hover:bg-surface-hover hover:text-secondary"
            title={t("mySkills.detail.backToFiles")}
            aria-label={t("mySkills.detail.backToFiles")}
          >
            <ArrowLeft className="h-4 w-4" />
          </button>
          <FilePath path={openFilePath} />
        </div>
        <div className="px-8 py-7">{content}</div>
      </DetailPaper>
    );
  }

  if (files.length === 0) return <DetailEmptyPaper icon={FileQuestion} text={t("mySkills.detail.noFiles")} />;

  return (
    <div className="app-panel divide-y divide-border-faint shadow-card">
      {files.map((file) => (
        <button
          key={file.relative_path}
          type="button"
          onClick={() => onOpen(file.relative_path)}
          className="flex w-full min-w-0 items-center gap-2.5 px-3.5 py-2 text-left transition-colors first:rounded-t-xl last:rounded-b-xl hover:bg-surface-hover"
        >
          {file.executable ? (
            <SquareTerminal className="h-3.5 w-3.5 shrink-0 text-muted" />
          ) : (
            <FileText className="h-3.5 w-3.5 shrink-0 text-muted" />
          )}
          <FilePath path={file.relative_path} />
          {file.executable && (
            <span
              className="shrink-0 rounded-full bg-surface-hover px-1.5 py-0.5 text-[11px] font-medium text-muted"
              title={t("mySkills.detail.executableHint")}
            >
              {t("mySkills.detail.executable")}
            </span>
          )}
          <span className="shrink-0 text-[12px] text-muted tabular-nums">{formatSize(file.size)}</span>
        </button>
      ))}
    </div>
  );
}

/** A skill-relative path as one truncating mono string: folder muted, file name brighter. */
function FilePath({ path }: { path: string }) {
  const slash = path.lastIndexOf("/");
  return (
    <span className="min-w-0 flex-1 truncate font-mono text-[12px]" title={path}>
      {slash >= 0 && <span className="text-muted">{path.slice(0, slash + 1)}</span>}
      <span className="text-secondary">{path.slice(slash + 1)}</span>
    </span>
  );
}
