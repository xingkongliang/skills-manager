// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

const api = vi.hoisted(() => ({
  logStartupEvent: vi.fn().mockResolvedValue(undefined),
  exportLogsZip: vi.fn().mockResolvedValue({ zip_path: "/tmp/logs.zip", file_count: 1 }),
}));
vi.mock("../lib/tauri", () => api);
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn() }));

function Boom(): never {
  throw new Error("render exploded");
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  // React reports caught errors via console.error; keep the test output clean.
  vi.spyOn(console, "error").mockImplementation(() => {});
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("ErrorBoundary", () => {
  it("renders children when nothing throws", () => {
    act(() => root.render(<ErrorBoundary><p>healthy</p></ErrorBoundary>));
    expect(container.textContent).toBe("healthy");
  });

  it("shows the fallback page instead of an empty root when a child throws", () => {
    act(() => root.render(<ErrorBoundary><Boom /></ErrorBoundary>));

    expect(container.querySelector('[role="alert"]')).not.toBeNull();
    // i18n is not initialized here, so the hard-coded English fallback shows.
    expect(container.textContent).toContain("Something went wrong");
    expect(container.querySelector("pre")?.textContent).toContain("render exploded");
    const buttons = [...container.querySelectorAll("button")].map((b) => b.textContent);
    expect(buttons).toEqual(["Copy details", "Export logs", "Reload"]);
    expect(api.logStartupEvent).toHaveBeenCalledWith(
      "render_error Error: render exploded",
      expect.any(Number),
    );
  });

  it("exports logs from the fallback page and shows where they went", async () => {
    act(() => root.render(<ErrorBoundary><Boom /></ErrorBoundary>));
    const exportButton = [...container.querySelectorAll("button")].find(
      (b) => b.textContent === "Export logs",
    )!;
    await act(async () => exportButton.click());
    expect(api.exportLogsZip).toHaveBeenCalledTimes(1);
    expect(container.textContent).toContain("Logs saved to /tmp/logs.zip");
  });
});
