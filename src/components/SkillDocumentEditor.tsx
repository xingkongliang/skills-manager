import { useCallback, useEffect, useRef, useState } from "react";
import { Check, FileText, Pencil, RefreshCw, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { getErrorMessage } from "../lib/error";
import { SkillMarkdown } from "./SkillMarkdown";
import { FrontmatterBadges, FrontmatterFields } from "./FrontmatterFields";
import { joinDocument, splitDocument } from "../lib/frontmatter";
import { ConfirmDialog } from "./ConfirmDialog";

/** The part of a document every workspace's detail panel has in common. */
export interface EditableDocument {
  filename: string;
  content: string;
  fingerprint: string;
}

/**
 * Marker the backend returns when the file changed under the editor. Matched
 * as a substring so the user is offered reload-or-overwrite instead of a raw
 * error they can do nothing with.
 */
const CHANGED_ON_DISK = "document_changed_on_disk";

/**
 * Marker the backend returns when the frontmatter no longer parses. Shown
 * beside the fields that caused it rather than as a toast that disappears
 * before the user can read the YAML error in it.
 */
const INVALID_FRONTMATTER = "invalid_frontmatter";

interface Props {
  document: EditableDocument | null;
  /** Save the draft. `expectedFingerprint` is `null` for a deliberate overwrite. */
  onSave: (content: string, expectedFingerprint: string | null) => Promise<EditableDocument>;
  /** Re-read the document from disk (used to recover from a conflict). */
  onReload: () => Promise<EditableDocument | null>;
  /** Kept in step with whether there is an unsaved draft. */
  onDirtyChange?: (dirty: boolean) => void;
  /** Hides the Edit button — the document is shown but not writable. */
  readOnly?: boolean;
}

/**
 * Read a skill's document, and edit it in place.
 *
 * Rendered markdown until the user asks to edit, then the raw file in a plain
 * textarea: a skill document is source that an agent parses, so the safest
 * editor is the one that shows exactly the bytes being stored.
 */
export function SkillDocumentEditor({
  document,
  onSave,
  onReload,
  onDirtyChange,
  readOnly,
}: Props) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [conflict, setConflict] = useState(false);
  const [frontmatterError, setFrontmatterError] = useState<string | null>(null);
  const [discardOpen, setDiscardOpen] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const content = document?.content ?? "";
  const dirty = editing && draft !== content;

  // Frontmatter gets its own fields for a skill's own document, and for any
  // other file that already carries a block. A plain README is left as plain
  // markdown rather than being offered fields it has no use for.
  const isSkillDocument = /^skill\.md$/i.test(document?.filename ?? "");
  const showFrontmatter = isSkillDocument || splitDocument(draft).frontmatter !== null;

  useEffect(() => {
    onDirtyChange?.(dirty);
  }, [dirty, onDirtyChange]);

  // A document arriving for a different skill must never leave the previous
  // skill's draft in the box.
  const documentKey = document ? `${document.filename}:${document.fingerprint}` : null;
  const lastKeyRef = useRef(documentKey);
  useEffect(() => {
    if (lastKeyRef.current === documentKey) return;
    lastKeyRef.current = documentKey;
    if (!editing) setDraft(content);
  }, [documentKey, content, editing]);

  const startEditing = useCallback(() => {
    setDraft(content);
    setConflict(false);
    setFrontmatterError(null);
    setEditing(true);
  }, [content]);

  useEffect(() => {
    if (editing) textareaRef.current?.focus();
  }, [editing]);

  const stopEditing = useCallback(() => {
    setEditing(false);
    setConflict(false);
    setFrontmatterError(null);
    setDiscardOpen(false);
  }, []);

  const save = useCallback(
    async (expectedFingerprint: string | null) => {
      if (!document || saving) return;
      setSaving(true);
      try {
        const saved = await onSave(draft, expectedFingerprint);
        setDraft(saved.content);
        setConflict(false);
        setFrontmatterError(null);
        setEditing(false);
        toast.success(t("skillEditor.saved", { filename: saved.filename }));
      } catch (error: unknown) {
        const message = getErrorMessage(error, t("common.error"));
        if (message.includes(CHANGED_ON_DISK)) {
          setConflict(true);
        } else if (message.includes(INVALID_FRONTMATTER)) {
          setFrontmatterError(message.split(`${INVALID_FRONTMATTER}: `).pop() ?? message);
        } else {
          toast.error(message);
        }
      } finally {
        setSaving(false);
      }
    },
    [document, draft, onSave, saving, t]
  );

  const reload = useCallback(async () => {
    const fresh = await onReload().catch(() => null);
    if (fresh) setDraft(fresh.content);
    setConflict(false);
    setEditing(false);
  }, [onReload]);

  const requestCancel = useCallback(() => {
    if (dirty) {
      setDiscardOpen(true);
      return;
    }
    stopEditing();
  }, [dirty, stopEditing]);

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void save(document?.fingerprint ?? null);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      // Stop the sheet from closing out from under an open editor — Escape
      // here means "leave the editor", not "leave the skill".
      event.stopPropagation();
      requestCancel();
    }
  };

  if (!document) {
    return (
      <div className="mt-12 text-center text-[13px] text-muted">{t("common.documentMissing")}</div>
    );
  }

  return (
    <div>
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <span className="inline-flex items-center gap-1.5 rounded-full border border-border-subtle bg-surface px-2.5 py-1 text-[12px] text-muted">
          <FileText className="h-3.5 w-3.5" />
          <span className="font-mono">{document.filename}</span>
          {dirty && <span className="text-accent-light">{t("skillEditor.unsaved")}</span>}
        </span>

        <div className="flex items-center gap-2">
          {editing ? (
            <>
              <button
                type="button"
                onClick={requestCancel}
                disabled={saving}
                className="inline-flex items-center gap-1.5 rounded-full bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition-colors hover:text-secondary disabled:opacity-60"
              >
                <X className="h-3.5 w-3.5" />
                {t("common.cancel")}
              </button>
              <button
                type="button"
                onClick={() => void save(document.fingerprint)}
                disabled={saving || !dirty}
                className="inline-flex items-center gap-1.5 rounded-full bg-accent px-3 py-1.5 text-[12px] font-medium text-white transition-opacity disabled:opacity-50"
              >
                <Check className="h-3.5 w-3.5" />
                {saving ? t("skillEditor.saving") : t("common.save")}
              </button>
            </>
          ) : (
            !readOnly && (
              <button
                type="button"
                onClick={startEditing}
                className="inline-flex items-center gap-1.5 rounded-full bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition-colors hover:text-secondary"
              >
                <Pencil className="h-3.5 w-3.5" />
                {t("skillEditor.edit")}
              </button>
            )
          )}
        </div>
      </div>

      {conflict && (
        <div className="mb-3 rounded-xl border border-amber-500/40 bg-amber-500/10 px-4 py-3">
          <p className="text-[13px] text-secondary">{t("skillEditor.conflictMessage")}</p>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void save(null)}
              disabled={saving}
              className="rounded-full bg-accent px-3 py-1.5 text-[12px] font-medium text-white disabled:opacity-50"
            >
              {t("skillEditor.conflictOverwrite")}
            </button>
            <button
              type="button"
              onClick={() => void reload()}
              disabled={saving}
              className="inline-flex items-center gap-1.5 rounded-full bg-surface-hover px-3 py-1.5 text-[12px] font-medium text-muted transition-colors hover:text-secondary disabled:opacity-60"
            >
              <RefreshCw className="h-3.5 w-3.5" />
              {t("skillEditor.conflictReload")}
            </button>
          </div>
        </div>
      )}

      {editing ? (
        <>
          {showFrontmatter && (
            <FrontmatterFields content={draft} onChange={setDraft} error={frontmatterError} />
          )}
          <textarea
            ref={textareaRef}
            value={showFrontmatter ? splitDocument(draft).body : draft}
            onChange={(event) =>
              setDraft(
                showFrontmatter
                  ? joinDocument(splitDocument(draft).frontmatter, event.target.value)
                  : event.target.value
              )
            }
            onKeyDown={handleKeyDown}
            spellCheck={false}
            className={cn(
              "min-h-[380px] w-full resize-y rounded-xl border border-border-subtle bg-background px-4 py-3",
              "font-mono text-[12.5px] leading-6 text-secondary outline-none",
              "focus:border-accent-border"
            )}
          />
        </>
      ) : (
        <>
          <FrontmatterBadges content={content} />
          <SkillMarkdown content={content} />
        </>
      )}

      <ConfirmDialog
        open={discardOpen}
        tone="warning"
        title={t("skillEditor.discardTitle")}
        message={t("skillEditor.discardMessage")}
        confirmLabel={t("skillEditor.discardConfirm")}
        onClose={() => setDiscardOpen(false)}
        onConfirm={async () => stopEditing()}
      />
    </div>
  );
}
