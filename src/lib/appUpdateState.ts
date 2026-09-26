import type { AppUpdateInfo } from "./tauri";

export function visibleAppUpdate(info: AppUpdateInfo, installedVersion: string | null): AppUpdateInfo {
  return installedVersion === info.latest_version ? { ...info, has_update: false } : info;
}
