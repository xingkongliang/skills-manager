import type { ManagedSkill, ToolInfo } from "../lib/tauri";
import { cn } from "../utils";

function shortLabel(displayName: string, key: string): string {
  const words = displayName.trim().split(/\s+/).filter(Boolean);
  if (words.length >= 2) {
    return (words[0][0] + words[1][0]).toUpperCase();
  }
  const word = words[0] || key;
  return word.slice(0, 2).toUpperCase();
}

type DotState = "synced" | "available" | "orphan";

interface Dot {
  key: string;
  displayName: string;
  state: DotState;
}

interface Props {
  skill: ManagedSkill;
  tools: ToolInfo[];
  limit?: number;
  size?: "sm" | "md";
  className?: string;
}

export function SyncDots({ skill, tools, limit, size = "md", className }: Props) {
  const installed = tools.filter((t) => t.installed);
  const installedKeys = new Set(installed.map((t) => t.key));
  const syncedKeys = new Set(skill.targets.map((t) => t.tool));

  const dots: Dot[] = installed.map((tool) => ({
    key: tool.key,
    displayName: tool.display_name,
    state: syncedKeys.has(tool.key) ? "synced" : "available",
  }));

  // Include synced targets whose agent is no longer installed so the
  // indicator never disappears when the rest of the UI still treats the
  // skill as synced. Fall back to the tool key if we have no display name.
  for (const target of skill.targets) {
    if (installedKeys.has(target.tool)) continue;
    const known = tools.find((t) => t.key === target.tool);
    dots.push({
      key: target.tool,
      displayName: known?.display_name || target.tool,
      state: "orphan",
    });
  }

  const visible = typeof limit === "number" ? dots.slice(0, limit) : dots;
  const hiddenCount = dots.length - visible.length;

  const dim = size === "sm"
    ? "h-[16px] w-[16px] text-[8px]"
    : "h-[18px] w-[18px] text-[9px]";

  const stateClass: Record<DotState, string> = {
    synced: "border-transparent bg-[var(--color-text-primary)] text-[var(--color-bg)]",
    available: "border-border-subtle bg-surface-hover text-faint",
    orphan:
      "border-amber-500/40 bg-amber-500/10 text-amber-600 dark:text-amber-400",
  };

  const stateTitle: Record<DotState, string> = {
    synced: " · synced",
    available: "",
    orphan: " · synced · agent unavailable",
  };

  return (
    <div className={cn("flex items-center gap-[2px]", className)}>
      {visible.map((dot) => (
        <span
          key={dot.key}
          title={`${dot.displayName}${stateTitle[dot.state]}`}
          className={cn(
            "inline-flex select-none items-center justify-center rounded-[4px] border font-mono font-semibold tracking-tight transition-colors",
            dim,
            stateClass[dot.state],
          )}
        >
          {shortLabel(dot.displayName, dot.key)}
        </span>
      ))}
      {hiddenCount > 0 && (
        <span
          title={`+${hiddenCount} more agents`}
          className={cn(
            "inline-flex select-none items-center justify-center rounded-[4px] border border-border-subtle bg-surface-hover font-mono font-semibold text-faint",
            dim,
          )}
        >
          +{hiddenCount}
        </span>
      )}
    </div>
  );
}
