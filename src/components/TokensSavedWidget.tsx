// Tokens-saved widget: shows estimated context tokens saved by the
// current disclosure mode vs the "full" baseline.
//
// NOTE: TokenEstimateBadge (Task 17) does not yet exist, so `estimateTokens`
// is inlined here. Once the shared badge lands, import from it instead.

export type DisclosureMode = "full" | "hybrid" | "router_only";

const AVG_DESC = 80;
const AVG_ROUTER = 120;

export function estimateTokens(
  mode: DisclosureMode,
  ess: number,
  packs: number,
  avg = 8,
): number {
  switch (mode) {
    case "full":
      return (ess + packs * avg) * AVG_DESC;
    case "hybrid":
      return ess * AVG_DESC + packs * AVG_ROUTER;
    case "router_only":
      return packs * AVG_ROUTER;
  }
}

interface TokensSavedWidgetProps {
  currentMode: DisclosureMode;
  /** Sum of skill_count across essential packs in the active scenario. */
  essentialSkillCount: number | null;
  /** Count of non-essential packs in the active scenario. */
  nonEssentialPackCount: number | null;
}

export function TokensSavedWidget({
  currentMode,
  essentialSkillCount,
  nonEssentialPackCount,
}: TokensSavedWidgetProps) {
  const hasData =
    essentialSkillCount !== null && nonEssentialPackCount !== null;

  if (!hasData) {
    // TODO: wire up once PackRecord exposes is_essential + skill_count
    // (Bundle A backend work; frontend integration task).
    return (
      <div className="rounded border p-3 bg-white">
        <div className="text-sm text-gray-500">Estimated tokens saved vs Full</div>
        <div className="text-2xl font-semibold">~—</div>
        <div className="text-xs text-gray-500 mt-1">— current / — baseline</div>
      </div>
    );
  }

  const ess = essentialSkillCount;
  const packs = nonEssentialPackCount;
  const baseline = estimateTokens("full", ess, packs);
  const current = estimateTokens(currentMode, ess, packs);
  const saved = Math.max(0, baseline - current);

  return (
    <div className="rounded border p-3 bg-white">
      <div className="text-sm text-gray-500">Estimated tokens saved vs Full</div>
      <div className="text-2xl font-semibold">~{saved.toLocaleString()}</div>
      <div className="text-xs text-gray-500 mt-1">
        {current.toLocaleString()} current / {baseline.toLocaleString()} baseline
      </div>
    </div>
  );
}
