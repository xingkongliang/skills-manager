import type { ConfigMap, MapMcpServer, MapSkill, Usage, UsageEntry } from "./tauri";

export type SkillState = "on" | "manual" | "off";

export function normalizeState(state: string): SkillState {
  if (state === "on") return "on";
  if (state === "off") return "off";
  return "manual";
}

export function groupBy<T>(items: T[], key: (item: T) => string): Record<string, T[]> {
  const out: Record<string, T[]> = {};
  for (const item of items) (out[key(item)] ??= []).push(item);
  return out;
}

export function stateCounts(skills: MapSkill[]): Record<SkillState, number> {
  const counts: Record<SkillState, number> = { on: 0, manual: 0, off: 0 };
  for (const s of skills) counts[normalizeState(s.state)]++;
  return counts;
}

// ── Usage: matched against transcript tool names ──

export function skillUsage(usage: Usage, skill: MapSkill): UsageEntry | null {
  return usage.skills[skill.name] ?? null;
}

/** Claude Code turns a server name into `mcp__<name>__tool`, replacing other characters with `_`. */
export function mcpUsage(usage: Usage, server: MapMcpServer): UsageEntry | null {
  return usage.mcp[server.name.replace(/[^A-Za-z0-9_-]/g, "_")] ?? null;
}

export function daysSince(iso: string, now = Date.now()): number {
  return Math.max(0, Math.floor((now - Date.parse(iso)) / 86_400_000));
}

export function unusedCount(usage: Usage, skills: MapSkill[]): number {
  return skills.filter((s) => !skillUsage(usage, s)).length;
}

// ── Map: nodes in columns, joined by labelled edges ──

export type NodeKind = "config" | "skills" | "plugin" | "mcp" | "external" | "project" | "memory";
export type EdgeKind = "on" | "off" | "manual" | "link" | "mcp";

export interface MapNode {
  id: string;
  column: 0 | 1 | 2 | 3;
  kind: NodeKind;
  title: string;
  path: string;
  skills?: MapSkill[];
  stats?: Array<[string, string]>;
  chips?: string[];
}

export interface MapEdge {
  from: string;
  to: string;
  kind: EdgeKind;
  /** i18n key and its count, rendered by the view. */
  label?: { key: string; count: number };
}

const parentDir = (p: string, levels = 1) => p.split("/").slice(0, -levels).join("/");
const topDir = (p: string) => p.split("/").slice(0, 3).join("/");

export function buildGraph(map: ConfigMap, maxProjects = 12): { nodes: MapNode[]; edges: MapEdge[] } {
  const nodes: MapNode[] = [];
  const edges: MapEdge[] = [];
  const byGroup = groupBy(map.skills, (s) => s.group);
  const groups = Object.keys(byGroup).sort((a, b) => byGroup[b].length - byGroup[a].length);

  nodes.push({
    id: "settings", column: 0, kind: "config", title: "settings.json", path: "~/.claude/settings.json",
    stats: [["overrides", String(map.override_count)], ["plugins", String(map.plugins.length)], ["hooks", String(map.hooks.length)]],
  });
  nodes.push({
    id: "claude-json", column: 0, kind: "config", title: ".claude.json", path: "~/.claude.json",
    stats: [["mcpServers", String(map.mcp_servers.filter((m) => m.scope !== "settings").length)]],
  });

  for (const group of groups) {
    const skills = byGroup[group];
    const isPlugin = skills[0].source === "plugin";
    const id = `group:${group}`;
    nodes.push({ id, column: 1, kind: isPlugin ? "plugin" : "skills", title: isPlugin ? group.split("@")[0] : group, path: group, skills });
    if (isPlugin) {
      const enabled = map.plugins.find((p) => p.id === group)?.enabled ?? false;
      // Colour and dash already say enabled/disabled; a label per plugin would only stack up.
      edges.push({ from: "settings", to: id, kind: enabled ? "on" : "off" });
    } else {
      const overridden = skills.filter((s) => s.state !== "on").length;
      if (overridden > 0) edges.push({ from: "settings", to: id, kind: "manual", label: { key: "configMap.edge.overrides", count: overridden } });
    }
  }

  // Symlinks: a skill folder pointing into another known group, or outside.
  const internal: Record<string, number> = {};
  const external: Record<string, MapSkill[]> = {};
  for (const s of map.skills) {
    if (!s.link) continue;
    const target = groups.find((g) => g !== s.group && s.link!.startsWith(`${g}/`));
    if (target) {
      const key = `${s.group}\u0000${target}`;
      internal[key] = (internal[key] ?? 0) + 1;
    } else {
      (external[parentDir(s.link, 2) || s.link] ??= []).push(s);
    }
  }
  for (const [key, count] of Object.entries(internal)) {
    const [from, to] = key.split("\u0000");
    edges.push({ from: `group:${from}`, to: `group:${to}`, kind: "link", label: { key: "configMap.edge.symlinks", count } });
  }
  for (const [dir, skills] of Object.entries(external)) {
    const id = `external:${dir}`;
    nodes.push({ id, column: 2, kind: "external", title: dir.split("/").pop() || dir, path: dir, stats: [["linkedSkills", String(skills.length)]] });
    const fromGroups = groupBy(skills, (s) => s.group);
    for (const [g, list] of Object.entries(fromGroups)) {
      edges.push({ from: `group:${g}`, to: id, kind: "link", label: { key: "configMap.edge.symlinks", count: list.length } });
    }
  }

  const mcpGroups = groupBy(map.mcp_servers, (m) => `${m.scope}\u0000${m.defined_in}`);
  for (const [key, servers] of Object.entries(mcpGroups)) {
    const [scope, definedIn] = key.split("\u0000");
    const id = `mcp:${key}`;
    const title = scope === "project" ? definedIn.split("/").pop() || definedIn : definedIn.split("/").pop()!;
    nodes.push({ id, column: 2, kind: "mcp", title, path: definedIn, chips: servers.map((s) => s.name) });
    edges.push({ from: scope === "settings" ? "settings" : "claude-json", to: id, kind: "mcp", label: { key: "configMap.edge.servers", count: servers.length } });
  }

  for (const d of map.dot_dirs.filter((d) => d.path !== "~/.claude" && d.path !== "~/.agents").slice(0, maxProjects)) {
    nodes.push({
      id: `dot:${d.path}`, column: 3, kind: "project", title: d.path.split("/").slice(-2).join("/"), path: d.path,
      stats: [["skillsCommandsAgents", `${d.skills} · ${d.commands} · ${d.agents}`]], chips: d.files,
    });
  }

  const memoryGroups = groupBy(map.memory_files.filter((m) => m.path !== "~/.claude/CLAUDE.md"), (m) => topDir(m.dir));
  const global = map.memory_files.find((m) => m.path === "~/.claude/CLAUDE.md");
  if (global) {
    nodes.push({ id: "global-memory", column: 0, kind: "memory", title: "CLAUDE.md", path: global.path, stats: [["size", `${(global.size / 1024).toFixed(1)} kB`]] });
  }
  for (const dir of Object.keys(memoryGroups).sort((a, b) => memoryGroups[b].length - memoryGroups[a].length).slice(0, 10)) {
    const files = memoryGroups[dir];
    nodes.push({
      id: `memory:${dir}`, column: 3, kind: "memory", title: dir, path: dir,
      stats: [["claudeAgents", `${files.filter((f) => f.file !== "AGENTS.md").length} · ${files.filter((f) => f.file === "AGENTS.md").length}`], ["symlinks", String(files.filter((f) => f.symlink).length)]],
    });
  }

  return { nodes, edges };
}

// ── Tree: only the folders that hold something agent-related ──

export type TreeItemType = "skill" | "claude" | "agents" | "dotdir" | "mcp" | "plugin";

export interface TreeNode {
  name: string;
  path: string;
  children: Record<string, TreeNode>;
  items: Array<{ type: TreeItemType; label: string }>;
  counts: Partial<Record<TreeItemType, number>>;
}

export function buildTree(map: ConfigMap): TreeNode {
  const root: TreeNode = { name: "~", path: "~", children: {}, items: [], counts: {} };
  const add = (path: string, type: TreeItemType, label: string) => {
    const parts = path.split("/");
    if (parts[0] !== "~") return; // outside home: not part of the tree
    let node = root;
    for (let i = 1; i < parts.length; i++) {
      node.counts[type] = (node.counts[type] ?? 0) + 1;
      const seg = parts[i];
      node = node.children[seg] ??= { name: seg, path: parts.slice(0, i + 1).join("/"), children: {}, items: [], counts: {} };
    }
    node.counts[type] = (node.counts[type] ?? 0) + 1;
    node.items.push({ type, label });
  };
  for (const s of map.skills) if (s.source !== "plugin") add(s.path, "skill", s.name);
  for (const m of map.memory_files) add(m.dir, m.file === "AGENTS.md" ? "agents" : "claude", m.file);
  for (const d of map.dot_dirs) add(d.path, "dotdir", d.kind);
  for (const p of map.plugins) add(p.path, "plugin", p.id);
  return root;
}

/** True when this node, an item in it, or anything below it matches the query. */
export function treeMatches(node: TreeNode, query: string): boolean {
  if (!query) return true;
  const q = query.toLowerCase();
  if (node.path.toLowerCase().includes(q)) return true;
  if (node.items.some((i) => i.label.toLowerCase().includes(q))) return true;
  return Object.values(node.children).some((c) => treeMatches(c, q));
}
