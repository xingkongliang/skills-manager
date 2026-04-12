import type { LucideIcon } from "lucide-react";
import {
  Package,
  Zap,
  Shield,
  Sparkles,
  Puzzle,
  Cpu,
  Globe,
  Layers,
  Rocket,
  Wrench,
} from "lucide-react";

export interface PackIconOption {
  key: string;
  label: string;
  icon: LucideIcon;
}

export const PACK_ICON_OPTIONS: PackIconOption[] = [
  { key: "package", label: "Package", icon: Package },
  { key: "zap", label: "Zap", icon: Zap },
  { key: "shield", label: "Shield", icon: Shield },
  { key: "sparkles", label: "Sparkles", icon: Sparkles },
  { key: "puzzle", label: "Puzzle", icon: Puzzle },
  { key: "cpu", label: "CPU", icon: Cpu },
  { key: "globe", label: "Globe", icon: Globe },
  { key: "layers", label: "Layers", icon: Layers },
  { key: "rocket", label: "Rocket", icon: Rocket },
  { key: "wrench", label: "Wrench", icon: Wrench },
];

const PACK_ICON_MAP = new Map(
  PACK_ICON_OPTIONS.map((option) => [option.key, option] as const)
);

export function getPackIcon(iconKey?: string | null): PackIconOption {
  if (iconKey && PACK_ICON_MAP.has(iconKey)) {
    return PACK_ICON_MAP.get(iconKey)!;
  }
  return PACK_ICON_OPTIONS[0];
}

export interface PackColorOption {
  key: string;
  label: string;
  textClass: string;
  bgClass: string;
  borderClass: string;
  /** Tailwind class for the dot / swatch indicator */
  dotClass: string;
}

export const PACK_COLOR_OPTIONS: PackColorOption[] = [
  {
    key: "blue",
    label: "Blue",
    textClass: "text-blue-400",
    bgClass: "bg-blue-500/12",
    borderClass: "border-blue-500/30",
    dotClass: "bg-blue-400",
  },
  {
    key: "emerald",
    label: "Emerald",
    textClass: "text-emerald-400",
    bgClass: "bg-emerald-500/12",
    borderClass: "border-emerald-500/30",
    dotClass: "bg-emerald-400",
  },
  {
    key: "amber",
    label: "Amber",
    textClass: "text-amber-400",
    bgClass: "bg-amber-500/12",
    borderClass: "border-amber-500/30",
    dotClass: "bg-amber-400",
  },
  {
    key: "rose",
    label: "Rose",
    textClass: "text-rose-400",
    bgClass: "bg-rose-500/12",
    borderClass: "border-rose-500/30",
    dotClass: "bg-rose-400",
  },
  {
    key: "violet",
    label: "Violet",
    textClass: "text-violet-400",
    bgClass: "bg-violet-500/12",
    borderClass: "border-violet-500/30",
    dotClass: "bg-violet-400",
  },
  {
    key: "cyan",
    label: "Cyan",
    textClass: "text-cyan-400",
    bgClass: "bg-cyan-500/12",
    borderClass: "border-cyan-500/30",
    dotClass: "bg-cyan-400",
  },
  {
    key: "orange",
    label: "Orange",
    textClass: "text-orange-400",
    bgClass: "bg-orange-500/12",
    borderClass: "border-orange-500/30",
    dotClass: "bg-orange-400",
  },
  {
    key: "pink",
    label: "Pink",
    textClass: "text-pink-400",
    bgClass: "bg-pink-500/12",
    borderClass: "border-pink-500/30",
    dotClass: "bg-pink-400",
  },
];

const PACK_COLOR_MAP = new Map(
  PACK_COLOR_OPTIONS.map((option) => [option.key, option] as const)
);

export function getPackColor(colorKey?: string | null): PackColorOption {
  if (colorKey && PACK_COLOR_MAP.has(colorKey)) {
    return PACK_COLOR_MAP.get(colorKey)!;
  }
  return PACK_COLOR_OPTIONS[0];
}
