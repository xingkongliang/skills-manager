import { describe, expect, it } from "vitest";
import { LatestRequestGate } from "./latestRequestGate";

describe("LatestRequestGate", () => {
  it("同一作用域只允许最后开始的请求提交结果", () => {
    const gate = new LatestRequestGate();
    gate.setScope("project-a");

    const first = gate.begin("project-a");
    const second = gate.begin("project-a");

    expect(gate.isCurrent(first)).toBe(false);
    expect(gate.isCurrent(second)).toBe(true);
  });

  it("切换作用域会立即使旧项目请求失效", () => {
    const gate = new LatestRequestGate();
    gate.setScope("project-a");
    const projectARequest = gate.begin("project-a");

    gate.setScope("project-b");
    const projectBRequest = gate.begin("project-b");

    expect(gate.isCurrent(projectARequest)).toBe(false);
    expect(gate.isCurrent(projectBRequest)).toBe(true);
  });

  it("旧作用域的延迟回调不能重新夺回当前作用域", () => {
    const gate = new LatestRequestGate();
    gate.setScope("project-a");
    gate.begin("project-a");

    gate.setScope("project-b");
    const projectBRequest = gate.begin("project-b");
    const delayedProjectARequest = gate.begin("project-a");

    expect(gate.isCurrent(delayedProjectARequest)).toBe(false);
    expect(gate.isCurrent(projectBRequest)).toBe(true);
  });

  it("组件卸载时可以显式使尚未完成的请求失效", () => {
    const gate = new LatestRequestGate();
    gate.setScope("project-a");
    const request = gate.begin("project-a");

    gate.invalidate();

    expect(gate.isCurrent(request)).toBe(false);
  });
});
