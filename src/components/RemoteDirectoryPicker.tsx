import {
  AlertTriangle,
  ArrowUp,
  ChevronRight,
  File,
  Folder,
  Loader2,
  Server,
  X,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { useApp } from "../context/AppContext";
import { useModalFocus } from "../hooks/useModalFocus";
import { getErrorMessage } from "../lib/error";
import { getActiveHostId } from "../lib/hostCall";
import { type PickOptions, type PickRequest, setRemotePicker } from "../lib/pickPath";
import { parentPath, pathBreadcrumbs, pickableEntries } from "../lib/remotePath";
import * as api from "../lib/tauri";
import { cn } from "../utils";

interface Props {
  hostName: string;
  request: PickRequest;
  /** The host's home folder when not given. */
  startPath?: string;
  onClose: (path: null | string) => void;
}

/** Lists `target`; with `homeOnError`, a start path that is gone or unreadable falls back to home. */
async function listDirectoryOrHome(target: string | undefined, homeOnError: boolean) {
  try {
    return await api.listDirectory(target);
  } catch (e) {
    if (homeOnError && target) return api.listDirectory(undefined);
    throw e;
  }
}

/** Browses a remote host's folders to choose a path there. */
export function RemoteDirectoryPicker({ hostName, request, startPath, onClose }: Props) {
  const { t } = useTranslation();
  const extensions = "files" in request ? request.files : null;
  const [path, setPath] = useState(startPath ?? "");
  const [pathInput, setPathInput] = useState(startPath ?? "");
  const [listing, setListing] = useState<api.DirectoryListing | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<null | string>(null);
  const [selectedFile, setSelectedFile] = useState<null | string>(null);
  const panelRef = useModalFocus(true, () => onClose(null));
  // Only the latest listing is shown when folders are opened quickly.
  const loadId = useRef(0);

  const show = useCallback(
    (target: string | undefined, id: number, homeOnError: boolean): Promise<void> =>
      listDirectoryOrHome(target, homeOnError)
        .then(
          (next) => {
            if (id !== loadId.current) return;
            setListing(next);
            setPath(next.path);
            setPathInput(next.path);
            setError(null);
          },
          (e) => {
            if (id !== loadId.current) return;
            setListing(null);
            setError(getErrorMessage(e, t("common.error")));
          },
        )
        .finally(() => {
          if (id === loadId.current) setLoading(false);
        }),
    [t],
  );

  useEffect(() => {
    void show(startPath, ++loadId.current, true);
  }, [show, startPath]);

  const openFolder = (target: string) => {
    const trimmed = target.trim();

    if (!trimmed) return;
    setPath(trimmed);
    setPathInput(trimmed);
    setLoading(true);
    setError(null);
    setSelectedFile(null);
    void show(trimmed, ++loadId.current, false);
  };

  const up = parentPath(path);
  const entries = listing ? pickableEntries(listing.entries, extensions) : [];
  const choice = extensions ? selectedFile : listing && !loading ? listing.path : null;

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center">
      <div
        className="absolute inset-0 bg-black/70 backdrop-blur-sm"
        onClick={() => onClose(null)}
      />
      <div
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby="remote-picker-title"
        className="relative flex max-h-[calc(85vh/var(--app-scale))] w-full max-w-[560px] flex-col rounded-xl border border-border bg-surface p-5 shadow-2xl outline-none"
      >
        <div className="mb-3 flex items-center justify-between">
          <h2
            id="remote-picker-title"
            className="flex items-center gap-2 text-[13px] font-semibold text-primary"
          >
            <Server className="h-4 w-4 text-accent" />
            {t(extensions ? "remotePicker.titleFile" : "remotePicker.titleFolder", {
              name: hostName,
            })}
          </h2>
          <button
            onClick={() => onClose(null)}
            aria-label={t("common.cancel")}
            className="rounded p-1 text-muted outline-none transition-colors hover:text-secondary"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        <form
          className="mb-2 flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            openFolder(pathInput);
          }}
        >
          <input
            type="text"
            value={pathInput}
            onChange={(e) => setPathInput(e.target.value)}
            placeholder={t("remotePicker.pathPlaceholder")}
            aria-label={t("remotePicker.pathLabel")}
            spellCheck={false}
            className="placeholder-faint min-w-0 flex-1 rounded-lg border border-border-subtle bg-background px-3 py-1.5 font-mono text-[13px] text-secondary transition-all focus:border-border focus:outline-none"
          />
          <button
            type="submit"
            disabled={!pathInput.trim()}
            className="rounded-lg border border-border-subtle bg-background px-3 text-[13px] font-medium text-tertiary outline-none transition-all hover:border-border hover:text-secondary disabled:opacity-50"
          >
            {t("remotePicker.go")}
          </button>
        </form>

        <div className="mb-2 flex min-w-0 items-center gap-1">
          <button
            onClick={() => up && openFolder(up)}
            disabled={!up}
            title={t("remotePicker.up")}
            aria-label={t("remotePicker.up")}
            className="shrink-0 rounded-md p-1.5 text-muted outline-none transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-40"
          >
            <ArrowUp className="h-3.5 w-3.5" />
          </button>
          <nav
            aria-label={t("remotePicker.pathLabel")}
            className="scrollbar-hide flex min-w-0 items-center overflow-x-auto text-[12px]"
          >
            {path &&
              pathBreadcrumbs(path).map((crumb, i, all) => (
                <span key={crumb.path} className="flex shrink-0 items-center">
                  {i > 1 && <ChevronRight className="h-3 w-3 text-faint" />}
                  <button
                    onClick={() => openFolder(crumb.path)}
                    className={cn(
                      "rounded px-1.5 py-0.5 font-mono transition-colors outline-none hover:bg-surface-hover",
                      i === all.length - 1 ? "text-primary" : "text-muted hover:text-secondary",
                    )}
                  >
                    {crumb.name}
                  </button>
                </span>
              ))}
          </nav>
        </div>

        <div className="h-[300px] overflow-y-auto rounded-lg border border-border-subtle bg-background p-1">
          {loading ? (
            <div className="flex h-full items-center justify-center gap-2 text-[13px] text-muted">
              <Loader2 className="h-4 w-4 animate-spin" />
              {t("common.loading")}
            </div>
          ) : error ? (
            <div
              role="alert"
              className="flex h-full flex-col items-center justify-center gap-2 px-6 text-center text-[13px] text-tertiary"
            >
              <AlertTriangle className="h-4 w-4 text-amber-500" />
              <p>{t("remotePicker.loadFailed")}</p>
              <p className="break-all font-mono text-[12px] text-muted">{error}</p>
            </div>
          ) : entries.length === 0 ? (
            <div className="flex h-full items-center justify-center text-[13px] text-muted">
              {t(extensions ? "remotePicker.emptyFile" : "remotePicker.emptyFolder")}
            </div>
          ) : (
            entries.map((entry) => {
              const Icon = entry.is_dir ? Folder : File;

              return (
                <button
                  key={entry.path}
                  onClick={() =>
                    entry.is_dir ? openFolder(entry.path) : setSelectedFile(entry.path)
                  }
                  onDoubleClick={() => !entry.is_dir && onClose(entry.path)}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[13px] transition-colors outline-none focus-visible:ring-2 focus-visible:ring-border",
                    selectedFile === entry.path
                      ? "bg-accent-bg text-primary"
                      : "text-secondary hover:bg-surface-hover",
                  )}
                >
                  <Icon
                    className={cn(
                      "w-3.5 h-3.5 shrink-0",
                      entry.is_dir ? "text-accent" : "text-muted",
                    )}
                  />
                  <span className="truncate">{entry.name}</span>
                </button>
              );
            })
          )}
        </div>

        <div className="mt-4 flex items-center justify-between gap-3">
          <p
            className="min-w-0 truncate font-mono text-[12px] text-muted"
            title={choice ?? undefined}
          >
            {choice ?? (extensions ? t("remotePicker.chooseFileHint") : "")}
          </p>
          <div className="flex shrink-0 gap-2">
            <button
              onClick={() => onClose(null)}
              className="rounded-lg px-3 py-1.5 text-[13px] font-medium text-tertiary outline-none transition-colors hover:bg-surface-hover hover:text-secondary"
            >
              {t("common.cancel")}
            </button>
            <button
              onClick={() => choice && onClose(choice)}
              disabled={!choice}
              className="rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white outline-none transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
            >
              {t("remotePicker.select")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

interface PendingPick {
  hostId: null | string;
  request: PickRequest;
  opts: PickOptions;
  resolve: (path: null | string) => void;
}

/** Mounted once: answers `pickPath` with the browser while a host is active. */
export function RemotePickerHost() {
  const { activeHost, hostSession } = useApp();
  const [pending, setPending] = useState<null | PendingPick>(null);

  useEffect(() => {
    setRemotePicker(
      (request, opts) =>
        new Promise((resolve) => setPending({ hostId: getActiveHostId(), request, opts, resolve })),
    );

    return () => setRemotePicker(null);
  }, []);

  const finish = useCallback(
    (path: null | string) => {
      pending?.resolve(path);
      setPending(null);
    },
    [pending],
  );

  // A switch while the browser is open cancels the choice: the path would
  // belong to the other machine. Dropping it keeps the browser from coming
  // back when the app returns to that host.
  if (pending && pending.hostId !== (activeHost?.id ?? null)) {
    pending.resolve(null);
    setPending(null);

    return null;
  }

  if (!pending || !activeHost) return null;

  return (
    <RemoteDirectoryPicker
      hostName={activeHost.name}
      request={pending.request}
      startPath={pending.opts.startPath || hostSession?.info.home}
      onClose={finish}
    />
  );
}
