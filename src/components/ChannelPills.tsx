import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import { channelLabel } from "../lib/skillChannels";

interface Props {
  /** Channels to render, already in display order — see channelsForSkills. */
  channels: string[];
  selected: ReadonlySet<string>;
  onToggle: (channel: string) => void;
  className?: string;
}

/**
 * The Channel pills that filter a skill list. Both My Skills and the drawer
 * that adds skills to an Agent or a project render this, so the two read the
 * same: same order, same look, same treatment of a channel this build does not
 * name (shown as the raw `source_type` rather than dropped, which would look
 * like the filter cannot reach those skills at all).
 */
export function ChannelPills({ channels, selected, onToggle, className }: Props) {
  const { t } = useTranslation();

  return (
    <div className={cn("flex flex-wrap items-center gap-1.5", className)}>
      {channels.map((channel) => {
        const active = selected.has(channel);
        return (
          <button
            key={channel}
            type="button"
            aria-pressed={active}
            onClick={() => onToggle(channel)}
            className={cn(
              "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
              active
                ? "bg-accent text-white dark:bg-accent dark:text-white"
                : "bg-surface-hover text-muted hover:text-secondary"
            )}
          >
            {channelLabel(t, channel)}
          </button>
        );
      })}
    </div>
  );
}
