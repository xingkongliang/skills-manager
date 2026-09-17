import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../components/ConfirmDialog";

/**
 * Keep a detail sheet from closing over an unsaved draft.
 *
 * Returns the close handler to hand the sheet plus the dialog to render beside
 * it, so each panel wires the guard in three lines instead of repeating the
 * same confirm state.
 */
export function useUnsavedChangesGuard(close: () => void) {
  const { t } = useTranslation();
  const [dirty, setDirty] = useState(false);
  const [asking, setAsking] = useState(false);

  const requestClose = useCallback(() => {
    if (dirty) {
      setAsking(true);
      return;
    }
    close();
  }, [close, dirty]);

  const confirmClose = useCallback(() => {
    setAsking(false);
    setDirty(false);
    close();
  }, [close]);

  const dialog = useMemo(
    () => (
      <ConfirmDialog
        open={asking}
        tone="warning"
        title={t("skillEditor.discardTitle")}
        message={t("skillEditor.discardMessage")}
        confirmLabel={t("skillEditor.discardConfirm")}
        onClose={() => setAsking(false)}
        onConfirm={async () => confirmClose()}
      />
    ),
    [asking, confirmClose, t]
  );

  return { onDirtyChange: setDirty, requestClose, dialog };
}
