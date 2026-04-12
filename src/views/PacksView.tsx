import { useState, useEffect, useCallback } from "react";
import {
  Plus,
  Pencil,
  Trash2,
  X,
  Search,
  ArrowLeft,
  Package,
  ChevronRight,
} from "lucide-react";
import { toast } from "sonner";
import { cn } from "../utils";
import { useApp } from "../context/AppContext";
import { ConfirmDialog } from "../components/ConfirmDialog";
import * as api from "../lib/tauri";
import type { PackRecord, SkillRecord, ManagedSkill } from "../lib/tauri";
import {
  PACK_ICON_OPTIONS,
  PACK_COLOR_OPTIONS,
  getPackIcon,
  getPackColor,
} from "../lib/packIcons";

// ─── Create / Edit Pack Dialog ───────────────────────────────────────────────

interface PackDialogProps {
  open: boolean;
  pack?: PackRecord | null;
  onClose: () => void;
  onSave: (name: string, description?: string, icon?: string, color?: string) => Promise<void>;
}

function PackDialog({ open, pack, onClose, onSave }: PackDialogProps) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [icon, setIcon] = useState(PACK_ICON_OPTIONS[0].key);
  const [color, setColor] = useState(PACK_COLOR_OPTIONS[0].key);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (open) {
      setName(pack?.name || "");
      setDescription(pack?.description || "");
      setIcon(pack?.icon || PACK_ICON_OPTIONS[0].key);
      setColor(pack?.color || PACK_COLOR_OPTIONS[0].key);
    }
  }, [open, pack]);

  if (!open) return null;

  const handleSave = async () => {
    if (!name.trim()) return;
    setLoading(true);
    try {
      await onSave(
        name.trim(),
        description.trim() || undefined,
        icon,
        color
      );
      onClose();
    } finally {
      setLoading(false);
    }
  };

  const inputClass =
    "w-full bg-background border border-border-subtle rounded-[4px] px-3 py-2 text-[13px] text-secondary focus:outline-none focus:border-border transition-all placeholder-faint";

  const isEdit = !!pack;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative bg-surface border border-border rounded-xl w-full max-w-[400px] p-5 shadow-2xl">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-[13px] font-semibold text-primary">
            {isEdit ? "Edit Pack" : "Create Pack"}
          </h2>
          <button
            onClick={onClose}
            className="text-muted hover:text-secondary p-1 rounded transition-colors outline-none"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="space-y-3">
          <div>
            <label className="block text-[13px] font-medium text-tertiary mb-1">Name</label>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. Frontend Essentials"
              className={inputClass}
              autoFocus
              onKeyDown={(e) => e.key === "Enter" && handleSave()}
            />
          </div>
          <div>
            <label className="block text-[13px] font-medium text-tertiary mb-1">Description</label>
            <input
              type="text"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="A brief description of this pack"
              className={inputClass}
            />
          </div>
          <div>
            <label className="block text-[13px] font-medium text-tertiary mb-1.5">Icon</label>
            <div className="grid grid-cols-5 gap-1.5">
              {PACK_ICON_OPTIONS.map((option) => {
                const Icon = option.icon;
                const selected = option.key === icon;
                const packColor = getPackColor(color);
                return (
                  <button
                    key={option.key}
                    type="button"
                    onClick={() => setIcon(option.key)}
                    className={cn(
                      "flex h-9 items-center justify-center rounded-lg border bg-background transition-all outline-none",
                      selected
                        ? `${packColor.borderClass} ${packColor.bgClass} ${packColor.textClass}`
                        : "border-border-subtle text-muted hover:border-border hover:text-secondary"
                    )}
                    title={option.label}
                  >
                    <Icon className="h-3.5 w-3.5" />
                  </button>
                );
              })}
            </div>
          </div>
          <div>
            <label className="block text-[13px] font-medium text-tertiary mb-1.5">Color</label>
            <div className="flex gap-2">
              {PACK_COLOR_OPTIONS.map((option) => {
                const selected = option.key === color;
                return (
                  <button
                    key={option.key}
                    type="button"
                    onClick={() => setColor(option.key)}
                    className={cn(
                      "h-7 w-7 rounded-full border-2 flex items-center justify-center transition-all outline-none",
                      selected
                        ? `${option.borderClass} ${option.bgClass}`
                        : "border-transparent hover:border-border-subtle"
                    )}
                    title={option.label}
                  >
                    <span className={cn("h-3.5 w-3.5 rounded-full", option.dotClass)} />
                  </button>
                );
              })}
            </div>
          </div>
          <div className="flex justify-end gap-2 pt-1">
            <button
              onClick={onClose}
              className="px-3 py-1.5 rounded-[4px] text-[13px] font-medium text-tertiary hover:text-secondary hover:bg-surface-hover transition-colors outline-none"
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              disabled={!name.trim() || loading}
              className="px-3 py-1.5 rounded-[4px] bg-accent-dark hover:bg-accent text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-accent-border outline-none"
            >
              {loading ? "Saving..." : isEdit ? "Save" : "Create"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── Add Skills to Pack Dialog ───────────────────────────────────────────────

interface AddSkillsDialogProps {
  open: boolean;
  pack: PackRecord | null;
  existingSkillIds: Set<string>;
  allSkills: ManagedSkill[];
  onClose: () => void;
  onAdd: (skillIds: string[]) => Promise<void>;
}

function AddSkillsDialog({ open, pack, existingSkillIds, allSkills, onClose, onAdd }: AddSkillsDialogProps) {
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (open) {
      setSearch("");
      setSelected(new Set());
    }
  }, [open]);

  if (!open || !pack) return null;

  const availableSkills = allSkills.filter(
    (s) => !existingSkillIds.has(s.id)
  );

  const filtered = search.trim()
    ? availableSkills.filter(
        (s) =>
          s.name.toLowerCase().includes(search.toLowerCase()) ||
          (s.description || "").toLowerCase().includes(search.toLowerCase())
      )
    : availableSkills;

  const toggleSkill = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleAdd = async () => {
    if (selected.size === 0) return;
    setLoading(true);
    try {
      await onAdd(Array.from(selected));
      onClose();
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <div className="relative bg-surface border border-border rounded-xl w-full max-w-[480px] p-5 shadow-2xl max-h-[70vh] flex flex-col">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-[13px] font-semibold text-primary">
            Add Skills to {pack.name}
          </h2>
          <button
            onClick={onClose}
            className="text-muted hover:text-secondary p-1 rounded transition-colors outline-none"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Search */}
        <div className="relative mb-3">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-faint" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search skills..."
            className="w-full bg-background border border-border-subtle rounded-[4px] pl-8 pr-3 py-2 text-[13px] text-secondary focus:outline-none focus:border-border transition-all placeholder-faint"
            autoFocus
          />
        </div>

        {/* Skill list */}
        <div className="flex-1 overflow-y-auto min-h-0 border border-border-subtle rounded-lg divide-y divide-border-subtle">
          {filtered.length === 0 ? (
            <div className="p-6 text-center text-[13px] text-muted">
              {availableSkills.length === 0
                ? "All skills are already in this pack"
                : "No skills match your search"}
            </div>
          ) : (
            filtered.map((skill) => {
              const isSelected = selected.has(skill.id);
              return (
                <button
                  key={skill.id}
                  onClick={() => toggleSkill(skill.id)}
                  className={cn(
                    "flex items-center gap-3 px-3 py-2.5 w-full text-left transition-colors outline-none",
                    isSelected
                      ? "bg-accent-bg"
                      : "hover:bg-surface-hover"
                  )}
                >
                  <div
                    className={cn(
                      "w-4 h-4 rounded border flex items-center justify-center shrink-0 transition-colors",
                      isSelected
                        ? "bg-accent-dark border-accent-border text-white"
                        : "border-border bg-surface"
                    )}
                  >
                    {isSelected && (
                      <svg className="w-2.5 h-2.5" viewBox="0 0 12 12" fill="none">
                        <path d="M2 6L5 9L10 3" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
                      </svg>
                    )}
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="text-[13px] font-medium text-secondary truncate">{skill.name}</div>
                    {skill.description && (
                      <div className="text-[12px] text-muted truncate mt-0.5">{skill.description}</div>
                    )}
                  </div>
                  <span className="text-[10px] px-1.5 py-px rounded bg-surface-hover text-muted border border-border font-medium shrink-0">
                    {skill.source_type}
                  </span>
                </button>
              );
            })
          )}
        </div>

        <div className="flex items-center justify-between pt-3 mt-auto">
          <span className="text-[12px] text-muted">
            {selected.size > 0
              ? `${selected.size} skill${selected.size > 1 ? "s" : ""} selected`
              : `${availableSkills.length} available`}
          </span>
          <div className="flex gap-2">
            <button
              onClick={onClose}
              className="px-3 py-1.5 rounded-[4px] text-[13px] font-medium text-tertiary hover:text-secondary hover:bg-surface-hover transition-colors outline-none"
            >
              Cancel
            </button>
            <button
              onClick={handleAdd}
              disabled={selected.size === 0 || loading}
              className="px-3 py-1.5 rounded-[4px] bg-accent-dark hover:bg-accent text-white text-[13px] font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-accent-border outline-none"
            >
              {loading ? "Adding..." : `Add ${selected.size > 0 ? selected.size : ""} Skill${selected.size !== 1 ? "s" : ""}`}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── Pack Detail View ────────────────────────────────────────────────────────

interface PackDetailProps {
  pack: PackRecord;
  onBack: () => void;
  onEdit: (pack: PackRecord) => void;
  onDelete: (pack: PackRecord) => void;
  onRefresh: () => void;
}

function PackDetail({ pack, onBack, onEdit, onDelete, onRefresh }: PackDetailProps) {
  const { managedSkills } = useApp();
  const [skills, setSkills] = useState<SkillRecord[]>([]);
  const [showAddSkills, setShowAddSkills] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<SkillRecord | null>(null);

  const packIcon = getPackIcon(pack.icon);
  const packColor = getPackColor(pack.color);
  const PackIcon = packIcon.icon;

  const loadSkills = useCallback(async () => {
    try {
      const s = await api.getSkillsForPack(pack.id);
      setSkills(s);
    } catch {
      toast.error("Failed to load pack skills");
    }
  }, [pack.id]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadSkills();
  }, [loadSkills]);

  const existingSkillIds = new Set(skills.map((s) => s.id));

  const handleAddSkills = async (skillIds: string[]) => {
    try {
      for (const skillId of skillIds) {
        await api.addSkillToPack(pack.id, skillId);
      }
      await loadSkills();
      onRefresh();
      toast.success(`Added ${skillIds.length} skill${skillIds.length > 1 ? "s" : ""}`);
    } catch {
      toast.error("Failed to add skills");
    }
  };

  const handleRemoveSkill = async () => {
    if (!removeTarget) return;
    try {
      await api.removeSkillFromPack(pack.id, removeTarget.id);
      await loadSkills();
      onRefresh();
      toast.success(`Removed "${removeTarget.name}"`);
    } catch {
      toast.error("Failed to remove skill");
    }
  };

  return (
    <>
      {/* Header */}
      <div className="app-page-header">
        <div className="flex items-center gap-3">
          <button
            onClick={onBack}
            className="text-muted hover:text-secondary p-1 rounded transition-colors outline-none"
          >
            <ArrowLeft className="w-4 h-4" />
          </button>
          <div
            className={cn(
              "w-8 h-8 rounded-lg flex items-center justify-center border",
              packColor.bgClass,
              packColor.borderClass,
              packColor.textClass
            )}
          >
            <PackIcon className="w-4 h-4" />
          </div>
          <div className="flex-1 min-w-0">
            <h1 className="app-page-title">{pack.name}</h1>
            {pack.description && (
              <p className="app-page-subtitle">{pack.description}</p>
            )}
          </div>
          <div className="flex items-center gap-1.5">
            <button
              onClick={() => onEdit(pack)}
              className="p-2 rounded-lg text-muted hover:text-secondary hover:bg-surface-hover transition-colors outline-none"
              title="Edit pack"
            >
              <Pencil className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => onDelete(pack)}
              className="p-2 rounded-lg text-muted hover:text-red-400 hover:bg-surface-hover transition-colors outline-none"
              title="Delete pack"
            >
              <Trash2 className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>

      {/* Toolbar */}
      <div className="app-toolbar">
        <span className="app-section-title">
          Skills ({skills.length})
        </span>
        <button
          onClick={() => setShowAddSkills(true)}
          className="app-button-secondary !py-1.5 !px-3 !text-[12px]"
        >
          <Plus className="w-3.5 h-3.5" />
          Add Skills
        </button>
      </div>

      {/* Skills list */}
      {skills.length === 0 ? (
        <div className="app-panel flex flex-col items-center justify-center py-12 text-center">
          <Package className="w-8 h-8 text-muted mb-3" />
          <p className="text-[13px] text-muted mb-1">No skills in this pack yet</p>
          <p className="text-[12px] text-faint mb-4">Add skills to bundle them together</p>
          <button
            onClick={() => setShowAddSkills(true)}
            className="app-button-primary !py-1.5 !px-3 !text-[12px]"
          >
            <Plus className="w-3.5 h-3.5" />
            Add Skills
          </button>
        </div>
      ) : (
        <div className="app-panel overflow-hidden divide-y divide-border-subtle">
          {skills.map((skill) => (
            <div
              key={skill.id}
              className="flex items-center justify-between px-3.5 py-2.5 hover:bg-surface-hover transition-colors"
            >
              <div className="flex items-center gap-2.5 min-w-0">
                <div className="w-6 h-6 rounded-[4px] flex items-center justify-center text-[13px] font-semibold bg-accent-bg text-accent-light shrink-0">
                  {skill.name.charAt(0).toUpperCase()}
                </div>
                <div className="min-w-0">
                  <h4 className="text-[13px] text-secondary font-medium flex items-center gap-1.5 truncate">
                    {skill.name}
                    <span className="text-[9px] px-1.5 py-px rounded bg-surface-hover text-muted border border-border font-normal">
                      {skill.source_type}
                    </span>
                  </h4>
                  {skill.description && (
                    <p className="text-[12px] text-muted mt-0.5 truncate">{skill.description}</p>
                  )}
                </div>
              </div>
              <button
                onClick={() => setRemoveTarget(skill)}
                className="p-1.5 rounded text-faint hover:text-red-400 transition-colors outline-none shrink-0"
                title="Remove from pack"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
          ))}
        </div>
      )}

      <AddSkillsDialog
        open={showAddSkills}
        pack={pack}
        existingSkillIds={existingSkillIds}
        allSkills={managedSkills}
        onClose={() => setShowAddSkills(false)}
        onAdd={handleAddSkills}
      />

      <ConfirmDialog
        open={removeTarget !== null}
        message={`Remove "${removeTarget?.name || ""}" from this pack?`}
        onClose={() => setRemoveTarget(null)}
        onConfirm={handleRemoveSkill}
      />
    </>
  );
}

// ─── Main Packs View ─────────────────────────────────────────────────────────

export function PacksView() {
  const [packs, setPacks] = useState<PackRecord[]>([]);
  const [selectedPack, setSelectedPack] = useState<PackRecord | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [editTarget, setEditTarget] = useState<PackRecord | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<PackRecord | null>(null);
  const [packSkillCounts, setPackSkillCounts] = useState<Record<string, number>>({});

  const loadPacks = useCallback(async () => {
    try {
      const p = await api.getAllPacks();
      setPacks(p);
      // Load skill counts for each pack
      const counts: Record<string, number> = {};
      await Promise.all(
        p.map(async (pack) => {
          try {
            const skills = await api.getSkillsForPack(pack.id);
            counts[pack.id] = skills.length;
          } catch {
            counts[pack.id] = 0;
          }
        })
      );
      setPackSkillCounts(counts);
    } catch {
      toast.error("Failed to load packs");
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadPacks();
  }, [loadPacks]);

  const handleCreatePack = async (
    name: string,
    description?: string,
    icon?: string,
    color?: string
  ) => {
    await api.createPack(name, description, icon, color);
    await loadPacks();
    toast.success("Pack created");
  };

  const handleUpdatePack = async (
    name: string,
    description?: string,
    icon?: string,
    color?: string
  ) => {
    if (!editTarget) return;
    await api.updatePack(editTarget.id, name, description, icon, color);
    await loadPacks();
    // If we're viewing this pack, refresh it
    if (selectedPack?.id === editTarget.id) {
      const updated = await api.getPackById(editTarget.id);
      if (updated) setSelectedPack(updated);
    }
    toast.success("Pack updated");
  };

  const handleDeletePack = async () => {
    if (!deleteTarget) return;
    await api.deletePack(deleteTarget.id);
    if (selectedPack?.id === deleteTarget.id) {
      setSelectedPack(null);
    }
    await loadPacks();
    toast.success("Pack deleted");
  };

  // Detail view
  if (selectedPack) {
    return (
      <div className="app-page app-page-narrow">
        <PackDetail
          pack={selectedPack}
          onBack={() => setSelectedPack(null)}
          onEdit={(p) => setEditTarget(p)}
          onDelete={(p) => setDeleteTarget(p)}
          onRefresh={loadPacks}
        />

        <PackDialog
          open={editTarget !== null}
          pack={editTarget}
          onClose={() => setEditTarget(null)}
          onSave={handleUpdatePack}
        />

        <ConfirmDialog
          open={deleteTarget !== null}
          message={`Delete pack "${deleteTarget?.name || ""}"? Skills in this pack will not be deleted.`}
          onClose={() => setDeleteTarget(null)}
          onConfirm={handleDeletePack}
        />
      </div>
    );
  }

  // List view
  return (
    <div className="app-page app-page-narrow">
      <div className="app-page-header">
        <h1 className="app-page-title">Packs</h1>
        <p className="app-page-subtitle">
          Group skills into reusable packs and assign them to scenarios
        </p>
      </div>

      {/* Toolbar */}
      <div className="app-toolbar">
        <span className="app-section-title">
          {packs.length} Pack{packs.length !== 1 ? "s" : ""}
        </span>
        <button
          onClick={() => setShowCreate(true)}
          className="app-button-primary !py-1.5 !px-3 !text-[12px]"
        >
          <Plus className="w-3.5 h-3.5" />
          New Pack
        </button>
      </div>

      {/* Pack cards */}
      {packs.length === 0 ? (
        <div className="app-panel flex flex-col items-center justify-center py-16 text-center">
          <Package className="w-10 h-10 text-muted mb-3" />
          <p className="text-[14px] font-medium text-secondary mb-1">No packs yet</p>
          <p className="text-[13px] text-muted mb-5 max-w-xs">
            Packs let you bundle skills together so you can quickly assign groups of skills to different scenarios.
          </p>
          <button
            onClick={() => setShowCreate(true)}
            className="app-button-primary"
          >
            <Plus className="w-4 h-4" />
            Create your first pack
          </button>
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {packs.map((pack) => {
            const packIcon = getPackIcon(pack.icon);
            const packColor = getPackColor(pack.color);
            const PackIcon = packIcon.icon;
            const skillCount = packSkillCounts[pack.id] ?? 0;

            return (
              <div
                key={pack.id}
                className="app-panel group relative flex items-start gap-3.5 p-4 cursor-pointer transition-colors hover:border-border"
                onClick={() => setSelectedPack(pack)}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") setSelectedPack(pack);
                }}
              >
                {/* Icon */}
                <div
                  className={cn(
                    "w-9 h-9 rounded-lg flex items-center justify-center border shrink-0",
                    packColor.bgClass,
                    packColor.borderClass,
                    packColor.textClass
                  )}
                >
                  <PackIcon className="w-4.5 h-4.5" />
                </div>

                {/* Content */}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center justify-between">
                    <h3 className="text-[14px] font-medium text-secondary truncate">{pack.name}</h3>
                    <ChevronRight className="w-3.5 h-3.5 text-faint group-hover:text-muted transition-colors shrink-0 ml-2" />
                  </div>
                  {pack.description && (
                    <p className="text-[12px] text-muted mt-0.5 truncate">{pack.description}</p>
                  )}
                  <div className="flex items-center gap-3 mt-2">
                    <span className="text-[11px] text-faint">
                      {skillCount} skill{skillCount !== 1 ? "s" : ""}
                    </span>
                  </div>
                </div>

                {/* Hover actions */}
                <div className="absolute right-2 top-2 flex items-center gap-0.5 invisible opacity-0 group-hover:visible group-hover:opacity-100 transition-opacity">
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      setEditTarget(pack);
                    }}
                    className="p-1.5 rounded text-faint hover:text-secondary hover:bg-surface-hover transition-colors outline-none"
                    title="Edit"
                  >
                    <Pencil className="w-3 h-3" />
                  </button>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      setDeleteTarget(pack);
                    }}
                    className="p-1.5 rounded text-faint hover:text-red-400 hover:bg-surface-hover transition-colors outline-none"
                    title="Delete"
                  >
                    <Trash2 className="w-3 h-3" />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <PackDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onSave={handleCreatePack}
      />

      <PackDialog
        open={editTarget !== null}
        pack={editTarget}
        onClose={() => setEditTarget(null)}
        onSave={handleUpdatePack}
      />

      <ConfirmDialog
        open={deleteTarget !== null}
        message={`Delete pack "${deleteTarget?.name || ""}"? Skills in this pack will not be deleted.`}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeletePack}
      />
    </div>
  );
}
