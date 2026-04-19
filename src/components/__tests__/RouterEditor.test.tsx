// Unit tests for RouterEditor.
//
// NOTE: This repo does not currently ship a JS test runner (vitest/jest) or
// @testing-library/react in package.json. These tests are written to the
// Vitest + @testing-library/react API as specified by the Phase 10 Task 15
// plan (docs/superpowers/plans/2026-04-19-progressive-disclosure.md) so that
// they are ready to run once the test harness is added (planned in a later
// task). The file is excluded from the production `tsc -b` build via the
// existing Vite project config (tests live under __tests__ and are not
// imported by app code).
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { RouterEditor } from "../RouterEditor";

describe("RouterEditor", () => {
  it("disables Save when description is empty", () => {
    render(
      <RouterEditor packId="p1" initial={{ description: "" }} onSave={vi.fn()} />,
    );
    expect(screen.getByRole("button", { name: /save/i })).toBeDisabled();
  });

  it("warns when description exceeds 600 chars", () => {
    const long = "x".repeat(601);
    render(
      <RouterEditor
        packId="p1"
        initial={{ description: long }}
        onSave={vi.fn()}
      />,
    );
    expect(screen.getByTestId("char-counter").className).toContain(
      "text-red-600",
    );
  });

  it("shows yellow warning between 401 and 600 chars", () => {
    const mid = "x".repeat(500);
    render(
      <RouterEditor
        packId="p1"
        initial={{ description: mid }}
        onSave={vi.fn()}
      />,
    );
    expect(screen.getByTestId("char-counter").className).toContain(
      "text-yellow-600",
    );
  });

  it("calls onSave with trimmed description + body", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(
      <RouterEditor packId="p1" initial={{ description: "" }} onSave={onSave} />,
    );
    fireEvent.change(screen.getByLabelText(/router description/i), {
      target: { value: "  hello  " },
    });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    expect(onSave).toHaveBeenCalledWith({ description: "hello", body: null });
  });

  it("converts empty body to null", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(
      <RouterEditor
        packId="p1"
        initial={{ description: "desc", body: "" }}
        onSave={onSave}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    expect(onSave).toHaveBeenCalledWith({ description: "desc", body: null });
  });

  it("renders Generate button when onGenerate provided", () => {
    render(
      <RouterEditor
        packId="p1"
        initial={{ description: "d" }}
        onSave={vi.fn()}
        onGenerate={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", { name: /generate with claude code/i }),
    ).toBeInTheDocument();
  });
});
