// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MultiSelectToolbar } from "./MultiSelectToolbar";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const labels = {
  hint: "Click skills to select",
  selected: "5 selected",
  selectAll: "Select All",
  deselectAll: "Deselect All",
  cancel: "Cancel",
  more: "More",
};

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

function render(overrides: Partial<Parameters<typeof MultiSelectToolbar>[0]> = {}) {
  act(() => {
    root.render(
      <MultiSelectToolbar
        selectedCount={5}
        isAllSelected={false}
        actions={[]}
        labels={labels}
        onSelectAll={() => {}}
        onCancel={() => {}}
        {...overrides}
      />
    );
  });
  return container.textContent ?? "";
}

describe("MultiSelectToolbar", () => {
  it("shows only the count when every selected skill is on screen", () => {
    const text = render();
    expect(text).toContain("5 selected");
    expect(text).not.toContain("collapsed");
  });

  it("names the hidden skills when some are inside collapsed sources", () => {
    const text = render({ labels: { ...labels, hiddenSelected: "(2 in collapsed sources)" } });
    expect(text).toContain("5 selected");
    expect(text).toContain("(2 in collapsed sources)");
  });

  it("keeps the hidden note out of the hint state", () => {
    // No selection means nothing is hidden; the note would contradict the hint.
    const text = render({ selectedCount: 0, labels: { ...labels, hiddenSelected: "(2 in)" } });
    expect(text).toContain("Click skills to select");
    expect(text).not.toContain("(2 in)");
  });
});
