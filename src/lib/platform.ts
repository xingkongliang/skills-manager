/**
 * The operating system, as the webview sees it.
 *
 * There is no OS plugin in this project, so the user agent is the signal — the
 * platform-specific UI elsewhere already reads it the same way.
 */
export const IS_MACOS = navigator.userAgent.includes("Mac");
export const IS_WINDOWS = navigator.userAgent.includes("Windows");

export type FileManagerPlatform = "finder" | "explorer" | "linux";

/**
 * Which file manager this OS has, so a label can name the app the user actually
 * sees. Linux has no single name — GNOME calls it Files, KDE calls it Dolphin —
 * so the generic name is used there and stays honest.
 */
export const fileManagerPlatform: FileManagerPlatform = IS_MACOS
  ? "finder"
  : IS_WINDOWS
    ? "explorer"
    : "linux";

/** Translation key for the file manager's name in the user's language. */
export const fileManagerNameKey = `platform.fileManager.${fileManagerPlatform}`;
