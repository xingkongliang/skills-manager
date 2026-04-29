import { useEffect, useRef, useState } from "react";
import { Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "../utils";
import type { ManagedSkill } from "../lib/tauri";

interface Props {
  skill: ManagedSkill;
  onConfirm: (skill: ManagedSkill) => void;
  buttonClassName?: string;
}

export function DeleteSkillButton({ skill, onConfirm, buttonClassName }: Props) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handlePointer = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", handlePointer);
    document.addEventListener("keydown", handleEscape);
    return () => {
      document.removeEventListener("mousedown", handlePointer);
      document.removeEventListener("keydown", handleEscape);
    };
  }, [open]);

  const handleConfirm = (e: React.MouseEvent) => {
    e.stopPropagation();
    setOpen(false);
    onConfirm(skill);
  };

  return (
    <div ref={containerRef} className="relative">
      <button
        onClick={(e) => {
          e.stopPropagation();
          setOpen((v) => !v);
        }}
        className={cn(
          "rounded text-faint transition-colors hover:text-red-400",
          open && "text-red-400",
          buttonClassName
        )}
        title={t("mySkills.delete")}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
      {open && (
        <div
          className="absolute right-0 top-full z-30 mt-1 w-72 rounded-lg border border-border bg-surface p-3 shadow-lg"
          onClick={(e) => e.stopPropagation()}
        >
          <p className="mb-3 text-[12px] leading-[16px] text-tertiary">
            {t("mySkills.deleteConfirm", { name: skill.name })}
          </p>
          <div className="flex justify-end gap-2">
            <button
              onClick={(e) => {
                e.stopPropagation();
                setOpen(false);
              }}
              className="rounded-[4px] px-2 py-1 text-[12px] font-medium text-tertiary transition-colors hover:bg-surface-hover hover:text-secondary outline-none"
            >
              {t("common.cancel")}
            </button>
            <button
              onClick={handleConfirm}
              className="rounded-[4px] border border-red-500/50 bg-red-600/90 px-2 py-1 text-[12px] font-medium text-white transition-colors hover:bg-red-500 outline-none"
            >
              {t("common.delete")}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
