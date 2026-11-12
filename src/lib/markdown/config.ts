import remarkGfm from "remark-gfm";

export const supportsMarkdownGfm = (() => {
  try { new RegExp("(?<=a)b"); return true; } catch { return false; }
})();
export const markdownPlugins = supportsMarkdownGfm ? [remarkGfm] : [];

export function stripMarkdownFrontmatter(content: string) {
  const opening = /^---\r?\n/.exec(content);
  if (!opening) return content;
  const rest = content.slice(opening[0].length);
  const closing = /(?:^|\n)---(?:\r?\n|$)/.exec(rest);
  return closing ? rest.slice(closing.index + closing[0].length).trimStart() : content;
}
