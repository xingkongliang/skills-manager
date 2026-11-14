// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WhatsNewDialog } from "./WhatsNewDialog";
import type { ChangelogEntry } from "../lib/changelog";

const opener = vi.hoisted(() => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@tauri-apps/plugin-opener", () => opener);
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn() }));
vi.mock("../context/AppContext", () => ({ useApp: vi.fn() }));
vi.mock("../lib/tauri", () => ({ getSettings: vi.fn(), setSettings: vi.fn() }));

const entries: ChangelogEntry[] = [
  {
    version: "1.44.0",
    date: "2026-11-12",
    overview: "- Projects from the CLI.",
    userFacing: "- **Manage project skills** — see [docs](https://example.com/docs).\n- Theme follows the system.",
  },
];

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("WhatsNewDialog", () => {
  it("lists each release and closes on Got it", async () => {
    const onClose = vi.fn();
    await act(async () => root.render(<WhatsNewDialog entries={entries} onClose={onClose} />));

    const text = container.textContent ?? "";
    expect(text).toContain("v1.44.0");
    expect(text).toContain("2026-11-12");
    expect(text).toContain("Projects from the CLI.");
    expect(text).toContain("Theme follows the system.");
    expect(container.querySelectorAll("li")).toHaveLength(2);

    // A link in an entry opens outside the app instead of navigating the webview.
    const link = container.querySelector<HTMLAnchorElement>('a[href="https://example.com/docs"]')!;
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    await act(async () => link.dispatchEvent(click));
    expect(click.defaultPrevented).toBe(true);
    expect(opener.openUrl).toHaveBeenCalledWith("https://example.com/docs");

    const gotIt = [...container.querySelectorAll("button")].find((b) => b.textContent === "whatsNew.gotIt")!;
    await act(async () => gotIt.click());
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
