import type { TFunction } from "i18next";
import { getErrorKind, getErrorMessage } from "./error";

/**
 * Map a git backup error to the plain-language copy under `settings.gitError*`.
 * Shared by the Backup page and the first-run restore dialog.
 */
export function mapGitErrorMessage(error: unknown, t: TFunction): string {
  const kind = getErrorKind(error);
  const message = getErrorMessage(error, "");

  // A clone or re-clone that left the user's files (or old history) beside
  // the library says where; no category message may hide that, network
  // included.
  if (message.includes("kept at")) {
    return `${t("settings.gitErrorFilesKeptAside")} (${message.trim()})`;
  }
  // Before the phrase checks below: the skill names it lists could match them.
  const differing = differingSkillNames(error);
  if (differing) {
    return t("settings.gitErrorCloneLocalDiffers", { names: differing.join(", ") });
  }

  if (kind === "network") return t("settings.gitErrorNetwork");
  if (
    message.includes("Authentication failed")
    || message.includes("Permission denied")
    || message.includes("could not read Username")
  ) {
    return t("settings.gitErrorAuth");
  }
  if (
    message.includes("Could not resolve host")
    || message.includes("Failed to connect")
    || message.includes("Connection timed out")
    || /connection\s+refused/i.test(message)
  ) {
    return t("settings.gitErrorNetwork");
  }
  if (message.includes("unrelated histories") || message.includes("refusing to merge")) {
    return t("settings.gitErrorUnrelatedHistories");
  }
  if (
    message.includes("[rejected]")
    || message.includes("non-fast-forward")
    || message.includes("fetch first")
    || message.includes("failed to push some refs")
  ) {
    return t("settings.gitErrorRejected");
  }
  if (message.includes("no upstream") || message.includes("has no upstream branch")) {
    return t("settings.gitErrorNoUpstream");
  }
  if (message.includes("CONFLICT") || message.includes("conflict")) {
    return t("settings.gitErrorConflict");
  }
  if (message.includes("not a git repository")) {
    return t("settings.gitErrorNotRepo");
  }
  const detail = message.trim();
  return detail && detail !== "Error"
    ? `${t("settings.gitErrorGeneric")} (${detail})`
    : t("settings.gitErrorGeneric");
}

/** The skills a clone or re-clone stopped on because their local versions
 *  differ from the remote's (`CLONE_LOCAL_DIFFERS`); `null` for any other
 *  error, including one that left files beside the library. */
export function differingSkillNames(error: unknown): string[] | null {
  const message = getErrorMessage(error, "");
  if (message.includes("kept at")) return null;
  // One name per line, so a comma in a name cannot split it.
  const match = /CLONE_LOCAL_DIFFERS: .*?copies:\n(.+)$/s.exec(message);
  return match ? match[1].split("\n") : null;
}
