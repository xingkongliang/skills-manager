import { useEffect, useState } from "react";
import { X, FolderOpen, Search, Check } from "lucide-react";
import { useTranslation } from "react-i18next";
import { open as dialogOpen } from "@tauri-apps/plugin-dialog";
import { cn } from "../utils";
import * as api from "../lib/tauri";

interface Props {
  open: boolean;
  onClose: () => void;
  onAdded: () => Promise<void>;
}

export function AddProjectDialog({ open, onClose, onAdded }: Props) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<"manual" | "scan" | "linked">("manual");
  const [scanRoot, setScanRoot] = useState("");
  const [scanning, setScanning] = useState(false);
  const [scanResults, setScanResults] = useState<string[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [adding, setAdding] = useState(false);
  const [scanned, setScanned] = useState(false);
  const [linkedName, setLinkedName] = useState("");
  const [linkedPath, setLinkedPath] = useState("");

  useEffect(() => {
    if (!open) return;
    setTab("manual");
    setScanRoot("");
    setScanning(false);
    setScanResults([]);
    setSelected(new Set());
    setAdding(false);
    setScanned(false);
    setLinkedName("");
    setLinkedPath("");
  }, [open]);

  if (!open) return null;

  const handleSelectFolder = async () => {
    const dir = await dialogOpen({ directory: true, multiple: false });
    if (!dir) return;
    setAdding(true);
    try {
      await api.addProject(dir as string);
      await onAdded();
      onClose();
    } catch {
      // error handled by toast in parent
    } finally {
      setAdding(false);
    }
  };

  const handleScan = async () => {
    if (!scanRoot.trim()) return;
    setScanning(true);
    setScanned(false);
    setScanResults([]);
    setSelected(new Set());
    try {
      const results = await api.scanProjects(scanRoot.trim());
      setScanResults(results);
      setSelected(new Set(results));
      setScanned(true);
    } finally {
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
          await api.addProject(path);
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
    const dir = await dialogOpen({ directory: true, multiple: false });
    if (dir) setScanRoot(dir as string);
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
    "w-full bg-background border border-border-subtle rounded-[4px] px-3 py-2 text-[13px] text-secondary focus:outline-none focus:border-border transition-all placeholder-faint";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative bg-surface border border-border rounded-xl w-full max-w-[480px] p-5 shadow-2xl">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-[13px] font-semibold text-primary">
            {t("project.addProjectTitle")}
          </h2>
          <button
            onClick={onClose}
            className="text-muted hover:text-secondary p-1 rounded transition-colors outline-none"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Tabs */}
        <div className="flex gap-1 mb-4 p-0.5 bg-background rounded-lg border border-border-subtle">
          {(["manual", "scan", "linked"] as const).map((key) => (
            <button
              key={key}
              onClick={() => setTab(key)}
              className={cn(
                "flex-1 py-1.5 text-[13px] font-medium rounded-md transition-all outline-none",
                tab === key
                  ? "bg-surface text-primary shadow-sm"
                  : "text-muted hover:text-secondary"
              )}
            >
              {t(
                key === "manual"
                  ? "project.tabManual"
                  : key === "scan"
                    ? "project.tabScan"
                    : "project.tabLinked"
              )}
            </button>
          ))}
        </div>

        {tab === "manual" ? (
          <div className="space-y-3">
            <p className="text-[13px] text-tertiary">
              {t("project.addManual")}
            </p>
            <button
              onClick={handleSelectFolder}
              disabled={adding}
              className="flex items-center gap-2 w-full px-3 py-2.5 rounded-lg border border-dashed border-border-subtle hover:border-border bg-background text-[13px] text-tertiary hover:text-secondary transition-all outline-none"
            >
              <FolderOpen className="w-4 h-4 text-muted" />
              {adding ? t("common.loading") : t("project.addManual")}
            </button>
          </div>
        ) : tab === "scan" ? (
          <div className="space-y-3">
            <div className="flex gap-2">
              <input
                type="text"
                value={scanRoot}
                onChange={(e) => setScanRoot(e.target.value)}
                placeholder={t("project.scanDirPlaceholder")}
                className={cn(inputClass, "flex-1")}
                onKeyDown={(e) => e.key === "Enter" && handleScan()}
              />
              <button
                onClick={handleSelectBrowse}
                className="px-2.5 rounded-[4px] border border-border-subtle bg-background text-muted hover:text-secondary hover:border-border transition-all outline-none"
                title={t("project.scanDir")}
              >
                <FolderOpen className="w-4 h-4" />
              </button>
              <button
                onClick={handleScan}
                disabled={!scanRoot.trim() || scanning}
                className="px-3 py-1.5 rounded-[4px] bg-accent-dark hover:bg-accent text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-accent-border outline-none"
              >
                {scanning ? (
                  t("project.scanning")
                ) : (
                  <Search className="w-4 h-4" />
                )}
              </button>
            </div>

            {scanned && scanResults.length === 0 && (
              <p className="text-[13px] text-muted py-4 text-center">
                {t("project.scanNoResult")}
              </p>
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
                        prev.size === scanResults.length
                          ? new Set()
                          : new Set(scanResults)
                      )
                    }
                    className="text-[12px] text-accent hover:underline outline-none"
                  >
                    {selected.size === scanResults.length
                      ? t("project.deselectAll")
                      : t("project.selectAll")}
                  </button>
                </div>
                <div className="max-h-[240px] overflow-y-auto space-y-1">
                  {scanResults.map((path) => (
                    <button
                      key={path}
                      onClick={() => toggleSelect(path)}
                      className={cn(
                        "flex items-center gap-2 w-full px-3 py-2 rounded-lg text-left text-[13px] transition-all outline-none",
                        selected.has(path)
                          ? "bg-accent-bg/50 text-primary border border-accent-border/30"
                          : "bg-background text-tertiary border border-border-subtle hover:border-border"
                      )}
                    >
                      <div
                        className={cn(
                          "w-4 h-4 rounded border flex items-center justify-center shrink-0",
                          selected.has(path)
                            ? "bg-accent-dark border-accent-border text-white"
                            : "border-border-subtle"
                        )}
                      >
                        {selected.has(path) && <Check className="w-3 h-3" />}
                      </div>
                      <span className="truncate">{path}</span>
                    </button>
                  ))}
                </div>
                <div className="flex justify-end pt-1">
                  <button
                    onClick={handleAddSelected}
                    disabled={selected.size === 0 || adding}
                    className="px-3 py-1.5 rounded-[4px] bg-accent-dark hover:bg-accent text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-accent-border outline-none"
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
            <p className="text-[13px] text-tertiary">
              {t("project.addLinkedHint")}
            </p>
            <input
              type="text"
              value={linkedName}
              onChange={(e) => setLinkedName(e.target.value)}
              placeholder={t("project.linkedNamePlaceholder")}
              className={inputClass}
            />
            <div className="flex gap-2">
              <input
                type="text"
                value={linkedPath}
                onChange={(e) => setLinkedPath(e.target.value)}
                placeholder={t("project.linkedPathPlaceholder")}
                className={cn(inputClass, "flex-1")}
              />
              <button
                onClick={async () => {
                  const dir = await dialogOpen({ directory: true, multiple: false });
                  if (dir) setLinkedPath(dir as string);
                }}
                className="px-2.5 rounded-[4px] border border-border-subtle bg-background text-muted hover:text-secondary hover:border-border transition-all outline-none"
                title={t("project.selectSkillsDir")}
              >
                <FolderOpen className="w-4 h-4" />
              </button>
            </div>
            <p className="text-[12px] leading-5 text-muted">
              {t("project.linkedDisabledPathHint")}
            </p>
            <button
              onClick={handleAddLinkedWorkspace}
              disabled={adding || !linkedName.trim() || !linkedPath.trim()}
              className="flex items-center justify-center gap-2 w-full px-3 py-2.5 rounded-lg border border-dashed border-border-subtle hover:border-border bg-background text-[13px] text-tertiary hover:text-secondary transition-all outline-none disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <FolderOpen className="w-4 h-4 text-muted" />
              {adding ? t("common.loading") : t("project.addLinkedWorkspace")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
