// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TranslationProviderSettings } from "./TranslationProviderSettings";

const api = vi.hoisted(() => ({ getTranslationSettings: vi.fn(), saveTranslationSettings: vi.fn(), testTranslationSettings: vi.fn(), cancelSkillTranslation: vi.fn() }));
vi.mock("../lib/tauri", () => api);
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
const settings = { provider: "google" as const, api_base_url: "https://api.openai.com/v1", model: "", cli_path: "", timeout_seconds: 120, has_api_key: false, cache_namespace: "google" };
let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.resetAllMocks(); vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  api.getTranslationSettings.mockResolvedValue(settings);
  api.saveTranslationSettings.mockImplementation(async (value) => ({ ...value, has_api_key: true, cache_namespace: "api-model" }));
  api.testTranslationSettings.mockResolvedValue({ results: [{ text: "你好，世界。" }], error: null, retry_after_ms: null });
  api.cancelSkillTranslation.mockResolvedValue(undefined);
  container = document.createElement("div"); document.body.append(container); root = createRoot(container);
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); vi.unstubAllGlobals(); });
function button(key: string) { return [...container.querySelectorAll("button")].find((el) => el.textContent === key)!; }
function field(label: string) { return [...container.querySelectorAll("label")].find((el) => el.querySelector("span")?.textContent === label)!.querySelector("input, select") as HTMLInputElement | HTMLSelectElement; }
async function fill(label: string, value: string) {
  const input = field(label);
  const prototype = input instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event(input instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  });
}
describe("TranslationProviderSettings", () => {
  it("saves an arbitrary API model and custom endpoint, then tests the saved settings", async () => {
    await act(async () => root.render(<TranslationProviderSettings />));
    await fill("reader.providerTitle", "api");
    await fill("reader.apiBaseUrl", "http://localhost:1234/v1");
    await fill("reader.providerModel", "my-custom-model");
    await fill("reader.apiKey", "new-secret");
    expect(button("reader.testProvider").disabled).toBe(true);
    await act(async () => button("common.save").click());
    expect(api.saveTranslationSettings).toHaveBeenCalledWith({ provider: "api", api_base_url: "http://localhost:1234/v1", model: "my-custom-model", cli_path: "", timeout_seconds: 120 }, "new-secret");
    expect(field("reader.apiKey").value).toBe("");
    expect((field("reader.apiKey") as HTMLInputElement).placeholder).toBe("reader.apiKeySaved");
    expect(button("reader.testProvider").disabled).toBe(false);
    await act(async () => button("reader.testProvider").click());
    expect(container.querySelector('[role="status"]')?.textContent).toBe("reader.providerTestSuccess");
  });
  it("uses a custom CLI path without asking for API credentials", async () => {
    await act(async () => root.render(<TranslationProviderSettings />));
    await fill("reader.providerTitle", "codex");
    await fill("reader.cliPath", "/custom install/codex");
    await act(async () => button("common.save").click());
    expect(api.saveTranslationSettings).toHaveBeenCalledWith(expect.objectContaining({ provider: "codex", cli_path: "/custom install/codex", model: "" }), null);
    expect(container.querySelector('input[type="password"]')).toBeNull();
  });
});
