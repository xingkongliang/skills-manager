import { useCallback, useEffect, useRef, useState } from "react";
import {
  Folder,
  ChevronDown,
  ChevronUp,
  Github,
  HardDrive,
  Globe,
  Check,
  Plus,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import {
  getSkillDocument,
  getSourceSkillDocument,
  setSkillPresets,
  type ManagedSkill,
  type Preset,
  type Project,
  type SkillDocument,
  type SourceSkillDocument,
  type SkillToolToggle,
  type ToolInfo,
} from "../lib/tauri";
import { DocumentDiffViewer } from "./DocumentDiffViewer";
import { DetailSheet } from "./DetailSheet";
import { SkillMarkdown } from "./SkillMarkdown";
import { AgentToggleSection, type AgentToggleItem } from "./AgentToggleSection";
import { SkillProjectsSection } from "./SkillProjectsSection";
import { SyncDots } from "./SyncDots";
import { getErrorMessage } from "../lib/error";

interface Props {
  skill: ManagedSkill | null;
  onClose: () => void;
  tools?: ToolInfo[];
  toolToggles?: SkillToolToggle[] | null;
  togglingTool?: string | null;
  onToggleTool?: (tool: string, enabled: boolean) => void;
  projects?: Project[];
  onProjectsChanged?: () => void;
  presets?: Preset[];
  onRefresh?: () => void;
}

export function SkillDetailPanel({
  skill,
  onClose,
  tools,
  toolToggles,
  togglingTool,
  onToggleTool,
  projects,
  onProjectsChanged,
  presets,
  onRefresh,
}: Props) {
  if (!skill) return null;

  const panelKey = [
    skill.id,
    skill.updated_at,
    skill.source_type,
    skill.source_ref ?? "",
    skill.source_revision ?? "",
    skill.remote_revision ?? "",
  ].join(":");

  return (
    <SkillDetailPanelContent
      key={panelKey}
      skill={skill}
      onClose={onClose}
      tools={tools}
      toolToggles={toolToggles}
      togglingTool={togglingTool}
      onToggleTool={onToggleTool}
      projects={projects}
      onProjectsChanged={onProjectsChanged}
      presets={presets}
      onRefresh={onRefresh}
    />
  );
}

function SkillDetailPanelContent({
  skill,
  onClose,
  tools,
  toolToggles,
  togglingTool,
  onToggleTool,
  projects,
  onProjectsChanged,
  presets,
  onRefresh,
}: {
  skill: ManagedSkill;
  onClose: () => void;
  tools?: ToolInfo[];
  toolToggles?: SkillToolToggle[] | null;
  togglingTool?: string | null;
  onToggleTool?: (tool: string, enabled: boolean) => void;
  projects?: Project[];
  onProjectsChanged?: () => void;
  presets?: Preset[];
  onRefresh?: () => void;
}) {
  const { t } = useTranslation();
  const [doc, setDoc] = useState<SkillDocument | null>(null);
  const [sourceDoc, setSourceDoc] = useState<SourceSkillDocument | null>(null);
  const [loading, setLoading] = useState(true);
  const [isMetadataExpanded, setIsMetadataExpanded] = useState(false);
  const [contentTab, setContentTab] = useState<"local" | "diff" | "source">("local");
  const localRequestIdRef = useRef(0);
  const sourceRequestIdRef = useRef(0);
  const skillId = skill.id;
  const [presetToggleId, setPresetToggleId] = useState<string | null>(null);

  const handleTogglePreset = useCallback(async (presetId: string) => {
    if (presetToggleId) return;
    setPresetToggleId(presetId);
    try {
      const currentPresets = skill.preset_ids;
      const nextPresets = currentPresets.includes(presetId)
        ? currentPresets.filter((id) => id !== presetId)
        : [...currentPresets, presetId];
      await setSkillPresets(skill.id, nextPresets);
      toast.success(currentPresets.includes(presetId) ? t("presetSelector.removed") : t("presetSelector.added"));
      onRefresh?.();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("presetSelector.error")));
    } finally {
      setPresetToggleId(null);
    }
  }, [skill.id, skill.preset_ids, presetToggleId, onRefresh]);

  const supportsSourceDiff =
    skill.source_type === "git"
    || skill.source_type === "skillssh"
    || ((skill.source_type === "local" || skill.source_type === "import") && !!skill.source_ref);
  const [sourceLoading, setSourceLoading] = useState(supportsSourceDiff);
  const localDocVersion = `${skill.id}:${skill.updated_at}`;
  const sourceDocVersion = [
    skill.id,
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

  useEffect(() => {
    if (!supportsSourceDiff) {
      return;
    }

    sourceRequestIdRef.current += 1;
    const requestId = sourceRequestIdRef.current;

    getSourceSkillDocument(skillId)
      .then((nextDoc) => {
        if (requestId === sourceRequestIdRef.current) {
          setSourceDoc(nextDoc);
        }
      })
      .catch(() => {
        if (requestId === sourceRequestIdRef.current) {
          setSourceDoc(null);
        }
      })
      .finally(() => {
        if (requestId === sourceRequestIdRef.current) {
          setSourceLoading(false);
        }
      });
  }, [skillId, supportsSourceDiff, sourceDocVersion]);

  const sourceIcon = (type: string) => {
    switch (type) {
      case "git":
      case "skillssh":
        return <Github className="h-3.5 w-3.5" />;
      case "local":
      case "import":
        return <HardDrive className="h-3.5 w-3.5" />;
      default:
        return <Globe className="h-3.5 w-3.5" />;
    }
  };

  const sourceTypeLabel = (type: string) => (type === "skillssh" ? "skills.sh" : type);

  const metadataItems = [
    { label: t("mySkills.sourceType"), value: sourceTypeLabel(skill.source_type) },
    { label: t("mySkills.sourceRef"), value: skill.source_ref },
    { label: t("mySkills.sourceResolved"), value: skill.source_ref_resolved },
    { label: t("mySkills.sourceBranch"), value: skill.source_branch },
    { label: t("mySkills.sourceSubpath"), value: skill.source_subpath },
    { label: t("mySkills.sourceRevision"), value: skill.source_revision },
  ].filter((item) => Boolean(item.value));

  const activeDoc = doc?.skill_id === skill.id ? doc : null;
  const activeSourceDoc = sourceDoc?.skill_id === skill.id ? sourceDoc : null;
  const toggleItems: AgentToggleItem[] = (toolToggles ?? []).map((toggle) => ({
    key: toggle.tool,
    displayName: toggle.display_name,
    enabled: toggle.enabled,
    isAvailable: toggle.installed && toggle.globally_enabled,
    disabled: !toggle.installed || !toggle.globally_enabled,
    badgeLabel: !toggle.installed
      ? t("mySkills.agentToggleNotInstalled")
      : !toggle.globally_enabled
        ? t("mySkills.agentToggleDisabledGlobally")
        : null,
  }));

  const meta = (
    <>
      <div className="flex flex-wrap items-center gap-2 text-[12.5px] text-muted">
        {tools && <SyncDots skill={skill} tools={tools} size="sm" includeOrphan />}
        {skill.tags.length > 0 && (
          <>
            {tools && <span className="mx-0.5 h-3 w-px bg-border-subtle" />}
            {skill.tags.map((tag) => (
              <span
                key={tag}
                className="inline-flex items-center rounded-full bg-surface-hover px-2 py-0.5 text-[11px] font-medium text-secondary"
              >
                {tag}
              </span>
            ))}
          </>
        )}
      </div>
      <div className="mt-3 flex min-w-0 items-center gap-2 text-[13px] text-muted">
        <Folder className="h-3.5 w-3.5 shrink-0" />
        <span className="font-mono truncate" title={skill.central_path}>
          {skill.central_path}
        </span>
      </div>
      {presets && presets.length > 0 && (
        <div className="mt-4 rounded-xl border border-border-subtle bg-surface/70 px-4 py-3">
          <div className="mb-2 text-[13px] font-semibold text-secondary">{t("presetSelector.title")}</div>
          <div className="flex flex-wrap gap-2">
            {presets.map((preset) => {
              const enabled = skill.preset_ids.includes(preset.id);
              return (
                <button
                  key={preset.id}
                  type="button"
                  onClick={() => handleTogglePreset(preset.id)}
                  disabled={presetToggleId === preset.id}
                  className={cn(
                    "inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[13px] font-medium transition-colors",
                    enabled
                      ? "bg-accent text-white"
                      : "border border-border-subtle bg-bg-secondary text-muted hover:text-secondary"
                  )}
                >
                  {enabled ? <Check className="h-3 w-3" /> : <Plus className="h-3 w-3" />}
                  {preset.icon && <span>{preset.icon}</span>}
                  {preset.name}
                </button>
              );
            })}
          </div>
        </div>
      )}
      {metadataItems.length > 0 && (
        <div className="mt-4 rounded-xl border border-border-subtle bg-surface/70">
          <button
            type="button"
            onClick={() => setIsMetadataExpanded((prev) => !prev)}
            aria-expanded={isMetadataExpanded}
            aria-controls="skill-source-metadata"
            className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left"
          >
            <span className="flex min-w-0 items-center gap-2">
              <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-border-subtle bg-bg-secondary px-2 py-1 text-[12px] text-muted">
                {sourceIcon(skill.source_type)}
                {sourceTypeLabel(skill.source_type)}
              </span>
              <span className="truncate text-[13px] font-medium text-secondary">
                {t("mySkills.sourceType")}
              </span>
            </span>
            <span className="inline-flex shrink-0 items-center gap-1 text-[12px] text-muted">
              <span>
                {isMetadataExpanded
                  ? t("mySkills.collapseAgentToggles")
                  : t("mySkills.expandAgentToggles")}
              </span>
              {isMetadataExpanded ? (
                <ChevronUp className="h-3.5 w-3.5" />
              ) : (
                <ChevronDown className="h-3.5 w-3.5" />
              )}
            </span>
          </button>
          {isMetadataExpanded && (
            <div id="skill-source-metadata" className="border-t border-border-subtle px-4 py-3">
              <div className="grid gap-2 md:grid-cols-2">
                {metadataItems.map((item) => (
                  <div key={item.label} className="min-w-0">
                    <div className="text-[11px] font-medium uppercase tracking-[0.08em] text-faint">
                      {item.label}
                    </div>
                    <div
                      className="mt-0.5 truncate font-mono text-[12.5px] text-secondary"
                      title={item.value ?? undefined}
                    >
                      {item.value}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </>
  );

  return (
    <DetailSheet
      open={true}
      title={skill.name}
      description={skill.description ? <p className="line-clamp-3">{skill.description}</p> : undefined}
      meta={meta}
      onClose={onClose}
    >
      {toolToggles && onToggleTool && (
        <AgentToggleSection
          items={toggleItems}
          togglingKey={togglingTool}
          onToggle={onToggleTool}
          className="mb-4"
        />
      )}

      {projects && projects.length > 0 && (
        <SkillProjectsSection
          skill={skill}
          projects={projects}
          onChanged={onProjectsChanged}
        />
      )}

      {supportsSourceDiff && (
        <div className="mb-4 flex flex-wrap items-center gap-2">
          {(["local", "diff", "source"] as const).map((tab) => (
            <button
              key={tab}
              type="button"
              onClick={() => setContentTab(tab)}
              className={cn(
                "rounded-full px-3 py-1.5 text-[12px] font-medium transition-colors",
                contentTab === tab
                  ? "bg-accent text-white"
                  : "bg-surface-hover text-muted hover:text-secondary"
              )}
              disabled={(tab === "diff" || tab === "source") && sourceLoading}
            >
              {tab === "local"
                ? t("mySkills.docTabs.local")
                : tab === "diff"
                  ? t("mySkills.docTabs.diff")
                  : t("mySkills.docTabs.source")}
            </button>
          ))}
          {activeSourceDoc && (
            <span className="rounded-full border border-border-subtle bg-surface px-2 py-1 text-[12px] text-muted">
              {activeSourceDoc.source_label} · {activeSourceDoc.revision.slice(0, 7)}
            </span>
          )}
        </div>
      )}

      {loading ? (
        <div className="mt-12 text-center text-[13px] text-muted">{t("common.loading")}</div>
      ) : contentTab === "diff" ? (
        activeDoc && activeSourceDoc ? (
          <DocumentDiffViewer original={activeDoc.content} updated={activeSourceDoc.content} />
        ) : sourceLoading ? (
          <div className="mt-12 text-center text-[13px] text-muted">{t("common.loading")}</div>
        ) : (
          <div className="mt-12 text-center text-[13px] text-muted">{t("mySkills.sourceDiffUnavailable")}</div>
        )
      ) : contentTab === "source" ? (
        sourceLoading ? (
          <div className="mt-12 text-center text-[13px] text-muted">{t("common.loading")}</div>
        ) : activeSourceDoc ? (
          <SkillMarkdown content={activeSourceDoc.content} />
        ) : (
          <div className="mt-12 text-center text-[13px] text-muted">{t("mySkills.sourceDiffUnavailable")}</div>
        )
      ) : activeDoc ? (
        <SkillMarkdown content={activeDoc.content} />
      ) : (
        <div className="mt-12 text-center text-[13px] text-muted">{t("common.documentMissing")}</div>
      )}
    </DetailSheet>
  );
}
