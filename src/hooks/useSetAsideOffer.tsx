import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { differingSkillNames, mapGitErrorMessage } from "../lib/gitErrors";

/**
 * A clone or re-clone stops when same-named local skills differ from the
 * remote's (`CLONE_LOCAL_DIFFERS`). `attempt` runs one and, if it stops that
 * way, offers to save those local versions beside the library and run it
 * again — passing back exactly the list the user was shown. Render `dialog`
 * last, so it opens on top of whatever dialog started the clone.
 */
export function useSetAsideOffer() {
  const { t } = useTranslation();
  const [offer, setOffer] = useState<{
    names: string[];
    run: (setAside: string[]) => Promise<void>;
  } | null>(null);
  const confirmed = useRef(false);
  // The list changed between showing it and running: ask again with the new one.
  const askAgain = useRef<typeof offer>(null);

  const attempt = async (run: (setAside: string[]) => Promise<void>) => {
    try {
      await run([]);
    } catch (error) {
      const names = differingSkillNames(error);
      if (!names) throw error;
      setOffer({ names, run });
    }
  };

  const announceLocalCopies = (path: string | null) => {
    if (path) toast.success(t("settings.gitLocalCopiesSaved", { path }), { duration: 20000 });
  };

  const dialog = (
    <ConfirmDialog
      open={offer !== null}
      title={t("settings.gitSetAsideTitle")}
      message={t("settings.gitSetAsideMessage")}
      details={offer?.names}
      tone="warning"
      confirmLabel={t("settings.gitSetAsideConfirm")}
      lockWhileRunning
      onClose={() => {
        // The dialog that started the clone has closed; say nothing happened.
        if (!confirmed.current) toast.info(t("settings.gitSetAsideCancelled"));
        confirmed.current = false;
        setOffer(askAgain.current);
        askAgain.current = null;
      }}
      onConfirm={async () => {
        if (!offer) return;
        confirmed.current = true;
        try {
          await offer.run(offer.names);
        } catch (error) {
          const names = differingSkillNames(error);
          if (names) {
            askAgain.current = { names, run: offer.run };
            toast.info(t("settings.gitSetAsideListChanged"));
          } else {
            toast.error(mapGitErrorMessage(error, t), { duration: 12000 });
          }
        }
      }}
    />
  );

  return { attempt, announceLocalCopies, dialog };
}
