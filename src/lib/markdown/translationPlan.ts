import { needsTranslation } from "../readerLanguage";
import { unified, type Plugin } from "unified";
import remarkParse from "remark-parse";
import type { Root } from "mdast";
import { markdownPlugins, stripMarkdownFrontmatter } from "./config";

interface AstNode {
  type: string; value?: string; children?: AstNode[];
  position?: { start: { offset?: number }; end: { offset?: number } };
  data?: Record<string, unknown>;
}
export interface TranslationBlock {
  id: string; request: string; tokens: string[]; skipped: boolean;
}
export interface TranslationPlan {
  markdown: string; blocks: TranslationBlock[]; anchors: Map<string, string>;
}
const nodeId = (node: AstNode) => `${node.type}:${node.position?.start.offset ?? 0}:${node.position?.end.offset ?? 0}`;
const PROTECTED = /⟦S[^⟧]*⟧|https?:\/\/[^\s<>]+|(?:\.{0,2}\/|~\/)[\w./@%+~:-]+|\b[\w.-]+\.(?:md|py|sh|ts|tsx|js|json|ya?ml|toml|dart|rs)\b|\$\{[^}]+\}|\{\{[^}]+\}\}/g;

function requestText(node: AstNode, tokens: string[]): string {
  const protect = (value: string) => { const token = `⟦S${tokens.length}⟧`; tokens.push(value); return token; };
  if (node.type === "inlineCode") return protect(node.value ?? "");
  if (node.type === "html" || node.type === "image" || node.type === "imageReference") return "";
  if (node.type === "text") return (node.value ?? "").replace(PROTECTED, protect);
  if (node.type === "break") return "\n";
  return node.children?.map((child) => requestText(child, tokens)).join("") ?? "";
}

export function buildTranslationPlan(content: string, target = "zh-CN"): TranslationPlan {
  const markdown = stripMarkdownFrontmatter(content).replace(/\r\n?/g, "\n");
  const processor = unified().use(remarkParse);
  for (const plugin of markdownPlugins) processor.use(plugin);
  const tree = processor.parse(markdown) as unknown as AstNode;
  const blocks: TranslationBlock[] = [];
  const anchors = new Map<string, string>();
  const headingIds = new Set<string>();
  function visit(node: AstNode) {
    if (["paragraph", "heading", "tableCell"].includes(node.type)) {
      const tokens: string[] = [];
      const request = requestText(node, tokens).trim();
      const prose = request.replace(/⟦S\d+⟧/g, "");
      const skipped = !needsTranslation(prose, target);
      blocks.push({ id: nodeId(node), request, tokens, skipped });
      if (node.type === "heading") {
        const plain = restoreProtectedText(request, tokens);
        const base = plain.toLowerCase().replace(/[^\w\u0080-\uffff\s-]/g, "").trim().replace(/\s+/g, "-") || "section";
        let anchor = base; let suffix = 0;
        while (headingIds.has(anchor)) anchor = `${base}-${++suffix}`;
        headingIds.add(anchor);
        anchors.set(nodeId(node), anchor);
      }
      return;
    }
    if (node.type !== "code" && node.type !== "html") node.children?.forEach(visit);
  }
  visit(tree);
  return { markdown, blocks, anchors };
}

export function restoreProtectedText(text: string, tokens: string[]) {
  const seen = new Set<number>();
  if ((text.match(/⟦S[^⟧]*⟧/g) ?? []).some((marker) => !/^⟦S\d+⟧$/.test(marker))) throw new Error("reader.protectionFailed");
  const restored = text.replace(/⟦S(\d+)⟧/g, (_, raw: string) => {
    const index = Number(raw);
    if (index >= tokens.length || seen.has(index)) throw new Error("reader.protectionFailed");
    seen.add(index);
    return tokens[index];
  });
  if (seen.size !== tokens.length) throw new Error("reader.protectionFailed");
  return restored;
}

export function splitTranslationText(text: string): string[] {
  const pieces: string[] = [];
  let remaining = text;
  while (remaining.length > 3500) {
    let end = 3500;
    const boundary = remaining.lastIndexOf(" ", end - 1);
    if (boundary > 1750) end = boundary + 1;
    if (/[\uD800-\uDBFF]/.test(remaining[end - 1])) end--;
    const tokenStart = remaining.lastIndexOf("⟦", end - 1);
    const tokenEnd = remaining.indexOf("⟧", tokenStart);
    if (tokenStart >= 0 && tokenEnd >= end) end = tokenStart;
    pieces.push(remaining.slice(0, end));
    remaining = remaining.slice(end);
  }
  if (remaining) pieces.push(remaining);
  return pieces;
}

export function readerRemarkPlugin(plan: TranslationPlan, translations?: Readonly<Record<string, string>>): Plugin<[], Root> {
  const ids = new Set(plan.blocks.map((b) => b.id));
  return () => (root) => {
    function transform(parent: AstNode) {
      parent.children = parent.children?.flatMap((node) => {
        transform(node);
        const id = nodeId(node);
        const anchor = plan.anchors.get(id);
        if (anchor || (translations && ids.has(id))) {
          const properties: Record<string, unknown> = { ...node.data?.hProperties as Record<string, unknown> };
          if (anchor) properties.id = anchor;
          if (translations && ids.has(id)) properties["data-reader-block"] = id;
          node.data = { ...node.data, hProperties: properties };
          if (translations && node.type === "paragraph" && parent.type === "listItem") node.data.hName = "div";
        }
        const translated = translations?.[id];
        if (!translated) return [node];
        const translation: AstNode = { type: node.type === "tableCell" ? "emphasis" : "paragraph", children: [{ type: "text", value: translated }], data: { hName: node.type === "tableCell" ? "span" : "div", hProperties: { className: "reader-translation block mt-1 mb-3 border-l-2 border-accent-border pl-3 text-[13px] leading-6 text-primary", lang: "zh" } } };
        if (node.type === "tableCell") { node.children?.push(translation); return [node]; }
        return [node, translation];
      });
    }
    transform(root as unknown as AstNode);
  };
}
