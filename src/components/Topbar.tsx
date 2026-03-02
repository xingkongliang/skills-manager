import { type CSSProperties, useEffect } from "react";
import { Search, HelpCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useApp } from "../context/AppContext";

export function Topbar() {
  const { t } = useTranslation();
  const { activeScenario, managedSkills, openGlobalSearch, openHelp } = useApp();
  const enabled = activeScenario?.skill_count ?? 0;
  const total = managedSkills.length;

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        openGlobalSearch();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [openGlobalSearch]);

  return (
    <div
      className="h-[52px] border-b border-border-subtle flex items-end pb-2.5 justify-between px-5 bg-background/90 backdrop-blur-md sticky top-0 z-20 shrink-0"
      style={{ WebkitAppRegion: "drag" } as CSSProperties}
    >
      <button
        type="button"
        onClick={openGlobalSearch}
        className="relative group text-left"
        style={{ WebkitAppRegion: "no-drag" } as CSSProperties}
      >
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted group-focus-within:text-tertiary transition-colors pointer-events-none" />
        <div className="w-[240px] h-[30px] pl-8 pr-9 rounded-[5px] border border-border-subtle bg-surface text-[12px] leading-[30px] text-secondary transition-colors group-hover:border-border">
          {t("search.placeholder")}
        </div>
        <span className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[10px] text-faint bg-surface-hover px-1 py-px rounded border border-border pointer-events-none">
          ⌘K
        </span>
      </button>

      <div className="flex items-center gap-3.5" style={{ WebkitAppRegion: "no-drag" } as CSSProperties}>
        <div className="flex items-center gap-2">
          <div className="relative flex h-[8px] w-[8px]">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-25" />
            <span className="relative inline-flex rounded-full h-[8px] w-[8px] bg-emerald-500" />
          </div>
          <span className="text-[12px] text-tertiary">
            <span className="text-muted text-[11px] mr-1.5">{t("topbar.supportedSkills")}</span>
            <span className="text-secondary font-medium">{enabled}</span>
            <span className="text-faint mx-0.5">/</span>
            <span>{total}</span>
          </span>
        </div>

        <button
          type="button"
          onClick={openHelp}
          className="text-muted hover:text-tertiary transition-colors p-1.5 rounded outline-none"
        >
          <HelpCircle className="w-4 h-4" />
        </button>
      </div>
    </div>
  );
}
