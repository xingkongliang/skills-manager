import { Plus, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { getTagColor } from "../lib/skillTags";
import { cn } from "../utils";

interface Props {
  tags: string[];
  /** Every tag in the library: picks the colours and fills the suggestions. */
  allTags: string[];
  /** Controlled, so a card can lift itself above its siblings while editing. */
  editing: boolean;
  value: string;
  onValueChange: (value: string) => void;
  onStartEditing: () => void;
  onStopEditing: () => void;
  /** Adds `tag`, or the typed value when omitted. */
  onAdd: (tag?: string) => void;
  onRemove: (tag: string) => void;
  className?: string;
  addButtonClassName?: string;
  /** Shown beside the "+" while there are no tags, so it is not a lone glyph. */
  emptyAddLabel?: string;
}

/** A skill's tags: hover a tag for its remove button, "+" to add one. */
export function SkillTagEditor({
  tags,
  allTags,
  editing,
  value,
  onValueChange,
  onStartEditing,
  onStopEditing,
  onAdd,
  onRemove,
  className,
  addButtonClassName,
  emptyAddLabel,
}: Props) {
  const { t } = useTranslation();
  const needle = value.trim().toLowerCase();
  const options = allTags.filter(
    (tag) => !tags.includes(tag) && (!needle || tag.toLowerCase().includes(needle))
  );

  return (
    <div className={cn("flex flex-wrap items-center gap-1", className)}>
      {tags.map((tag) => (
        <span
          key={tag}
          className={cn(
            "group/tag inline-flex items-center gap-0.5 rounded-full px-2 py-0.5 text-[11px] font-medium",
            getTagColor(tag, allTags)
          )}
        >
          {tag}
          <button
            onClick={(e) => { e.stopPropagation(); onRemove(tag); }}
            className="hidden group-hover/tag:inline-flex rounded-full p-0 opacity-60 hover:opacity-100"
          >
            <X className="h-2.5 w-2.5" />
          </button>
        </span>
      ))}
      {editing ? (
        <div className="relative" onClick={(e) => e.stopPropagation()}>
          <input
            type="text"
            value={value}
            onChange={(e) => onValueChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") { onAdd(); }
              if (e.key === "Escape") {
                // Consumed here: a detail sheet behind the input must not close too.
                e.preventDefault();
                onStopEditing();
              }
            }}
            onBlur={() => {
              if (value.trim()) onAdd();
              else onStopEditing();
            }}
            placeholder={t("mySkills.tags.addTag")}
            className="h-5 w-28 rounded-full border border-border-subtle bg-transparent px-1.5 text-[11px] text-secondary outline-none focus:border-accent"
            autoCapitalize="none"
            autoCorrect="off"
            autoComplete="off"
            spellCheck={false}
            autoFocus
          />
          {options.length > 0 && (
            <div className="absolute left-0 top-6 z-50 max-h-56 min-w-[112px] max-w-[180px] overflow-y-auto rounded-md border border-border-subtle bg-surface p-1 shadow-lg">
              {options.map((tagOption) => (
                <button
                  key={tagOption}
                  type="button"
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={(e) => { e.stopPropagation(); onAdd(tagOption); }}
                  className="w-full truncate rounded-md px-1.5 py-1 text-left text-[11px] text-secondary hover:bg-surface-hover"
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
          onClick={(e) => { e.stopPropagation(); onStartEditing(); }}
          className={cn(
            "inline-flex items-center rounded-full p-0.5 text-faint transition-colors hover:text-muted",
            emptyAddLabel && tags.length === 0 && "gap-1 pr-1.5 text-[11px] font-medium",
            addButtonClassName
          )}
          title={t("mySkills.tags.addTag")}
        >
          <Plus className="h-3 w-3" />
          {emptyAddLabel && tags.length === 0 && <span>{emptyAddLabel}</span>}
        </button>
      )}
    </div>
  );
}
