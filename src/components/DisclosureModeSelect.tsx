export type DisclosureMode = "full" | "hybrid" | "router_only";

type Props = {
  value: DisclosureMode;
  onChange: (mode: DisclosureMode) => void;
  disabled?: boolean;
};

const MODE_LABELS: Record<DisclosureMode, string> = {
  full: "Full — all skills visible (legacy)",
  hybrid: "Hybrid — essentials + routers (recommended)",
  router_only: "Router only — minimum tokens",
};

export function DisclosureModeSelect({ value, onChange, disabled }: Props) {
  return (
    <label className="inline-flex items-center gap-2">
      <span className="text-sm font-medium">Disclosure mode</span>
      <select
        className="border rounded px-2 py-1 text-sm"
        value={value}
        onChange={(e) => onChange(e.target.value as DisclosureMode)}
        disabled={disabled}
      >
        {(Object.keys(MODE_LABELS) as DisclosureMode[]).map((m) => (
          <option key={m} value={m}>{MODE_LABELS[m]}</option>
        ))}
      </select>
    </label>
  );
}
