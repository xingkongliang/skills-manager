import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { getErrorMessage } from "../lib/error";
import { fileManagerNameKey } from "../lib/platform";
import * as api from "../lib/tauri";

/**
 * Shows a workspace's folder in the OS file manager.
 *
 * Shared by the sidebar and the project page so the two entry points cannot
 * drift: the same action has to be labelled, and fail, the same way in both.
 */
export function useRevealProjectFolder() {
  const { t } = useTranslation();

  // Names the app the user actually sees — Finder, File Explorer, or the
  // desktop's own file manager — instead of a generic "folder".
  const label = t("project.revealInFileManager", { app: t(fileManagerNameKey) });

  const reveal = async (projectId: string) => {
    try {
      await api.revealProjectFolder(projectId);
    } catch (error) {
      // Worth reporting: the backend checks the folder still exists first, so a
      // failure here means the workspace moved and the user should know.
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  return { label, reveal };
}
