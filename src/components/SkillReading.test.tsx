// @vitest-environment jsdom
/// <reference types="node" />
import { webcrypto } from "node:crypto";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SkillDetailPanel } from "./SkillDetailPanel";
import type { ManagedSkill, TranslationCacheEntry } from "../lib/tauri";

const api = vi.hoisted(() => ({
  openSkillBrowser: vi.fn(), closeSkillBrowser: vi.fn(), cancelOpenSkillBrowser: vi.fn(),
  readSkillFile: vi.fn(), listSkillDirectory: vi.fn(), getSettings: vi.fn(), setSettings: vi.fn(),
  getTranslationCache: vi.fn(), putTranslationCache: vi.fn(), translateSkillBlocks: vi.fn(),
  cancelSkillTranslation: vi.fn(), getTranslationSettings: vi.fn(),
  getSkillDocument: vi.fn(), getSourceSkillDocument: vi.fn(), getSkillSourceDiff: vi.fn(),
}));
vi.mock("../lib/tauri", async (original) => ({ ...await original<typeof import("../lib/tauri")>(), ...api }));
vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

const skill: ManagedSkill = {
  id: "example", name: "Example skill", description: "A short introduction.",
  source_type: "local", source_ref: null, source_ref_resolved: null, source_subpath: null,
  source_branch: null, source_revision: null, remote_revision: null, update_status: "local_only",
  last_checked_at: null, last_check_error: null, central_path: "/fixture/example",
  enabled: true, created_at: 0, updated_at: 0, status: "ok", targets: [], preset_ids: [], tags: [],
};

let container: HTMLDivElement;
let root: Root;
const onClose = vi.fn();
let cache: Map<string, TranslationCacheEntry>;

beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("crypto", webcrypto);
  localStorage.clear();
  cache = new Map();
  api.getSkillDocument.mockResolvedValue({ skill_id: skill.id, filename: "SKILL.md", central_path: skill.central_path, content: "# Original detail\n\nThe complete skill document stays here." });
  api.getSourceSkillDocument.mockResolvedValue({ skill_id: skill.id, filename: "SKILL.md", source_label: "example/source", revision: "abcdef123456", content: "# Source document\n\nThe upstream skill document." });
  api.getSkillSourceDiff.mockResolvedValue({ skill_id: skill.id, source_label: "example/source", revision: "abcdef123456", entries: [] });
  let sessions = 0;
  api.openSkillBrowser.mockImplementation(async () => ({ session_id: `session-${++sessions}`, source_label: "local", revision: null, default_path: "SKILL.md", read_only: false }));
  api.closeSkillBrowser.mockResolvedValue(undefined);
  api.cancelOpenSkillBrowser.mockResolvedValue(undefined);
  api.readSkillFile.mockResolvedValue({ relative_path: "SKILL.md", content_hash: "hash", size_bytes: 30, mime: "text/markdown", kind: "markdown", content: "# Reading example\n\nRead this document.", base64: null, reason: null });
  api.listSkillDirectory.mockResolvedValue({ entries: [], next_cursor: null, truncated: false });
  api.getSettings.mockResolvedValue(null);
  api.setSettings.mockResolvedValue(undefined);
  api.cancelSkillTranslation.mockResolvedValue(undefined);
  api.getTranslationSettings.mockResolvedValue({ provider: "google", api_base_url: "", model: "", cli_path: "", timeout_seconds: 60, has_api_key: false, cache_namespace: "google" });
  api.getTranslationCache.mockImplementation(async (keys: string[]) => ({ epoch: 0, persistent: true, entries: keys.flatMap((key) => cache.has(key) ? [cache.get(key)!] : []) }));
  api.putTranslationCache.mockImplementation(async (entries: TranslationCacheEntry[]) => { entries.forEach((entry) => cache.set(entry.cache_key, entry)); return true; });
  api.translateSkillBlocks.mockImplementation(async (_session: string, _request: string, texts: string[]) => ({ results: texts.map(() => ({ text: "缓存译文" })), error: null, retry_after_ms: null }));
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render(currentSkill = skill) {
  await act(async () => root.render(<SkillDetailPanel skill={currentSkill} onClose={onClose} />));
}
async function click(label: string) {
  const button = [...document.querySelectorAll("button")].find((node) => node.textContent === label);
  expect(button, `Button ${label}`).toBeDefined();
  await act(async () => button!.click());
}
async function openReader(bilingual = false) {
  await click("reader.read");
  await vi.waitFor(async () => {
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
    expect(document.querySelector('[role="dialog"] article')).not.toBeNull();
  });
  if (bilingual) await click("reader.bilingual");
}
async function translated() {
  await vi.waitFor(async () => {
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
    expect(document.querySelector(".reader-translation")?.textContent).toBe("缓存译文");
  });
}

describe("Independent skill reading", () => {
  it("keeps the complete original detail preview and opens the file browser only on demand", async () => {
    localStorage.setItem("reader_mode", "bilingual");
    await render();
    expect(document.body.textContent).toContain("A short introduction.");
    expect(document.body.textContent).toContain("reader.read");
    expect(document.querySelector("article")?.textContent).toContain("The complete skill document stays here.");
    expect(api.getSkillDocument).toHaveBeenCalledWith(skill.id);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.querySelector("aside")).toBeNull();
    expect(api.openSkillBrowser).not.toHaveBeenCalled();
    expect(api.readSkillFile).not.toHaveBeenCalled();
    expect(api.getTranslationSettings).not.toHaveBeenCalled();
    expect(api.translateSkillBlocks).not.toHaveBeenCalled();
  });

  it("preserves local, source and diff tabs and reads the matching origin independently", async () => {
    await render({ ...skill, source_type: "git", source_ref: "https://github.com/example/source" });
    expect(document.querySelector("article")?.textContent).toContain("Original detail");
    expect(document.body.textContent).toContain("example/source · abcdef1");
    expect(api.getSkillSourceDiff).not.toHaveBeenCalled();

    await click("mySkills.docTabs.source");
    expect(document.querySelector("article")?.textContent).toContain("The upstream skill document.");
    expect(api.openSkillBrowser).not.toHaveBeenCalled();
    await openReader();
    expect(api.openSkillBrowser.mock.calls.at(-1)?.[0]).toEqual({ kind: "managed", skill_id: skill.id, origin: "source" });
    await click("reader.backToDetails");
    expect(document.querySelector("article")?.textContent).toContain("The upstream skill document.");

    await click("mySkills.docTabs.diff");
    expect(api.getSkillSourceDiff).toHaveBeenCalledWith(skill.id);
    expect(document.body.textContent).toContain("mySkills.sourceDiff.noChanges");
    expect(document.body.textContent).not.toContain("reader.read");
    await click("mySkills.docTabs.local");
    expect(document.querySelector("article")?.textContent).toContain("The complete skill document stays here.");
    await openReader();
    expect(api.openSkillBrowser.mock.calls.at(-1)?.[0]).toEqual({ kind: "managed", skill_id: skill.id, origin: "local" });
  });

  it("opens original mode explicitly and returns to the same detail position and focus", async () => {
    localStorage.setItem("reader_mode", "bilingual");
    await render();
    const detail = document.querySelector<HTMLElement>(".overflow-y-auto")!;
    const preview = detail.querySelector("article");
    detail.scrollTop = 180;
    const entry = [...document.querySelectorAll("button")].find((node) => node.textContent === "reader.read")!;
    entry.focus();
    await openReader();
    const reader = document.querySelector('[role="dialog"]')!;
    expect(reader.textContent).toContain("Read this document.");
    expect(reader.querySelector('[aria-pressed="true"]')?.textContent).toBe("reader.original");
    expect(api.translateSkillBlocks).not.toHaveBeenCalled();
    await click("reader.backToDetails");
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(detail.isConnected).toBe(true);
    expect(detail.querySelector("article")).toBe(preview);
    expect(detail.scrollTop).toBe(180);
    expect(document.activeElement).toBe(entry);
    expect(container.inert).toBeFalsy();
    expect(onClose).not.toHaveBeenCalled();
    expect(api.closeSkillBrowser).toHaveBeenCalledWith("session-1");
    expect(api.getSkillDocument).toHaveBeenCalledTimes(1);
  });

  it("opens other files through the directory tree without changing the detail preview", async () => {
    api.listSkillDirectory.mockImplementation(async (_session: string, directory: string) => ({
      entries: directory === "references"
        ? [{ relative_path: "references/guide.md", name: "guide.md", kind: "file", size_bytes: 80, preview_kind: "markdown", accessible: true }]
        : [{ relative_path: "references", name: "references", kind: "directory", size_bytes: null, preview_kind: null, accessible: true }],
      next_cursor: null, truncated: false,
    }));
    api.readSkillFile.mockImplementation(async (_session: string, path: string) => ({ relative_path: path, content_hash: path, size_bytes: 80, mime: "text/markdown", kind: "markdown", content: path === "references/guide.md" ? "# Guide\n\nAn additional reference document." : "# Skill entry", base64: null, reason: null }));
    await render();
    const preview = document.querySelector("article");
    await openReader();
    await click("reader.files");
    await click("references");
    await click("guide.md");
    expect(document.querySelector('[role="dialog"] article')?.textContent).toContain("An additional reference document.");
    expect(preview?.textContent).toContain("The complete skill document stays here.");
    await click("reader.backToDetails");
    expect(document.querySelector("article")).toBe(preview);
  });

  it("reuses translated paragraphs when the bilingual reader is reopened", async () => {
    localStorage.setItem("reader_mode", "original");
    await render();
    await openReader(true);
    await translated();
    const calls = api.translateSkillBlocks.mock.calls.length;
    expect(calls).toBeGreaterThan(0);
    await click("reader.backToDetails");
    await openReader(true);
    await translated();
    expect(api.translateSkillBlocks).toHaveBeenCalledTimes(calls);
  });

  it("translates only changed paragraphs after a document changes", async () => {
    await render();
    await openReader(true);
    await translated();
    const calls = api.translateSkillBlocks.mock.calls.length;
    await click("reader.backToDetails");
    api.readSkillFile.mockResolvedValue({ relative_path: "SKILL.md", content_hash: "new-hash", size_bytes: 60, mime: "text/markdown", kind: "markdown", content: "# Reading example\n\nThis paragraph has changed.", base64: null, reason: null });
    await openReader(true);
    await vi.waitFor(async () => {
      await act(async () => { await new Promise((resolve) => setTimeout(resolve, 10)); });
      expect(api.translateSkillBlocks.mock.calls.slice(calls).flatMap((call) => call[2])).toEqual(["This paragraph has changed."]);
    });
  });

  it("Escape exits only the reader and keeps the detail open", async () => {
    await render();
    await openReader();
    await act(async () => {
      document.activeElement!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
    });
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(document.body.textContent).toContain("Example skill");
    expect(onClose).not.toHaveBeenCalled();
  });
});
