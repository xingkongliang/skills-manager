import { Loader2, Plus, RefreshCw, RotateCcw, Trash2, X } from "lucide-react";
import { useRef } from "react";
import { useTranslation } from "react-i18next";

import { canRefreshSkill } from "../lib/librarySkillQuery";
import { skillCreator } from "../lib/skillCreator";
import { getTagColor } from "../lib/skillTags";
import { tagSuggestions } from "../lib/tagFilter";
import type { ManagedSkill } from "../lib/tauri";
import { cn } from "../utils";
import { CardActionMenu } from "./CardActionMenu";
import { CreatorBadge } from "./CreatorBadge";
import {
  type LibrarySkillItemProps,
  refreshLabel,
  renderDragHandle,
  sourceIcon,
  sourceTypeLabel,
  statusBadge,
} from "./librarySkillItem";
import { SortableItem } from "./SortableItem";
import { SyncDots } from "./SyncDots";
import { ToggleSwitch } from "./ToggleSwitch";

interface LibrarySkillCardProps extends LibrarySkillItemProps {
  /** This card's inline tag input is open. */
  tagEditing: boolean;
  tagInput: string;
  onTagInputChange: (value: string) => void;
  onTagEditSkillChange: (skillId: null | string) => void;
  onAddTag: (skill: ManagedSkill, inputValue?: string) => void;
  onRemoveTag: (skill: ManagedSkill, tag: string) => void;
}

/** A skill in the library's grid view. */
export function LibrarySkillCard({
  skill,
  displayName,
  hideCreator,
  enabledInPreset,
  hasViewedPreset,
  viewedPresetName,
  tools,
  allTags,
  isMultiSelect,
  selected,
  canDrag,
  deleting,
  hasConflict,
  checking,
  updating,
  menuOpen,
  pendingTool,
  onToggleSelect,
  onOpenDetail,
  onOpenBackup,
  onMenuSkillChange,
  onCheckUpdate,
  onRefresh,
  onRelinkSource,
  onDetachSource,
  onDelete,
  onTogglePreset,
  onToggleTarget,
  tagEditing,
  tagInput,
  onTagInputChange,
  onTagEditSkillChange,
  onAddTag,
  onRemoveTag,
}: LibrarySkillCardProps) {
  const { t } = useTranslation();
  const tagInputRef = useRef<HTMLInputElement>(null);
  const badge = statusBadge(skill, t);

  const hasUpdate = skill.update_status === "update_available" && canRefreshSkill(skill);

  // The header pill is hidden in multi-select, so the body badge has to
  // take over — otherwise the update state vanishes entirely.
  const showUpdatePill = hasUpdate && !isMultiSelect;

  const isMissingLocalSource =
    skill.update_status === "source_missing" &&
    (skill.source_type === "local" || skill.source_type === "import");

  const creator = skillCreator(skill);

  const getTagOptions = (skill: ManagedSkill, keyword: string) =>
    tagSuggestions(allTags, skill.tags, keyword);

  return (
    <SortableItem
      id={skill.id}
      disabled={!canDrag}
      className={tagEditing || menuOpen ? "relative z-30" : undefined}
    >
      {(handleProps) => (
        <div
          className={cn(
            "app-panel group relative flex h-full cursor-pointer flex-col shadow-card transition-all hover:-translate-y-px hover:border-border hover:shadow-card-hover",
            isMultiSelect && selected && "ring-1 ring-accent border-accent/40",
          )}
          onClick={() => (isMultiSelect ? onToggleSelect(skill.id) : onOpenDetail(skill.id))}
        >
          {deleting && (
            <div className="bg-surface/70 absolute inset-0 z-20 flex items-center justify-center rounded-xl backdrop-blur-[1px]">
              <Loader2 className="h-5 w-5 animate-spin text-muted" />
            </div>
          )}

          <div className="flex items-center gap-2.5 px-3.5 pb-1.5 pt-3">
            {/* Fixed-width slot: status dot / drag handle on hover / checkbox in multi-select */}
            <div className="relative flex h-4 w-4 shrink-0 items-center justify-center">
              {isMultiSelect ? (
                <input
                  type="checkbox"
                  checked={selected}
                  aria-label={t("mySkills.selectSkill", { name: displayName })}
                  onClick={(event) => event.stopPropagation()}
                  onChange={() => onToggleSelect(skill.id)}
                  className="h-4 w-4 accent-accent"
                />
              ) : (
                <>
                  <span
                    className={cn(
                      "h-2 w-2 rounded-full transition-opacity",
                      canDrag && "group-hover:opacity-0",
                      enabledInPreset
                        ? "bg-accent-light shadow-[0_0_0_3px_var(--color-accent-bg)]"
                        : "bg-surface-active",
                    )}
                    title={
                      enabledInPreset ? t("mySkills.enabledButton") : t("mySkills.notInPreset")
                    }
                  />
                  {canDrag && renderDragHandle(handleProps, t("mySkills.dragToReorder"))}
                </>
              )}
            </div>
            <h3
              className="flex-1 truncate text-[14px] font-semibold text-primary group-hover:text-accent-light"
              title={displayName}
            >
              <button
                type="button"
                className="max-w-full truncate text-left focus-visible:underline"
                onClick={(event) => {
                  event.stopPropagation();
                  onOpenDetail(skill.id);
                }}
              >
                {displayName}
              </button>
            </h3>
            {showUpdatePill && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  onRefresh(skill);
                }}
                disabled={updating}
                className="bg-amber-500/12 inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium text-amber-700 outline-none transition-colors hover:bg-amber-500/20 disabled:opacity-50 dark:text-amber-400"
                title={refreshLabel(skill, t)}
              >
                <RotateCcw className={cn("h-2.5 w-2.5", updating && "animate-spin")} />
                {t("mySkills.updateActions.update")}
              </button>
            )}
            {!isMultiSelect && (
              <>
                <CardActionMenu
                  label={t("mySkills.moreActions")}
                  onOpenChange={(open) => onMenuSkillChange(open ? skill.id : null)}
                  className={cn(
                    "transition-opacity",
                    menuOpen
                      ? "opacity-100"
                      : "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100",
                  )}
                  actions={[
                    {
                      key: "check",
                      label: t("mySkills.updateActions.check"),
                      icon: <RefreshCw className={cn("h-3.5 w-3.5", checking && "animate-spin")} />,
                      disabled: checking,
                      onSelect: () => onCheckUpdate(skill),
                    },
                    ...(canRefreshSkill(skill)
                      ? [
                          {
                            key: "refresh",
                            label: refreshLabel(skill, t),
                            icon: (
                              <RotateCcw
                                className={cn("h-3.5 w-3.5", updating && "animate-spin")}
                              />
                            ),
                            disabled: updating,
                            onSelect: () => onRefresh(skill),
                          },
                        ]
                      : []),
                    {
                      key: "delete",
                      label: t("common.delete"),
                      icon: <Trash2 className="h-3.5 w-3.5" />,
                      danger: true,
                      onSelect: () => onDelete(skill),
                    },
                  ]}
                />
                <ToggleSwitch
                  checked={enabledInPreset}
                  disabled={!hasViewedPreset}
                  onChange={() => onTogglePreset(skill)}
                  title={enabledInPreset ? t("mySkills.inPreset") : t("mySkills.notInPreset")}
                />
              </>
            )}
          </div>

          <div className="px-3.5 pb-3">
            <p className="line-clamp-2 text-[13px] leading-[18px] text-muted">
              {skill.description || "—"}
            </p>
            {((badge && !showUpdatePill) || hasConflict) && (
              <div className="mt-2 flex flex-wrap items-center gap-1.5">
                {hasConflict && (
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onOpenBackup();
                    }}
                    className="bg-amber-500/12 rounded-full px-2 py-0.5 text-[13px] font-medium text-amber-700 transition-colors hover:bg-amber-500/20 dark:text-amber-400"
                    title={t("mySkills.needsAttentionHint")}
                  >
                    {t("mySkills.needsAttention")}
                  </button>
                )}
                {badge && !showUpdatePill && (
                  <span
                    className={cn(
                      "rounded-full px-2 py-0.5 text-[13px] font-medium",
                      badge.className,
                    )}
                  >
                    {badge.label}
                  </span>
                )}
                {isMissingLocalSource && (
                  <>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        onRelinkSource(skill);
                      }}
                      disabled={updating}
                      className="rounded-full border border-border-subtle px-2 py-0.5 text-[12px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                    >
                      {t("mySkills.updateActions.relink")}
                    </button>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        onDetachSource(skill);
                      }}
                      disabled={updating}
                      className="rounded-full border border-border-subtle px-2 py-0.5 text-[12px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                    >
                      {t("mySkills.updateActions.detachSource")}
                    </button>
                  </>
                )}
              </div>
            )}
            <div className="mt-2 flex flex-wrap items-center gap-1">
              {skill.tags.map((tag) => (
                <span
                  key={tag}
                  className={cn(
                    "group/tag inline-flex items-center gap-0.5 rounded-full px-2 py-0.5 text-[11px] font-medium",
                    getTagColor(tag, allTags),
                  )}
                >
                  {tag}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      onRemoveTag(skill, tag);
                    }}
                    aria-label={t("mySkills.tags.removeTag", { tag })}
                    className="inline-flex rounded-full p-0 opacity-0 hover:opacity-100 focus:opacity-100 group-focus-within/tag:opacity-60 group-hover/tag:opacity-60"
                  >
                    <X className="h-2.5 w-2.5" />
                  </button>
                </span>
              ))}
              {tagEditing ? (
                <div
                  className="relative"
                  onClick={(e) => e.stopPropagation()}
                  onBlur={(e) => {
                    if (e.currentTarget.contains(e.relatedTarget)) return;

                    if (e.target === tagInputRef.current && tagInput.trim()) onAddTag(skill);
                    else {
                      onTagEditSkillChange(null);
                      onTagInputChange("");
                    }
                  }}
                >
                  <input
                    ref={tagInputRef}
                    type="text"
                    value={tagInput}
                    onChange={(e) => onTagInputChange(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        onAddTag(skill);
                      }

                      if (e.key === "Escape") {
                        onTagEditSkillChange(null);
                        onTagInputChange("");
                      }
                    }}
                    placeholder={t("mySkills.tags.addTag")}
                    className="h-5 w-28 rounded-full border border-border-subtle bg-transparent px-1.5 text-[11px] text-secondary outline-none focus:border-accent"
                    autoCapitalize="none"
                    autoCorrect="off"
                    autoComplete="off"
                    spellCheck={false}
                    autoFocus
                  />
                  {getTagOptions(skill, tagInput).length > 0 && (
                    <div className="absolute left-0 top-6 z-50 max-h-56 min-w-[112px] max-w-[180px] overflow-y-auto rounded-md border border-border-subtle bg-surface p-1 shadow-lg">
                      {getTagOptions(skill, tagInput).map((tagOption) => (
                        <button
                          key={tagOption}
                          type="button"
                          onMouseDown={(e) => e.preventDefault()}
                          onClick={(e) => {
                            e.stopPropagation();
                            onAddTag(skill, tagOption);
                          }}
                          className="w-full truncate rounded px-1.5 py-1 text-left text-[11px] text-secondary hover:bg-surface-hover"
                          title={tagOption}
                        >
                          {tagOption}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              ) : (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onTagEditSkillChange(skill.id);
                    onTagInputChange("");
                  }}
                  className="inline-flex items-center rounded-full p-0.5 text-faint opacity-0 transition-colors hover:text-muted group-focus-within:opacity-100 group-hover:opacity-100"
                  title={t("mySkills.tags.addTag")}
                >
                  <Plus className="h-3 w-3" />
                </button>
              )}
            </div>
          </div>

          <div className="mt-auto flex flex-wrap items-center justify-between gap-2 border-t border-border-faint px-3.5 py-2.5">
            <div className="flex min-w-0 items-center gap-1.5">
              <span className="inline-flex shrink-0 items-center gap-1 text-[12px] text-muted">
                {sourceIcon(skill.source_type)}
                {sourceTypeLabel(skill)}
              </span>
              {!hideCreator && creator.kind !== "local" && (
                <>
                  <span className="text-faint">·</span>
                  <CreatorBadge creator={creator} />
                </>
              )}
              {enabledInPreset && (
                <>
                  <span className="text-faint">·</span>
                  <span className="truncate text-[12px] font-medium text-amber-700 dark:text-amber-400/80">
                    {viewedPresetName}
                  </span>
                </>
              )}
            </div>
            <div className="flex items-center gap-1.5">
              <span className="text-[11px] text-muted">{t("dashboard.synced")}</span>{" "}
              <SyncDots
                className="shrink-0"
                skill={skill}
                tools={tools}
                limit={6}
                onToggle={
                  isMultiSelect
                    ? undefined
                    : (tool, enabled) => onToggleTarget(skill, tool, enabled)
                }
                pendingKey={pendingTool}
              />
            </div>
          </div>
        </div>
      )}
    </SortableItem>
  );
}
