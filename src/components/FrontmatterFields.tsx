import { useId, useState } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import {
  KNOWN_FIELDS,
  MODEL_SUGGESTIONS,
  joinDocument,
  listFields,
  readField,
  splitDocument,
  writeField,
  type KnownField,
} from "../lib/frontmatter";

/** Fields that read better as a single line than as a paragraph. */
const SINGLE_LINE: ReadonlySet<KnownField> = new Set([
  "name",
  "model",
  "allowed-tools",
  "license",
]);

interface Props {
  /** The whole document — frontmatter and body. */
  content: string;
  onChange: (content: string) => void;
  /** The YAML error a refused save reported, if there was one. */
  error?: string | null;
}

/**
 * Structured editing of a skill's frontmatter, over the raw block.
 *
 * The document text stays the single source of truth: each field reads its
 * value out of the block and writes back only its own line, so a key this
 * editor has never heard of survives an edit to `model` untouched. The raw
 * block is right there under a disclosure for everything the fields cannot
 * express — a list, a folded description, a comment.
 */
export function FrontmatterFields({ content, onChange, error }: Props) {
  const { t } = useTranslation();
  const [rawOpen, setRawOpen] = useState(false);
  const modelListId = useId();

  const { frontmatter, body } = splitDocument(content);

  const setField = (key: KnownField, value: string) => {
    onChange(joinDocument(writeField(frontmatter, key, value), body));
  };

  const setRaw = (raw: string) => {
    onChange(joinDocument(raw, body));
  };

  const declared = listFields(frontmatter);
  const extraKeys = declared.filter(
    (key) => !KNOWN_FIELDS.includes(key as KnownField)
  );

  return (
    <div className="mb-3 rounded-xl border border-border-subtle bg-surface/70 px-4 py-3">
      <div className="mb-3 flex items-center justify-between gap-2">
        <span className="text-[11px] font-medium uppercase tracking-[0.08em] text-faint">
          {t("skillEditor.frontmatter")}
        </span>
        {extraKeys.length > 0 && (
          <span className="text-[11px] text-muted">
            {t("skillEditor.otherKeys", { keys: extraKeys.join(", ") })}
          </span>
        )}
      </div>

      {error && (
        <div className="mb-3 rounded-lg border border-red-500/40 bg-red-500/10 px-3 py-2">
          <p className="text-[12px] font-medium text-secondary">
            {t("skillEditor.invalidFrontmatter")}
          </p>
          <p className="mt-0.5 font-mono text-[11.5px] leading-5 text-muted">{error}</p>
        </div>
      )}

      <div className="grid gap-3">
        {KNOWN_FIELDS.map((key) => {
          const field = readField(frontmatter, key);
          const value = field?.value ?? "";
          const disabled = field?.multiline ?? false;

          return (
            <label key={key} className="grid gap-1">
              <span className="text-[11.5px] font-medium text-muted">
                <span className="font-mono">{key}</span>
                {disabled && (
                  <span className="ml-2 font-normal text-faint">
                    {t("skillEditor.multilineValue")}
                  </span>
                )}
              </span>
              {SINGLE_LINE.has(key) ? (
                <input
                  type="text"
                  value={value}
                  disabled={disabled}
                  list={key === "model" ? modelListId : undefined}
                  placeholder={t(`skillEditor.placeholder.${key}`)}
                  onChange={(event) => setField(key, event.target.value)}
                  className={fieldClass}
                />
              ) : (
                <textarea
                  rows={2}
                  value={value}
                  disabled={disabled}
                  placeholder={t(`skillEditor.placeholder.${key}`)}
                  onChange={(event) => setField(key, event.target.value)}
                  className={cn(fieldClass, "resize-y leading-6")}
                />
              )}
            </label>
          );
        })}
      </div>

      <datalist id={modelListId}>
        {MODEL_SUGGESTIONS.map((model) => (
          <option key={model} value={model} />
        ))}
      </datalist>

      <button
        type="button"
        onClick={() => setRawOpen((open) => !open)}
        aria-expanded={rawOpen}
        className="mt-3 inline-flex items-center gap-1 text-[12px] text-muted transition-colors hover:text-secondary"
      >
        {rawOpen ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
        {t("skillEditor.rawFrontmatter")}
      </button>

      {rawOpen && (
        <textarea
          value={frontmatter ?? ""}
          spellCheck={false}
          placeholder={t("skillEditor.rawFrontmatterPlaceholder")}
          onChange={(event) => setRaw(event.target.value)}
          className={cn(
            fieldClass,
            "mt-2 min-h-[140px] resize-y font-mono text-[12.5px] leading-6"
          )}
        />
      )}
    </div>
  );
}

const fieldClass = cn(
  "w-full rounded-lg border border-border-subtle bg-background px-3 py-2",
  "text-[12.5px] text-secondary outline-none placeholder:text-faint",
  "focus:border-accent-border disabled:opacity-60"
);

/**
 * The frontmatter worth seeing without opening the editor. `name` and
 * `description` are already the panel's title and subtitle, so showing them
 * again would just be the same words twice.
 */
export function FrontmatterBadges({ content }: { content: string }) {
  const { frontmatter } = splitDocument(content);
  const badges = (["model", "allowed-tools", "license"] as const)
    .map((key) => ({ key, field: readField(frontmatter, key) }))
    .filter((item) => item.field && item.field.value);

  if (badges.length === 0) return null;

  return (
    <div className="mb-3 flex flex-wrap items-center gap-1.5">
      {badges.map(({ key, field }) => (
        <span
          key={key}
          className="inline-flex items-center gap-1.5 rounded-full border border-border-subtle bg-surface px-2.5 py-1 text-[11.5px]"
          title={`${key}: ${field?.value ?? ""}`}
        >
          <span className="font-mono text-faint">{key}</span>
          <span className="max-w-[260px] truncate text-secondary">{field?.value}</span>
        </span>
      ))}
    </div>
  );
}
