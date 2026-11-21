import { describe, expect, it } from "vitest";
import { buildGraph, buildTree, daysSince, mcpUsage, skillUsage, treeMatches, unusedCount } from "./configMap";
import type { ConfigMap } from "./tauri";

const sample: ConfigMap = {
  home: "~",
  skills: [
    { name: "alpha", description: "", source: "user", group: "~/.claude/skills", path: "~/.claude/skills/alpha", link: "~/.agents/skills/alpha", state: "off" },
    { name: "alpha", description: "", source: "agents", group: "~/.agents/skills", path: "~/.agents/skills/alpha", link: null, state: "on" },
    { name: "beta", description: "", source: "user", group: "~/.claude/skills", path: "~/.claude/skills/beta", link: "~/src/kit/skills/beta", state: "user-invocable-only" },
    { name: "p:gamma", description: "", source: "plugin", group: "p@market", path: "~/.claude/plugins/cache/p/skills/gamma", link: null, state: "on" },
  ],
  plugins: [{ id: "p@market", version: "1", enabled: true, skills: ["gamma"], commands: [], agents: [], has_hooks: false, has_mcp: false, path: "~/.claude/plugins/cache/p" }],
  mcp_servers: [{ name: "srv", scope: "user", defined_in: "~/.claude.json", transport: "stdio", env_keys: ["TOKEN"] }],
  hooks: [],
  memory_files: [
    { path: "~/.claude/CLAUDE.md", dir: "~/.claude", file: "CLAUDE.md", size: 2048, symlink: false },
    { path: "~/projects/app/CLAUDE.md", dir: "~/projects/app", file: "CLAUDE.md", size: 10, symlink: false },
  ],
  dot_dirs: [{ path: "~/projects/app/.claude", kind: ".claude", skills: 1, commands: 0, agents: 0, files: [] }],
  override_count: 2,
  truncated: false,
  usage: { sessions: 3, since: "2026-01-01T00:00:00Z", retention_days: 30, skills: { alpha: { count: 2, last_used: "2026-01-05T00:00:00Z" } }, mcp: { srv: { count: 1, last_used: "2026-01-05T00:00:00Z" } } },
};

describe("buildGraph", () => {
  const { nodes, edges } = buildGraph(sample);
  const ids = nodes.map((n) => n.id);

  it("creates one node per skill group, plugin included", () => {
    expect(ids).toContain("group:~/.claude/skills");
    expect(ids).toContain("group:~/.agents/skills");
    expect(ids).toContain("group:p@market");
  });

  it("links symlinks to a known group, and others to an external target", () => {
    expect(edges).toContainEqual(expect.objectContaining({ from: "group:~/.claude/skills", to: "group:~/.agents/skills", kind: "link" }));
    expect(ids).toContain("external:~/src/kit");
  });

  it("marks overrides and plugin state from settings", () => {
    expect(edges).toContainEqual(expect.objectContaining({ from: "settings", to: "group:~/.claude/skills", kind: "manual", label: { key: "configMap.edge.overrides", count: 2 } }));
    expect(edges).toContainEqual(expect.objectContaining({ from: "settings", to: "group:p@market", kind: "on" }));
  });

  it("groups MCP servers by where they are declared", () => {
    expect(nodes.find((n) => n.kind === "mcp")?.chips).toEqual(["srv"]);
  });
});

describe("buildTree", () => {
  const root = buildTree(sample);

  it("aggregates counts up to the root and skips plugin skills", () => {
    expect(root.counts.skill).toBe(3);
    expect(root.children[".claude"].children.skills.counts.skill).toBe(2);
  });

  it("filters by path or item label", () => {
    expect(treeMatches(root.children.projects, "app")).toBe(true);
    expect(treeMatches(root.children.projects, "nope")).toBe(false);
    expect(treeMatches(root.children[".agents"], "alpha")).toBe(true);
  });
});

describe("usage", () => {
  it("matches skills by name and MCP servers by sanitised name", () => {
    expect(skillUsage(sample.usage, sample.skills[0])?.count).toBe(2);
    expect(skillUsage(sample.usage, sample.skills[2])).toBeNull();
    expect(mcpUsage(sample.usage, sample.mcp_servers[0])?.count).toBe(1);
    expect(mcpUsage({ ...sample.usage, mcp: { claude_ai_Notes: { count: 4, last_used: "" } } }, { ...sample.mcp_servers[0], name: "claude.ai Notes" })?.count).toBe(4);
  });

  it("counts unused skills and days since last use", () => {
    expect(unusedCount(sample.usage, sample.skills)).toBe(2);
    expect(daysSince("2026-01-01T00:00:00Z", Date.parse("2026-01-11T12:00:00Z"))).toBe(10);
  });
});
