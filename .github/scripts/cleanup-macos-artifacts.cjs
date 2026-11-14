module.exports = async function cleanup({ github, context, core }) {
  if (!context.ref.startsWith("refs/heads/")) return;
  const branch = context.ref.slice("refs/heads/".length);
  const { data: currentRun } = await github.rest.actions.getWorkflowRun({
    ...context.repo,
    run_id: context.runId,
  });
  const architecture = context.payload.inputs?.architecture;
  const targets =
    context.eventName === "push" || architecture === "both"
      ? ["aarch64-apple-darwin", "x86_64-apple-darwin"]
      : [architecture === "x64" ? "x86_64-apple-darwin" : "aarch64-apple-darwin"];
  const prefixes = targets.map((target) => `skills-manager-macos-${target}-`);
  const currentArtifacts = await github.paginate(github.rest.actions.listWorkflowRunArtifacts, {
    ...context.repo,
    run_id: context.runId,
    per_page: 100,
  });
  // Never discard the previous download until every requested architecture is available.
  for (const prefix of prefixes) {
    if (
      !currentArtifacts.some((artifact) => !artifact.expired && artifact.name.startsWith(prefix))
    ) {
      throw new Error(
        `Current build has no downloadable installer for ${prefix}; keeping old artifacts`,
      );
    }
  }
  const runs = await github.paginate(github.rest.actions.listWorkflowRuns, {
    ...context.repo,
    workflow_id: currentRun.workflow_id,
    branch,
    per_page: 100,
  });
  for (const run of runs) {
    if (
      run.head_branch !== branch ||
      run.workflow_id !== currentRun.workflow_id ||
      run.id >= context.runId
    )
      continue;
    const artifacts = await github.paginate(github.rest.actions.listWorkflowRunArtifacts, {
      ...context.repo,
      run_id: run.id,
      per_page: 100,
    });
    for (const artifact of artifacts) {
      if (!prefixes.some((prefix) => artifact.name.startsWith(prefix))) continue;
      try {
        await github.rest.actions.deleteArtifact({ ...context.repo, artifact_id: artifact.id });
        core.info(`Deleted ${artifact.name} from older run ${run.id} on ${branch}`);
      } catch (error) {
        if (error.status !== 404) throw error;
        core.info(`Artifact ${artifact.id} was already deleted`);
      }
    }
  }
};
