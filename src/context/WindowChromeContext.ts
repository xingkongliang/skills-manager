import { createContext, createElement, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import * as api from "../lib/tauri";

export const APP_TITLE_BAR_HEIGHT = 28;

const APP_WINDOW_CONTROLS_SETTING = "app_window_controls";
const CONTENT_TOP_PADDING = 20;
const SIDEBAR_TOP_PADDING = 10;
const IS_MACOS = navigator.userAgent.includes("Mac");

interface WindowChromeContextValue {
  appWindowControls: boolean;
  showTitleBarChrome: boolean;
  contentTopPadding: number;
  sidebarTopPadding: number;
  detailSheetTopOffset: number;
  setAppWindowControls: (enabled: boolean) => Promise<void>;
}

function getWindowChromeLayout(appWindowControls: boolean) {
  const showTitleBarChrome = appWindowControls || IS_MACOS;
  const titleBarHeight = showTitleBarChrome ? APP_TITLE_BAR_HEIGHT : 0;

  return {
    showTitleBarChrome,
    contentTopPadding: titleBarHeight + CONTENT_TOP_PADDING,
    sidebarTopPadding: titleBarHeight + SIDEBAR_TOP_PADDING,
    detailSheetTopOffset: titleBarHeight,
  };
}

function parseAppWindowControlsSetting(value: string | null): boolean {
  const normalized = (value ?? "false").trim().toLowerCase();
  return normalized === "true" || normalized === "1" || normalized === "yes" || normalized === "on";
}

async function applyAppWindowControlsPreference(enabled: boolean): Promise<void> {
  await getCurrentWindow().setDecorations(!enabled);
}

const WindowChromeContext = createContext<WindowChromeContextValue | null>(null);

export function WindowChromeProvider({ children }: { children: ReactNode }) {
  const [appWindowControls, setAppWindowControlsState] = useState(false);

  useEffect(() => {
    let disposed = false;

    api.getSettings(APP_WINDOW_CONTROLS_SETTING)
      .then(async (value) => {
        const enabled = parseAppWindowControlsSetting(value);
        if (disposed) return;
        setAppWindowControlsState(enabled);
        await applyAppWindowControlsPreference(enabled);
      })
      .catch(() => {});

    return () => {
      disposed = true;
    };
  }, []);

  const setAppWindowControls = useCallback(async (enabled: boolean) => {
    const previous = appWindowControls;
    if (enabled === previous) return;

    setAppWindowControlsState(enabled);
    try {
      await api.setSettings(APP_WINDOW_CONTROLS_SETTING, enabled ? "true" : "false");
      await applyAppWindowControlsPreference(enabled);
    } catch (error) {
      setAppWindowControlsState(previous);
      await api.setSettings(APP_WINDOW_CONTROLS_SETTING, previous ? "true" : "false").catch(() => {});
      await applyAppWindowControlsPreference(previous).catch(() => {});
      throw error;
    }
  }, [appWindowControls]);

  const value = useMemo(
    () => ({
      appWindowControls,
      setAppWindowControls,
      ...getWindowChromeLayout(appWindowControls),
    }),
    [appWindowControls, setAppWindowControls]
  );

  return createElement(WindowChromeContext.Provider, { value }, children);
}

export function useWindowChrome(): WindowChromeContextValue {
  const context = useContext(WindowChromeContext);
  if (!context) throw new Error("useWindowChrome must be used within WindowChromeProvider");
  return context;
}
