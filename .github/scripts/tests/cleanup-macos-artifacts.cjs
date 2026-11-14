const test = require("node:test");
const assert = require("node:assert/strict");
const cleanup = require("../cleanup-macos-artifacts.cjs");

const artifact = (id, target, expired = false) => ({
  id,
  name: `skills-manager-macos-${target}-commit`,
  expired,
});
const arm = (id) => artifact(id, "aarch64-apple-darwin");
const intel = (id) => artifact(id, "x86_64-apple-darwin");

function fixture({ current = [arm(200), intel(201)], eventName = "push", architecture } = {}) {
  const deleted = [];
  const inspected = [];
  const branch = "feature/nested/branch";
  const runs = [
    { id: 21, workflow_id: 7, head_branch: branch },
    { id: 20, workflow_id: 7, head_branch: branch },
    { id: 19, workflow_id: 7, head_branch: "another-branch" },
    { id: 18, workflow_id: 8, head_branch: branch },
    { id: 17, workflow_id: 7, head_branch: branch, conclusion: "failure" },
    { id: 16, workflow_id: 7, head_branch: branch, conclusion: "success" },
  ];
  const github = {
    rest: {
      actions: {
        getWorkflowRun: async () => ({ data: { workflow_id: 7 } }),
        listWorkflowRunArtifacts: "artifacts",
        listWorkflowRuns: "runs",
        deleteArtifact: async ({ artifact_id }) => deleted.push(artifact_id),
      },
    },
    paginate: async (route, options) => {
      if (route === "runs") {
        assert.equal(options.branch, branch);
        assert.equal(options.workflow_id, 7);
        return runs;
      }
      inspected.push(options.run_id);
      if (options.run_id === 20) return current;
      if (options.run_id === 17) return [arm(170)];
      return [arm(160), intel(161), { id: 162, name: "unrelated-artifact" }];
    },
  };
  const context = {
    repo: { owner: "BUPTHELLO", repo: "skills-manager" },
    ref: `refs/heads/${branch}`,
    runId: 20,
    eventName,
    payload: { inputs: { architecture } },
  };
  return { github, context, core: { info() {} }, deleted, inspected };
}

test("cleans old installers on the same branch, including partial failed runs", async () => {
  const state = fixture();
  await cleanup(state);
  assert.deepEqual(state.deleted, [170, 160, 161]);
  assert.deepEqual(state.inspected, [20, 17, 16]);
});

test("keeps all previous installers when one requested architecture is missing or expired", async () => {
  for (const current of [[arm(200)], [arm(200), artifact(201, "x86_64-apple-darwin", true)]]) {
    const state = fixture({ current });
    await assert.rejects(cleanup(state), /keeping old artifacts/);
    assert.deepEqual(state.deleted, []);
    assert.deepEqual(state.inspected, [20]);
  }
});

test("a manual single architecture build preserves the other architecture", async () => {
  const state = fixture({
    current: [arm(200)],
    eventName: "workflow_dispatch",
    architecture: "arm64",
  });
  await cleanup(state);
  assert.deepEqual(state.deleted, [170, 160]);
});

test("tag runs never clean branch artifacts", async () => {
  const state = fixture();
  state.context.ref = "refs/tags/v1.0.0";
  await cleanup(state);
  assert.deepEqual(state.deleted, []);
  assert.deepEqual(state.inspected, []);
});
