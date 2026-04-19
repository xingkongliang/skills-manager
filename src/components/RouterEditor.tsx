import { useState } from "react";

type Initial = { description: string; body?: string | null };

type Props = {
  packId: string;
  initial: Initial;
  onSave: (v: { description: string; body: string | null }) => Promise<void>;
  onGenerate?: () => void;
  onPreview?: () => void;
};

export function RouterEditor({
  initial,
  onSave,
  onGenerate,
  onPreview,
}: Props) {
  const [desc, setDesc] = useState(initial.description);
  const [body, setBody] = useState(initial.body ?? "");
  const len = desc.length;
  const color =
    len <= 400 ? "text-green-600" : len <= 600 ? "text-yellow-600" : "text-red-600";
  const canSave = desc.trim().length > 0;

  return (
    <div className="space-y-3">
      <label className="block">
        <span className="text-sm font-medium">Router description</span>
        <textarea
          className="w-full border rounded p-2 font-mono text-sm"
          rows={3}
          value={desc}
          onChange={(e) => setDesc(e.target.value)}
          aria-label="Router description"
        />
      </label>
      <div data-testid="char-counter" className={`text-xs ${color}`}>
        {len} chars (target 150–400)
      </div>

      <label className="block">
        <span className="text-sm font-medium">
          Body (optional — leave empty for auto-render)
        </span>
        <textarea
          className="w-full border rounded p-2 font-mono text-sm"
          rows={8}
          value={body}
          onChange={(e) => setBody(e.target.value)}
          aria-label="Router body"
        />
      </label>

      <div className="flex gap-2">
        <button
          type="button"
          className="px-3 py-1 bg-blue-600 text-white rounded disabled:opacity-50"
          disabled={!canSave}
          onClick={() =>
            onSave({ description: desc.trim(), body: body.trim() || null })
          }
        >
          Save
        </button>
        {onGenerate && (
          <button
            type="button"
            className="px-3 py-1 border rounded"
            onClick={onGenerate}
          >
            Generate with Claude Code
          </button>
        )}
        {onPreview && (
          <button
            type="button"
            className="px-3 py-1 border rounded"
            onClick={onPreview}
          >
            Preview Sync Output
          </button>
        )}
      </div>
    </div>
  );
}
