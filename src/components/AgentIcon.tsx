import { useState, type ReactNode } from "react";
import { Globe } from "lucide-react";
import { cn } from "../utils";
import { getAgentIconSrc, agentIconNeedsDarkInvert } from "../lib/agentIcons";
import { useApp } from "../context/AppContext";

interface AgentIconProps {
  agentKey: string;
  /**
   * Icon key to render instead of `agentKey`. Optional — custom agents whose
   * generated key has no matching bundled icon are resolved automatically
   * from the app's tool list (see `ToolInfo.icon`), so most callers don't
   * need to pass this explicitly. Only useful to force a specific icon.
   */
  iconOverride?: string | null;
  displayName?: string;
  className?: string;
  imageClassName?: string;
  fallback?: ReactNode;
}

export function AgentIcon({
  agentKey,
  iconOverride,
  displayName,
  className,
  imageClassName,
  fallback,
}: AgentIconProps) {
  const { tools } = useApp();
  const resolvedOverride =
    iconOverride ?? tools.find((tool) => tool.key === agentKey)?.icon ?? null;
  const iconKey = resolvedOverride || agentKey;
  const src = getAgentIconSrc(iconKey);
  const [failedSrc, setFailedSrc] = useState<string | null>(null);
  const hasFailed = src === failedSrc;

  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center justify-center overflow-hidden rounded-md border border-border-subtle bg-surface",
        className
      )}
      title={displayName}
      aria-hidden="true"
    >
      {src && !hasFailed ? (
        <img
          src={src}
          alt=""
          draggable={false}
          className={cn(
            "h-full w-full object-contain",
            agentIconNeedsDarkInvert(iconKey) && "dark:invert",
            imageClassName
          )}
          onError={() => setFailedSrc(src)}
        />
      ) : (
        fallback ?? <Globe className="h-1/2 w-1/2 text-muted" />
      )}
    </span>
  );
}
