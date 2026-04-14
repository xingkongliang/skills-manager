import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import { Copy, Save, X, FileText, Plus, Pencil, Trash2, ChefHat, Sparkles, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import * as api from "../lib/tauri";
import { getErrorMessage } from "../lib/error";
import type { Recipe } from "../lib/tauri";

export interface ScenarioPromptEditorHandle {
  insertSkillAtCursor: (name: string) => void;
}

interface ScenarioPromptEditorProps {
  scenarioId: string;
  scenarioName: string;
  onExit: () => void;
  onTemplateChange?: (template: string) => void;
}

/** Marker format used to represent a skill tag in the raw template text. */
const SKILL_TAG_RE = /\[skill::([^\]]+)\]/g;

function makeSkillTag(name: string) {
  return `[skill::${name}]`;
}

/** Render template to plain text for clipboard export. */
function renderToPlainText(template: string): string {
  return template.replace(SKILL_TAG_RE, (_, name) => name);
}

/** Extract all skill names referenced in the template text. */
export function extractUsedSkillNames(text: string): Set<string> {
  const names = new Set<string>();
  const re = /\[skill::([^\]]+)\]/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    names.add(m[1]);
  }
  return names;
}

export const ScenarioPromptEditor = forwardRef<
  ScenarioPromptEditorHandle,
  ScenarioPromptEditorProps
>(function ScenarioPromptEditor({ scenarioId, scenarioName, onExit, onTemplateChange }, ref) {
  const { t } = useTranslation();
  const [template, setTemplate] = useState("");
  const [aiGenerating, setAiGenerating] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Recipe state
  const [recipes, setRecipes] = useState<Recipe[]>([]);
  const [selectedRecipeId, setSelectedRecipeId] = useState<string | null>(null);
  const [newRecipeName, setNewRecipeName] = useState("");
  const [showNewRecipeInput, setShowNewRecipeInput] = useState(false);
  const [editingRecipeId, setEditingRecipeId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");

  // Load recipes for this scenario
  const loadRecipes = useCallback(async () => {
    try {
      const list = await api.getRecipesForScenario(scenarioId);
      setRecipes(list);
      return list;
    } catch {
      return [];
    }
  }, [scenarioId]);

  // Load scenario prompt template (fallback when no recipe selected)
  const loadScenarioTemplate = useCallback(async () => {
    try {
      const saved = await api.getScenarioPromptTemplate(scenarioId);
      return saved ?? "";
    } catch {
      return "";
    }
  }, [scenarioId]);

  // Init: load recipes + scenario template
  useEffect(() => {
    setLoaded(false);
    setSelectedRecipeId(null);
    Promise.all([loadRecipes(), loadScenarioTemplate()]).then(([, scenarioTpl]) => {
      setTemplate(scenarioTpl);
      onTemplateChange?.(scenarioTpl);
      setLoaded(true);
    });
  }, [scenarioId]);

  // When selected recipe changes, load its template
  useEffect(() => {
    if (!selectedRecipeId) return;
    const recipe = recipes.find((r) => r.id === selectedRecipeId);
    if (recipe) {
      const tpl = recipe.prompt_template ?? "";
      setTemplate(tpl);
      onTemplateChange?.(tpl);
    }
  }, [selectedRecipeId]);

  /** Insert a skill tag at the current cursor position in the textarea. */
  const insertSkillAtCursor = useCallback(
    (skillName: string) => {
      const ta = textareaRef.current;
      const tag = makeSkillTag(skillName);
      if (ta) {
        const start = ta.selectionStart ?? template.length;
        const end = ta.selectionEnd ?? start;
        const next = template.slice(0, start) + tag + template.slice(end);
        setTemplate(next);
        onTemplateChange?.(next);
        requestAnimationFrame(() => {
          ta.focus();
          const pos = start + tag.length;
          ta.setSelectionRange(pos, pos);
        });
      } else {
        const next = template + tag;
        setTemplate(next);
        onTemplateChange?.(next);
      }
    },
    [template]
  );

  useImperativeHandle(ref, () => ({ insertSkillAtCursor }), [insertSkillAtCursor]);

  const handleSave = async () => {
    try {
      if (selectedRecipeId) {
        await api.saveRecipePromptTemplate(selectedRecipeId, template || null);
      } else {
        await api.saveScenarioPromptTemplate(scenarioId, template || null);
      }
      toast.success(t("mySkills.promptEditor.saved"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleCopy = async () => {
    const text = renderToPlainText(template);
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t("mySkills.promptEditor.copied"));
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleAiGenerate = async () => {
    const apiKeyCheck = await api.getSettings("codebuddy_api_key");
    if (!apiKeyCheck) {
      toast.error(t("mySkills.aiTaggingNoApiKey"));
      return;
    }
    setAiGenerating(true);
    try {
      const skills = await api.getSkillsForScenario(scenarioId);
      const skillList = skills.map((s: { name: string; description: string | null }) => ({
        name: s.name,
        description: s.description || "",
      }));
      const result = await api.invokeCodebuddyAgent("generate_scenario_prompt", {
        scenarioName,
        skills: skillList,
      });
      if (!result.prompt) {
        toast.error(t("mySkills.aiGeneratePromptError"));
        return;
      }
      setTemplate(result.prompt);
      onTemplateChange?.(result.prompt);
      if (result.recipes?.length) {
        const results = await Promise.allSettled(
          result.recipes.map((r) =>
            api.createRecipe(scenarioId, r.name, null, null, r.prompt_template)
          )
        );
        let created = 0;
        let skipped = 0;
        const newRecipes: typeof recipes = [];
        for (const r of results) {
          if (r.status === "fulfilled") {
            created++;
            newRecipes.push(r.value);
          } else {
            const errMsg = getErrorMessage(r.reason, "");
            if (!errMsg.includes("UNIQUE")) {
              console.error("[AI Generate Recipe]", errMsg);
            }
            skipped++;
          }
        }
        setRecipes((prev) => [...prev, ...newRecipes]);
        if (created > 0 || skipped > 0) {
          toast.success(t("mySkills.aiGeneratePromptWithRecipes", { created, skipped }));
        }
      } else {
        toast.success(t("mySkills.aiGeneratePromptSuccess"));
      }
    } catch (error: unknown) {
      console.error("[AI Generate Prompt]", getErrorMessage(error, ""));
      toast.error(getErrorMessage(error, t("mySkills.aiGeneratePromptError")));
    } finally {
      setAiGenerating(false);
    }
  };

  const handleRemoveTag = (skillName: string) => {
    const next = template.replace(makeSkillTag(skillName), "");
    setTemplate(next);
    onTemplateChange?.(next);
  };

  // ── Recipe CRUD ──

  const handleCreateRecipe = async () => {
    if (!newRecipeName.trim()) return;
    try {
      const recipe = await api.createRecipe(scenarioId, newRecipeName.trim());
      toast.success(t("mySkills.recipes.created"));
      setNewRecipeName("");
      setShowNewRecipeInput(false);
      await loadRecipes();
      setSelectedRecipeId(recipe.id);
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleDeleteRecipe = async (recipe: Recipe) => {
    try {
      await api.deleteRecipe(recipe.id);
      toast.success(t("mySkills.recipes.deleted"));
      if (selectedRecipeId === recipe.id) {
        setSelectedRecipeId(null);
        const scenarioTpl = await loadScenarioTemplate();
        setTemplate(scenarioTpl);
        onTemplateChange?.(scenarioTpl);
      }
      await loadRecipes();
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleRenameRecipe = async (recipeId: string) => {
    if (!editingName.trim()) {
      setEditingRecipeId(null);
      return;
    }
    try {
      const recipe = recipes.find((r) => r.id === recipeId);
      await api.updateRecipe(recipeId, editingName.trim(), recipe?.description, recipe?.icon);
      toast.success(t("mySkills.recipes.updated"));
      setEditingRecipeId(null);
      await loadRecipes();
    } catch {
      toast.error(t("common.error"));
    }
  };

  const handleSelectScenarioPrompt = async () => {
    setSelectedRecipeId(null);
    const scenarioTpl = await loadScenarioTemplate();
    setTemplate(scenarioTpl);
    onTemplateChange?.(scenarioTpl);
  };

  // Render the preview with inline skill badges
  const renderPreview = () => {
    const parts = template.split(SKILL_TAG_RE);
    return parts.map((part, i) => {
      if (i % 2 === 1) {
        return (
          <span
            key={i}
            className="mx-0.5 inline-flex items-center gap-1 rounded bg-accent/15 px-1.5 py-0.5 text-[12px] font-semibold text-accent"
          >
            {part}
            <button
              onClick={() => handleRemoveTag(part)}
              className="rounded-full p-0 text-accent/50 hover:text-accent"
            >
              <X className="h-2.5 w-2.5" />
            </button>
          </span>
        );
      }
      return part ? (
        <span key={i} className="whitespace-pre-wrap">
          {part}
        </span>
      ) : null;
    });
  };

  if (!loaded) return null;

  return (
    <div className="flex h-full flex-col gap-3">
      {/* Header */}
      <div className="flex items-center justify-between gap-2">
        <h3 className="flex min-w-0 items-center gap-1.5 text-[13px] font-semibold text-secondary">
          <FileText className="h-3.5 w-3.5 shrink-0 text-accent" />
          <span className="truncate">{t("mySkills.promptEditor.title")}</span>
        </h3>
        <div className="flex shrink-0 items-center gap-1">
          <button
            onClick={handleSave}
            className="inline-flex items-center gap-1 whitespace-nowrap rounded-md px-2 py-1 text-[12px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
          >
            <Save className="h-3 w-3 shrink-0" />
            {t("mySkills.promptEditor.save")}
          </button>
          <button
            onClick={handleCopy}
            disabled={!template.trim()}
            className="inline-flex items-center gap-1 whitespace-nowrap rounded-md px-2 py-1 text-[12px] font-medium text-accent transition-colors hover:bg-accent-bg disabled:opacity-40"
          >
            <Copy className="h-3 w-3 shrink-0" />
            {t("mySkills.promptEditor.copy")}
          </button>
          <button
            onClick={handleAiGenerate}
            disabled={aiGenerating}
            className="inline-flex items-center gap-1 whitespace-nowrap rounded-md px-2 py-1 text-[12px] font-medium text-accent transition-colors hover:bg-accent-bg disabled:opacity-40"
          >
            {aiGenerating ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : (
              <Sparkles className="h-3 w-3" />
            )}
            {aiGenerating
              ? t("mySkills.aiGeneratePromptLoading")
              : t("mySkills.aiGeneratePrompt")}
          </button>
          <button
            onClick={() => { handleSave(); onExit(); }}
            className="inline-flex items-center gap-1 whitespace-nowrap rounded-md px-2 py-1 text-[12px] font-medium text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
          >
            <X className="h-3 w-3 shrink-0" />
            {t("mySkills.promptEditor.exit")}
          </button>
        </div>
      </div>

      {/* Recipe list */}
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-1.5 text-[11px] font-medium uppercase tracking-wider text-muted">
            <ChefHat className="h-3 w-3" />
            {t("mySkills.recipes.title")}
          </div>
          <button
            onClick={() => setShowNewRecipeInput(true)}
            className="inline-flex items-center gap-0.5 rounded px-1.5 py-0.5 text-[11px] font-medium text-accent transition-colors hover:bg-accent-bg"
          >
            <Plus className="h-3 w-3" />
            {t("mySkills.recipes.newRecipe")}
          </button>
        </div>

        <div className="flex flex-wrap gap-1">
          {/* Scenario default prompt button */}
          <button
            onClick={handleSelectScenarioPrompt}
            className={`rounded-md border px-2 py-1 text-[12px] font-medium transition-colors ${
              selectedRecipeId === null
                ? "border-accent bg-accent/10 text-accent"
                : "border-border-subtle text-muted hover:border-accent/50 hover:text-secondary"
            }`}
          >
            {t("mySkills.recipes.scenarioPrompt")}
          </button>

          {/* Recipe buttons */}
          {recipes.map((recipe) => (
            <div key={recipe.id} className="relative group/recipe">
              {editingRecipeId === recipe.id ? (
                <input
                  autoFocus
                  value={editingName}
                  onChange={(e) => setEditingName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") handleRenameRecipe(recipe.id);
                    if (e.key === "Escape") setEditingRecipeId(null);
                  }}
                  onBlur={() => handleRenameRecipe(recipe.id)}
                  className="rounded-md border border-accent bg-transparent px-2 py-1 text-[12px] font-medium text-primary outline-none"
                />
              ) : (
                <>
                  <button
                    onClick={() => setSelectedRecipeId(recipe.id)}
                    className={`rounded-md border px-2 py-1 text-[12px] font-medium transition-colors ${
                      selectedRecipeId === recipe.id
                        ? "border-accent bg-accent/10 text-accent"
                        : "border-border-subtle text-muted hover:border-accent/50 hover:text-secondary"
                    }`}
                  >
                    {recipe.icon ? `${recipe.icon} ` : ""}{recipe.name}
                  </button>
                  <div className="absolute -top-2 -right-2 z-10 hidden items-center gap-px rounded-full bg-surface-hover px-1 py-0.5 shadow group-hover/recipe:flex">
                    <button
                      onClick={() => { setEditingRecipeId(recipe.id); setEditingName(recipe.name); }}
                      className="rounded p-0.5 text-faint transition-colors hover:text-secondary"
                    >
                      <Pencil className="h-2.5 w-2.5" />
                    </button>
                    <button
                      onClick={() => handleDeleteRecipe(recipe)}
                      className="rounded p-0.5 text-faint transition-colors hover:text-red-400"
                    >
                      <Trash2 className="h-2.5 w-2.5" />
                    </button>
                  </div>
                </>
              )}
            </div>
          ))}

          {/* New recipe inline input */}
          {showNewRecipeInput && (
            <input
              autoFocus
              value={newRecipeName}
              onChange={(e) => setNewRecipeName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleCreateRecipe();
                if (e.key === "Escape") { setShowNewRecipeInput(false); setNewRecipeName(""); }
              }}
              onBlur={() => { if (newRecipeName.trim()) { handleCreateRecipe(); } else { setShowNewRecipeInput(false); setNewRecipeName(""); } }}
              placeholder={t("mySkills.recipes.namePlaceholder")}
              className="rounded-md border border-accent bg-transparent px-2 py-1 text-[12px] font-medium text-primary outline-none placeholder:text-faint"
            />
          )}
        </div>
      </div>

      {/* Textarea */}
      <div className="flex flex-1 flex-col rounded-lg border border-border-subtle bg-bg-secondary">
        <textarea
          ref={textareaRef}
          value={template}
          onChange={(e) => { setTemplate(e.target.value); onTemplateChange?.(e.target.value); }}
          placeholder={t("mySkills.promptEditor.placeholder")}
          className="min-h-[180px] flex-1 resize-none bg-transparent p-3 text-[13px] leading-relaxed text-primary outline-none placeholder:text-faint"
          spellCheck={false}
        />
      </div>

      {/* Live preview */}
      {template.trim() && (
        <div className="flex flex-col gap-1.5">
          <div className="text-[11px] font-medium uppercase tracking-wider text-muted">
            {t("mySkills.promptEditor.preview")}
          </div>
          <div className="rounded-lg border border-border-subtle bg-bg-secondary p-3 text-[13px] leading-relaxed text-secondary">
            {renderPreview()}
          </div>
        </div>
      )}
    </div>
  );
});
