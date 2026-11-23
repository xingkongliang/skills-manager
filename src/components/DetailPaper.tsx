import type { ReactNode } from "react";
import { Loader2, type LucideIcon } from "lucide-react";
import { cn } from "../utils";

/** The white sheet a detail sheet's document sits on, over the grey sheet background. */
export function DetailPaper({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("app-panel shadow-card px-8 py-7", className)}>{children}</div>;
}

/**
 * Empty, failed and loading states inside a detail sheet: a faint icon over
 * one line of text. Place it in a DetailPaper, or alone where the paper would be.
 */
export function DetailEmptyState({
  icon,
  text,
  loading = false,
}: {
  icon?: LucideIcon;
  text: string;
  loading?: boolean;
}) {
  const Icon = loading ? Loader2 : icon;
  return (
    <div className="flex flex-col items-center gap-2 py-12 text-center">
      {Icon && <Icon className={cn("h-5 w-5 text-faint", loading && "animate-spin")} />}
      <p className="text-[13px] text-muted">{text}</p>
    </div>
  );
}

/** A DetailEmptyState on its own paper, for a column that has nothing else to show. */
export function DetailEmptyPaper(props: { icon?: LucideIcon; text: string; loading?: boolean }) {
  return (
    <DetailPaper>
      <DetailEmptyState {...props} />
    </DetailPaper>
  );
}
