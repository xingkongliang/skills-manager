import { useState, useEffect, useCallback, useMemo, useRef } from "react";
import { useParams, useNavigate } from "react-router-dom";
import {
  FolderOpen,
  Search,
  LayoutGrid,
  List,
  RefreshCw,
  FileText,
  Download,
  Upload,
  RotateCcw,
  Layers,
  X,
  Loader2,
  Trash2,
  SquareCheck,
  Square,
  Plus,
  CircleSlash,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useApp } from "../context/AppContext";
import { useMultiSelect } from "../hooks/useMultiSelect";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { MultiSelectToolbar } from "../components/MultiSelectToolbar";
import { BatchTagDialog } from "../components/BatchTagDialog";
import { DetailSheet } from "../components/DetailSheet";
import { AgentToggleSection, type AgentToggleItem } from "../components/AgentToggleSection";
import { ToggleSwitch } from "../components/ToggleSwitch";
import { ProjectAgentDots } from "../components/ProjectAgentDots";
import { PresetBar } from "../components/PresetBar";
import { SkillMarkdown } from "../components/SkillMarkdown";
import { DocumentDiffViewer } from "../components/DocumentDiffViewer";
import { getTagActiveColor, getTagColor, pruneStaleTagFilters, UNTAGGED_FILTER } from "../lib/skillTags";
import { cn } from "../utils";
import * as api from "../lib/tauri";
import type { ProjectSkill, ManagedSkill, ProjectAgentTarget } from "../lib/tauri";
import { getErrorMessage } from "../lib/error";
import { AddSkillsSheet } from "../components/AddSkillsSheet";

const PROJECT_DEFAULT_EXPORT_AGENTS_KEY = "project_default_export_agents";
const PROJECT_EXPORT_AGENT_PRIORITY = ["claude_code", "codex", "cursor", "gemini_cli", "github_copilot"];

const projectLastUsedAgentsKey = (projectId: string) =>
  `project_last_used_export_agents:${projectId}`;

interface ProjectSkillGroup {
  id: string;
  name: string;
  dir_name: string;
  relative_path: string;
  description: string | null;
  files: string[];
  variants: ProjectSkill[];
  enabledCount: number;
  totalCount: number;
  primaryVariant: ProjectSkill;
  status: ProjectSkill["sync_status"];
  tags: string[];
  centerSkillIds: string[];
}

// Keys of project agents that can actually receive skills right now: both
// installed on disk and enabled by the user. Used everywhere export targets
// are derived so disabled/uninstalled agents never get project-local skills.
function enabledInstalledAgentKeys(targets: ProjectAgentTarget[]): string[] {
  return targets.filter((target) => target.installed && target.enabled).map((target) => target.key);
}

function getDefaultExportAgents(targets: ProjectAgentTarget[], savedValue?: string | null) {
  const enabledKeys = enabledInstalledAgentKeys(targets);
  const availableKeys = new Set(enabledKeys);
  if (savedValue) {
    try {
      const parsed = JSON.parse(savedValue);
      if (Array.isArray(parsed)) {
        const filtered = parsed.filter((item): item is string => typeof item === "string" && availableKeys.has(item));
        if (filtered.length > 0) {
          return Array.from(new Set(filtered));
        }
      }
    } catch {
      // Ignore invalid persisted settings and fall back to built-in defaults.
    }
  }

  const prioritized = PROJECT_EXPORT_AGENT_PRIORITY.filter((key) => availableKeys.has(key));
  const fallback = enabledKeys;
  return Array.from(new Set((prioritized.length > 0 ? prioritized : fallback).slice(0, 3)));
}

function getSyncStatusMeta(t: (key: string) => string, status: ProjectSkill["sync_status"]) {
  switch (status) {
    case "in_sync":
      return {
        label: t("project.syncStatus.inSync"),
        className: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
      };
    case "project_newer":
      return {
        label: t("project.syncStatus.projectNewer"),
        className: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
      };
    case "center_newer":
      return {
        label: t("project.syncStatus.centerNewer"),
        className: "bg-sky-500/10 text-sky-700 dark:text-sky-300",
      };
    case "diverged":
      return {
        label: t("project.syncStatus.diverged"),
        className: "bg-violet-500/10 text-violet-700 dark:text-violet-300",
      };
    default:
      return {
        label: t("project.syncStatus.projectOnly"),
        className: "bg-surface-hover text-muted",
      };
  }
}

function getAssignedAgents(variants: ProjectSkill[]) {
  return Array.from(new Set(variants.map((variant) => variant.agent))).sort();
}

function getAgentDotTargets(variants: ProjectSkill[]) {
  const seen = new Set<string>();
  const targets: { key: string; display_name: string }[] = [];
  for (const v of variants) {
    if (!seen.has(v.agent)) {
      seen.add(v.agent);
      targets.push({ key: v.agent, display_name: v.agent_display_name });
    }
  }
  return targets;
}

function getGroupStatus(variants: ProjectSkill[]): ProjectSkill["sync_status"] {
  const priority: ProjectSkill["sync_status"][] = [
    "diverged",
    "project_newer",
    "center_newer",
    "project_only",
    "in_sync",
  ];
  for (const status of priority) {
    if (variants.some((variant) => variant.sync_status === status)) {
      return status;
    }
  }
  return "project_only";
}

export function ProjectDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { t } = useTranslation();
  const { projects, presets, managedSkills, refreshManagedSkills, refreshPresets, refreshProjects } = useApp();
  const [skills, setSkills] = useState<ProjectSkill[]>([]);
  const [projectAgentTargets, setProjectAgentTargets] = useState<ProjectAgentTarget[]>([]);
  const [selectedExportAgents, setSelectedExportAgents] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [viewMode, setViewMode] = useState<"grid" | "list">("grid");
  const [filterMode, setFilterMode] = useState<"all" | "enabled" | "disabled">("all");
  const [search, setSearch] = useState("");
  const [tagFilters, setTagFilters] = useState<Set<string>>(new Set());
  const [detailSkill, setDetailSkill] = useState<ProjectSkillGroup | null>(null);
  const [docContent, setDocContent] = useState<string | null>(null);
  const [docLoading, setDocLoading] = useState(false);
  const [centerDocContent, setCenterDocContent] = useState<string | null>(null);
  const [centerDocLoading, setCenterDocLoading] = useState(false);
  const [updatingCenterSkill, setUpdatingCenterSkill] = useState<string | null>(null);
  const [updatingProjectSkill, setUpdatingProjectSkill] = useState<string | null>(null);
  const [batchUpdatingCenter, setBatchUpdatingCenter] = useState(false);
  const [batchUpdatingProject, setBatchUpdatingProject] = useState(false);
  const [togglingSkill, setTogglingSkill] = useState<string | null>(null);
  const [togglingAgentTarget, setTogglingAgentTarget] = useState<{ skillKey: string; agent: string } | null>(null);
  const [showExportDialog, setShowExportDialog] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<ProjectSkillGroup | null>(null);
  const [batchDeleteConfirm, setBatchDeleteConfirm] = useState(false);
  const [batchTagDialogOpen, setBatchTagDialogOpen] = useState(false);
  const PROJECT_ADD_CALLOUT_KEY = "skills-manager.projectAddCalloutDismissed";
  const [showAddCallout, setShowAddCallout] = useState(() => {
    try {
      return localStorage.getItem(PROJECT_ADD_CALLOUT_KEY) !== "1";
    } catch {
      return false;
    }
  });
  const dismissAddCallout = () => {
    setShowAddCallout(false);
    try {
      localStorage.setItem(PROJECT_ADD_CALLOUT_KEY, "1");
    } catch {
      // ignore
    }
  };

  const project = projects.find((p) => p.id === id);
  const getSkillKey = useCallback((skill: Pick<ProjectSkillGroup, "id">) => {
    return skill.id;
  }, []);

  // Scanning a project is slow enough that switching projects can let the older
  // scan land last, swapping another project's skills in under this route — and
  // now also pruning this project's tag filter against the other one's tags.
  // Same request-id guard as WorkspaceView's local-skill load.
  const skillsRequestRef = useRef(0);
  const loadSkills = useCallback(async () => {
    if (!id) return;
    const requestId = ++skillsRequestRef.current;
    setLoading(true);
    try {
      const result = await api.getProjectSkills(id);
      if (skillsRequestRef.current === requestId) setSkills(result);
    } catch (e) {
      console.error("Failed to load project skills:", e);
    } finally {
      if (skillsRequestRef.current === requestId) setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    loadSkills();
  }, [loadSkills]);

  useEffect(() => {
    let cancelled = false;
    const loadProjectAgentTargets = async () => {
      if (!id) return;
      try {
        const result = await api.getProjectAgentTargets(id);
        if (!cancelled) {
          setProjectAgentTargets(result);
        }
      } catch (e) {
        console.error("Failed to load project agent targets:", e);
      }
    };
    loadProjectAgentTargets();
    return () => {
      cancelled = true;
    };
  }, [id]);

  useEffect(() => {
    if (!project && !loading) {
      navigate("/");
    }
  }, [project, loading, navigate]);

  const groupedSkills = useMemo<ProjectSkillGroup[]>(() => {
    const groups = new Map<string, ProjectSkillGroup>();
    for (const skill of skills) {
      const key = skill.relative_path.toLowerCase();
      const existing = groups.get(key);
      if (existing) {
        existing.variants.push(skill);
        existing.enabledCount += skill.enabled ? 1 : 0;
        existing.totalCount += 1;
        existing.files = Array.from(new Set([...existing.files, ...skill.files])).sort();
        existing.tags = Array.from(new Set([...existing.tags, ...skill.tags])).sort((a, b) => a.localeCompare(b));
        if (skill.center_skill_id && !existing.centerSkillIds.includes(skill.center_skill_id)) {
          existing.centerSkillIds.push(skill.center_skill_id);
          existing.centerSkillIds.sort((a, b) => a.localeCompare(b));
        }
        if (!existing.description && skill.description) {
          existing.description = skill.description;
        }
        continue;
      }
      groups.set(key, {
        id: key,
        name: skill.name,
        dir_name: skill.dir_name,
        relative_path: skill.relative_path,
        description: skill.description,
        files: [...skill.files],
        variants: [skill],
        enabledCount: skill.enabled ? 1 : 0,
        totalCount: 1,
        primaryVariant: skill,
        status: skill.sync_status,
        tags: [...skill.tags].sort((a, b) => a.localeCompare(b)),
        centerSkillIds: skill.center_skill_id ? [skill.center_skill_id] : [],
      });
    }
    return Array.from(groups.values())
      .map((group) => ({
        ...group,
        variants: [...group.variants].sort((a, b) => a.agent_display_name.localeCompare(b.agent_display_name)),
        primaryVariant: [...group.variants].sort((a, b) => a.agent_display_name.localeCompare(b.agent_display_name))[0],
        status: getGroupStatus(group.variants),
      }))
      .sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  }, [skills]);

  useEffect(() => {
    if (!detailSkill) return;
    const refreshed = groupedSkills.find((skill) => skill.id === detailSkill.id) ?? null;
    if (!refreshed) {
      setDetailSkill(null);
      setDocContent(null);
      return;
    }
    if (refreshed !== detailSkill) {
      setDetailSkill(refreshed);
    }
  }, [detailSkill, groupedSkills]);

  const filtered = useMemo(() => {
    return groupedSkills.filter((skill) => {
      const matchesSearch =
        skill.name.toLowerCase().includes(search.toLowerCase()) ||
        (skill.description || "").toLowerCase().includes(search.toLowerCase());
      if (!matchesSearch) return false;
      if (tagFilters.size > 0) {
        const wantUntagged = tagFilters.has(UNTAGGED_FILTER);
        const matchUntagged = wantUntagged && skill.tags.length === 0;
        const matchTag = skill.tags.some((tag) => tagFilters.has(tag));
        if (!matchUntagged && !matchTag) return false;
      }
      if (filterMode === "enabled") return skill.enabledCount > 0;
      if (filterMode === "disabled") return skill.enabledCount === 0;
      return true;
    });
  }, [groupedSkills, search, filterMode, tagFilters]);

  const {
    isMultiSelect, setIsMultiSelect,
    selectedIds,
    toggleSelect,
    isAllSelected,
    anyDisabled,
    handleSelectAll,
    exitMultiSelect,
  } = useMultiSelect({
    items: groupedSkills,
    filtered,
    getKey: getSkillKey,
    isItemActive: (s) => s.enabledCount === s.totalCount,
  });

  const exportTargets = useMemo(() => {
    if (projectAgentTargets.length > 0) return projectAgentTargets;
    return [{ key: "claude_code", display_name: "Claude Code", enabled: true, installed: true, is_custom: false }];
  }, [projectAgentTargets]);

  const projectSkillDirNamesByAgent = useMemo(() => {
    const map: Record<string, string[]> = {};
    for (const skill of skills) {
      if (!map[skill.agent]) {
        map[skill.agent] = [];
      }
      map[skill.agent].push(skill.relative_path.toLowerCase());
    }
    return map;
  }, [skills]);

  const projectCenterSkillIdsByAgent = useMemo(() => {
    const map: Record<string, string[]> = {};
    for (const skill of skills) {
      if (!skill.center_skill_id) continue;
      if (!map[skill.agent]) {
        map[skill.agent] = [];
      }
      map[skill.agent].push(skill.center_skill_id);
    }
    return map;
  }, [skills]);

  const projectPresetVariants = useMemo(() => {
    const map = new Map<string, ProjectSkill>();
    for (const skill of skills) {
      if (!skill.center_skill_id) continue;
      map.set(`${skill.center_skill_id}::${skill.agent}`, skill);
    }
    return map;
  }, [skills]);

  const findProjectPresetVariant = useCallback(
    (skill: ManagedSkill, agentKey: string) =>
      projectPresetVariants.get(`${skill.id}::${agentKey}`) ?? null,
    [projectPresetVariants]
  );

  useEffect(() => {
    let cancelled = false;
    const loadDefaultExportAgents = async () => {
      const savedValue = await api.getSettings(PROJECT_DEFAULT_EXPORT_AGENTS_KEY).catch(() => null);
      if (cancelled) return;
      setSelectedExportAgents(getDefaultExportAgents(exportTargets, savedValue));
    };
    loadDefaultExportAgents();
    return () => {
      cancelled = true;
    };
  }, [exportTargets]);

  const [lastUsedExportAgents, setLastUsedExportAgents] = useState<string[] | null>(null);
  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    api.getSettings(projectLastUsedAgentsKey(id))
      .then((raw) => {
        if (cancelled) return;
        if (!raw) {
          setLastUsedExportAgents(null);
          return;
        }
        try {
          const parsed = JSON.parse(raw);
          if (Array.isArray(parsed)) {
            setLastUsedExportAgents(parsed.filter((x): x is string => typeof x === "string"));
            return;
          }
        } catch {
          // fall through
        }
        setLastUsedExportAgents(null);
      })
      .catch(() => {
        if (!cancelled) setLastUsedExportAgents(null);
      });
    return () => {
      cancelled = true;
    };
  }, [id]);

  const handlePersistLastUsedAgents = useCallback(
    (agents: string[]) => {
      setLastUsedExportAgents(agents);
      if (id) {
        void api.setSettings(projectLastUsedAgentsKey(id), JSON.stringify(agents)).catch(() => {});
      }
    },
    [id],
  );

  const initialSheetAgents = useMemo(() => {
    const availableKeys = new Set(enabledInstalledAgentKeys(exportTargets));
    if (lastUsedExportAgents && lastUsedExportAgents.length > 0) {
      const filtered = lastUsedExportAgents.filter((k) => availableKeys.has(k));
      if (filtered.length > 0) return filtered;
    }
    return selectedExportAgents.filter((k) => availableKeys.has(k));
  }, [exportTargets, lastUsedExportAgents, selectedExportAgents]);

  const presetBarAgentKeys = useMemo(() => {
    const availableKeys = new Set(enabledInstalledAgentKeys(exportTargets));
    return selectedExportAgents.filter((key) => availableKeys.has(key));
  }, [exportTargets, selectedExportAgents]);

  const enabledCount = groupedSkills.filter((s) => s.enabledCount > 0).length;
  const allTags = useMemo(() => {
    const tags = new Set<string>();
    for (const skill of groupedSkills) {
      for (const tag of skill.tags) {
        if (tag.trim()) tags.add(tag);
      }
    }
    return Array.from(tags).sort((a, b) => a.localeCompare(b));
  }, [groupedSkills]);

  // Prune tag filters whose pill disappeared (e.g. its last skill was deleted),
  // otherwise a stale filter silently hides everything. An empty skill list
  // says nothing about which tags are valid, so wait for one before pruning.
  useEffect(() => {
    if (groupedSkills.length === 0) return;
    const hasUntagged = groupedSkills.some((skill) => skill.tags.length === 0);
    setTagFilters((prev) => pruneStaleTagFilters(prev, allTags, hasUntagged));
  }, [allTags, groupedSkills]);

  const selectedSkills = useMemo(
    () => groupedSkills.filter((skill) => selectedIds.has(getSkillKey(skill))),
    [getSkillKey, groupedSkills, selectedIds]
  );
  const selectedTaggableSkills = useMemo(
    () => selectedSkills.filter((skill) => skill.centerSkillIds.length > 0),
    [selectedSkills]
  );
  const anyCanUpdateCenter = useMemo(
    () => selectedSkills.some((skill) => (
      skill.status === "project_only" ||
      skill.status === "project_newer" ||
      skill.status === "diverged"
    )),
    [selectedSkills]
  );
  const anyCanUpdateProject = useMemo(
    () => selectedSkills.some((skill) => (
      skill.status === "project_newer" ||
      skill.status === "center_newer" ||
      skill.status === "diverged"
    )),
    [selectedSkills]
  );

  const handleOpenDetail = async (skill: ProjectSkillGroup) => {
    setDetailSkill(skill);
    setDocContent(null);
    setDocLoading(true);
    setCenterDocContent(null);
    setCenterDocLoading(false);
    if (!project || !id) return;

    const centerSkillId = skill.centerSkillIds.length > 0 ? skill.centerSkillIds[0] : null;

    if (centerSkillId) {
      setCenterDocLoading(true);
      api.getSkillDocument(centerSkillId)
        .then((doc) => setCenterDocContent(doc.content))
        .catch(() => setCenterDocContent(null))
        .finally(() => setCenterDocLoading(false));
    }

    try {
      const doc = await api.getProjectSkillDocument(
        id,
        skill.primaryVariant.relative_path,
        skill.primaryVariant.agent
      );
      setDocContent(doc.content);
    } catch {
      setDocContent(null);
    } finally {
      setDocLoading(false);
    }
  };

  // Push one variant to the center, then realign the rest from it.
  //
  // in_sync is the only status that proves a variant holds nothing of its own:
  // it is a content-hash match. center_newer does NOT prove it —
  // classify_sync_status reaches that status only after the hashes already
  // differed, then picks a side by mtime — and project_only was never pushed at
  // all. So any variant that is not in_sync may carry unique content.
  //
  // With more than one such variant there is no safe push. Writing the center
  // rebuilds its directory and moves its mtime to now, so every other unproven
  // variant re-reads as center_newer; the card then drops "update to center"
  // (which needs project_only/project_newer/diverged) and offers only "update
  // to project", which overwrites every variant — and the backend refuses only
  // project_newer, so nothing stops it. Refuse and name the conflict instead,
  // the way 1.34.0 answers a write that would destroy something.
  const pushSkillToCenterAndAlign = async (
    skill: ProjectSkillGroup
  ): Promise<{ alignFailed: number; conflicting: number }> => {
    if (!id) return { alignFailed: 0, conflicting: 0 };

    const unproven = skill.variants.filter((v) => v.sync_status !== "in_sync");
    if (unproven.length > 1) {
      return { alignFailed: 0, conflicting: unproven.length };
    }

    const winner = unproven[0] ?? skill.primaryVariant;
    await api.updateProjectSkillToCenter(id, winner.relative_path, winner.agent);

    // Every remaining variant is in_sync, so pulling the freshly written center
    // over it discards nothing — and it keeps a multi-agent group from flipping
    // to "center_newer" off the stale-but-clean siblings right after the user
    // updated *to* center. Serially: two agents' skills roots can be symlinks
    // onto one real directory, and each realign removes and rebuilds its
    // target, so concurrent calls on one path make a call fail for no reason.
    let alignFailed = 0;
    for (const variant of skill.variants.filter((v) => v !== winner)) {
      try {
        await api.updateProjectSkillFromCenter(id, variant.relative_path, variant.agent);
      } catch {
        alignFailed += 1;
      }
    }
    return { alignFailed, conflicting: 0 };
  };

  const handleUpdateCenter = async (skill: ProjectSkillGroup) => {
    if (!id) return;
    setUpdatingCenterSkill(getSkillKey(skill));
    try {
      const { alignFailed, conflicting } = await pushSkillToCenterAndAlign(skill);
      if (conflicting > 0) {
        toast.warning(
          t("project.updateCenterConflict", { name: skill.name, count: conflicting })
        );
      } else if (alignFailed > 0) {
        toast.warning(
          t("project.updateCenterAlignFailed", { name: skill.name, count: alignFailed })
        );
      } else {
        toast.success(t("project.updateCenterSuccess", { name: skill.name }));
      }
      await Promise.all([refreshManagedSkills(), refreshPresets(), loadSkills()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setUpdatingCenterSkill(null);
    }
  };

  const handleUpdateProject = async (skill: ProjectSkillGroup) => {
    if (!id) return;
    setUpdatingProjectSkill(getSkillKey(skill));
    try {
      await Promise.all(
        skill.variants.map((variant) =>
          api.updateProjectSkillFromCenter(id, variant.relative_path, variant.agent)
        )
      );
      if (skill.status === "project_newer") {
        toast.success(t("project.resetFromCenterSuccess", { name: skill.name }));
      } else {
        toast.success(t("project.updateProjectSuccess", { name: skill.name }));
      }
      await Promise.all([loadSkills(), refreshProjects()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setUpdatingProjectSkill(null);
    }
  };

  const handleToggleSkill = async (skill: ProjectSkillGroup) => {
    if (!id) return;
    setTogglingSkill(getSkillKey(skill));
    try {
      const nextEnabled = skill.enabledCount !== skill.totalCount;
      await Promise.all(
        skill.variants.map((variant) =>
          api.toggleProjectSkill(id, variant.relative_path, variant.agent, nextEnabled)
        )
      );
      if (skill.enabledCount === skill.totalCount) {
        toast.success(t("project.skillDisabled", { name: skill.name }));
      } else {
        toast.success(t("project.skillEnabled", { name: skill.name }));
      }
      await loadSkills();
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setTogglingSkill(null);
    }
  };

  const handleToggleDetailAgent = async (skill: ProjectSkillGroup, agentKey: string, enabled: boolean) => {
    if (!id) return;
    if (togglingAgentTarget) return;
    const target = exportTargets.find((item) => item.key === agentKey);
    const displayName = target?.display_name ?? agentKey;
    const existingVariant = skill.variants.find((variant) => variant.agent === agentKey);

    setTogglingAgentTarget({ skillKey: getSkillKey(skill), agent: agentKey });
    try {
      if (enabled) {
        const centerSkillId = skill.centerSkillIds[0];
        if (!centerSkillId) {
          toast.error(t("project.agentAddRequiresCenter", { agent: displayName }));
          return;
        }
        await api.exportSkillToProject(centerSkillId, id, [agentKey]);
        toast.success(t("project.agentAdded", { agent: displayName, name: skill.name }));
      } else {
        if (!existingVariant) return;
        await api.deleteProjectSkill(id, existingVariant.relative_path, agentKey);
        toast.success(t("project.agentRemoved", { agent: displayName, name: skill.name }));
      }
      await Promise.all([loadSkills(), refreshProjects()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    } finally {
      setTogglingAgentTarget(null);
    }
  };

  const handleDeleteSkill = async () => {
    if (!id || !deleteTarget) return;
    try {
      await Promise.all(
        deleteTarget.variants.map((variant) =>
          api.deleteProjectSkill(id, variant.relative_path, variant.agent)
        )
      );
      toast.success(t("project.skillDeleted", { name: deleteTarget.name }));
      await Promise.all([loadSkills(), refreshProjects()]);
    } catch (error: unknown) {
      toast.error(getErrorMessage(error, t("common.error")));
    }
  };

  const handleBatchDeleteProject = async () => {
    if (!id) return;
    let deleted = 0;
    let failed = 0;
    for (const skill of selectedSkills) {
      try {
        await Promise.all(
          skill.variants.map((variant) =>
            api.deleteProjectSkill(id, variant.relative_path, variant.agent)
          )
        );
        deleted++;
      } catch {
        failed++;
        // continue deleting remaining
      }
    }
    if (deleted > 0) {
      toast.success(t("project.batchDeleted", { count: deleted }));
    }
    if (failed > 0) {
      toast.error(t("project.batchDeleteFailed", { count: failed }));
    }
    exitMultiSelect();
    setBatchDeleteConfirm(false);
    await Promise.all([loadSkills(), refreshProjects()]);
  };

  const handleBatchToggleProject = async () => {
    if (!id) return;
    const enabling = anyDisabled;
    let count = 0;
    let failed = 0;
    for (const skill of selectedSkills) {
      try {
        if (enabling && skill.enabledCount !== skill.totalCount) {
          await Promise.all(
            skill.variants.map((variant) =>
              api.toggleProjectSkill(id, variant.relative_path, variant.agent, true)
            )
          );
          count++;
        } else if (!enabling && skill.enabledCount > 0) {
          await Promise.all(
            skill.variants.map((variant) =>
              api.toggleProjectSkill(id, variant.relative_path, variant.agent, false)
            )
          );
          count++;
        }
      } catch {
        failed++;
        // continue with remaining
      }
    }
    if (count > 0) {
      toast.success(enabling
        ? t("project.batchEnabled", { count })
        : t("project.batchDisabled", { count }));
    }
    if (failed > 0) {
      toast.error(t("project.batchToggleFailed", { count: failed }));
    }
    await loadSkills();
  };

  const handleBatchUpdateCenter = async () => {
    if (!id) return;
    setBatchUpdatingCenter(true);
    try {
      let updated = 0;
      let failed = 0;
      let conflicting = 0;
      for (const skill of selectedSkills) {
        const canUpdateCenter =
          skill.status === "project_only" ||
          skill.status === "project_newer" ||
          skill.status === "diverged";
        if (!canUpdateCenter) continue;
        try {
          const { alignFailed, conflicting: conflictingForSkill } =
            await pushSkillToCenterAndAlign(skill);
          // Refused outright: neither written nor failed, so it is counted on
          // its own rather than folded into either total.
          if (conflictingForSkill > 0) {
            conflicting += 1;
            continue;
          }
          // The push landed but some sibling failed to realign → the group is
          // not fully in sync, so count it as failed rather than reporting a
          // clean success.
          if (alignFailed > 0) failed++;
          else updated++;
        } catch {
          failed++;
        }
      }
      if (updated > 0) {
        toast.success(t("project.batchUpdatedCenter", { count: updated }));
      }
      if (conflicting > 0) {
        toast.warning(t("project.batchUpdateCenterConflict", { count: conflicting }));
      }
      if (failed > 0) {
        toast.error(t("project.batchUpdateCenterFailed", { count: failed }));
      }
      await Promise.all([refreshManagedSkills(), refreshPresets(), loadSkills()]);
    } finally {
      setBatchUpdatingCenter(false);
    }
  };

  const handleBatchUpdateProject = async () => {
    if (!id) return;
    setBatchUpdatingProject(true);
    try {
      let updated = 0;
      let failed = 0;
      for (const skill of selectedSkills) {
        const canUpdateProject =
          skill.status === "project_newer" ||
          skill.status === "center_newer" ||
          skill.status === "diverged";
        if (!canUpdateProject) continue;
        try {
          await Promise.all(
            skill.variants.map((variant) =>
              api.updateProjectSkillFromCenter(id, variant.relative_path, variant.agent)
            )
          );
          updated++;
        } catch {
          failed++;
        }
      }
      if (updated > 0) {
        toast.success(t("project.batchUpdatedProject", { count: updated }));
      }
      if (failed > 0) {
        toast.error(t("project.batchUpdateProjectFailed", { count: failed }));
      }
      await Promise.all([loadSkills(), refreshProjects()]);
    } finally {
      setBatchUpdatingProject(false);
    }
  };

  const handleBatchEditTags = async (adds: string[], removes: string[]) => {
    const skillMap = new Map(managedSkills.map((skill) => [skill.id, skill]));
    const centerIds = Array.from(new Set(selectedTaggableSkills.flatMap((skill) => skill.centerSkillIds)));
    let updated = 0;
    let failed = 0;

    for (const centerSkillId of centerIds) {
      const centerSkill = skillMap.get(centerSkillId);
      if (!centerSkill) continue;
      const removeSet = new Set(removes);
      const nextTags = centerSkill.tags.filter((tag) => !removeSet.has(tag));
      for (const tag of adds) {
        if (!nextTags.includes(tag)) nextTags.push(tag);
      }
      const changed =
        nextTags.length !== centerSkill.tags.length ||
        nextTags.some((tag, index) => tag !== centerSkill.tags[index]);
      if (!changed) continue;

      try {
        await api.setSkillTags(centerSkillId, nextTags);
        updated++;
      } catch {
        failed++;
      }
    }

    if (updated > 0) {
      toast.success(t("project.batchTagsUpdated", { count: updated }));
    }
    if (failed > 0) {
      toast.error(t("project.batchTagsFailed", { count: failed }));
    }
    await Promise.all([refreshManagedSkills(), loadSkills()]);
  };

  const presetSkillExistsInProject = useCallback(
    (skill: ManagedSkill, agentKey: string) => {
      return findProjectPresetVariant(skill, agentKey) !== null;
    },
    [findProjectPresetVariant]
  );

  const handleAddPresetSkillToProject = useCallback(
    async (skill: ManagedSkill, agentKey: string) => {
      if (!id) return;
      await api.exportSkillToProject(skill.id, id, [agentKey]);
    },
    [id]
  );

  const handleRemovePresetSkillFromProject = useCallback(
    async (skill: ManagedSkill, agentKey: string) => {
      if (!id) return;
      const projectVariant = findProjectPresetVariant(skill, agentKey);
      if (!projectVariant) throw new Error(t("project.skillDirectoryNotFound"));
      await api.deleteProjectSkill(id, projectVariant.relative_path, agentKey);
    },
    [findProjectPresetVariant, id, t]
  );

  const handlePresetActionComplete = useCallback(async () => {
    await Promise.all([loadSkills(), refreshProjects()]);
  }, [loadSkills, refreshProjects]);

  if (!project) return null;

  return (
    <div className="app-page">
      <div className="app-page-header flex flex-col gap-2.5 pb-3 pr-2">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="min-w-0 flex-[1_1_260px]">
            <h1 className="app-page-title flex items-center gap-2.5">
              <FolderOpen className="h-5 w-5 text-accent" />
              {project.name}
              <span className="app-badge">{groupedSkills.length}</span>
            </h1>
            <p className="mt-1 truncate text-[12px] leading-5 text-muted" title={project.path}>
              {project.path}
              {groupedSkills.length > 0 && ` \u00B7 ${enabledCount} / ${groupedSkills.length} ${t("project.enabled")}`}
            </p>
          </div>

          <div className="flex min-w-0 flex-[2_1_560px] flex-wrap items-center justify-end gap-2">
            <div className="relative w-full min-w-[220px] max-w-[300px]">
              <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted" />
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t("project.searchPlaceholder")}
                className="app-input h-9 w-full rounded-md pl-8 font-medium"
                autoCapitalize="none"
                autoCorrect="off"
                spellCheck={false}
              />
            </div>
            <div className="app-segmented shrink-0">
              {(["all", "enabled", "disabled"] as const).map((mode) => (
                <button
                  key={mode}
                  onClick={() => setFilterMode(mode)}
                  className={cn(
                    "app-segmented-button",
                    filterMode === mode && "app-segmented-button-active"
                  )}
                >
                  {t(`project.filters.${mode}`)}
                </button>
              ))}
            </div>

            <div className="app-segmented shrink-0">
              <button
                onClick={loadSkills}
                className="rounded-md p-2 text-muted transition-colors outline-none hover:bg-surface-hover hover:text-secondary"
                title={t("common.refresh")}
              >
                <RefreshCw className={cn("h-4 w-4", loading && "animate-spin")} />
              </button>
              <button
                onClick={() => setViewMode("grid")}
                className={cn(
                  "rounded-md p-2 transition-colors outline-none",
                  viewMode === "grid" ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                )}
              >
                <LayoutGrid className="h-4 w-4" />
              </button>
              <button
                onClick={() => setViewMode("list")}
                className={cn(
                  "rounded-md p-2 transition-colors outline-none",
                  viewMode === "list" ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                )}
              >
                <List className="h-4 w-4" />
              </button>
              <button
                onClick={() => isMultiSelect ? exitMultiSelect() : setIsMultiSelect(true)}
                className={cn(
                  "rounded-md p-2 transition-colors outline-none",
                  isMultiSelect ? "bg-surface-active text-secondary" : "text-muted hover:text-tertiary"
                )}
                title={isMultiSelect ? t("project.cancelSelect") : t("project.selectMode")}
              >
                <SquareCheck className="h-4 w-4" />
              </button>
            </div>

            <div className="relative shrink-0">
              <button
                onClick={() => {
                  setShowExportDialog(true);
                  dismissAddCallout();
                }}
                className="inline-flex h-9 items-center gap-1.5 rounded-md bg-accent px-3 text-[13px] font-medium text-white transition-colors hover:bg-accent-hover"
              >
                <Plus className="h-3.5 w-3.5" />
                {t("project.addSkill")}
              </button>
              {showAddCallout && groupedSkills.length > 0 && (
                <div className="absolute right-0 top-full z-20 mt-2 w-72 rounded-md border border-border bg-surface p-3 text-[12px] leading-snug shadow-lg">
                  <button
                    onClick={dismissAddCallout}
                    className="absolute right-1.5 top-1.5 rounded p-0.5 text-faint hover:text-secondary"
                    aria-label={t("common.close")}
                  >
                    <X className="h-3 w-3" />
                  </button>
                  <p className="pr-4 text-secondary">{t("project.addCallout")}</p>
                </div>
              )}
            </div>
          </div>
        </div>

        {allTags.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[12px] text-muted">{t("mySkills.tags.filter")}</span>
            <button
              onClick={() => setTagFilters(new Set())}
              className={cn(
                "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                tagFilters.size === 0
                  ? "bg-accent text-white dark:bg-accent dark:text-white"
                  : "bg-surface-hover text-muted hover:text-secondary"
              )}
            >
              {t("mySkills.tags.allTags")}
            </button>
            {groupedSkills.some((s) => s.tags.length === 0) && (() => {
              const isActive = tagFilters.has(UNTAGGED_FILTER);
              return (
                <button
                  onClick={() => {
                    setTagFilters((prev) => {
                      const next = new Set(prev);
                      if (next.has(UNTAGGED_FILTER)) next.delete(UNTAGGED_FILTER);
                      else next.add(UNTAGGED_FILTER);
                      return next;
                    });
                  }}
                  className={cn(
                    "inline-flex items-center gap-1 rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                    isActive
                      ? "bg-surface-active text-primary"
                      : "border border-dashed border-border text-muted hover:text-secondary"
                  )}
                  title={t("mySkills.tags.untagged")}
                >
                  <CircleSlash className="h-3 w-3" />
                  {t("mySkills.tags.untagged")}
                </button>
              );
            })()}
            {allTags.map((tag) => {
              const active = tagFilters.has(tag);
              return (
                <button
                  key={tag}
                  onClick={() => {
                    setTagFilters((prev) => {
                      const next = new Set(prev);
                      if (next.has(tag)) next.delete(tag);
                      else next.add(tag);
                      return next;
                    });
                  }}
                  className={cn(
                    "rounded-full px-2.5 py-0.5 text-[12px] font-medium transition-colors",
                    active ? getTagActiveColor(tag, allTags) : getTagColor(tag, allTags)
                  )}
                >
                  {tag}
                </button>
              );
            })}
          </div>
        )}

        {/* Preset bar */}
        {presets.length > 0 && presetBarAgentKeys.length > 0 && (
          <PresetBar
            presets={presets}
            managedSkills={managedSkills}
            agentKeys={presetBarAgentKeys}
            existsInWorkspace={presetSkillExistsInProject}
            onAddSkill={handleAddPresetSkillToProject}
            onRemoveSkill={handleRemovePresetSkillFromProject}
            onComplete={handlePresetActionComplete}
          />
        )}
      </div>

      {isMultiSelect && (
        <MultiSelectToolbar
          selectedCount={selectedIds.size}
          isAllSelected={isAllSelected}
          anyDisabled={anyDisabled}
          anyCanUpdateCenter={anyCanUpdateCenter}
          anyCanUpdateProject={anyCanUpdateProject}
          showToggle={project.supports_skill_toggle}
          updatingCenter={batchUpdatingCenter}
          updatingProject={batchUpdatingProject}
          labels={{
            hint: t("project.selectHint"),
            selected: t("project.selectedCount", { count: selectedIds.size }),
            updateCenter: t("project.batchUpdateCenter", { count: selectedIds.size }),
            updateProject: t("project.batchUpdateProject", { count: selectedIds.size }),
            delete: t("project.deleteSelected", { count: selectedIds.size }),
            enable: t("project.batchEnable", { count: selectedIds.size }),
            disable: t("project.batchDisable", { count: selectedIds.size }),
            selectAll: t("project.selectAll"),
            deselectAll: t("project.deselectAll"),
            cancel: t("common.cancel"),
            editTags: t("project.batchEditTags", { count: selectedTaggableSkills.length }),
          }}
          onUpdateCenter={handleBatchUpdateCenter}
          onUpdateProject={handleBatchUpdateProject}
          onDelete={() => setBatchDeleteConfirm(true)}
          onToggle={handleBatchToggleProject}
          onSelectAll={handleSelectAll}
          onCancel={exitMultiSelect}
          onEditTags={selectedTaggableSkills.length > 0 ? () => setBatchTagDialogOpen(true) : undefined}
        />
      )}

      {loading ? (
        <div className="flex flex-1 flex-col items-center justify-center pb-20 text-center">
          <div className="text-[13px] text-muted">{t("common.loading")}</div>
        </div>
      ) : filtered.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center pb-20 text-center">
          <Layers className="mb-4 h-12 w-12 text-faint" />
          <h3 className="mb-1.5 text-[14px] font-semibold text-tertiary">
            {groupedSkills.length === 0 ? t("project.noSkills") : t("mySkills.noMatch")}
          </h3>
          <p className="max-w-md text-[13px] text-muted">
            {groupedSkills.length === 0 ? t("project.noSkillsHint") : ""}
          </p>
          {groupedSkills.length === 0 && (
            <button
              onClick={() => {
                setShowExportDialog(true);
                dismissAddCallout();
              }}
              className="mt-4 inline-flex items-center gap-1.5 rounded-md bg-accent px-4 py-2 text-[13px] font-medium text-white transition-colors hover:bg-accent-hover"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("project.addSkillsCta")}
            </button>
          )}
        </div>
      ) : (
        <div
          className={cn(
            "pb-8",
            viewMode === "grid"
              ? "grid grid-cols-2 gap-3 lg:grid-cols-3"
              : "flex flex-col gap-0.5"
          )}
        >
          {filtered.map((skill) => {
            const skillKey = getSkillKey(skill);
            const isSelected = selectedIds.has(skillKey);
            const isUpdatingCenter = updatingCenterSkill === skillKey;
            const isUpdatingProject = updatingProjectSkill === skillKey;
            const isToggling = togglingSkill === skillKey;
            const canUpdateCenter =
              skill.status === "project_only" ||
              skill.status === "project_newer" ||
              skill.status === "diverged";
            const canUpdateProject =
              skill.status === "project_newer" ||
              skill.status === "center_newer" ||
              skill.status === "diverged";
            const statusMeta = getSyncStatusMeta(t, skill.status);
            const assignedAgents = getAssignedAgents(skill.variants);

            if (viewMode === "grid") {
              return (
                <div
                  key={skillKey}
                  className={cn(
                    "app-panel group relative flex h-full cursor-pointer flex-col overflow-hidden shadow-card transition-all hover:-translate-y-px hover:border-border hover:shadow-card-hover",
                    isMultiSelect && isSelected && "ring-1 ring-accent border-accent/40"
                  )}
                  onClick={() =>
                    isMultiSelect ? toggleSelect(skillKey) : handleOpenDetail(skill)
                  }
                >
                  <div className="flex items-center gap-2.5 px-3.5 pt-3 pb-1.5">
                    {/* Fixed slot: status dot, or the checkbox in multi-select */}
                    <div className="flex h-4 w-4 shrink-0 items-center justify-center">
                      {isMultiSelect ? (
                        isSelected
                          ? <SquareCheck className="h-3.5 w-3.5 text-accent" />
                          : <Square className="h-3.5 w-3.5 text-faint" />
                      ) : (
                        <span
                          className={cn(
                            "h-2 w-2 rounded-full",
                            skill.enabledCount === skill.totalCount
                              ? "bg-accent-light shadow-[0_0_0_3px_var(--color-accent-bg)]"
                              : skill.enabledCount > 0
                                ? "bg-amber-500 shadow-[0_0_0_3px_rgba(245,158,11,0.15)]"
                                : "bg-surface-active"
                          )}
                          title={`${skill.enabledCount}/${skill.totalCount}`}
                        />
                      )}
                    </div>
                    <h3
                      className="flex-1 truncate text-[14px] font-semibold text-primary"
                      title={skill.name}
                    >
                      {skill.name}
                    </h3>
                    {skill.files.length > 0 && (
                      <span className="flex items-center gap-1 text-[12px] text-faint shrink-0">
                        <FileText className="w-3 h-3" />
                        {skill.files.length}
                      </span>
                    )}
                  </div>

                  <div className="px-3.5 pb-3">
                    <p className="text-[13px] leading-[18px] text-muted truncate">
                      {skill.description || "\u2014"}
                    </p>
                    {skill.tags.length > 0 && (
                      <div className="mt-2 flex flex-wrap items-center gap-1">
                        {skill.tags.map((tag) => (
                          <span
                            key={tag}
                            className={cn(
                              "inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium",
                              getTagColor(tag, allTags)
                            )}
                          >
                            {tag}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>

                  <div className="mt-auto flex items-center justify-between gap-2 border-t border-border-faint px-3.5 py-2.5">
                    <div className="flex min-w-0 items-center gap-1.5">
                      <span className={cn("rounded-full px-2 py-0.5 text-[12px] font-medium", statusMeta.className)}>
                        {statusMeta.label}
                      </span>
                      {skill.enabledCount === 0 && (
                        <span className="rounded-full bg-red-500/10 px-2 py-0.5 text-[12px] font-medium text-red-600 dark:text-red-300">
                          {t("project.disabled")}
                        </span>
                      )}
                    </div>
                    {!isMultiSelect && (
                      <div className="flex items-center gap-1.5 shrink-0">
                        <ProjectAgentDots
                          assignedAgents={assignedAgents}
                          targets={exportTargets}
                          limit={4}
                          size="sm"
                          onToggle={(agentKey, enabled) => handleToggleDetailAgent(skill, agentKey, enabled)}
                          pendingKey={
                            togglingAgentTarget?.skillKey === skillKey
                              ? togglingAgentTarget.agent
                              : null
                          }
                        />
                        {canUpdateCenter && (
                          <button
                            onClick={(e) => { e.stopPropagation(); handleUpdateCenter(skill); }}
                            disabled={isUpdatingCenter || isUpdatingProject}
                            className="rounded px-2 py-1 text-[13px] font-medium text-muted transition-colors outline-none hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                            title={t("project.updateCenter")}
                          >
                            {isUpdatingCenter ? (
                              <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : (
                              <Upload className="h-3.5 w-3.5" />
                            )}
                          </button>
                        )}
                        {canUpdateProject && (
                          <button
                            onClick={(e) => { e.stopPropagation(); handleUpdateProject(skill); }}
                            disabled={isUpdatingCenter || isUpdatingProject}
                            className="rounded px-2 py-1 text-[13px] font-medium text-muted transition-colors outline-none hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                            title={
                              skill.status === "project_newer"
                                ? t("project.resetFromCenter")
                                : t("project.updateProject")
                            }
                          >
                            {isUpdatingProject ? (
                              <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            ) : skill.status === "project_newer" ? (
                              <RotateCcw className="h-3.5 w-3.5" />
                            ) : (
                              <Download className="h-3.5 w-3.5" />
                            )}
                          </button>
                        )}
                        {project.supports_skill_toggle ? (
                          <ToggleSwitch
                            checked={skill.enabledCount === skill.totalCount}
                            loading={isToggling}
                            onChange={() => handleToggleSkill(skill)}
                            title={
                              skill.enabledCount === skill.totalCount
                                ? t("project.enabled")
                                : t("project.enableSkill")
                            }
                          />
                        ) : null}
                        <button
                          onClick={(e) => { e.stopPropagation(); setDeleteTarget(skill); }}
                          className="rounded px-2 py-1 text-muted transition-colors outline-none hover:bg-red-500/10 hover:text-red-500"
                          title={t("project.deleteSkill")}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    )}
                  </div>
                </div>
              );
            }

            // List view
            return (
              <div
                key={skillKey}
                className={cn(
                  "app-panel group flex cursor-pointer items-center gap-3.5 rounded-xl border-transparent px-3.5 py-3 transition-all hover:border-border hover:bg-surface-hover",
                  isMultiSelect && isSelected && "ring-1 ring-accent border-accent/40"
                )}
                onClick={() =>
                  isMultiSelect ? toggleSelect(skillKey) : handleOpenDetail(skill)
                }
              >
                <div className="flex h-4 w-4 shrink-0 items-center justify-center">
                  {isMultiSelect ? (
                    isSelected
                      ? <SquareCheck className="h-3.5 w-3.5 text-accent" />
                      : <Square className="h-3.5 w-3.5 text-faint" />
                  ) : (
                    <span
                      className={cn(
                        "h-2 w-2 shrink-0 rounded-full",
                        skill.enabledCount === skill.totalCount
                          ? "bg-accent-light shadow-[0_0_0_3px_var(--color-accent-bg)]"
                          : skill.enabledCount > 0
                            ? "bg-amber-500 shadow-[0_0_0_3px_rgba(245,158,11,0.15)]"
                            : "bg-surface-active"
                      )}
                      title={`${skill.enabledCount}/${skill.totalCount}`}
                    />
                  )}
                </div>
                <h3
                  className="w-[180px] shrink-0 truncate text-[14px] font-semibold text-secondary"
                  title={skill.name}
                >
                  {skill.name}
                </h3>

                <p className="min-w-0 flex-1 truncate text-[13px] text-muted">
                  {skill.description || "\u2014"}
                </p>

                {skill.tags.length > 0 && (
                  <div className="flex shrink-0 items-center gap-1.5">
                    {skill.tags.map((tag) => (
                      <span
                        key={tag}
                        className={cn(
                          "inline-flex items-center rounded-full px-1.5 py-0.5 text-[11px] font-medium",
                          getTagColor(tag, allTags)
                        )}
                      >
                        {tag}
                      </span>
                    ))}
                  </div>
                )}

                <div className="flex shrink-0 items-center gap-2.5">
                  <span className={cn("rounded-full px-2 py-0.5 text-[12px] font-medium", statusMeta.className)}>
                    {statusMeta.label}
                  </span>
                  {skill.enabledCount === 0 && (
                    <span className="rounded-full bg-red-500/10 px-2 py-0.5 text-[12px] font-medium text-red-600 dark:text-red-300">
                      {t("project.disabled")}
                    </span>
                  )}
                  {skill.files.length > 0 && (
                    <span className="flex items-center gap-1 text-[12px] text-faint">
                      <FileText className="w-3 h-3" />
                      {skill.files.length}
                    </span>
                  )}
                  <ProjectAgentDots
                    assignedAgents={assignedAgents}
                    targets={exportTargets}
                    limit={4}
                    size="sm"
                    onToggle={
                      isMultiSelect
                        ? undefined
                        : (agentKey, enabled) => handleToggleDetailAgent(skill, agentKey, enabled)
                    }
                    pendingKey={
                      togglingAgentTarget?.skillKey === skillKey
                        ? togglingAgentTarget.agent
                        : null
                    }
                  />
                </div>

                {!isMultiSelect && (
                  <>
                    <div className="flex shrink-0 items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                      {canUpdateCenter && (
                        <button
                          onClick={(e) => { e.stopPropagation(); handleUpdateCenter(skill); }}
                          disabled={isUpdatingCenter || isUpdatingProject}
                          className="rounded p-0.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                          title={t("project.updateCenter")}
                        >
                          {isUpdatingCenter ? (
                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          ) : (
                            <Upload className="h-3.5 w-3.5" />
                          )}
                        </button>
                      )}
                      {canUpdateProject && (
                        <button
                          onClick={(e) => { e.stopPropagation(); handleUpdateProject(skill); }}
                          disabled={isUpdatingCenter || isUpdatingProject}
                          className="rounded p-0.5 text-muted transition-colors hover:bg-surface-hover hover:text-secondary disabled:opacity-50"
                          title={
                            skill.status === "project_newer"
                              ? t("project.resetFromCenter")
                              : t("project.updateProject")
                          }
                        >
                          {isUpdatingProject ? (
                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          ) : skill.status === "project_newer" ? (
                            <RotateCcw className="h-3.5 w-3.5" />
                          ) : (
                            <Download className="h-3.5 w-3.5" />
                          )}
                        </button>
                      )}
                    </div>
                    {project.supports_skill_toggle ? (
                      <ToggleSwitch
                        checked={skill.enabledCount === skill.totalCount}
                        loading={isToggling}
                        onChange={() => handleToggleSkill(skill)}
                        title={
                          skill.enabledCount === skill.totalCount
                            ? t("project.enabled")
                            : t("project.enableSkill")
                        }
                      />
                    ) : null}
                    <button
                      onClick={(e) => { e.stopPropagation(); setDeleteTarget(skill); }}
                      className="shrink-0 rounded p-0.5 text-muted transition-colors hover:bg-red-500/10 hover:text-red-500"
                      title={t("project.deleteSkill")}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </button>
                  </>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* Skill Document Detail Panel */}
      {detailSkill && project && (
        <ProjectSkillDetailPanel
          skill={detailSkill}
          targets={exportTargets}
          togglingAgent={
            togglingAgentTarget?.skillKey === getSkillKey(detailSkill)
              ? togglingAgentTarget.agent
              : null
          }
          onToggleAgent={(agentKey, enabled) => handleToggleDetailAgent(detailSkill, agentKey, enabled)}
          docContent={docContent}
          docLoading={docLoading}
          centerDocContent={centerDocContent}
          centerDocLoading={centerDocLoading}
          onClose={() => setDetailSkill(null)}
        />
      )}

      {/* Delete Confirm Dialog */}
      <ConfirmDialog
        open={!!deleteTarget}
        title={t("project.deleteSkill")}
        message={t("project.deleteSkillConfirm", { name: deleteTarget?.name })}
        tone="danger"
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeleteSkill}
      />

      {/* Batch Delete Confirm Dialog */}
      <ConfirmDialog
        open={batchDeleteConfirm}
        title={t("project.deleteSkill")}
        message={t("project.batchDeleteConfirm", { count: selectedIds.size })}
        tone="danger"
        onClose={() => setBatchDeleteConfirm(false)}
        onConfirm={handleBatchDeleteProject}
      />

      <BatchTagDialog
        open={batchTagDialogOpen}
        skills={selectedTaggableSkills}
        allTags={allTags}
        onClose={() => setBatchTagDialogOpen(false)}
        onApply={handleBatchEditTags}
      />

      {id && (
        <AddSkillsSheet
          open={showExportDialog}
          onClose={() => setShowExportDialog(false)}
          target={{
            kind: "project",
            projectId: id,
            projectName: project?.name ?? "",
            exportTargets,
            projectSkillDirNamesByAgent,
            projectCenterSkillIdsByAgent,
            initialSelectedAgents: initialSheetAgents,
            onPersistLastUsed: handlePersistLastUsedAgents,
          }}
          managedSkills={managedSkills}
          onInstalled={async () => {
            await Promise.all([loadSkills(), refreshProjects()]);
          }}
        />
      )}
    </div>
  );
}

function ProjectSkillDetailPanel({
  skill,
  targets,
  togglingAgent,
  onToggleAgent,
  docContent,
  docLoading,
  centerDocContent,
  centerDocLoading,
  onClose,
}: {
  skill: ProjectSkillGroup;
  targets: ProjectAgentTarget[];
  togglingAgent: string | null;
  onToggleAgent: (agentKey: string, enabled: boolean) => void;
  docContent: string | null;
  docLoading: boolean;
  centerDocContent: string | null;
  centerDocLoading: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [contentTab, setContentTab] = useState<"local" | "diff" | "center">("local");
  const supportsCenterDiff = skill.centerSkillIds.length > 0;
  const toggleItems: AgentToggleItem[] = targets.map((target) => {
    const variant = skill.variants.find((item) => item.agent === target.key);
    return {
      key: target.key,
      displayName: target.display_name,
      enabled: Boolean(variant),
      isAvailable: target.installed && target.enabled,
      disabled: (!variant && (!target.installed || !target.enabled)),
      badgeLabel: !target.installed
        ? t("mySkills.agentToggleNotInstalled")
        : !target.enabled
          ? t("mySkills.agentToggleDisabledGlobally")
          : variant && !variant.enabled
            ? t("project.disabled")
            : null,
    };
  });
  const meta = (
    <>
      <div className="flex flex-wrap items-center gap-2 text-[12.5px] text-muted">
        <ProjectAgentDots
          assignedAgents={getAssignedAgents(skill.variants)}
          targets={getAgentDotTargets(skill.variants).map((t) => ({
            key: t.key,
            display_name: t.display_name,
            enabled: true,
            installed: true,
            is_custom: false,
          }))}
        />
        {skill.tags.length > 0 && (
          <>
            <span className="mx-0.5 h-3 w-px bg-border-subtle" />
            {skill.tags.map((tag) => (
              <span
                key={tag}
                className="inline-flex items-center rounded-full bg-surface-hover px-2 py-0.5 text-[11px] font-medium text-secondary"
              >
                {tag}
              </span>
            ))}
          </>
        )}
      </div>
      <div className="mt-3 flex items-center gap-4 text-[12.5px] text-muted">
        <div className="flex min-w-0 items-center gap-1.5">
          <FolderOpen className="h-3.5 w-3.5 shrink-0" />
          <span className="font-mono truncate">{skill.primaryVariant.path}</span>
        </div>
        {skill.files.length > 0 && (
          <div className="flex shrink-0 items-center gap-1.5">
            <FileText className="h-3.5 w-3.5" />
            {skill.files.join(", ")}
          </div>
        )}
      </div>
    </>
  );

  return (
    <DetailSheet
      open={true}
      title={skill.name}
      description={skill.description ? <p className="line-clamp-3">{skill.description}</p> : undefined}
      meta={meta}
      onClose={onClose}
    >
      <AgentToggleSection
        items={toggleItems}
        togglingKey={togglingAgent}
        onToggle={onToggleAgent}
        className="mb-4"
      />

      {supportsCenterDiff && (
        <div className="mb-4 flex flex-wrap items-center gap-2">
          {(["local", "diff", "center"] as const).map((tab) => (
            <button
              key={tab}
              type="button"
              onClick={() => setContentTab(tab)}
              className={cn(
                "rounded-full px-3 py-1.5 text-[12px] font-medium transition-colors",
                contentTab === tab
                  ? "bg-accent text-white"
                  : "bg-surface-hover text-muted hover:text-secondary"
              )}
              disabled={(tab === "diff" || tab === "center") && centerDocLoading}
            >
              {tab === "local"
                ? t("mySkills.docTabs.local")
                : tab === "diff"
                  ? t("mySkills.docTabs.diff")
                  : t("project.docTabs.center")}
            </button>
          ))}
        </div>
      )}

      {docLoading ? (
        <div className="mt-12 text-center text-[13px] text-muted">{t("common.loading")}</div>
      ) : contentTab === "diff" ? (
        docContent && centerDocContent ? (
          <DocumentDiffViewer original={docContent} updated={centerDocContent} />
        ) : centerDocLoading ? (
          <div className="mt-12 text-center text-[13px] text-muted">{t("common.loading")}</div>
        ) : (
          <div className="mt-12 text-center text-[13px] text-muted">{t("mySkills.sourceDiffUnavailable")}</div>
        )
      ) : contentTab === "center" ? (
        centerDocLoading ? (
          <div className="mt-12 text-center text-[13px] text-muted">{t("common.loading")}</div>
        ) : centerDocContent ? (
          <SkillMarkdown content={centerDocContent} />
        ) : (
          <div className="mt-12 text-center text-[13px] text-muted">{t("mySkills.sourceDiffUnavailable")}</div>
        )
      ) : docContent ? (
        <SkillMarkdown content={docContent} />
      ) : (
        <div className="mt-12 text-center text-[13px] text-muted">{t("common.documentMissing")}</div>
      )}
    </DetailSheet>
  );
}
