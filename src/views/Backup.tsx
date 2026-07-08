import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Check,
  Cloud,
  Copy,
  ExternalLink,
  Github,
  History,
  Loader2,
  Pencil,
  RefreshCw,
  Save,
  ShieldCheck,
  Unlink,
  Upload,
  Wrench,
  XCircle,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { writeText as clipboardWriteText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { cn } from "../utils";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { GitRecoveryDialog } from "../components/GitRecoveryDialog";
import { GitSetupDialog } from "../components/GitSetupDialog";
import { useApp } from "../context/AppContext";
import { getErrorKind, getErrorMessage } from "../lib/error";
import { mapGitErrorMessage } from "../lib/gitErrors";
import * as api from "../lib/tauri";
import type {
  GitBackupSizeReport,
  GitBackupStatus,
  GitBackupVersion,
  GitUpstreamHealth,
} from "../lib/tauri";

type BackupMode =
  | "loading"
  | "uninitialized"
  | "needs_remote"
  | "needs_fix"
  | "up_to_date"
  | "pending_changes";

type LoadingAction = "start" | "sync" | "recovery" | "save" | "disconnect" | "github" | null;

const DEFAULT_GITHUB_REPO = "skills-manager-backup";
const GITHUB_TOKEN_URL =
  "https://github.com/settings/tokens/new?scopes=repo&description=Skills%20Manager%20Backup";
type RecoveryReason = GitUpstreamHealth | "conflict";

function displaySnapshotLabel(tag: string) {
  const raw = tag.startsWith("sm-v-") ? tag.slice("sm-v-".length) : tag;
  const parts = raw.split("-");
  if (parts.length < 3) return raw;
  return `${parts[0]}-${parts[1]}`;
}

function formatSnapshotWhen(tag: string | null) {
  if (!tag) return null;
  const label = displaySnapshotLabel(tag);
  const match = label.match(/^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})$/);
  if (!match) return label;
  const [, year, month, day, hour, min] = match;
  return `${year}-${month}-${day} ${hour}:${min}`;
}

function formatDateTime(iso: string) {
  if (!iso) return "-";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString();
}

function formatBytes(bytes: number) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

export function Backup() {
  const { t } = useTranslation();
  const { managedSkills, refreshManagedSkills } = useApp();
  const [gitStatus, setGitStatus] = useState<GitBackupStatus | null>(null);
  const [remoteInput, setRemoteInput] = useState("");
  const [remoteConfig, setRemoteConfig] = useState("");
  const [versions, setVersions] = useState<GitBackupVersion[]>([]);
  const [versionsLoading, setVersionsLoading] = useState(false);
  const [loading, setLoading] = useState<LoadingAction>(null);
  const [setupOpen, setSetupOpen] = useState(false);
  const [recoveryOpen, setRecoveryOpen] = useState(false);
  const [recoveryReason, setRecoveryReason] = useState<RecoveryReason>("unrelated_histories");
  const [restoreVersionTag, setRestoreVersionTag] = useState<string | null>(null);
  const [restoringVersionTag, setRestoringVersionTag] = useState<string | null>(null);
  const [disconnectConfirmOpen, setDisconnectConfirmOpen] = useState(false);
  const [backupError, setBackupError] = useState<string | null>(null);
  const [sizeReport, setSizeReport] = useState<GitBackupSizeReport | null>(null);
  const [githubToken, setGithubToken] = useState("");
  const [githubRepoName, setGithubRepoName] = useState(DEFAULT_GITHUB_REPO);
  const [githubError, setGithubError] = useState<string | null>(null);
  const [patMode, setPatMode] = useState(false);
  const [deviceInfo, setDeviceInfo] = useState<api.GithubDeviceFlowStart | null>(null);
  const deviceCancelRef = useRef(false);
  const [deviceName, setDeviceName] = useState("");
  const [deviceNameDraft, setDeviceNameDraft] = useState("");
  const [deviceNameEditing, setDeviceNameEditing] = useState(false);
  const [autoBackupEnabled, setAutoBackupEnabled] = useState(true);
  const [autoBackupSaving, setAutoBackupSaving] = useState(false);
  const [pendingConflicts, setPendingConflicts] = useState<api.PendingConflict[]>([]);
  const [resolvingConflict, setResolvingConflict] = useState<string | null>(null);
  // §3.1 disconnect matrix rows 2–3 + reconnect guidance after revocation.
  const [authMethod, setAuthMethod] = useState("");
  const [revokeConfirmOpen, setRevokeConfirmOpen] = useState(false);
  const [deleteRemoteConfirmOpen, setDeleteRemoteConfirmOpen] = useState(false);
  const [reconnectMode, setReconnectMode] = useState(false);
  const [backupErrorRaw, setBackupErrorRaw] = useState("");

  // Abandon an in-flight device-flow poll loop when leaving the page.
  useEffect(() => () => {
    deviceCancelRef.current = true;
  }, []);

  const mapGitError = useCallback(
    (error: unknown) => mapGitErrorMessage(error, t),
    [t],
  );

  const isSyncConflictError = (error: unknown) => {
    const message = getErrorMessage(error, "");
    return message.includes("SYNC_CONFLICT") || message.includes("CONFLICT");
  };

  const isRecoverableSetupError = (error: unknown) => {
    const message = getErrorMessage(error, "");
    return (
      message.includes("unrelated histories")
      || message.includes("refusing to merge")
      || message.includes("[rejected]")
      || message.includes("non-fast-forward")
      || message.includes("fetch first")
      || message.includes("failed to push some refs")
      || message.includes("no upstream")
      || isSyncConflictError(error)
    );
  };

  const refreshGitStatus = useCallback(async (fetchRemote = false) => {
    try {
      if (fetchRemote) {
        await api.gitBackupFetch().catch(() => {});
      }
      const status = await api.gitBackupStatus();
      setGitStatus(status);
      return status;
    } catch {
      return null;
    }
  }, []);

  const refreshVersions = useCallback(async () => {
    setVersionsLoading(true);
    try {
      const items = await api.gitBackupListVersions(50);
      setVersions(items);
    } catch {
      setVersions([]);
    } finally {
      setVersionsLoading(false);
    }
  }, []);

  // "Needs attention" sync conflicts (merge-engine design §4).
  const refreshPendingConflicts = useCallback(async () => {
    try {
      setPendingConflicts(await api.gitBackupPendingConflicts());
    } catch {
      setPendingConflicts([]);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      // §3.7: move any token embedded in the remote URL into the OS keychain
      // before the URL is read or displayed. Idempotent and best-effort —
      // offline machines simply retry on the next visit.
      const migrated = await api.gitBackupMigrateCredentials().catch(() => null);
      if (migrated) {
        toast.info(t("backup.credentialsMigrated"));
      }
      api.backupDeviceName().then(setDeviceName).catch(() => {});
      api.getSettings("backup_auto_enabled")
        .then((v) => {
          const normalized = (v ?? "").trim().toLowerCase();
          setAutoBackupEnabled(!["off", "false", "0", "no"].includes(normalized));
        })
        .catch(() => {});
      // A failed automatic backup persists until a backup succeeds (§3.4) —
      // resurface it when the page opens.
      api.getSettings("backup_last_auto_error")
        .then((v) => {
          const raw = (v ?? "").trim();
          if (raw) {
            setBackupError(mapGitError(raw));
            setBackupErrorRaw(raw);
          }
        })
        .catch(() => {});
      api.getSettings("github_auth_method")
        .then((v) => setAuthMethod((v ?? "").trim()))
        .catch(() => {});
      const savedRemote = (await api.getSettings("git_backup_remote_url").catch(() => null))?.trim() || "";
      setRemoteInput(savedRemote);
      setRemoteConfig(savedRemote);
      const status = await refreshGitStatus(true);
      if (status?.is_repo) {
        await refreshVersions();
        void refreshPendingConflicts();
        api.gitBackupSizeReport().then(setSizeReport).catch(() => setSizeReport(null));
      }
    })();
  }, [mapGitError, refreshGitStatus, refreshPendingConflicts, refreshVersions, t]);

  // Live updates from the background auto-backup rounds.
  useEffect(() => {
    const unlistenPromise = listen<{ ok: boolean; pending: boolean; error: string | null }>(
      "backup-auto-completed",
      (event) => {
        setBackupError(event.payload.error ? mapGitError(event.payload.error) : null);
        setBackupErrorRaw(event.payload.error ?? "");
        void refreshGitStatus();
        void refreshVersions();
        void refreshPendingConflicts();
      },
    );
    return () => {
      void unlistenPromise.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [mapGitError, refreshGitStatus, refreshPendingConflicts, refreshVersions]);

  const handleToggleAutoBackup = async () => {
    const next = !autoBackupEnabled;
    setAutoBackupSaving(true);
    try {
      await api.setSettings("backup_auto_enabled", next ? "on" : "off");
      setAutoBackupEnabled(next);
    } catch {
      toast.error(t("common.error"));
    } finally {
      setAutoBackupSaving(false);
    }
  };

  useEffect(() => {
    if (gitStatus?.is_repo) {
      void refreshVersions();
    } else {
      setVersions([]);
    }
  }, [gitStatus?.is_repo, refreshVersions]);

  const mode: BackupMode = useMemo(() => {
    if (!gitStatus) return "loading";
    if (!gitStatus.is_repo) return "uninitialized";
    if (!gitStatus.remote_url && !remoteConfig) return "needs_remote";
    if (
      gitStatus.upstream_health === "unrelated_histories"
      || gitStatus.upstream_health === "detached"
    ) {
      return "needs_fix";
    }
    if (gitStatus.upstream_health === "no_upstream") return "pending_changes";
    if (gitStatus.has_changes || gitStatus.ahead > 0 || gitStatus.behind > 0) return "pending_changes";
    return "up_to_date";
  }, [gitStatus, remoteConfig]);

  const statusMeta = useMemo(() => {
    // A failed backup stays visible (with a plain-language reason and a retry
    // action) instead of vanishing with the toast — §3.4 three-state language.
    if (backupError) {
      return {
        icon: XCircle,
        title: t("backup.status.failed"),
        description: backupError,
        className: "border-red-500/40 bg-red-500/10",
        iconClassName: "text-red-500",
      };
    }
    switch (mode) {
      case "loading":
        return {
          icon: Loader2,
          title: t("backup.status.loading"),
          description: t("backup.status.loadingDesc"),
          className: "border-border bg-surface",
          iconClassName: "text-muted animate-spin",
        };
      case "uninitialized":
      case "needs_remote":
        return {
          icon: Cloud,
          title: t("backup.status.notConnected"),
          description: t("backup.status.notConnectedDesc"),
          className: "border-border bg-surface",
          iconClassName: "text-muted",
        };
      case "needs_fix":
        return {
          icon: AlertTriangle,
          title: t("backup.status.needsFix"),
          description: t("backup.status.needsFixDesc"),
          className: "border-red-500/40 bg-red-500/10",
          iconClassName: "text-red-500",
        };
      case "pending_changes": {
        // Three distinct situations wear this state; naming them precisely
        // matters because "back up" reads as push-only and makes users fear
        // overwriting the remote when only remote updates exist.
        const localCount = Math.max(gitStatus?.ahead ?? 0, gitStatus?.has_changes ? 1 : 0);
        const remoteCount = gitStatus?.behind ?? 0;
        const remoteOnly = remoteCount > 0 && localCount === 0;
        const both = remoteCount > 0 && localCount > 0;
        return {
          icon: remoteOnly ? RefreshCw : Upload,
          title: remoteOnly ? t("backup.status.remoteOnly") : t("backup.status.pending"),
          description: remoteOnly
            ? t("backup.status.remoteOnlyDesc", { remote: remoteCount })
            : both
              ? t("backup.status.pendingBothDesc", { local: localCount, remote: remoteCount })
              : (gitStatus?.changed_skill_count ?? 0) > 0
                ? t("backup.status.pendingSkills", { count: gitStatus?.changed_skill_count })
                : t("backup.status.pendingDesc", { local: localCount, remote: remoteCount }),
          className: "border-amber-500/40 bg-amber-500/10",
          iconClassName: "text-amber-600 dark:text-amber-400",
        };
      }
      case "up_to_date":
        return {
          icon: CheckCircle2,
          title: t("backup.status.synced"),
          description: t("backup.status.syncedDesc", {
            when: formatSnapshotWhen(gitStatus?.current_snapshot_tag ?? null) ?? t("backup.status.noSnapshot"),
          }),
          className: "border-emerald-500/30 bg-emerald-500/10",
          iconClassName: "text-emerald-600 dark:text-emerald-400",
        };
    }
  }, [backupError, gitStatus, mode, t]);

  const handleSaveRemote = async () => {
    const trimmed = remoteInput.trim();
    setLoading("save");
    try {
      // Never persist credentials embedded in the URL: they go to the OS
      // keychain and only the sanitized URL is saved and shown (§3.7).
      const effective = trimmed ? await api.gitBackupSanitizeRemoteUrl(trimmed) : "";
      await api.setSettings("git_backup_remote_url", effective);
      if (effective && gitStatus?.is_repo) {
        await api.gitBackupSetRemote(effective);
      }
      setRemoteInput(effective);
      setRemoteConfig(effective);
      toast.success(t("settings.gitConfigSaved"));
      await refreshGitStatus();
    } catch (error) {
      toast.error(mapGitError(error));
    } finally {
      setLoading(null);
    }
  };

  const handleSetupClone = async () => {
    setLoading("start");
    try {
      await api.gitBackupClone(remoteConfig);
      toast.success(t("settings.gitCloneSuccess"));
      await Promise.all([refreshGitStatus(true), refreshManagedSkills(), refreshVersions()]);
    } catch (error) {
      toast.error(mapGitError(error));
      throw error;
    } finally {
      setLoading(null);
    }
  };

  const handleSetupInit = async () => {
    setLoading("start");
    try {
      await api.gitBackupInit();
      if (remoteConfig) {
        await api.gitBackupSetRemote(remoteConfig);
      }
      toast.success(t("settings.gitInitSuccess"));
      await Promise.all([refreshGitStatus(true), refreshVersions()]);
    } catch (error) {
      toast.error(mapGitError(error));
      throw error;
    } finally {
      setLoading(null);
    }
  };

  const handleRecoveryReclone = async () => {
    if (!remoteConfig) {
      toast.info(t("settings.gitNeedRemoteSetup"));
      return;
    }
    setLoading("recovery");
    try {
      await api.gitBackupReclone(remoteConfig);
      toast.success(t("settings.gitRecoveryRecloneSuccess"));
      await Promise.all([refreshGitStatus(true), refreshManagedSkills(), refreshVersions()]);
    } catch (error) {
      toast.error(mapGitError(error));
      throw error;
    } finally {
      setLoading(null);
    }
  };

  const handleBackupNow = async () => {
    setLoading("sync");
    try {
      let status = await api.gitBackupStatus();
      if (!status.is_repo) {
        setSetupOpen(true);
        return;
      }
      if (!status.remote_url && remoteConfig) {
        await api.gitBackupSetRemote(remoteConfig);
        status = await api.gitBackupStatus();
      }
      if (!status.remote_url) {
        toast.info(t("settings.gitNeedRemoteSetup"));
        return;
      }
      if (
        status.upstream_health === "unrelated_histories"
        || status.upstream_health === "detached"
      ) {
        setRecoveryReason(status.upstream_health);
        setRecoveryOpen(true);
        return;
      }
      // One backend transaction: commit → merge → snapshot → push, retried
      // internally when another device pushes concurrently (§9 并发收敛).
      const outcome = await api.gitBackupSync(t("settings.gitCommitPlaceholder"));
      const merge = outcome.merge;
      if (merge && merge.engine === "object" && !merge.legacy_fallback) {
        // Object merge (merge-engine design §8): human-readable outcome.
        if (merge.new_conflicts.length > 0) {
          toast.warning(
            t("backup.merge.newConflicts", { count: merge.new_conflicts.length }),
            { duration: 10000 },
          );
        } else {
          toast.success(t("backup.merge.applied", { count: merge.updated.length }));
        }
        if (merge.old_client_warning) {
          toast.warning(merge.old_client_warning, { duration: 12000 });
        }
        void refreshPendingConflicts();
      } else if (merge) {
        toast.success(t("settings.gitPullSuccess"));
      }
      if (merge) {
        await refreshManagedSkills();
      }
      if (outcome.pushed && outcome.snapshot_tag) {
        toast.success(t("mySkills.gitSyncSuccessWithVersion", { tag: displaySnapshotLabel(outcome.snapshot_tag) }));
      } else if (!merge) {
        toast.success(t("settings.gitUpToDate"));
      }
      setBackupError(null);
      setBackupErrorRaw("");
      await Promise.all([refreshGitStatus(true), refreshVersions()]);
    } catch (error) {
      setBackupError(mapGitError(error));
      setBackupErrorRaw(getErrorMessage(error, ""));
      const message = getErrorMessage(error, "");
      if (message.includes("pending on both devices")) {
        // Object-merge block (§4 双侧声明): the fix is resolving the pending
        // conflict on one device — reclone/recovery would be wrong advice.
        toast.error(t("backup.conflicts.blockedBothDevices"), { duration: 12000 });
      } else if (isRecoverableSetupError(error)) {
        toast.error(mapGitError(error));
        const latest = await refreshGitStatus();
        setRecoveryReason(isSyncConflictError(error) ? "conflict" : (latest?.upstream_health ?? "unrelated_histories"));
        setRecoveryOpen(true);
      } else {
        toast.error(mapGitError(error));
      }
    } finally {
      setLoading(null);
    }
  };

  const handleResolveConflict = async (
    skillId: string,
    action: api.ResolveConflictAction,
  ) => {
    setResolvingConflict(skillId);
    try {
      const safetyTag = await api.gitBackupResolveConflict(skillId, action);
      toast.success(
        t("backup.conflicts.resolved", { tag: displaySnapshotLabel(safetyTag) }),
      );
      await Promise.all([
        refreshPendingConflicts(),
        refreshGitStatus(),
        refreshVersions(),
        refreshManagedSkills(),
      ]);
    } catch (error) {
      toast.error(mapGitError(error));
    } finally {
      setResolvingConflict(null);
    }
  };

  const conflictDisplayName = (conflict: api.PendingConflict) => {
    const managed = managedSkills.find((skill) => skill.id === conflict.skill_id);
    if (managed?.name) return managed.name;
    const fromPath = conflict.theirs_path?.split("/").pop();
    return fromPath || conflict.skill_id.slice(0, 8);
  };

  const mapGithubError = (error: unknown) => {
    const message = getErrorMessage(error, "");
    if (message.includes("GITHUB_TOKEN_INVALID")) return t("backup.github.errorToken");
    if (message.includes("GITHUB_SCOPE")) return t("backup.github.errorScope");
    if (message.includes("KEYCHAIN_UNAVAILABLE")) return t("backup.github.errorKeychain");
    if (message.includes("GITHUB_DEVICE_EXPIRED")) return t("backup.github.deviceExpired");
    if (message.includes("GITHUB_DEVICE_DENIED")) return t("backup.github.deviceDenied");
    if (message.includes("GITHUB_NETWORK") || getErrorKind(error) === "network") {
      // §3.2: when github.com is unreachable, point at the PAT fallback too.
      return `${t("settings.gitErrorNetwork")} ${t("backup.github.deviceFallbackPat")}`;
    }
    return mapGitError(error);
  };

  /** Shared tail of both connect paths: wire the repo locally and either
   * restore the existing backup or push the first one. */
  const finishGithubConnect = async (res: api.GithubBackupConnectResult) => {
    setReconnectMode(false);
    setBackupError(null);
    setBackupErrorRaw("");
    api.getSettings("github_auth_method")
      .then((v) => setAuthMethod((v ?? "").trim()))
      .catch(() => {});
    setRemoteInput(res.url);
    setRemoteConfig(res.url);
    if (res.repo_created) {
      const repo = res.url.replace(/^https:\/\/github\.com\//, "").replace(/\.git$/, "");
      toast.success(t("backup.github.repoCreated", { repo }));
    }
    if (!res.repo_private) {
      // Connecting a backup to a PUBLIC repo is almost never intentional.
      toast.warning(t("backup.github.publicRepoWarning"), { duration: 15000 });
    }
    const status = await api.gitBackupStatus();
    if (res.remote_has_content) {
      // Existing backup: restore it (or just rewire when a repo already exists).
      if (!status.is_repo) {
        await api.gitBackupClone(res.url);
      } else {
        await api.gitBackupSetRemote(res.url);
      }
      toast.success(t("backup.github.connectedRestored"));
      await Promise.all([refreshGitStatus(true), refreshManagedSkills(), refreshVersions()]);
    } else {
      // Fresh backup: initialize if needed, wire the remote, run the first backup.
      if (!status.is_repo) {
        await api.gitBackupInit();
      }
      await api.gitBackupSetRemote(res.url);
      await refreshGitStatus();
      await handleBackupNow();
    }
  };

  const handleGithubConnect = async () => {
    const token = githubToken.trim();
    if (!token) return;
    setLoading("github");
    setGithubError(null);
    try {
      const res = await api.githubBackupConnect(
        token,
        githubRepoName.trim() || DEFAULT_GITHUB_REPO,
      );
      // Token is in the OS keychain now; drop it from component state.
      setGithubToken("");
      await finishGithubConnect(res);
    } catch (error) {
      setGithubError(mapGithubError(error));
    } finally {
      setLoading(null);
    }
  };

  const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

  const handleDeviceFlow = async () => {
    setLoading("github");
    setGithubError(null);
    deviceCancelRef.current = false;
    try {
      const info = await api.githubDeviceFlowStart();
      setDeviceInfo(info);
      void openUrl(info.verification_uri);

      const repoName = githubRepoName.trim() || DEFAULT_GITHUB_REPO;
      let intervalSec = Math.max(info.interval, 5);
      const deadline = Date.now() + info.expires_in * 1000;
      while (!deviceCancelRef.current && Date.now() < deadline) {
        await sleep(intervalSec * 1000);
        if (deviceCancelRef.current) return;
        const poll = await api.githubDeviceFlowPoll(info.device_code, repoName);
        if (poll.status === "slow_down") {
          intervalSec += 5;
          continue;
        }
        if (poll.status === "connected" && poll.result) {
          setDeviceInfo(null);
          await finishGithubConnect(poll.result);
          return;
        }
        // "pending" → keep polling.
      }
      if (!deviceCancelRef.current) {
        setGithubError(t("backup.github.deviceExpired"));
      }
    } catch (error) {
      setGithubError(mapGithubError(error));
    } finally {
      setDeviceInfo(null);
      setLoading(null);
    }
  };

  const cancelDeviceFlow = () => {
    deviceCancelRef.current = true;
    setDeviceInfo(null);
    setLoading(null);
  };

  const handleRestoreVersion = async () => {
    if (!restoreVersionTag) return;
    setRestoringVersionTag(restoreVersionTag);
    try {
      const safetyTag = await api.gitBackupRestoreVersion(restoreVersionTag);
      toast.success(t("mySkills.gitVersionRestoreSuccess", { tag: displaySnapshotLabel(restoreVersionTag) }));
      toast.info(t("backup.restoreSafetyPoint", { tag: displaySnapshotLabel(safetyTag) }));
      await Promise.all([refreshGitStatus(), refreshVersions(), refreshManagedSkills()]);
      setRestoreVersionTag(null);
    } catch (error) {
      toast.error(mapGitError(error));
    } finally {
      setRestoringVersionTag(null);
    }
  };

  const handleSaveDeviceName = async () => {
    const draft = deviceNameDraft.trim();
    setDeviceNameEditing(false);
    if (!draft || draft === deviceName) return;
    try {
      const saved = await api.backupSetDeviceName(draft);
      setDeviceName(saved);
      toast.success(t("backup.device.renamed"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleDisconnect = async () => {
    setLoading("disconnect");
    try {
      await api.gitBackupRemoveRemote();
      setRemoteInput("");
      setRemoteConfig("");
      toast.success(t("settings.gitDisconnected"));
      await refreshGitStatus();
    } catch {
      toast.error(t("common.error"));
    } finally {
      setDisconnectConfirmOpen(false);
      setLoading(null);
    }
  };

  // Must match core/github_api.rs OAUTH_CLIENT_ID (public device-flow id).
  const GITHUB_OAUTH_CLIENT_ID = "Ov23li4a3SMdhIiKo7IE";
  const remoteUrlValue = gitStatus?.remote_url || remoteConfig || "";
  const isGithubRemote = remoteUrlValue.includes("github.com");
  const githubRepoWebUrl = (() => {
    const match = remoteUrlValue.match(/github\.com[/:]([^/]+\/[^/]+?)(\.git)?$/);
    return match ? `https://github.com/${match[1]}` : null;
  })();
  // Token revoked/expired on the GitHub side → offer an explicit reconnect
  // instead of only a failure card (backup redesign Phase 2 待办).
  const authErrorNeedsReconnect =
    isGithubRemote
    && /authentication failed|401|403|invalid.{0,24}(credentials|token)|could not read username/i.test(
      backupErrorRaw,
    );

  // §3.1 row 2: revoking is done on GitHub's side (a public device-flow app
  // has no client secret, so tokens cannot be revoked via API) — open the
  // right page and disconnect this machine.
  const handleRevokeAuthorization = async () => {
    setRevokeConfirmOpen(false);
    const oauthUrl = `https://github.com/settings/connections/applications/${GITHUB_OAUTH_CLIENT_ID}`;
    const patUrl = "https://github.com/settings/tokens";
    if (authMethod === "pat") {
      openUrl(patUrl).catch(() => {});
    } else if (authMethod === "oauth") {
      openUrl(oauthUrl).catch(() => {});
    } else {
      // Connected before the method was recorded (or wired manually): the
      // credential could be either kind — open both pages so nothing stays
      // silently authorized.
      openUrl(oauthUrl).catch(() => {});
      openUrl(patUrl).catch(() => {});
    }
    await handleDisconnect();
  };

  // §3.1 row 3: repo deletion needs the `delete_repo` scope our tokens
  // deliberately don't have — GitHub's own settings page (with its type-the-
  // repo-name confirmation) is the safe double-confirm path.
  const handleOpenDeleteRemote = async () => {
    setDeleteRemoteConfirmOpen(false);
    if (githubRepoWebUrl) {
      await openUrl(`${githubRepoWebUrl}/settings#danger-zone`).catch(() => {});
      toast.info(t("backup.disconnect.deleteRemoteOpened"), { duration: 12000 });
    }
  };

  const StatusIcon = statusMeta.icon;
  const canBackupNow = mode === "pending_changes" || mode === "up_to_date";
  const remoteLabel = gitStatus?.remote_url || remoteConfig || t("backup.connection.none");
  const branchLabel = gitStatus?.branch || t("backup.connection.unknown");

  return (
    <div className="app-page">
      <div className="app-page-header pr-2 pb-1 flex items-center justify-between gap-3">
        <div>
          <h1 className="app-page-title">{t("backup.title")}</h1>
          <p className="mt-1 text-[13px] text-muted">{t("backup.subtitle")}</p>
        </div>
        <button
          type="button"
          onClick={() => refreshGitStatus(true)}
          disabled={!!loading}
          className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-border bg-surface px-2.5 text-[13px] font-medium text-tertiary transition-colors hover:bg-surface-hover disabled:opacity-50"
        >
          <RefreshCw className="h-3.5 w-3.5" />
          {t("settings.refresh")}
        </button>
      </div>

      <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_340px]">
        <div className="space-y-4">
          <section className={cn("app-panel border p-4", statusMeta.className)}>
            <div className="flex flex-wrap items-start justify-between gap-4">
              <div className="flex min-w-0 items-start gap-3">
                <div className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-[6px] border border-border-subtle bg-surface">
                  <StatusIcon className={cn("h-5 w-5", statusMeta.iconClassName)} />
                </div>
                <div className="min-w-0">
                  <h2 className="text-[15px] font-semibold text-primary">{statusMeta.title}</h2>
                  <p className="mt-1 text-[13px] leading-5 text-muted">{statusMeta.description}</p>
                  <div className="mt-3 grid gap-2 text-[12px] text-tertiary sm:grid-cols-2">
                    <div className="min-w-0">
                      <div className="text-faint">{t("backup.connection.repository")}</div>
                      <div className="truncate font-mono text-secondary" title={remoteLabel}>{remoteLabel}</div>
                    </div>
                    <div>
                      <div className="text-faint">{t("backup.connection.branch")}</div>
                      <div className="font-mono text-secondary">{branchLabel}</div>
                    </div>
                    <div className="min-w-0">
                      <div className="text-faint">{t("backup.device.label")}</div>
                      {deviceNameEditing ? (
                        <div className="mt-0.5 flex items-center gap-1">
                          <input
                            type="text"
                            value={deviceNameDraft}
                            onChange={(event) => setDeviceNameDraft(event.target.value)}
                            onKeyDown={(event) => {
                              if (event.key === "Enter") void handleSaveDeviceName();
                              if (event.key === "Escape") setDeviceNameEditing(false);
                            }}
                            autoFocus
                            maxLength={64}
                            className="h-6 min-w-0 flex-1 rounded-[4px] border border-border-subtle bg-background px-1.5 text-[12px] text-secondary outline-none focus:border-border"
                          />
                          <button
                            type="button"
                            onClick={handleSaveDeviceName}
                            className="rounded p-0.5 text-muted transition-colors hover:text-secondary"
                            title={t("common.save")}
                          >
                            <Check className="h-3.5 w-3.5" />
                          </button>
                        </div>
                      ) : (
                        <div className="flex items-center gap-1">
                          <span className="truncate text-secondary">{deviceName || "-"}</span>
                          <button
                            type="button"
                            onClick={() => {
                              setDeviceNameDraft(deviceName);
                              setDeviceNameEditing(true);
                            }}
                            className="rounded p-0.5 text-faint transition-colors hover:text-secondary"
                            title={t("backup.device.rename")}
                          >
                            <Pencil className="h-3 w-3" />
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              </div>

              <div className="flex shrink-0 flex-wrap items-center gap-2">
                {authErrorNeedsReconnect && (
                  <button
                    type="button"
                    onClick={() => setReconnectMode(true)}
                    disabled={!!loading}
                    className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-amber-500/40 bg-amber-500/10 px-3 text-[13px] font-medium text-amber-700 transition-colors hover:bg-amber-500/15 disabled:opacity-50 dark:text-amber-300"
                  >
                    <Github className="h-3.5 w-3.5" />
                    {t("backup.github.reconnect")}
                  </button>
                )}
                {mode === "needs_fix" ? (
                  <button
                    type="button"
                    onClick={() => {
                      setRecoveryReason(gitStatus?.upstream_health ?? "unrelated_histories");
                      setRecoveryOpen(true);
                    }}
                    disabled={!!loading}
                    className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-red-500/40 bg-red-500/10 px-3 text-[13px] font-medium text-red-600 transition-colors hover:bg-red-500/15 disabled:opacity-50 dark:text-red-300"
                  >
                    <Wrench className="h-3.5 w-3.5" />
                    {t("settings.gitRecoveryTitle")}
                  </button>
                ) : mode === "uninitialized" || mode === "needs_remote" ? (
                  <button
                    type="button"
                    onClick={() => setSetupOpen(true)}
                    disabled={!!loading || !remoteConfig}
                    className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-accent-border bg-accent-dark px-3 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:opacity-50"
                  >
                    {loading === "start" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Cloud className="h-3.5 w-3.5" />}
                    {t("settings.gitStartBackup")}
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={handleBackupNow}
                    disabled={!!loading || !canBackupNow}
                    className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-accent-border bg-accent-dark px-3 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:opacity-50"
                  >
                    {loading === "sync" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Upload className="h-3.5 w-3.5" />}
                    {backupError
                      ? t("backup.actions.retry")
                      : mode === "up_to_date"
                        ? t("backup.actions.backupAgain")
                        : (gitStatus?.behind ?? 0) > 0
                          ? t("backup.actions.syncNow")
                          : t("backup.actions.backupNow")}
                  </button>
                )}
              </div>
            </div>
          </section>

          {pendingConflicts.length > 0 && (
            <section className="app-panel border-amber-500/40 bg-amber-500/5 p-4">
              <div className="mb-1 flex items-center gap-2">
                <AlertTriangle className="h-4 w-4 text-amber-600 dark:text-amber-300" />
                <h2 className="text-[14px] font-semibold text-secondary">
                  {t("backup.conflicts.title")}
                </h2>
                <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] font-medium text-amber-700 dark:text-amber-300">
                  {pendingConflicts.length}
                </span>
              </div>
              <p className="mb-3 text-[13px] leading-5 text-muted">
                {t("backup.conflicts.desc")}
                {(gitStatus?.behind ?? 0) > 0 && (
                  <>
                    {" "}
                    {t("backup.conflicts.autoPaused")}
                  </>
                )}
              </p>
              <ul className="space-y-2">
                {pendingConflicts.map((conflict) => {
                  const busy = resolvingConflict === conflict.skill_id;
                  return (
                    <li
                      key={conflict.skill_id}
                      className="flex flex-wrap items-center justify-between gap-2 rounded-[6px] border border-border-subtle bg-bg-secondary px-3 py-2"
                    >
                      <div className="min-w-0">
                        <div className="truncate text-[13px] font-medium text-primary">
                          {conflictDisplayName(conflict)}
                        </div>
                        <div className="text-[12px] text-muted">
                          {t("backup.conflicts.itemDesc")}
                        </div>
                      </div>
                      <div className="flex shrink-0 flex-wrap items-center gap-1.5">
                        {busy ? (
                          <Loader2 className="h-4 w-4 animate-spin text-muted" />
                        ) : (
                          <>
                            <button
                              type="button"
                              onClick={() => handleResolveConflict(conflict.skill_id, "keep_local")}
                              disabled={!!resolvingConflict || !!loading}
                              className="rounded-[4px] border border-border-subtle px-2.5 py-1 text-[12px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                            >
                              {t("backup.conflicts.keepLocal")}
                            </button>
                            <button
                              type="button"
                              onClick={() => handleResolveConflict(conflict.skill_id, "use_remote")}
                              disabled={!!resolvingConflict || !!loading}
                              className="rounded-[4px] border border-border-subtle px-2.5 py-1 text-[12px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                            >
                              {t("backup.conflicts.useRemote")}
                            </button>
                            <button
                              type="button"
                              onClick={() => handleResolveConflict(conflict.skill_id, "keep_both")}
                              disabled={!!resolvingConflict || !!loading}
                              className="rounded-[4px] border border-border-subtle px-2.5 py-1 text-[12px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                            >
                              {t("backup.conflicts.keepBoth")}
                            </button>
                          </>
                        )}
                      </div>
                    </li>
                  );
                })}
              </ul>
            </section>
          )}

          {(reconnectMode || (!gitStatus?.remote_url && !remoteConfig)) && (
            <section className="app-panel p-4">
              <div className="mb-3 flex items-center gap-2">
                <Github className="h-4 w-4 text-muted" />
                <h2 className="text-[14px] font-semibold text-secondary">
                  {reconnectMode ? t("backup.github.reconnectTitle") : t("backup.github.title")}
                </h2>
              </div>
              <p className="mb-3 text-[13px] leading-5 text-muted">{t("backup.github.desc")}</p>

              {deviceInfo ? (
                <div className="space-y-3">
                  <div className="flex flex-col items-center gap-2 rounded-[6px] border border-border-subtle bg-bg-secondary px-4 py-4">
                    <div className="font-mono text-[26px] font-bold tracking-[0.25em] text-primary">
                      {deviceInfo.user_code}
                    </div>
                    <button
                      type="button"
                      onClick={() => {
                        void clipboardWriteText(deviceInfo.user_code);
                        toast.success(t("backup.github.deviceCodeCopied"));
                      }}
                      className="inline-flex items-center gap-1 text-[12px] text-muted transition-colors hover:text-secondary"
                    >
                      <Copy className="h-3 w-3" />
                      {t("backup.github.deviceCopyCode")}
                    </button>
                  </div>
                  <p className="text-[13px] leading-5 text-muted">
                    {t("backup.github.deviceWaitDesc", { uri: deviceInfo.verification_uri })}
                  </p>
                  <div className="flex items-center justify-between gap-2">
                    <span className="inline-flex items-center gap-1.5 text-[12px] text-muted">
                      <Loader2 className="h-3 w-3 animate-spin" />
                      {t("backup.github.deviceWaiting")}
                    </span>
                    <button
                      type="button"
                      onClick={cancelDeviceFlow}
                      className="rounded-[4px] px-2.5 py-1 text-[12px] font-medium text-tertiary transition-colors hover:bg-surface-hover hover:text-secondary"
                    >
                      {t("common.cancel")}
                    </button>
                  </div>
                </div>
              ) : (
                <div className="space-y-2">
                  <div className="flex flex-wrap items-center gap-2">
                    <button
                      type="button"
                      onClick={handleDeviceFlow}
                      disabled={!!loading}
                      className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-accent-border bg-accent-dark px-3 text-[13px] font-medium text-white transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      {loading === "github" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Github className="h-3.5 w-3.5" />}
                      {loading === "github" ? t("backup.github.connecting") : t("backup.github.deviceSignIn")}
                    </button>
                    <input
                      type="text"
                      value={githubRepoName}
                      onChange={(event) => setGithubRepoName(event.target.value)}
                      disabled={loading === "github"}
                      title={t("backup.github.repoLabel")}
                      className="h-8 w-52 rounded-[4px] border border-border-subtle bg-background px-2.5 font-mono text-[13px] text-secondary outline-none transition-colors focus:border-border disabled:opacity-50"
                      autoCapitalize="none"
                      autoCorrect="off"
                      spellCheck={false}
                    />
                  </div>

                  {patMode ? (
                    <>
                      <div className="flex flex-wrap items-center gap-2">
                        <input
                          type="password"
                          value={githubToken}
                          onChange={(event) => {
                            setGithubToken(event.target.value);
                            setGithubError(null);
                          }}
                          placeholder={t("backup.github.tokenPlaceholder")}
                          disabled={loading === "github"}
                          className="h-8 min-w-0 flex-1 rounded-[4px] border border-border-subtle bg-background px-2.5 font-mono text-[13px] text-secondary outline-none transition-colors focus:border-border disabled:opacity-50"
                          autoCapitalize="none"
                          autoCorrect="off"
                          spellCheck={false}
                        />
                        <button
                          type="button"
                          onClick={handleGithubConnect}
                          disabled={!!loading || !githubToken.trim()}
                          className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-border bg-surface-hover px-2.5 text-[13px] font-medium text-tertiary transition-colors hover:bg-surface-active disabled:cursor-not-allowed disabled:opacity-50"
                        >
                          {t("backup.github.connect")}
                        </button>
                      </div>
                      <button
                        type="button"
                        onClick={() => void openUrl(GITHUB_TOKEN_URL)}
                        className="inline-flex items-center gap-1 text-[12px] text-muted transition-colors hover:text-secondary"
                      >
                        <ExternalLink className="h-3 w-3" />
                        {t("backup.github.tokenHint")}
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      onClick={() => setPatMode(true)}
                      className="text-[12px] text-muted transition-colors hover:text-secondary"
                    >
                      {t("backup.github.patToggle")}
                    </button>
                  )}

                  {githubError && (
                    <div className="rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-[12px] leading-5 text-red-600 dark:text-red-300">
                      {githubError}
                    </div>
                  )}
                </div>
              )}
            </section>
          )}

          <section className="app-panel p-4">
            <div className="mb-3 flex items-center gap-2">
              <Cloud className="h-4 w-4 text-muted" />
              <h2 className="text-[14px] font-semibold text-secondary">{t("backup.connection.title")}</h2>
            </div>
            <p className="mb-3 text-[13px] leading-5 text-muted">{t("backup.connection.desc")}</p>
            <div className="flex flex-wrap items-center gap-2">
              <input
                type="text"
                value={remoteInput}
                onChange={(event) => setRemoteInput(event.target.value)}
                placeholder={t("settings.gitRemoteUrlPlaceholder")}
                className="h-8 min-w-0 flex-1 rounded-[4px] border border-border-subtle bg-background px-2.5 font-mono text-[13px] text-secondary outline-none transition-colors focus:border-border"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
              />
              <button
                type="button"
                onClick={handleSaveRemote}
                disabled={loading === "save"}
                className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-border bg-surface-hover px-2.5 text-[13px] font-medium text-tertiary transition-colors hover:bg-surface-active disabled:opacity-50"
              >
                {loading === "save" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Save className="h-3.5 w-3.5" />}
                {t("common.save")}
              </button>
            </div>
          </section>

          <section className="app-panel p-4">
            <div className="mb-3 flex items-center justify-between gap-3">
              <div className="flex items-center gap-2">
                <History className="h-4 w-4 text-muted" />
                <h2 className="text-[14px] font-semibold text-secondary">{t("backup.history.title")}</h2>
              </div>
              <button
                type="button"
                onClick={refreshVersions}
                disabled={versionsLoading || !gitStatus?.is_repo}
                className="inline-flex h-7 items-center gap-1.5 rounded-[4px] px-2 text-[13px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
              >
                <RefreshCw className={cn("h-3 w-3", versionsLoading && "animate-spin")} />
                {t("settings.refresh")}
              </button>
            </div>

            {versionsLoading ? (
              <div className="py-6 text-center text-[13px] text-muted">{t("mySkills.gitVersionLoading")}</div>
            ) : versions.length === 0 ? (
              <div className="rounded-[6px] border border-dashed border-border-subtle py-6 text-center text-[13px] text-muted">
                {t("backup.history.empty")}
              </div>
            ) : (
              <div className="max-h-[360px] space-y-1.5 overflow-auto pr-1">
                {versions.map((version) => (
                  <div
                    key={version.tag}
                    className="flex items-center justify-between gap-3 rounded-[6px] border border-border-subtle bg-bg-secondary px-3 py-2"
                  >
                    <div className="min-w-0">
                      <div className="truncate text-[13px] font-semibold text-secondary">
                        {displaySnapshotLabel(version.tag)}
                      </div>
                      <div className="truncate text-[12px] text-muted">{version.message || version.commit}</div>
                      <div className="text-[11px] text-faint">
                        {version.author ? `${version.author} · ` : ""}
                        {version.commit} · {formatDateTime(version.committed_at)}
                      </div>
                    </div>
                    <button
                      type="button"
                      onClick={() => setRestoreVersionTag(version.tag)}
                      disabled={!!restoringVersionTag}
                      className="shrink-0 rounded-[4px] border border-border-subtle px-2 py-1 text-[12px] font-medium text-secondary transition-colors hover:bg-surface-hover disabled:opacity-50"
                    >
                      {restoringVersionTag === version.tag
                        ? t("mySkills.gitVersionRestoring")
                        : t("mySkills.gitVersionRestore")}
                    </button>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>

        <aside className="space-y-4">
          <section className="app-panel p-4">
            <div className="mb-3 flex items-center gap-2">
              <ShieldCheck className="h-4 w-4 text-muted" />
              <h2 className="text-[14px] font-semibold text-secondary">{t("backup.scope.title")}</h2>
            </div>
            <div className="space-y-2 text-[13px]">
              {["skills", "metadata"].map((key) => (
                <div key={key} className="flex items-start gap-2 text-tertiary">
                  <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-500" />
                  <span>{t(`backup.scope.included.${key}`)}</span>
                </div>
              ))}
              {["secrets", "local"].map((key) => (
                <div key={key} className="flex items-start gap-2 text-muted">
                  <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-faint" />
                  <span>{t(`backup.scope.excluded.${key}`)}</span>
                </div>
              ))}
            </div>
            {sizeReport && (sizeReport.oversized.length > 0 || sizeReport.total_bytes > sizeReport.repo_warn_bytes) ? (
              <div className="mt-3 space-y-1 rounded-[6px] border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-[12px] leading-5 text-amber-700 dark:text-amber-300">
                {sizeReport.total_bytes > sizeReport.repo_warn_bytes && (
                  <div>{t("backup.scope.repoTooLarge", { size: formatBytes(sizeReport.total_bytes) })}</div>
                )}
                {sizeReport.oversized.map((skill) => (
                  <div key={skill.name}>
                    {skill.excluded
                      ? t("backup.scope.oversizedExcluded", { name: skill.name, size: formatBytes(skill.bytes) })
                      : t("backup.scope.oversizedSkill", { name: skill.name, size: formatBytes(skill.bytes) })}
                  </div>
                ))}
              </div>
            ) : (
              <div className="mt-3 rounded-[6px] border border-border-subtle bg-bg-secondary px-3 py-2 text-[12px] leading-5 text-muted">
                {t("backup.scope.sizeHint")}
              </div>
            )}
          </section>

          <section className="app-panel p-4">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <h2 className="text-[14px] font-semibold text-secondary">{t("backup.auto.title")}</h2>
                <p className="mt-1 text-[12px] leading-5 text-muted">{t("backup.auto.desc")}</p>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={autoBackupEnabled}
                onClick={handleToggleAutoBackup}
                disabled={autoBackupSaving}
                className={cn(
                  "relative mt-0.5 inline-flex h-4 w-7 shrink-0 items-center rounded-full outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent",
                  autoBackupEnabled ? "bg-emerald-500" : "bg-zinc-300 dark:bg-zinc-600",
                  autoBackupSaving ? "cursor-wait opacity-60" : "cursor-pointer"
                )}
              >
                <span
                  className={cn(
                    "inline-flex h-3 w-3 items-center justify-center rounded-full bg-white shadow transition-transform",
                    autoBackupEnabled ? "translate-x-3.5" : "translate-x-0.5"
                  )}
                />
              </button>
            </div>
          </section>

          <section className="app-panel p-4">
            <div className="mb-3 flex items-center gap-2">
              <Unlink className="h-4 w-4 text-muted" />
              <h2 className="text-[14px] font-semibold text-secondary">{t("backup.disconnect.title")}</h2>
            </div>
            <p className="text-[13px] leading-5 text-muted">{t("backup.disconnect.desc")}</p>
            <div className="mt-3 flex flex-wrap items-center gap-2">
              <button
                type="button"
                onClick={() => setDisconnectConfirmOpen(true)}
                disabled={loading === "disconnect" || (!remoteConfig && !gitStatus?.remote_url)}
                className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-border bg-surface-hover px-2.5 text-[13px] font-medium text-tertiary transition-colors hover:bg-surface-active disabled:opacity-50"
              >
                {loading === "disconnect" ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Unlink className="h-3.5 w-3.5" />}
                {t("settings.gitDisconnect")}
              </button>
              {isGithubRemote && (
                <button
                  type="button"
                  onClick={() => setRevokeConfirmOpen(true)}
                  disabled={loading === "disconnect"}
                  className="inline-flex h-8 items-center gap-1.5 rounded-[4px] border border-border bg-surface-hover px-2.5 text-[13px] font-medium text-tertiary transition-colors hover:bg-surface-active disabled:opacity-50"
                >
                  <ExternalLink className="h-3.5 w-3.5" />
                  {t("backup.disconnect.revoke")}
                </button>
              )}
            </div>
            {isGithubRemote && (
              <p className="mt-2 text-[12px] leading-4 text-faint">
                {authMethod === "pat"
                  ? t("backup.disconnect.revokeHintPat")
                  : authMethod === "oauth"
                    ? t("backup.disconnect.revokeHintOauth")
                    : t("backup.disconnect.revokeHintUnknown")}
              </p>
            )}
            {githubRepoWebUrl && (
              <div className="mt-3 rounded-[6px] border border-red-500/40 bg-red-500/10 px-3 py-2.5">
                <div className="text-[13px] font-medium text-red-700 dark:text-red-300">
                  {t("backup.disconnect.deleteRemote")}
                </div>
                <p className="mt-1 text-[12px] leading-4 text-red-700/80 dark:text-red-300/80">
                  {t("backup.disconnect.deleteRemoteDesc")}
                </p>
                <button
                  type="button"
                  onClick={() => setDeleteRemoteConfirmOpen(true)}
                  className="mt-2 inline-flex h-7 items-center gap-1.5 rounded-[4px] border border-red-500/50 px-2.5 text-[12px] font-medium text-red-700 transition-colors hover:bg-red-500/15 dark:text-red-300"
                >
                  <ExternalLink className="h-3 w-3" />
                  {t("backup.disconnect.deleteRemoteAction")}
                </button>
              </div>
            )}
          </section>

          <section className="app-panel p-4">
            <h2 className="text-[14px] font-semibold text-secondary">{t("backup.summary.title")}</h2>
            <div className="mt-3 grid grid-cols-2 gap-2 text-[12px]">
              <div className="rounded-[6px] border border-border-subtle bg-bg-secondary px-3 py-2">
                <div className="text-faint">{t("backup.summary.skills")}</div>
                <div className="mt-1 text-[18px] font-semibold text-primary">{managedSkills.length}</div>
              </div>
              <div className="rounded-[6px] border border-border-subtle bg-bg-secondary px-3 py-2">
                <div className="text-faint">{t("backup.summary.snapshots")}</div>
                <div className="mt-1 text-[18px] font-semibold text-primary">{versions.length}</div>
              </div>
            </div>
          </section>
        </aside>
      </div>

      <ConfirmDialog
        open={restoreVersionTag !== null}
        title={t("mySkills.gitVersionRestoreTitle")}
        message={t("mySkills.gitVersionRestoreConfirm", { tag: displaySnapshotLabel(restoreVersionTag || "") })}
        tone="warning"
        confirmLabel={t("mySkills.gitVersionRestore")}
        onClose={() => setRestoreVersionTag(null)}
        onConfirm={handleRestoreVersion}
      />
      <ConfirmDialog
        open={disconnectConfirmOpen}
        title={t("backup.disconnect.confirmTitle")}
        message={t("backup.disconnect.confirmMessage")}
        tone="warning"
        confirmLabel={t("settings.gitDisconnect")}
        onClose={() => setDisconnectConfirmOpen(false)}
        onConfirm={handleDisconnect}
      />
      <ConfirmDialog
        open={revokeConfirmOpen}
        title={t("backup.disconnect.revokeConfirmTitle")}
        message={authMethod === "pat"
          ? t("backup.disconnect.revokeConfirmPat")
          : authMethod === "oauth"
            ? t("backup.disconnect.revokeConfirmOauth")
            : t("backup.disconnect.revokeConfirmUnknown")}
        tone="warning"
        confirmLabel={t("backup.disconnect.revoke")}
        onClose={() => setRevokeConfirmOpen(false)}
        onConfirm={handleRevokeAuthorization}
      />
      <ConfirmDialog
        open={deleteRemoteConfirmOpen}
        title={t("backup.disconnect.deleteRemoteAction")}
        message={t("backup.disconnect.deleteRemoteConfirm")}
        confirmLabel={t("backup.disconnect.deleteRemoteAction")}
        onClose={() => setDeleteRemoteConfirmOpen(false)}
        onConfirm={handleOpenDeleteRemote}
      />
      <GitSetupDialog
        open={setupOpen}
        hasRemote={!!remoteConfig}
        onClose={() => setSetupOpen(false)}
        onClone={handleSetupClone}
        onInit={handleSetupInit}
      />
      <GitRecoveryDialog
        open={recoveryOpen}
        reason={recoveryReason}
        onClose={() => setRecoveryOpen(false)}
        onReclone={handleRecoveryReclone}
      />
    </div>
  );
}
