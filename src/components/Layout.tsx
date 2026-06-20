import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { Outlet, useNavigate } from "react-router-dom";
import { Sidebar } from "./Sidebar";
import { StatusBanner } from "./StatusBanner";
import { CommandPalette } from "./CommandPalette";
import { useApp } from "../context/AppContext";
import { useTranslation } from "react-i18next";
import { useDragWindow } from "../hooks/useDragWindow";
import {
  APP_TITLE_BAR_HEIGHT,
  WindowChromeProvider,
  useWindowChrome,
} from "../context/WindowChromeContext";

function WindowControls() {
  const appWindow = getCurrentWindow();

  return (
    <div
      className="absolute right-2 top-0 z-[60] flex items-center gap-1"
      style={{ height: APP_TITLE_BAR_HEIGHT }}
    >
      <button
        type="button"
        aria-label="Minimize window"
        title="Minimize"
        onClick={() => appWindow.minimize()}
        className="flex h-6 w-8 items-center justify-center rounded-[4px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
      >
        <Minus className="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        aria-label="Maximize window"
        title="Maximize"
        onClick={() => appWindow.toggleMaximize()}
        className="flex h-6 w-8 items-center justify-center rounded-[4px] text-muted transition-colors hover:bg-surface-hover hover:text-secondary"
      >
        <Square className="h-3 w-3" />
      </button>
      <button
        type="button"
        aria-label="Close window"
        title="Close"
        onClick={() => appWindow.close()}
        className="flex h-6 w-8 items-center justify-center rounded-[4px] text-muted transition-colors hover:bg-danger/15 hover:text-danger"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

function LayoutContent() {
  const { t } = useTranslation();
  const { appError, refreshAppData } = useApp();
  const { appWindowControls, showTitleBarChrome, contentTopPadding } = useWindowChrome();
  const onDrag = useDragWindow();
  const navigate = useNavigate();

  // Cmd+, to open Settings
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        const target = e.target as HTMLElement;
        if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) return;
        e.preventDefault();
        navigate("/settings");
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r") {
        const target = e.target as HTMLElement;
        if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) return;
        e.preventDefault();
        refreshAppData();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [navigate, refreshAppData]);

  return (
    <div className="relative flex h-full w-full overflow-hidden bg-background text-primary">
      {showTitleBarChrome ? (
        <>
          {/* Full-width app title bar spanning sidebar and content. */}
          <div
            onMouseDown={onDrag}
            className="absolute inset-x-0 top-0 z-50 border-b border-border-subtle bg-bg-secondary"
            style={{ height: APP_TITLE_BAR_HEIGHT }}
          />
          {appWindowControls ? <WindowControls /> : null}
        </>
      ) : null}
      <Sidebar />
      <div className="relative flex min-w-[600px] flex-1 flex-col overflow-hidden">
        <div
          className="flex-1 overflow-y-auto px-5 pb-5 scrollbar-hide"
          style={{ paddingTop: contentTopPadding }}
        >
          <div className="mx-auto flex min-h-full max-w-[1200px] flex-col gap-4">
            {appError ? (
              <StatusBanner
                compact
                title={t("common.dataOutOfDate")}
                description={appError}
                actionLabel={t("common.retry")}
                onAction={refreshAppData}
                tone="danger"
              />
            ) : null}
            <Outlet />
          </div>
        </div>
      </div>
      <CommandPalette />
    </div>
  );
}

export function Layout() {
  return (
    <WindowChromeProvider>
      <LayoutContent />
    </WindowChromeProvider>
  );
}
