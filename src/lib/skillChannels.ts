/**
 * Which Channel pills to offer, and in what order.
 *
 * The drawer that adds skills to an Agent or a project and My Skills both filter
 * the same library by Channel (`source_type`), so they share this: one order,
 * one rule for channels the list does not know about, no drift between the two.
 */

/** The channels every library can hold, in the order they are shown. */
export const CHANNEL_ORDER = ["local", "import", "git", "skillssh"] as const;

export type KnownChannel = (typeof CHANNEL_ORDER)[number];

export function isKnownChannel(channel: string): channel is KnownChannel {
  return (CHANNEL_ORDER as readonly string[]).includes(channel);
}

/**
 * Channels to render for a given library: the known ones always, so the row
 * does not change shape as skills come and go, then any channel the list
 * reports that this build does not know about — `well-known` today — sorted so
 * the extra pills at least hold still.
 */
/**
 * What to call a Channel in the UI. A channel this build does not name is shown
 * as its raw `source_type` rather than hidden, so the pill still reaches those
 * skills and the user can see what they are called.
 */
export function channelLabel(
  t: (key: string) => string,
  channel: string
): string {
  return isKnownChannel(channel) ? t(`mySkills.sourceFilter.${channel}`) : channel;
}

export function channelsForSkills(skills: { source_type: string }[]): string[] {
  const extra = new Set<string>();
  for (const skill of skills) {
    if (skill.source_type && !isKnownChannel(skill.source_type)) {
      extra.add(skill.source_type);
    }
  }
  return [...CHANNEL_ORDER, ...Array.from(extra).sort()];
}
