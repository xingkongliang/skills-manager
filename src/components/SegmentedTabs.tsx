import { cn } from "../utils";

export interface SegmentedTab<K extends string> {
  key: K;
  label: string;
  disabled?: boolean;
}

interface Props<K extends string> {
  tabs: SegmentedTab<K>[];
  value: K;
  onChange: (key: K) => void;
  className?: string;
}

/** Tab strip drawn as the shared segmented control (see UI spec in AGENTS.md). */
export function SegmentedTabs<K extends string>({ tabs, value, onChange, className }: Props<K>) {
  return (
    <div role="tablist" className={cn("app-segmented w-fit", className)}>
      {tabs.map((tab) => (
        <button
          key={tab.key}
          type="button"
          role="tab"
          aria-selected={value === tab.key}
          disabled={tab.disabled}
          onClick={() => onChange(tab.key)}
          className={cn(
            "app-segmented-button py-1.5 disabled:cursor-not-allowed disabled:opacity-50",
            value === tab.key && "app-segmented-button-active"
          )}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}
