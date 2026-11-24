import { toast } from "sonner";
import i18n from "../i18n";
import { compactHomePath, formatList } from "../utils";
import * as api from "./tauri";

/**
 * After a deploy, warn when an agent now finds one skill in two directories it
 * reads, e.g. `~/.codex/skills` and the shared `~/.agents/skills` (#696).
 * Best-effort and non-blocking: the deploy has already happened.
 */
export async function warnDuplicateDeployments(skillIds: string[], tools: string[]) {
  if (skillIds.length === 0 || tools.length === 0) return;
  let duplicates: api.DuplicateDeployment[];
  try {
    duplicates = await api.findDuplicateDeployments(skillIds, tools);
  } catch (error) {
    // The deploy itself succeeded; a check that could not run is not an error for the user.
    console.warn("Duplicate deployment check failed:", error);
    return;
  }
  if (duplicates.length === 0) return;

  const unique = (values: string[]) => [...new Set(values)];
  const lang = i18n.language;
  const skills = unique(duplicates.map((d) => d.skill_name));
  toast.warning(
    i18n.t("common.duplicateDeployment", {
      agents: formatList(unique(duplicates.map((d) => d.agent_name)), lang),
      skills: formatList(skills.slice(0, 3), lang) + (skills.length > 3 ? " …" : ""),
    }),
    {
      description: unique(duplicates.flatMap((d) => d.dirs).map(compactHomePath)).join(" · "),
      duration: 10000,
    }
  );
}
