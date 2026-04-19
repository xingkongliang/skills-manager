import type { DisclosureMode } from "./DisclosureModeSelect";

type Props = {
  mode: DisclosureMode;
  essentialSkillCount: number;
  nonEssentialPackCount: number;
  averageSkillsPerPack?: number;
};

const AVG_DESC_TOKENS = 80;
const AVG_ROUTER_TOKENS = 120;

export function estimateTokens(
  mode: DisclosureMode,
  essentialSkillCount: number,
  nonEssentialPackCount: number,
  averageSkillsPerPack = 8,
): number {
  switch (mode) {
    case "full":
      return (essentialSkillCount + nonEssentialPackCount * averageSkillsPerPack) * AVG_DESC_TOKENS;
    case "hybrid":
      return essentialSkillCount * AVG_DESC_TOKENS + nonEssentialPackCount * AVG_ROUTER_TOKENS;
    case "router_only":
      return nonEssentialPackCount * AVG_ROUTER_TOKENS;
  }
}

export function TokenEstimateBadge({
  mode,
  essentialSkillCount,
  nonEssentialPackCount,
  averageSkillsPerPack = 8,
}: Props) {
  const tokens = estimateTokens(mode, essentialSkillCount, nonEssentialPackCount, averageSkillsPerPack);
  const color =
    tokens > 10000 ? "bg-red-100 text-red-800" :
    tokens > 3000 ? "bg-yellow-100 text-yellow-800" :
    "bg-green-100 text-green-800";
  return (
    <span className={`px-2 py-1 rounded text-xs ${color}`} data-testid="token-estimate-badge">
      ~{tokens.toLocaleString()} tokens
    </span>
  );
}
