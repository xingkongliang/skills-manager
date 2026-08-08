import { useEffect, useState } from "react";
import { X, PlugZap, Server, Download } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import * as api from "../lib/tauri";
import type { Host, SshImportCandidate } from "../lib/tauri";
import { cn } from "../utils";

interface Props {
  open: boolean;
  onClose: () => void;
  onAdded: (host: Host) => Promise<void>;
}

export function AddHostDialog({ open, onClose, onAdded }: Props) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [sshTarget, setSshTarget] = useState("");
  const [testing, setTesting] = useState(false);
  const [adding, setAdding] = useState(false);
  const [preview, setPreview] = useState<Host | null>(null);
  const [candidates, setCandidates] = useState<SshImportCandidate[]>([]);

  useEffect(() => {
    if (!open) return;
    setName("");
    setSshTarget("");
    setTesting(false);
    setAdding(false);
    setPreview(null);
    api.listImportableSshHosts()
      .then(setCandidates)
      .catch((error) => {
        console.error("Failed to list importable SSH hosts:", error);
        setCandidates([]);
      });
  }, [open]);

  if (!open) return null;

  const inputClass =
    "w-full bg-background border border-border-subtle rounded-lg px-3 py-2 text-[13px] text-secondary focus:outline-none focus:border-border transition-all placeholder-faint";

  const handleTest = async () => {
    if (!sshTarget.trim()) return;
    setTesting(true);
    try {
      const result = await api.testSshHostConnection(sshTarget.trim());
      setPreview(result);
      if (!name.trim()) {
        setName(sshTarget.trim());
      }
      toast.success(t("hosts.connectionSuccess"));
    } catch (error) {
      console.error("SSH connection test failed:", error);
      setPreview(null);
      toast.error(t("hosts.connectionFailed"));
    } finally {
      setTesting(false);
    }
  };

  const handleAdd = async () => {
    if (!name.trim() || !sshTarget.trim()) return;
    setAdding(true);
    try {
      const host = await api.addSshHost(name.trim(), sshTarget.trim());
      await onAdded(host);
      toast.success(t("hosts.added"));
      onClose();
    } catch (error) {
      console.error("Failed to add SSH host:", error);
      toast.error(t("hosts.addFailed"));
    } finally {
      setAdding(false);
    }
  };

  const applyCandidate = (candidate: SshImportCandidate) => {
    setSshTarget(candidate.alias);
    setName(candidate.alias);
    setPreview(null);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative bg-surface border border-border rounded-xl w-full max-w-[560px] p-5 shadow-2xl">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-[13px] font-semibold text-primary flex items-center gap-2">
            <Server className="w-4 h-4 text-accent-light" />
            {t("hosts.addHost")}
          </h2>
          <button onClick={onClose} className="text-muted hover:text-secondary p-1 rounded transition-colors outline-none">
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="space-y-3">
          {candidates.length > 0 ? (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-[13px] text-tertiary">
                <Download className="w-4 h-4" />
                {t("hosts.importFromSshConfig")}
              </div>
              <div className="max-h-32 overflow-y-auto space-y-1.5 rounded-lg border border-border-subtle bg-background p-2">
                {candidates.map((candidate) => (
                  <button
                    key={candidate.alias}
                    onClick={() => applyCandidate(candidate)}
                    className="flex w-full items-center justify-between rounded-lg px-2.5 py-2 text-left text-[13px] text-secondary hover:bg-surface-hover outline-none"
                  >
                    <div className="min-w-0">
                      <div className="font-medium text-primary truncate">{candidate.alias}</div>
                      <div className="text-muted truncate">
                        {candidate.user ? `${candidate.user}@` : ""}{candidate.host_name || candidate.alias}
                        {candidate.port ? `:${candidate.port}` : ""}
                      </div>
                    </div>
                    <span className="text-accent-light shrink-0">{t("hosts.use")}</span>
                  </button>
                ))}
              </div>
            </div>
          ) : null}

          <div>
            <label className="mb-1 block text-[13px] text-tertiary">{t("hosts.hostName")}</label>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder={t("hosts.hostNamePlaceholder")}
              className={inputClass}
            />
          </div>

          <div>
            <label className="mb-1 block text-[13px] text-tertiary">{t("hosts.sshTarget")}</label>
            <input
              value={sshTarget}
              onChange={(e) => setSshTarget(e.target.value)}
              placeholder={t("hosts.sshTargetPlaceholder")}
              className={inputClass}
            />
          </div>

          {preview ? (
            <div className="rounded-lg border border-emerald-500/30 bg-emerald-500/[0.08] px-3 py-2 text-[13px] text-secondary">
              <div className="font-medium text-primary mb-1">{preview.connection_label}</div>
              <div className="text-muted">{t("hosts.connectionVerified")}</div>
            </div>
          ) : null}
        </div>

        <div className="mt-5 flex justify-end gap-2">
          <button
            onClick={onClose}
            className="px-3 py-1.5 rounded-lg text-[13px] font-medium text-tertiary hover:text-secondary hover:bg-surface-hover transition-colors outline-none"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={handleTest}
            disabled={!sshTarget.trim() || testing || adding}
            className={cn(
              "px-3 py-1.5 rounded-lg text-[13px] font-medium transition-colors outline-none border",
              !sshTarget.trim() || testing || adding
                ? "opacity-50 cursor-not-allowed border-border-subtle text-muted"
                : "border-accent-border bg-background hover:bg-surface-hover text-secondary"
            )}
          >
            <span className="inline-flex items-center gap-1.5">
              <PlugZap className="w-4 h-4" />
              {testing ? t("hosts.testing") : t("hosts.testConnection")}
            </span>
          </button>
          <button
            onClick={handleAdd}
            disabled={!name.trim() || !sshTarget.trim() || adding}
            className="px-3 py-1.5 rounded-lg bg-accent-dark hover:bg-accent text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-accent-border outline-none"
          >
            {adding ? t("common.loading") : t("hosts.addHost")}
          </button>
        </div>
      </div>
    </div>
  );
}
