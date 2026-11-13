import { lazy, Suspense, useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ArrowLeft, BookOpen } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { SkillBrowserTarget } from "../lib/tauri";
import { IS_MACOS } from "../lib/platform";
import { cn } from "../utils";
import type { ReaderMode } from "./SkillFileBrowser";

const SkillFileBrowser = lazy(() => import("./SkillFileBrowser").then((module) => ({ default: module.SkillFileBrowser })));

interface Props {
  target: SkillBrowserTarget;
  title: string;
  description?: string;
}

export function SkillReaderEntry(props: Props) {
  // Changing skills must also dispose of the previous reader/session.
  return <ReaderEntry key={JSON.stringify(props.target)} {...props} />;
}

function ReaderEntry(props: Props) {
  const { t } = useTranslation();
  const [trigger, setTrigger] = useState<HTMLButtonElement | null>(null);
  const close = useCallback(() => setTrigger(null), []);

  return <>
    <button type="button" onClick={(e) => setTrigger(e.currentTarget)}
      className="inline-flex items-center gap-1.5 rounded-lg border border-border-subtle px-3 py-1.5 text-[12px] text-secondary transition-colors hover:bg-surface-hover">
      <BookOpen className="h-3.5 w-3.5" />{t("reader.read")}
    </button>
    {trigger && <ReaderView {...props} initialMode="original" trigger={trigger} onClose={close} />}
  </>;
}

function ReaderView({ target, title, description, initialMode, trigger, onClose }: Props & {
  initialMode: ReaderMode;
  trigger: HTMLButtonElement;
  onClose(): void;
}) {
  const { t } = useTranslation();
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);
  const back = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const view = panel.current;
    if (!view) return;
    // Keep the detail mounted (including its scroll position), but prevent
    // pointer/keyboard interaction and focus from reaching it while reading.
    const background = Array.from(document.body.children)
      .filter((node): node is HTMLElement => node instanceof HTMLElement && !node.contains(view))
      .map((node) => ({ node, inert: node.inert }));
    background.forEach(({ node }) => { node.inert = true; });
    back.current?.focus({ preventScroll: true });

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return;
      const rect = view.getBoundingClientRect();
      const top = document.elementFromPoint?.(rect.x + rect.width / 2, rect.y + rect.height / 2);
      if (top && !view.contains(top)) return;
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onClose();
      } else if (event.key === "Tab") {
        const controls = Array.from(view.querySelectorAll<HTMLElement>(
          'button:not(:disabled), a[href], select:not(:disabled), [tabindex="0"]',
        ));
        const first = controls[0];
        const last = controls.at(-1);
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault(); last?.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault(); first?.focus();
        }
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      background.forEach(({ node, inert }) => { node.inert = inert; });
      if (trigger.isConnected) trigger.focus({ preventScroll: true });
    };
  }, [onClose, trigger]);

  return createPortal(
    <div ref={panel} role="dialog" aria-modal="true" aria-labelledby={titleId}
      className={cn("fixed inset-x-0 bottom-0 z-[45] flex min-h-0 min-w-0 flex-col overflow-hidden overscroll-none bg-bg-secondary", IS_MACOS ? "top-[28px]" : "top-0")}>
      <header className="flex shrink-0 items-center gap-4 border-b border-border-subtle px-5 py-3">
        <button ref={back} type="button" onClick={onClose}
          className="inline-flex shrink-0 items-center gap-2 rounded-lg px-3 py-2 text-[13px] text-secondary transition-colors hover:bg-surface-hover">
          <ArrowLeft className="h-4 w-4" />{t("reader.backToDetails")}
        </button>
        <h2 id={titleId} title={title} className="min-w-0 truncate text-[17px] font-semibold text-primary">{title}</h2>
      </header>
      <Suspense fallback={<div className="p-6 text-[13px] text-muted">{t("common.loading")}</div>}>
        <SkillFileBrowser target={target} description={description} initialMode={initialMode} />
      </Suspense>
    </div>,
    document.body,
  );
}
