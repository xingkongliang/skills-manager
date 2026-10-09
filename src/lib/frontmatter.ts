/**
 * Minimal, surgical handling of a skill document's YAML frontmatter.
 *
 * Deliberately not a YAML round-trip. Loading a document into a parser and
 * dumping it back rewrites everything the author wrote — comments vanish,
 * quoting style changes, key order is normalised — for the sake of editing one
 * value. Here a field edit rewrites that field's line and nothing else, and
 * anything this file does not understand is handed to the raw editor exactly
 * as it was written.
 */

/** Frontmatter keys the structured editor offers fields for. */
export const KNOWN_FIELDS = [
  "name",
  "description",
  "model",
  "allowed-tools",
  "license",
] as const;

export type KnownField = (typeof KNOWN_FIELDS)[number];

/**
 * Values commonly accepted for `model`, offered as suggestions rather than a
 * closed list: every agent reads this field on its own terms, and a fixed
 * dropdown would lock out the next model and every non-Claude agent.
 */
export const MODEL_SUGGESTIONS = [
  "inherit",
  "opus",
  "sonnet",
  "haiku",
  "claude-opus-5",
  "claude-sonnet-5",
  "claude-haiku-4-5",
];

export interface SplitDocument {
  /** Text between the `---` fences, or `null` when there is no frontmatter. */
  frontmatter: string | null;
  /** Everything after the closing fence — the markdown the agent reads. */
  body: string;
}

export interface FieldValue {
  value: string;
  /**
   * The value continues onto later lines (a block scalar, a list, a nested
   * mapping). The structured field cannot represent it, so it defers to the
   * raw editor rather than flattening it.
   */
  multiline: boolean;
}

const KEY_LINE = /^([A-Za-z0-9_][A-Za-z0-9_.-]*)\s*:(?:\s(.*))?$/;

/** Split a document into its frontmatter block and its body. */
export function splitDocument(content: string): SplitDocument {
  const match = /^---[ \t]*\r?\n/.exec(content);
  if (!match) return { frontmatter: null, body: content };

  const start = match[0].length;
  const close = /^---[ \t]*(\r?\n|$)/m.exec(content.slice(start));
  if (!close) return { frontmatter: null, body: content };

  const frontmatter = content.slice(start, start + close.index);
  const body = content.slice(start + close.index + close[0].length);
  return { frontmatter: stripTrailingNewline(frontmatter), body };
}

/** Rebuild a document from a frontmatter block and a body. */
export function joinDocument(frontmatter: string | null, body: string): string {
  const trimmed = frontmatter?.trim();
  if (!trimmed) return body;
  return `---\n${stripTrailingNewline(frontmatter ?? "")}\n---\n${body}`;
}

/** Read one top-level scalar field out of a frontmatter block. */
export function readField(frontmatter: string | null, key: string): FieldValue | null {
  if (!frontmatter) return null;

  const lines = frontmatter.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const match = KEY_LINE.exec(lines[index]);
    if (!match || match[1] !== key) continue;

    const inline = (match[2] ?? "").trim();
    const continued = countContinuationLines(lines, index) > 0;
    return { value: continued ? inline : unquote(inline), multiline: continued };
  }
  return null;
}

/** Every top-level key the block declares, in the order they appear. */
export function listFields(frontmatter: string | null): string[] {
  if (!frontmatter) return [];
  return frontmatter
    .split("\n")
    .map((line) => KEY_LINE.exec(line)?.[1])
    .filter((key): key is string => Boolean(key));
}

/**
 * Set (or, with an empty value, remove) one top-level field.
 *
 * An existing key keeps its position; a new one is appended. Every other line
 * of the block — comments, blank lines, keys this editor knows nothing about —
 * comes through untouched.
 */
export function writeField(
  frontmatter: string | null,
  key: string,
  value: string
): string | null {
  const trimmed = value.trim();
  const lines = frontmatter === null ? [] : frontmatter.split("\n");

  for (let index = 0; index < lines.length; index += 1) {
    const match = KEY_LINE.exec(lines[index]);
    if (!match || match[1] !== key) continue;

    const span = 1 + countContinuationLines(lines, index);
    const replacement = trimmed ? [`${key}: ${quoteIfNeeded(trimmed)}`] : [];
    lines.splice(index, span, ...replacement);
    return normalizeBlock(lines);
  }

  if (!trimmed) return frontmatter;
  lines.push(`${key}: ${quoteIfNeeded(trimmed)}`);
  return normalizeBlock(lines);
}

/** How many lines after `index` belong to the value started on it. */
function countContinuationLines(lines: string[], index: number): number {
  let count = 0;
  for (let next = index + 1; next < lines.length; next += 1) {
    const line = lines[next];
    // A blank line inside a block scalar is part of it, but a trailing blank
    // line before the next key is not — look past it before deciding.
    if (line.trim() === "") {
      const following = lines.slice(next + 1).find((item) => item.trim() !== "");
      if (following === undefined || !/^\s/.test(following)) return count;
      count += 1;
      continue;
    }
    if (!/^\s/.test(line)) return count;
    count += 1;
  }
  return count;
}

/** Drop the blank lines a removal can leave at the edges of the block. */
function normalizeBlock(lines: string[]): string | null {
  while (lines.length && lines[0].trim() === "") lines.shift();
  while (lines.length && lines[lines.length - 1].trim() === "") lines.pop();
  return lines.length ? lines.join("\n") : null;
}

function stripTrailingNewline(text: string): string {
  return text.replace(/\r?\n$/, "");
}

/** Remove one layer of matching quotes, as a YAML reader would. */
function unquote(value: string): string {
  if (value.length >= 2 && value[0] === '"' && value.endsWith('"')) {
    return value.slice(1, -1).replace(/\\(["\\])/g, "$1");
  }
  if (value.length >= 2 && value[0] === "'" && value.endsWith("'")) {
    return value.slice(1, -1).replace(/''/g, "'");
  }
  return value;
}

/**
 * Quote a scalar only where a bare one would parse as something else — so an
 * ordinary description stays as readable in the file as the author typed it.
 */
function quoteIfNeeded(value: string): string {
  const needsQuotes =
    value === "" ||
    /^[-?:,[\]{}#&*!|>'"%@`]/.test(value) ||
    /:\s|\s#/.test(value) ||
    value !== value.trim() ||
    /^(true|false|null|yes|no|on|off|~)$/i.test(value) ||
    /^[-+]?[0-9.]+$/.test(value);

  if (!needsQuotes) return value;
  return `"${value.replace(/([\\"])/g, "\\$1")}"`;
}
