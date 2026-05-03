import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

async function importTypeScriptModule(moduleUrl) {
  const source = await readFile(moduleUrl, "utf8");
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
  });

  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
}

const { applyProjectSkillEnabledState, getProjectSkillVariantKey } = await importTypeScriptModule(
  new URL("./projectSkillState.ts", import.meta.url)
);

test("getProjectSkillVariantKey normalizes relative path casing", () => {
  assert.equal(
    getProjectSkillVariantKey({ agent: "claude_code", relative_path: "Foo/Bar" }),
    "claude_code::foo/bar"
  );
});

test("applyProjectSkillEnabledState updates only matching skill variants", () => {
  const skills = [
    { agent: "claude_code", relative_path: "foo/bar", enabled: true, marker: "keep-enabled" },
    { agent: "codex", relative_path: "foo/bar", enabled: true, marker: "disable-me" },
    { agent: "cursor", relative_path: "foo/baz", enabled: false, marker: "keep-disabled" },
  ];

  const next = applyProjectSkillEnabledState(
    skills,
    [{ agent: "codex", relative_path: "Foo/Bar" }],
    false
  );

  assert.deepEqual(
    next.map((skill) => ({ marker: skill.marker, enabled: skill.enabled })),
    [
      { marker: "keep-enabled", enabled: true },
      { marker: "disable-me", enabled: false },
      { marker: "keep-disabled", enabled: false },
    ]
  );
});
