import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import {
  Search,
  Layers,
  Download,
  Settings as SettingsIcon,
  FolderOpen,
  Folder,
  Home,
  ArrowRight,
} from "lucide-react";
import { useApp } from "../context/AppContext";
import { getScenarioIconOption } from "../lib/scenarioIcons";
import { cn } from "../utils";

type ItemKind = "skill" | "scenario" | "project" | "action";

interface PaletteItem {
  id: string;
  kind: ItemKind;
  label: string;
  sublabel?: string;
  icon: React.ReactNode;
  shortcut?: string;
  run: () => void;
}

export function CommandPalette() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const {
    managedSkills,
    scenarios,
    projects,
    activeScenario,
    switchScenario,
    openSkillDetailById,
  } = useApp();

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const close = useCallback(() => {
    setOpen(false);
    setQuery("");
    setActiveIndex(0);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const typing =
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.isContentEditable);

      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        if (typing && !open) return;
        e.preventDefault();
        setOpen((prev) => !prev);
      } else if (e.key === "Escape" && open) {
        e.preventDefault();
        close();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close]);

  useEffect(() => {
    if (open) {
      setActiveIndex(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const items = useMemo<PaletteItem[]>(() => {
    const q = query.trim().toLowerCase();

    const skillItems: PaletteItem[] = managedSkills
      .filter(
        (s) =>
          !q ||
          s.name.toLowerCase().includes(q) ||
          (s.description || "").toLowerCase().includes(q),
      )
      .slice(0, 8)
      .map((s) => ({
        id: `skill:${s.id}`,
        kind: "skill",
        label: s.name,
        sublabel: s.description || undefined,
        icon: <Layers className="h-3.5 w-3.5" />,
        run: () => {
          navigate("/my-skills");
          openSkillDetailById(s.id);
        },
      }));

    const scenarioItems: PaletteItem[] = scenarios
      .filter((s) => !q || s.name.toLowerCase().includes(q))
      .slice(0, 6)
      .map((s) => {
        const option = getScenarioIconOption(s);
        const Icon = option.icon;
        return {
          id: `scn:${s.id}`,
          kind: "scenario",
          label: s.name,
          sublabel: s.description || `${s.skill_count} skills`,
          icon: <Icon className="h-3.5 w-3.5" />,
          run: () => {
            if (activeScenario?.id !== s.id) {
              switchScenario(s.id);
            }
            if (!window.location.pathname.endsWith("/my-skills")) {
              navigate("/my-skills");
            }
          },
        };
      });

    const projectItems: PaletteItem[] = projects
      .filter(
        (p) =>
          !q ||
          p.name.toLowerCase().includes(q) ||
          p.path.toLowerCase().includes(q),
      )
      .slice(0, 5)
      .map((p) => ({
        id: `proj:${p.id}`,
        kind: "project",
        label: p.name,
        sublabel: p.path,
        icon: <Folder className="h-3.5 w-3.5" />,
        run: () => navigate(`/project/${p.id}`),
      }));

    const actionDefs: PaletteItem[] = [
      {
        id: "action:dashboard",
        kind: "action",
        label: t("sidebar.dashboard"),
        icon: <Home className="h-3.5 w-3.5" />,
        run: () => navigate("/"),
      },
      {
        id: "action:my-skills",
        kind: "action",
        label: t("sidebar.mySkills"),
        icon: <Layers className="h-3.5 w-3.5" />,
        run: () => navigate("/my-skills"),
      },
      {
        id: "action:install",
        kind: "action",
        label: t("sidebar.installSkills"),
        icon: <Download className="h-3.5 w-3.5" />,
        run: () => navigate("/install"),
      },
      {
        id: "action:install-local",
        kind: "action",
        label: t("commandPalette.scanImport"),
        icon: <FolderOpen className="h-3.5 w-3.5" />,
        run: () => navigate("/install?tab=local"),
      },
      {
        id: "action:settings",
        kind: "action",
        label: t("sidebar.settings"),
        icon: <SettingsIcon className="h-3.5 w-3.5" />,
        shortcut: "⌘,",
        run: () => navigate("/settings"),
      },
    ];
    const actions = actionDefs.filter((a) => !q || a.label.toLowerCase().includes(q));

    return [...skillItems, ...scenarioItems, ...projectItems, ...actions];
  }, [
    query,
    managedSkills,
    scenarios,
    projects,
    activeScenario?.id,
    switchScenario,
    openSkillDetailById,
    navigate,
    t,
  ]);

  useEffect(() => {
    if (activeIndex >= items.length) setActiveIndex(0);
  }, [items.length, activeIndex]);

  // Scroll active item into view
  useEffect(() => {
    if (!open) return;
    const el = listRef.current?.querySelector<HTMLDivElement>(
      `[data-palette-index="${activeIndex}"]`,
    );
    el?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, open]);

  if (!open) return null;

  const groups: { kind: ItemKind; label: string }[] = [
    { kind: "skill", label: t("commandPalette.skills") },
    { kind: "scenario", label: t("commandPalette.scenarios") },
    { kind: "project", label: t("commandPalette.projects") },
    { kind: "action", label: t("commandPalette.actions") },
  ];

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => Math.min(i + 1, Math.max(items.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = items[activeIndex];
      if (item) {
        item.run();
        close();
      }
    }
  };

  // Build render order by group, with flat index for keyboard nav
  let flatIndex = 0;
  const rendered = groups
    .map((g) => {
      const groupItems = items.filter((it) => it.kind === g.kind);
      if (groupItems.length === 0) return null;
      return (
        <div key={g.kind}>
          <div className="px-4 pt-3 pb-1 font-mono text-[10px] uppercase tracking-[0.12em] text-faint">
            {g.label} · {groupItems.length}
          </div>
          {groupItems.map((item) => {
            const idx = flatIndex++;
            const active = idx === activeIndex;
            return (
              <div
                key={item.id}
                data-palette-index={idx}
                role="option"
                aria-selected={active}
                onMouseEnter={() => setActiveIndex(idx)}
                onClick={() => {
                  item.run();
                  close();
                }}
                className={cn(
                  "flex cursor-pointer items-center gap-3 px-4 py-2 text-[13px]",
                  active ? "bg-surface-hover" : "hover:bg-surface-hover/60",
                )}
              >
                <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-border-subtle bg-surface text-muted">
                  {item.icon}
                </span>
                <div className="min-w-0 flex-1">
                  <div
                    className={cn(
                      "truncate",
                      item.kind === "skill" ? "font-mono" : "",
                      active ? "text-primary" : "text-secondary",
                    )}
                  >
                    {item.label}
                  </div>
                  {item.sublabel && (
                    <div className="truncate text-[12px] text-muted">
                      {item.sublabel}
                    </div>
                  )}
                </div>
                {item.shortcut && (
                  <span className="rounded border border-border-subtle bg-surface-hover px-1.5 py-0.5 font-mono text-[10px] text-faint">
                    {item.shortcut}
                  </span>
                )}
                {active && (
                  <ArrowRight className="h-3.5 w-3.5 shrink-0 text-muted" />
                )}
              </div>
            );
          })}
        </div>
      );
    })
    .filter(Boolean);

  return (
    <div
      className="fixed inset-0 z-[60] flex items-start justify-center bg-black/30 pt-[14vh] backdrop-blur-[2px]"
      onClick={close}
    >
      <div
        role="dialog"
        aria-label="Command palette"
        className="w-[min(640px,92vw)] overflow-hidden rounded-xl border border-border-subtle bg-surface shadow-2xl"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="flex items-center gap-3 border-b border-border-subtle px-4 py-3">
          <Search className="h-4 w-4 text-muted" />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("commandPalette.placeholder")}
            className="flex-1 bg-transparent text-[14px] text-primary outline-none placeholder:text-faint"
          />
          <span className="rounded border border-border-subtle bg-surface-hover px-1.5 py-0.5 font-mono text-[10px] text-faint">
            ESC
          </span>
        </div>

        <div
          ref={listRef}
          className="max-h-[60vh] overflow-y-auto pb-2"
          role="listbox"
        >
          {items.length === 0 ? (
            <div className="px-4 py-10 text-center text-[13px] text-muted">
              {t("commandPalette.empty")}
            </div>
          ) : (
            rendered
          )}
        </div>

        <div className="flex items-center gap-3 border-t border-border-subtle bg-bg-secondary px-4 py-2 text-[11px] text-muted">
          <span className="flex items-center gap-1">
            <kbd className="rounded border border-border-subtle bg-surface px-1 font-mono text-[10px]">
              ↑↓
            </kbd>
            {t("commandPalette.hints.navigate")}
          </span>
          <span className="flex items-center gap-1">
            <kbd className="rounded border border-border-subtle bg-surface px-1 font-mono text-[10px]">
              ↵
            </kbd>
            {t("commandPalette.hints.open")}
          </span>
          <span className="ml-auto flex items-center gap-1">
            <kbd className="rounded border border-border-subtle bg-surface px-1 font-mono text-[10px]">
              ⌘K
            </kbd>
            {t("commandPalette.hints.toggle")}
          </span>
        </div>
      </div>
    </div>
  );
}
