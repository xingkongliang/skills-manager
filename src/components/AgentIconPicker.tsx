import { Globe } from "lucide-react";
import { cn } from "../utils";
import { getAgentIconSrc, listAgentIconKeys } from "../lib/agentIcons";

interface AgentIconPickerProps {
  value: string | null;
  onChange: (iconKey: string | null) => void;
  label?: string;
  noneLabel?: string;
  className?: string;
}

const ICON_KEYS = listAgentIconKeys();

export function AgentIconPicker({
  value,
  onChange,
  label,
  noneLabel,
  className,
}: AgentIconPickerProps) {
  return (
    <div className={className}>
      {label && <label className="text-[12px] text-muted mb-1 block">{label}</label>}
      <div className="grid max-h-[180px] grid-cols-[repeat(auto-fill,minmax(32px,1fr))] gap-1.5 overflow-y-auto rounded-lg border border-border-subtle bg-background p-2">
        <button
          type="button"
          onClick={() => onChange(null)}
          title={noneLabel ?? "No icon"}
          className={cn(
            "flex h-8 items-center justify-center rounded-md border transition-all outline-none",
            value === null
              ? "border-accent bg-accent/10 text-accent"
              : "border-border-subtle text-muted hover:border-border hover:text-secondary"
          )}
        >
          <Globe className="h-3.5 w-3.5" />
        </button>
        {ICON_KEYS.map((key) => {
          const src = getAgentIconSrc(key);
          const selected = value === key;
          return (
            <button
              key={key}
              type="button"
              onClick={() => onChange(key)}
              title={key}
              className={cn(
                "flex h-8 items-center justify-center overflow-hidden rounded-md border bg-surface p-1 transition-all outline-none",
                selected
                  ? "border-accent ring-1 ring-accent"
                  : "border-border-subtle hover:border-border"
              )}
            >
              {src && <img src={src} alt="" draggable={false} className="h-full w-full object-contain" />}
            </button>
          );
        })}
      </div>
    </div>
  );
}
