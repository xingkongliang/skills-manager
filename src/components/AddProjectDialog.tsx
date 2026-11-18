import { Check, FolderOpen, Search, X } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { useModalFocus } from "../hooks/useModalFocus";
import { pickPath } from "../lib/pickPath";
import * as api from "../lib/tauri";
import { cn } from "../utils";
import { DeployModePicker } from "./DeployModePicker";

interface Props {
  open: boolean;
  onClose: () => void;
  onAdded: () => Promise<void>;
}

export function AddProjectDialog({ open, ...props }: Props) {
  if (!open) return null;

  return <AddProjectDialogContent {...props} />;
}

function AddProjectDialogContent({ onClose, onAdded }: Omit<Props, "open">) {
  const { t } = useTranslation();
  const titleId = useId();
  const scanRootId = useId();
  const dialogRef = useModalFocus(true, onClose);
  const scanPending = useRef(false);
  const [tab, setTab] = useState<"linked" | "manual" | "scan">("manual");
  const [scanRoot, setScanRoot] = useState("");
  const [scanning, setScanning] = useState(false);
  const [scanResults, setScanResults] = useState<string[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [adding, setAdding] = useState(false);
  const [scanned, setScanned] = useState(false);
  const [linkedName, setLinkedName] = useState("");
  const [linkedPath, setLinkedPath] = useState("");
  const [deployMode, setDeployMode] = useState<api.ProjectDeployMode>("link");

  useEffect(() => {
    api
      .getSettings("default_project_deploy_mode")
      .then((v) => {
        if (v === "copy") setDeployMode(v);
      })
      .catch(() => {});
  }, []);

  const handleSelectFolder = async () => {
    const dir = await pickPath({ directory: true });

    if (!dir) return;
    setAdding(true);

    try {
      await api.addProject(dir, deployMode);
      await onAdded();
      onClose();
    } catch {
      // error handled by toast in parent
    } finally {
      setAdding(false);
    }
  };

  const handleScan = async () => {
    const root = scanRoot.trim();

    if (!root || scanPending.current) return;
    scanPending.current = true;
    setScanning(true);
    setScanned(false);
    setScanResults([]);
    setSelected(new Set());

    try {
      const results = await api.scanProjects(root);
      setScanResults(results);
      setSelected(new Set(results));
      setScanned(true);
    } finally {
      scanPending.current = false;
      setScanning(false);
    }
  };

  const toggleSelect = (path: string) => {
    setSelected((prev) => {
      const next = new Set(prev);

      if (next.has(path)) next.delete(path);
      else next.add(path);

      return next;
    });
  };

  const handleAddSelected = async () => {
    if (selected.size === 0) return;
    setAdding(true);

    try {
      for (const path of selected) {
        try {
          await api.addProject(path, deployMode);
        } catch {
          // skip duplicates
        }
      }

      await onAdded();
      onClose();
    } finally {
      setAdding(false);
    }
  };

  const handleSelectBrowse = async () => {
    const dir = await pickPath({ directory: true }, { startPath: scanRoot.trim() });

    if (dir) setScanRoot(dir);
  };

  const handleAddLinkedWorkspace = async () => {
    if (!linkedName.trim() || !linkedPath.trim()) return;
    setAdding(true);

    try {
      await api.addLinkedWorkspace(linkedName.trim(), linkedPath.trim());
      await onAdded();
      onClose();
    } catch {
      // error handled by toast in parent
    } finally {
      setAdding(false);
    }
  };

  const inputClass =
    "w-full bg-background border border-border-subtle rounded-lg px-3 py-2 text-[13px] text-secondary focus:outline-none focus:border-border transition-all placeholder-faint";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className="relative w-full max-w-[480px] rounded-xl border border-border bg-surface p-5 shadow-2xl"
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 id={titleId} className="text-[13px] font-semibold text-primary">
            {t("project.addProjectTitle")}
          </h2>
          <button
            onClick={onClose}
            aria-label={t("common.close")}
            className="rounded p-1 text-muted outline-none transition-colors hover:text-secondary"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Tabs */}
        <div className="mb-4 flex gap-1 rounded-lg border border-border-subtle bg-background p-0.5">
          {(["manual", "scan", "linked"] as const).map((key) => (
            <button
              key={key}
              onClick={() => setTab(key)}
              className={cn(
                "flex-1 py-1.5 text-[13px] font-medium rounded-md transition-all outline-none",
                tab === key
                  ? "bg-surface text-primary shadow-sm"
                  : "text-muted hover:text-secondary",
              )}
            >
              {t(
                key === "manual"
                  ? "project.tabManual"
                  : key === "scan"
                    ? "project.tabScan"
                    : "project.tabLinked",
              )}
            </button>
          ))}
        </div>

        {tab !== "linked" && (
          <DeployModePicker value={deployMode} onChange={setDeployMode} className="mb-4" />
        )}

        {tab === "manual" ? (
          <div className="space-y-3">
            <p className="text-[13px] text-tertiary">{t("project.addManual")}</p>
            <button
              onClick={(event) => {
                event.currentTarget.focus();
                void handleSelectFolder();
              }}
              disabled={adding}
              className="flex w-full items-center gap-2 rounded-lg border border-dashed border-border-subtle bg-background px-3 py-2.5 text-[13px] text-tertiary outline-none transition-all hover:border-border hover:text-secondary"
            >
              <FolderOpen className="h-4 w-4 text-muted" />
              {adding ? t("common.loading") : t("project.addManual")}
            </button>
          </div>
        ) : tab === "scan" ? (
          <div className="space-y-3">
            <form
              className="flex gap-2"
              onSubmit={(event) => {
                event.preventDefault();
                void handleScan();
              }}
            >
              <label htmlFor={scanRootId} className="sr-only">
                {t("project.scanRoot")}
              </label>
              <input
                id={scanRootId}
                type="text"
                value={scanRoot}
                onChange={(e) => setScanRoot(e.target.value)}
                placeholder={t("project.scanDirPlaceholder")}
                className={cn(inputClass, "flex-1")}
                required
              />
              <button
                type="button"
                onClick={(event) => {
                  event.currentTarget.focus();
                  void handleSelectBrowse();
                }}
                className="rounded-lg border border-border-subtle bg-background px-2.5 text-muted outline-none transition-all hover:border-border hover:text-secondary"
                title={t("project.browseScanDir")}
                aria-label={t("project.browseScanDir")}
              >
                <FolderOpen className="h-4 w-4" />
              </button>
              <button
                type="submit"
                aria-label={scanning ? t("project.scanning") : t("project.scanDir")}
                disabled={!scanRoot.trim() || scanning}
                className="rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white outline-none transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
              >
                {scanning ? t("project.scanning") : <Search className="h-4 w-4" />}
              </button>
            </form>

            {scanned && scanResults.length === 0 && (
              <p className="py-4 text-center text-[13px] text-muted">{t("project.scanNoResult")}</p>
            )}

            {scanResults.length > 0 && (
              <>
                <div className="flex items-center justify-between">
                  <span className="text-[13px] text-tertiary">
                    {t("project.scanResult", { count: scanResults.length })}
                  </span>
                  <button
                    onClick={() =>
                      setSelected((prev) =>
                        prev.size === scanResults.length ? new Set() : new Set(scanResults),
                      )
                    }
                    className="text-[12px] text-accent outline-none hover:underline"
                  >
                    {selected.size === scanResults.length
                      ? t("project.deselectAll")
                      : t("project.selectAll")}
                  </button>
                </div>
                <div className="max-h-[240px] space-y-1 overflow-y-auto">
                  {scanResults.map((path) => (
                    <button
                      key={path}
                      onClick={() => toggleSelect(path)}
                      className={cn(
                        "flex items-center gap-2 w-full px-3 py-2 rounded-lg text-left text-[13px] transition-all outline-none",
                        selected.has(path)
                          ? "bg-accent-bg/50 text-primary border border-accent-border/30"
                          : "bg-background text-tertiary border border-border-subtle hover:border-border",
                      )}
                    >
                      <div
                        className={cn(
                          "w-4 h-4 rounded border flex items-center justify-center shrink-0",
                          selected.has(path)
                            ? "bg-accent-dark border-accent-border text-white"
                            : "border-border-subtle",
                        )}
                      >
                        {selected.has(path) && <Check className="h-3 w-3" />}
                      </div>
                      <span className="truncate">{path}</span>
                    </button>
                  ))}
                </div>
                <div className="flex justify-end pt-1">
                  <button
                    onClick={handleAddSelected}
                    disabled={selected.size === 0 || adding}
                    className="rounded-lg border border-accent-border bg-accent-dark px-3 py-1.5 text-[13px] font-medium text-white outline-none transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    {adding
                      ? t("common.loading")
                      : t("project.addSelected", { count: selected.size })}
                  </button>
                </div>
              </>
            )}
          </div>
        ) : (
          <div className="space-y-3">
            <p className="text-[13px] text-tertiary">{t("project.addLinkedHint")}</p>
            <input
              type="text"
              value={linkedName}
              onChange={(e) => setLinkedName(e.target.value)}
              placeholder={t("project.linkedNamePlaceholder")}
              aria-label={t("project.linkedNamePlaceholder")}
              className={inputClass}
            />
            <div className="flex gap-2">
              <input
                type="text"
                value={linkedPath}
                onChange={(e) => setLinkedPath(e.target.value)}
                placeholder={t("project.linkedPathPlaceholder")}
                aria-label={t("project.linkedPathPlaceholder")}
                className={cn(inputClass, "flex-1")}
              />
              <button
                onClick={async (event) => {
                  event.currentTarget.focus();
                  const dir = await pickPath({ directory: true }, { startPath: linkedPath.trim() });

                  if (dir) setLinkedPath(dir);
                }}
                className="rounded-lg border border-border-subtle bg-background px-2.5 text-muted outline-none transition-all hover:border-border hover:text-secondary"
                title={t("project.selectSkillsDir")}
              >
                <FolderOpen className="h-4 w-4" />
              </button>
            </div>
            <p className="text-[12px] leading-5 text-muted">
              {t("project.linkedDisabledPathHint")}
            </p>
            <button
              onClick={handleAddLinkedWorkspace}
              disabled={adding || !linkedName.trim() || !linkedPath.trim()}
              className="flex w-full items-center justify-center gap-2 rounded-lg border border-dashed border-border-subtle bg-background px-3 py-2.5 text-[13px] text-tertiary outline-none transition-all hover:border-border hover:text-secondary disabled:cursor-not-allowed disabled:opacity-50"
            >
              <FolderOpen className="h-4 w-4 text-muted" />
              {adding ? t("common.loading") : t("project.addLinkedWorkspace")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
