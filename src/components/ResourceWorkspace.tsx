import { useNavigate, useSearch } from "@tanstack/react-router";
import { type ReactNode, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { useApp } from "../context/AppContext";
import { useWorkspaceChanges } from "../context/WorkspaceChangesContext";
import { invokeHost } from "../lib/hostCall";
import { cn } from "../utils";
import { InstructionFileTree } from "./InstructionFileTree";
import { MarkdownEditor } from "./MarkdownEditor";
import { ResourceMatrix } from "./ResourceMatrix";
import { WorkspaceHeader } from "./WorkspaceHeader";

type ResourceTab = "instructions" | "mcps" | "skills";

export type WorkspaceScope = {
  kind: "global" | "library" | "project";
  projectId?: string;
  agentKey?: string;
  agentKeys?: null | string[];
  category?: "coding" | "lobster";
};

interface McpDefinition {
  id: string;
  name: string;
  transport: "http" | "sse" | "stdio";
  server: {
    command?: string;
    args?: string[];
    env?: Record<string, string>;
    url?: string;
    headers?: Record<string, string>;
    envFile?: string;
    cwd?: string;
  };
  auth?: {
    bearerTokenEnvVar?: string;
    credentialRefs?: Record<string, string>;
  };
  source?: { registry: "official"; serverId: string; version: string };
  revision: string;
  updatedAt: string;
}

type McpInput = {
  id?: string;
} & Omit<McpDefinition, "id" | "revision" | "updatedAt">;

export type McpTarget = { agentKey: string; projectId?: string };

type McpConflict = "keep" | "rename" | "replace";

type JsonValue = { [key: string]: JsonValue } | boolean | JsonValue[] | null | number | string;

type McpDeployOperation = {
  kind: "deploy";
  definitionId: string;
  conflict: McpConflict;
  newName?: string;
  overrideConfig?: { server: JsonValue };
};

/** What `mcps_request` accepts, for the actions this workspace sends. */
export type McpRequest =
  | { action: "apply"; previewId: string }
  | { action: "capabilities" | "list" }
  | { action: "catalog"; query: string; cursor?: string }
  | { action: "catalog"; serverId: string }
  | { action: "import"; target: McpTarget; name: string; conflict: McpConflict; newName?: string }
  | { action: "inspect"; target: McpTarget }
  | { action: "preview"; target: McpTarget; operations: McpDeployOperation[] }
  | { action: "recover"; recoveryId: string }
  | { action: "remove"; id: string; detach: boolean }
  | { action: "save"; definition: McpInput; expectedRevision?: string }
  | { action: "undeploy"; target: McpTarget; definitionId: string; serverName: string };

export type InstructionTarget = {
  agent_key: string;
  project_id?: string;
  worktree?: string;
  relative_dir?: string;
};

/** What `instructions_request` accepts, for the actions this workspace sends. */
export type InstructionsRequest =
  | {
      action: "apply";
      preview_id: string;
      resolutions: Record<string, "keep_local" | "take_library">;
    }
  | { action: "deployments" | "list" | "recover" }
  | { action: "get"; id: string }
  | { action: "preview"; target: InstructionTarget; instruction_id: string }
  | { action: "read"; target: InstructionTarget; path: string }
  | { action: "remove"; id: string; detach: boolean }
  | {
      action: "save";
      id?: string;
      name: string;
      description?: string;
      files: Record<string, string>;
      expected_revision?: string;
    }
  | { action: "scan"; target: InstructionTarget; include_dirs?: string[] }
  | { action: "undeploy"; deployment_id: string }
  | {
      action: "write";
      target: InstructionTarget;
      path: string;
      content: string;
      expected_revision: string;
    };

/** What `resource_sync_request` accepts. */
type ResourceSyncRequest =
  | { action: "list" }
  | { action: "resolve"; id: string; choice: "local" | "remote" };

type McpPreview = {
  previewId: string;
  changes: {
    name: string;
    kind: string;
    beforeSummary?: unknown;
    afterSummary?: unknown;
    conflict?: string;
    warnings: string[];
  }[];
};

type McpRecovery = { id: string; status: string; message: string };

type McpCapabilities = {
  targets: {
    agentKey: string;
    scopes: { kind: "global" | "project"; supported: boolean }[];
    features: string[];
    limitations: string[];
  }[];
};

const inputClass = "app-input w-full";

const RESOURCE_AGENT_KEYS = new Set(["claude_code", "codex", "antigravity", "hermes", "cursor"]);

/** Shared resource tabs embedded in each workspace. Skills retain their existing surface. */
export function ResourceWorkspace({
  scope,
  skills,
  allowResources = true,
}: {
  scope: WorkspaceScope;
  skills: ReactNode;
  allowResources?: boolean;
}) {
  const { activeHostId, tools } = useApp();

  return (
    <ResourceWorkspaceBody
      key={`${activeHostId ?? "local"}:${scope.kind}:${scope.projectId ?? ""}:${scope.agentKey ?? ""}`}
      hostId={activeHostId}
      tools={tools}
      scope={scope}
      skills={skills}
      allowResources={allowResources}
    />
  );
}

function ResourceWorkspaceBody({
  hostId,
  tools,
  scope,
  skills,
  allowResources,
}: {
  hostId: null | string;
  tools: ReturnType<typeof useApp>["tools"];
  scope: WorkspaceScope;
  skills: ReactNode;
  allowResources: boolean;
}) {
  const workspaceChanges = useWorkspaceChanges();
  const [editorOpen, setEditorOpen] = useState(false);
  const search = useSearch({ strict: false });
  const navigate = useNavigate();

  const resource = "resource" in search ? search.resource : undefined;

  const tab: ResourceTab =
    allowResources && (resource === "instructions" || resource === "mcps") ? resource : "skills";

  const tabListRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const list = tabListRef.current;
    const focused = document.activeElement;

    if (list?.contains(focused) && focused?.matches("[role='tab']")) {
      list.querySelector<HTMLButtonElement>("[role='tab'][aria-selected='true']")?.focus();
    }
  }, [tab]);

  const setTab = (next: ResourceTab) =>
    void navigate({
      to: ".",
      search: (previous) => ({ ...previous, resource: next === "skills" ? undefined : next }),
    });

  const supportedTools = useMemo(
    () =>
      tools.filter(
        (tool) =>
          tool.installed &&
          tool.enabled &&
          RESOURCE_AGENT_KEYS.has(tool.key) &&
          (!scope.category || tool.category === scope.category) &&
          (scope.agentKeys == null || scope.agentKeys.includes(tool.key)),
      ),
    [tools, scope.category, scope.agentKeys],
  );

  const { projects } = useApp();

  const [agentKey, setAgentKey] = useState(
    RESOURCE_AGENT_KEYS.has(scope.agentKey ?? "") ? (scope.agentKey ?? "") : "",
  );

  const [targetProjectId, setTargetProjectId] = useState(scope.projectId ?? "");

  const selectedProjectId =
    scope.kind === "project"
      ? scope.projectId
      : scope.kind === "library" && targetProjectId
        ? targetProjectId
        : undefined;

  const scopedAgent = RESOURCE_AGENT_KEYS.has(scope.agentKey ?? "") ? scope.agentKey : undefined;

  const target: McpTarget = { agentKey: scopedAgent || agentKey || supportedTools[0]?.key || "" };

  if (selectedProjectId) target.projectId = selectedProjectId;
  const unavailable = !target.agentKey;
  const tabs: ResourceTab[] = allowResources ? ["skills", "instructions", "mcps"] : ["skills"];

  return (
    <div className="app-page gap-4">
      <WorkspaceHeader scope={scope} />
      <div
        ref={tabListRef}
        role="tablist"
        aria-label="Resource type"
        className="flex flex-wrap items-center gap-2 border-b border-border-faint pb-2"
      >
        {tabs.map((key) => (
          <button
            key={key}
            role="tab"
            aria-label={key === "mcps" ? "MCPs" : key[0].toUpperCase() + key.slice(1)}
            aria-selected={tab === key}
            tabIndex={tab === key ? 0 : -1}
            onKeyDown={(event) => {
              if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
              event.preventDefault();
              const index = tabs.indexOf(key);

              const next =
                event.key === "Home"
                  ? tabs[0]
                  : event.key === "End"
                    ? tabs[tabs.length - 1]
                    : tabs[
                        (index + (event.key === "ArrowRight" ? 1 : tabs.length - 1)) % tabs.length
                      ];

              setTab(next);
            }}
            type="button"
            onClick={() => setTab(key)}
            className={cn(
              "rounded-lg px-3 py-1.5 text-[13px] font-medium",
              tab === key
                ? "bg-surface-active text-primary"
                : "text-muted hover:bg-surface-hover hover:text-primary",
            )}
          >
            {key === "mcps" ? "MCPs" : key[0].toUpperCase() + key.slice(1)}
            {key === "mcps" && (
              <span className="ml-1.5 rounded-full bg-amber-500/10 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-300">
                Alpha
              </span>
            )}
          </button>
        ))}
        {tab !== "skills" && !scopedAgent && editorOpen && (
          <label className="ml-auto flex items-center gap-2 text-[12px] text-muted">
            Agent
            <select
              aria-label="Agent"
              className="app-input py-1"
              value={agentKey || supportedTools[0]?.key || ""}
              onChange={async (event) => {
                const value = event.target.value;

                if (await workspaceChanges.confirmLeave()) setAgentKey(value);
              }}
            >
              {supportedTools.map((tool) => (
                <option key={tool.key} value={tool.key}>
                  {tool.display_name}
                </option>
              ))}
            </select>
          </label>
        )}
        {scope.kind === "library" && tab !== "skills" && (
          <label className="flex items-center gap-2 text-[12px] text-muted">
            Target scope
            <select
              aria-label="Target scope"
              className="app-input py-1"
              value={targetProjectId}
              onChange={async (event) => {
                const value = event.target.value;

                if (await workspaceChanges.confirmLeave()) setTargetProjectId(value);
              }}
            >
              <option value="">Global workspace</option>
              {projects
                .filter((project) => project.workspace_type !== "linked")
                .map((project) => (
                  <option key={project.id} value={project.id}>
                    {project.name}
                  </option>
                ))}
            </select>
          </label>
        )}
      </div>
      {tab === "skills" ? (
        skills
      ) : (
        <>
          <ResourceMatrix
            key={`${hostId ?? "local"}:${tab}:${target.projectId ?? "global"}:${scope.agentKey ?? "all"}`}
            kind={tab}
            onEditingChange={setEditorOpen}
            scope={scope}
            projectId={target.projectId}
            agents={
              scopedAgent
                ? supportedTools.filter((tool) => tool.key === scopedAgent)
                : supportedTools
            }
            editor={(initialId) =>
              tab === "mcps" ? (
                <McpLibrary
                  hostId={hostId}
                  target={target}
                  unavailable={unavailable}
                  initialId={initialId ?? ""}
                />
              ) : (
                <InstructionsLibrary
                  hostId={hostId}
                  scope={scope}
                  create={initialId === null}
                  target={target}
                  unavailable={unavailable}
                  initialId={initialId ?? ""}
                />
              )
            }
          />

          {scope.kind === "library" && <ResourceBackupConflicts hostId={hostId} kind={tab} />}
        </>
      )}
    </div>
  );
}

function ResourceBackupConflicts({
  hostId,
  kind,
}: {
  hostId: null | string;
  kind: "instructions" | "mcps";
}) {
  type Conflict = {
    id: string;
    key: string;
    kind: "instructions" | "mcps";
    name: string;
    local: null | Record<string, string>;
    remote: null | Record<string, string>;
    this_host: string;
    other_host: string;
  };

  const [conflicts, setConflicts] = useState<Conflict[]>([]);
  const [message, setMessage] = useState("");

  const request = <T,>(action: ResourceSyncRequest) =>
    invokeHost<T>(hostId, "resource_sync_request", { request: action });

  const refresh = async () => {
    try {
      const result = await request<{ conflicts: Conflict[] }>({
        action: "list",
      });

      setConflicts(result.conflicts.filter((item) => item.kind === kind));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  useEffect(() => {
    let current = true;
    void invokeHost<{ conflicts: Conflict[] }>(hostId, "resource_sync_request", {
      request: { action: "list" },
    })
      .then((result) => {
        if (current) setConflicts(result.conflicts.filter((item) => item.kind === kind));
      })
      .catch((error) => {
        if (current) setMessage(error instanceof Error ? error.message : String(error));
      });

    return () => {
      current = false;
    };
  }, [hostId, kind]);

  const resolve = async (id: string, choice: "local" | "remote") => {
    try {
      await request({ action: "resolve", id, choice });
      setMessage("Choice recorded in Sync. Every host applies it on its next sync.");
      await refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    }
  };

  if (!conflicts.length && !message) return null;

  return (
    <div className="app-panel space-y-3 p-4">
      <div>
        <h3 className="text-[13px] font-semibold">Sync needs a choice</h3>
        <p className="mt-1 text-[11px] text-muted">
          This item changed on two hosts. Sync kept one version for now. Compare the files and
          choose which version to keep on every host.
        </p>
      </div>
      {conflicts.map((item) => (
        <div key={item.id} className="border-t border-border-faint pt-3">
          <p className="text-[12px] font-medium">{item.name}</p>
          <div className="mt-2 grid gap-2 md:grid-cols-2">
            {(
              [
                [`${item.this_host}'s version (in Sync now)`, item.local],
                [`${item.other_host}'s version`, item.remote],
              ] as const
            ).map(([label, files]) => (
              <div key={label} className="min-w-0 rounded border border-border-faint p-2">
                <p className="text-[11px] font-semibold">{label}</p>
                {Object.entries(files ?? {}).map(([file, content]) => (
                  <details key={file} className="mt-1">
                    <summary className="cursor-pointer truncate text-[11px]">{file}</summary>
                    <pre className="max-h-36 overflow-auto whitespace-pre-wrap rounded bg-background p-2 text-[10px]">
                      {content}
                    </pre>
                  </details>
                ))}
              </div>
            ))}
          </div>
          <div className="mt-2 flex gap-2">
            <button className="app-button-secondary" onClick={() => void resolve(item.id, "local")}>
              Keep {item.this_host}'s version
            </button>
            <button
              className="app-button-secondary"
              onClick={() => void resolve(item.id, "remote")}
            >
              Keep {item.other_host}'s version
            </button>
          </div>
        </div>
      ))}
      {message && (
        <p role="status" className="text-[11px] text-muted">
          {message}
        </p>
      )}
    </div>
  );
}

function McpLibrary({
  initialId,
  hostId,
  target,
  unavailable,
}: {
  initialId: string;
  hostId: null | string;
  target: McpTarget;
  unavailable: boolean;
}) {
  const [items, setItems] = useState<McpDefinition[]>([]);
  const [selected, setSelected] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [preview, setPreview] = useState<McpPreview | null>(null);
  const [mcpRecoveries, setMcpRecoveries] = useState<McpRecovery[]>([]);
  const [name, setName] = useState("");

  const [transport, setTransport] = useState<McpDefinition["transport"]>("stdio");

  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [url, setUrl] = useState("");
  const [environment, setEnvironment] = useState("");
  const [headers, setHeaders] = useState("");
  const [workingDirectory, setWorkingDirectory] = useState("");
  const [envFile, setEnvFile] = useState("");
  const [tokenVariable, setTokenVariable] = useState("");
  const [credentialRefs, setCredentialRefs] = useState("");
  const [deploy, setDeploy] = useState(false);

  const [found, setFound] = useState<
    {
      name: string;
      managedId?: string;
      status: string;
      importable?: boolean;
      reason?: string;
      definition?: McpDefinition;
      override?: { server?: JsonValue } | null;
    }[]
  >([]);

  const [catalog, setCatalog] = useState<
    {
      name: string;
      title?: string;
      description?: string;
      repository?: string;
    }[]
  >([]);

  const [catalogCursor, setCatalogCursor] = useState<null | string>(null);
  const [catalogMetadata, setCatalogMetadata] = useState<unknown>(null);

  const [catalogDrafts, setCatalogDrafts] = useState<
    { label: string; definition: McpInput; requirements: string[] }[]
  >([]);

  const [catalogDraftIndex, setCatalogDraftIndex] = useState(0);
  const [catalogSource, setCatalogSource] = useState<McpDefinition["source"]>();
  const [catalogQuery, setCatalogQuery] = useState("");

  const [conflict, setConflict] = useState<McpConflict>("keep");

  const [newName, setNewName] = useState("");

  const [capabilities, setCapabilities] = useState<McpCapabilities["targets"]>([]);

  const [targetOverride, setTargetOverride] = useState("");

  const request = <T,>(action: McpRequest) =>
    invokeHost<T>(hostId, "mcps_request", { request: action });

  const refresh = async () => {
    setLoading(true);
    setMessage("");

    try {
      const result = await request<{ definitions: McpDefinition[] }>({
        action: "list",
      });

      setItems(result.definitions);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    let current = true;
    void invokeHost<{ definitions: McpDefinition[] }>(hostId, "mcps_request", {
      request: { action: "list" },
    })
      .then((result) => {
        if (current) setItems(result.definitions);
      })
      .catch((error) => {
        if (current) setMessage(error instanceof Error ? error.message : String(error));
      })
      .finally(() => {
        if (current) setLoading(false);
      });

    return () => {
      current = false;
    };
  }, [hostId]);
  useEffect(() => {
    let current = true;
    void invokeHost<McpCapabilities>(hostId, "mcps_request", {
      request: { action: "capabilities" },
    })
      .then((result) => {
        if (current) setCapabilities(result.targets);
      })
      .catch((error) => {
        if (current) setMessage(error instanceof Error ? error.message : String(error));
      });

    return () => {
      current = false;
    };
  }, [hostId]);
  const targetAgentKey = target.agentKey;
  const targetProjectId = target.projectId;
  useEffect(() => {
    if (unavailable) return;
    let current = true;
    const inspected: McpTarget = { agentKey: targetAgentKey };

    if (targetProjectId) inspected.projectId = targetProjectId;
    void invokeHost<{ entries: typeof found; recoveries?: McpRecovery[] }>(hostId, "mcps_request", {
      request: { action: "inspect", target: inspected },
    })
      .then((result) => {
        if (current) {
          setFound(result.entries);
          setMcpRecoveries(result.recoveries ?? []);
        }
      })
      .catch((error) => {
        if (current) setMessage(error instanceof Error ? error.message : String(error));
      });

    return () => {
      current = false;
    };
  }, [hostId, targetAgentKey, targetProjectId, unavailable]);
  const current = items.find((item) => item.id === selected);

  const targetCapability = capabilities.find((entry) => entry.agentKey === target.agentKey);

  const targetScope = targetCapability?.scopes.find(
    (entry) => entry.kind === (target.projectId ? "project" : "global"),
  );

  const hasCredentialReferences = Boolean(
    tokenVariable.trim() ||
    credentialRefs.trim() ||
    /\$\{env:/.test(environment + headers + targetOverride),
  );

  const deploymentSupported = Boolean(
    targetScope?.supported &&
    targetCapability?.features.includes(transport) &&
    !(target.agentKey === "antigravity" && hasCredentialReferences),
  );

  const startNew = () => {
    setPreview(null);
    setEnvFile("");
    setSelected("");
    setName("");
    setTransport("stdio");
    setCommand("");
    setArgs("");
    setUrl("");
    setEnvironment("");
    setHeaders("");
    setWorkingDirectory("");
    setTokenVariable("");
    setCredentialRefs("");
    setCatalogDrafts([]);
    setCatalogDraftIndex(0);
    setCatalogSource(undefined);
    setCatalogMetadata(null);
  };

  const edit = (item: McpDefinition) => {
    setPreview(null);
    setEnvFile(item.server.envFile ?? "");
    setSelected(item.id);
    setName(item.name);
    setTransport(item.transport);
    setCommand(item.server.command ?? "");
    setArgs((item.server.args ?? []).join("\n"));
    setUrl(item.server.url ?? "");
    setEnvironment(
      Object.entries(item.server.env ?? {})
        .map(([key, value]) => `${key}=${value}`)
        .join("\n"),
    );
    setHeaders(
      Object.entries(item.server.headers ?? {})
        .map(([key, value]) => `${key}=${value}`)
        .join("\n"),
    );
    setWorkingDirectory(item.server.cwd ?? "");
    setTokenVariable(item.auth?.bearerTokenEnvVar ?? "");
    setCredentialRefs(
      Object.entries(item.auth?.credentialRefs ?? {})
        .map(([key, value]) => `${key}=${value}`)
        .join("\n"),
    );
    setCatalogSource(item.source);
  };

  const parseMap = (text: string) =>
    Object.fromEntries(
      text
        .split("\n")
        .map((line) => line.trim())
        .filter(Boolean)
        .map((line) => {
          const index = line.indexOf("=");

          return index < 1
            ? [line, ""]
            : [line.slice(0, index).trim(), line.slice(index + 1).trim()];
        }),
    );

  const save = async () => {
    setBusy(true);
    setMessage("");

    try {
      const env = parseMap(environment);

      const secretLike = Object.entries({ ...env, ...parseMap(headers) }).find(
        ([key, value]) =>
          /(auth|secret|token|password|credential|api[_-]?key)/i.test(key) &&
          !value.includes("${env:"),
      );

      if (secretLike)
        throw new Error(
          "Use an environment reference for " +
            secretLike[0] +
            ", such as ${env:" +
            secretLike[0] +
            "}.",
        );

      if (tokenVariable.trim() && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(tokenVariable.trim()))
        throw new Error("Enter the environment variable name, not a credential value.");

      const auth: NonNullable<McpInput["auth"]> = {};

      if (tokenVariable.trim()) auth.bearerTokenEnvVar = tokenVariable.trim();
      const refs = parseMap(credentialRefs);

      if (Object.keys(refs).length) auth.credentialRefs = refs;

      const server: McpInput["server"] =
        transport === "stdio"
          ? {
              command: command.trim(),
              args: args
                .split("\n")
                .map((v) => v.trim())
                .filter(Boolean),
              env,
            }
          : { url: url.trim(), headers: parseMap(headers) };

      if (transport === "stdio" && envFile.trim()) server.envFile = envFile.trim();

      if (transport === "stdio" && workingDirectory.trim()) server.cwd = workingDirectory.trim();
      const definition: McpInput = { name: name.trim(), transport, server };

      if (selected) definition.id = selected;

      if (Object.keys(auth).length) definition.auth = auth;

      if (catalogSource) definition.source = catalogSource;
      const saveRequest: McpRequest = { action: "save", definition };

      if (current) saveRequest.expectedRevision = current.revision;
      const result = await request<{ definition: McpDefinition }>(saveRequest);

      await refresh();
      edit(result.definition);
      setMessage("Saved.");

      if (deploy) await createPreview(result.definition.id);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const createPreview = async (definitionId = selected) => {
    if (!definitionId || unavailable || !deploymentSupported) return;
    setBusy(true);
    setMessage("");

    try {
      const operation: McpDeployOperation = { kind: "deploy", definitionId, conflict };

      if (conflict === "rename" && newName.trim()) operation.newName = newName.trim();

      if (targetOverride.trim()) operation.overrideConfig = { server: JSON.parse(targetOverride) };
      setPreview(await request<McpPreview>({ action: "preview", target, operations: [operation] }));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    if (!preview) return;
    setBusy(true);

    try {
      const result = await request<{
        partial?: boolean;
        recoveryId?: string;
        message?: string;
      }>({ action: "apply", previewId: preview.previewId });

      setPreview(null);
      await refreshTarget();
      setMessage(
        result.partial
          ? (result.message ?? "The operation needs recovery before it can be completed.")
          : "Applied to this workspace.",
      );
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const refreshTarget = async () => {
    const result = await request<{
      entries: typeof found;
      recoveries?: McpRecovery[];
    }>({
      action: "inspect",
      target,
    });

    setFound(result.entries);
    setMcpRecoveries(result.recoveries ?? []);
  };

  const recoverMcp = async (recoveryId: string) => {
    setBusy(true);

    try {
      const result = await request<{ fileApplied: boolean }>({
        action: "recover",
        recoveryId,
      });

      await refreshTarget();
      setMessage(
        result.fileApplied
          ? "Deployment state recovered."
          : "Interrupted operation cleared; no configuration changes were applied.",
      );
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const undeploy = async (entry: (typeof found)[number]) => {
    const definitionId = entry.managedId ?? items.find((item) => item.name === entry.name)?.id;

    if (!definitionId) return;
    setBusy(true);

    try {
      const result = await request<{
        removed: boolean;
        preservedEdited: boolean;
        partial?: boolean;
        message?: string;
      }>({ action: "undeploy", target, definitionId, serverName: entry.name });

      await refreshTarget();
      setMessage(
        result.partial
          ? (result.message ?? "The operation needs recovery before it can be completed.")
          : result.preservedEdited
            ? "Edited configuration was preserved. This deployment still needs review."
            : result.removed
              ? "Removed from this workspace."
              : "No native configuration was removed.",
      );
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!selected) return;
    setBusy(true);

    try {
      await request({ action: "remove", id: selected, detach: true });
      startNew();
      await refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const browseCatalog = async (cursor?: string) => {
    setBusy(true);

    try {
      const catalogRequest: McpRequest = { action: "catalog", query: catalogQuery };

      if (cursor) catalogRequest.cursor = cursor;

      const result = await request<{
        servers: {
          name: string;
          title?: string;
          description?: string;
          repository?: string;
        }[];
        nextCursor?: null | string;
      }>(catalogRequest);

      setCatalog((previous) => (cursor ? [...previous, ...result.servers] : result.servers));
      setCatalogCursor(result.nextCursor ?? null);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const applyCatalogDraft = (draft: McpInput) => {
    setName(draft.name);
    setTransport(draft.transport);
    setCommand(draft.server.command ?? "");
    setArgs((draft.server.args ?? []).join("\n"));
    setUrl(draft.server.url ?? "");
    setEnvironment(
      Object.entries(draft.server.env ?? {})
        .map(([key, value]) => `${key}=${value}`)
        .join("\n"),
    );
    setHeaders(
      Object.entries(draft.server.headers ?? {})
        .map(([key, value]) => `${key}=${value}`)
        .join("\n"),
    );
    setWorkingDirectory(draft.server.cwd ?? "");
    setTokenVariable(draft.auth?.bearerTokenEnvVar ?? "");
    setCredentialRefs(
      Object.entries(draft.auth?.credentialRefs ?? {})
        .map(([key, value]) => `${key}=${value}`)
        .join("\n"),
    );
  };

  const selectCatalogDraft = (index: number) => {
    setCatalogDraftIndex(index);
    const draft = catalogDrafts[index]?.definition;

    if (draft) applyCatalogDraft(draft);
  };

  const selectCatalogServer = async (serverId: string) => {
    setBusy(true);

    try {
      const result = await request<{
        server: {
          name: string;
          version?: string;
          packages?: unknown;
          remotes?: unknown;
          repository?: string;
        };
        manualSetup?: string;
        draft?: McpInput;
        drafts?: {
          label: string;
          definition: McpInput;
          requirements: string[];
        }[];
      }>({ action: "catalog", serverId });

      const drafts =
        result.drafts ??
        (result.draft
          ? [
              {
                label: "Suggested connection",
                definition: result.draft,
                requirements: [],
              },
            ]
          : []);

      setCatalogDrafts(drafts);
      setCatalogDraftIndex(0);
      setCatalogSource(
        result.server.version
          ? { registry: "official", serverId, version: result.server.version }
          : undefined,
      );
      setSelected("");
      const suggested = drafts[0]?.definition;

      if (suggested) {
        applyCatalogDraft(suggested);
        setMessage("Review the suggested connection and requirements before saving.");
      } else {
        startNew();
        setName(result.server.name.split("/").pop() ?? result.server.name);
        setMessage(
          result.manualSetup ??
            "This catalog entry needs connection details. Add them before saving.",
        );
      }

      setCatalogMetadata({
        packages: result.server.packages,
        remotes: result.server.remotes,
        documentation: result.server.repository,
      });
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const importFound = async (name: string) => {
    setBusy(true);

    try {
      const importRequest: McpRequest = { action: "import", target, name, conflict };

      if (conflict === "rename" && newName) importRequest.newName = newName;

      const result = await request<{
        definition?: McpDefinition;
        draft?: McpInput;
        expectedRevision?: string;
      }>(importRequest);

      await refresh();

      if (result.definition) edit(result.definition);
      else if (result.draft) {
        edit({
          ...result.draft,
          id: result.draft.id ?? "",
          revision: result.expectedRevision ?? "",
          updatedAt: "",
        });
      }

      setMessage(
        "Review the imported draft and save it to add it to your library. No agent configuration was changed.",
      );
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const initialOpened = useRef(false);
  useEffect(() => {
    if (initialOpened.current || !initialId || !items.some((item) => item.id === initialId)) return;
    initialOpened.current = true;
    edit(items.find((item) => item.id === initialId)!);
  }, [initialId, items]);

  return (
    <section className="grid gap-4 lg:grid-cols-[240px_minmax(0,1fr)]">
      <div className="app-panel overflow-hidden">
        <div className="flex items-center justify-between border-b border-border-faint px-3 py-2">
          <h2 className="text-[13px] font-semibold">MCP library</h2>
          <button className="text-[12px] text-accent" disabled={busy} onClick={startNew}>
            New
          </button>
        </div>
        {loading ? (
          <p className="p-3 text-[12px] text-muted">Loading…</p>
        ) : items.length ? (
          items.map((item) => (
            <button
              key={item.id}
              disabled={busy}
              onClick={() => edit(item)}
              className={cn(
                "block w-full border-b border-border-faint px-3 py-2.5 text-left text-[13px] hover:bg-surface-hover",
                selected === item.id && "bg-surface-active",
              )}
            >
              <span className="block truncate font-medium text-primary">{item.name}</span>
              <span className="text-[11px] text-muted">{item.transport}</span>
              {item.source && <span className="ml-2 text-[10px] text-faint">Official catalog</span>}
            </button>
          ))
        ) : (
          <p className="p-3 text-[12px] text-muted">
            Create a reusable connection definition for your agents.
          </p>
        )}
      </div>
      <div className="app-panel space-y-4 p-4">
        <div>
          <h2 className="text-[14px] font-semibold">
            {selected ? "Edit MCP server" : "New MCP server"}
          </h2>
          <p className="mt-1 text-[12px] text-muted">
            Connection settings are saved once in the library. Credential values stay outside this
            app; enter references such as {"${env:API_TOKEN}"}.
          </p>
        </div>
        <div className="rounded-lg border border-border-faint p-3">
          <h3 className="text-[12px] font-semibold">Installed for this agent</h3>
          {found.length === 0 ? (
            <p className="mt-1 text-[11px] text-muted">No MCP connections detected.</p>
          ) : (
            found.map((entry) => (
              <div
                key={entry.name}
                className="mt-2 flex items-center gap-2 border-t border-border-faint pt-2"
              >
                <span className="min-w-0 flex-1 truncate text-[12px]">
                  {entry.name} · {entry.status}
                  {entry.override ? " · has agent-specific settings" : ""}
                </span>
                {entry.managedId && (
                  <button
                    className="text-[11px] text-danger"
                    disabled={busy}
                    onClick={() => void undeploy(entry)}
                  >
                    Remove from this agent
                  </button>
                )}
              </div>
            ))
          )}
        </div>
        <label className="block space-y-1 text-[12px] text-muted">
          Name
          <input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="block space-y-1 text-[12px] text-muted">
          Connection type
          <select
            className={inputClass}
            value={transport}
            // SAFETY: every <option> below is a McpDefinition transport.
            onChange={(e) => setTransport(e.target.value as McpDefinition["transport"])}
          >
            <option value="stdio">Local command</option>
            <option value="http">HTTP server</option>
            <option value="sse">Server-sent events</option>
          </select>
        </label>
        {transport === "stdio" ? (
          <>
            <label className="block space-y-1 text-[12px] text-muted">
              Command
              <input
                className={inputClass}
                value={command}
                onChange={(e) => setCommand(e.target.value)}
                placeholder="node"
              />
            </label>
            <label className="block space-y-1 text-[12px] text-muted">
              Arguments
              <textarea
                className={inputClass}
                rows={3}
                value={args}
                onChange={(e) => setArgs(e.target.value)}
                placeholder="One argument per line"
              />
            </label>
            <label className="block space-y-1 text-[12px] text-muted">
              Working directory
              <input
                className={inputClass}
                value={workingDirectory}
                onChange={(e) => setWorkingDirectory(e.target.value)}
              />
            </label>
            <label className="block space-y-1 text-[12px] text-muted">
              Environment file path (Cursor)
              <input
                className={inputClass}
                value={envFile}
                onChange={(event) => setEnvFile(event.target.value)}
                placeholder="Optional path; file contents stay outside the library"
              />
            </label>
            <label className="block space-y-1 text-[12px] text-muted">
              Environment values or references
              <textarea
                className={inputClass}
                rows={3}
                value={environment}
                onChange={(e) => setEnvironment(e.target.value)}
                placeholder="API_TOKEN=${env:API_TOKEN}"
              />
            </label>
          </>
        ) : (
          <>
            <label className="block space-y-1 text-[12px] text-muted">
              Server URL
              <input
                className={inputClass}
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                placeholder="https://example.com/mcp"
              />
            </label>
            <label className="block space-y-1 text-[12px] text-muted">
              Headers or environment references
              <textarea
                className={inputClass}
                rows={3}
                value={headers}
                onChange={(e) => setHeaders(e.target.value)}
                placeholder="Authorization=${env:MCP_AUTH}"
              />
            </label>
            <label className="block space-y-1 text-[12px] text-muted">
              Bearer token environment variable
              <input
                className={inputClass}
                value={tokenVariable}
                onChange={(e) => setTokenVariable(e.target.value)}
                placeholder="MCP_AUTH"
              />
            </label>
          </>
        )}
        <label className="block space-y-1 text-[12px] text-muted">
          Credential references
          <textarea
            className={inputClass}
            rows={2}
            value={credentialRefs}
            onChange={(e) => setCredentialRefs(e.target.value)}
            placeholder="provider=env:PROVIDER_CREDENTIAL"
          />
          <span className="block text-[11px]">
            Reference names only. Credential values are never entered here.
          </span>
        </label>
        <label className="flex items-start gap-2 text-[12px] text-muted">
          <input
            type="checkbox"
            checked={deploy}
            disabled={!deploymentSupported}
            onChange={(e) => setDeploy(e.target.checked)}
          />
          <span>
            After saving, prepare a deployment to this agent. Nothing is applied until I review the
            preview.
          </span>
        </label>
        <div className="flex flex-wrap gap-2">
          <button
            className="app-button-primary"
            disabled={
              busy || !name.trim() || (transport === "stdio" ? !command.trim() : !url.trim())
            }
            onClick={() => void save()}
          >
            {busy ? "Saving…" : "Save definition"}
          </button>
          {selected && (
            <>
              <button
                className="app-button-secondary"
                disabled={busy || unavailable || !deploymentSupported}
                onClick={() => void createPreview()}
              >
                Preview deployment
              </button>
              <LibraryRemoveButton key={selected} busy={busy} onRemove={remove} />
            </>
          )}
        </div>
        {(unavailable || !deploymentSupported) && (
          <p className="text-[12px] text-muted">
            {targetCapability?.limitations.join(" ") ||
              "This connection type or its credential references are unsupported for the selected agent and scope."}
          </p>
        )}
        {selected && (
          <div className="flex flex-wrap items-center gap-2 text-[12px] text-muted">
            <span>If a connection with the same name exists:</span>
            <select
              aria-label="Deployment conflict"
              className="app-input py-1"
              value={conflict}
              // SAFETY: every <option> below is a McpConflict value.
              onChange={(e) => setConflict(e.target.value as typeof conflict)}
            >
              <option value="keep">Keep existing</option>
              <option value="replace">Replace</option>
              <option value="rename">Rename incoming</option>
            </select>
            {conflict === "rename" && (
              <input
                className="app-input"
                aria-label="Deployment name"
                placeholder="New connection name"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
              />
            )}
          </div>
        )}
        {mcpRecoveries.map((recovery) => (
          <div key={recovery.id} className="rounded-lg border border-border-faint p-3 text-[12px]">
            <p>{recovery.message}</p>
            <button
              className="app-button-secondary mt-2"
              disabled={busy}
              onClick={() => void recoverMcp(recovery.id)}
            >
              Recover interrupted deployment
            </button>
          </div>
        ))}
        {targetScope?.supported && (
          <label className="block space-y-1 text-[12px] text-muted">
            Optional settings for this agent
            <textarea
              className={`${inputClass} font-mono text-[11px]`}
              rows={3}
              value={targetOverride}
              onChange={(e) => setTargetOverride(e.target.value)}
              placeholder={'{"command":"node"}'}
            />
            <span className="block text-[11px]">
              These values override the saved defaults for this agent only. Use environment
              references for credentials.
            </span>
          </label>
        )}
        {targetCapability?.limitations.map((limitation) => (
          <p key={limitation} className="text-[11px] text-muted">
            {limitation}
          </p>
        ))}
        {message && (
          <p role="status" className="text-[12px] text-muted">
            {message}
          </p>
        )}
        {preview && (
          <div className="rounded-lg border border-border-faint p-3">
            <h3 className="text-[13px] font-semibold">Review deployment</h3>
            {preview.changes.map((change, i) => (
              <div key={`${change.name}-${i}`} className="mt-2 text-[12px]">
                <p className="font-medium">
                  {change.kind}: {change.name}
                </p>
                {change.beforeSummary != null && (
                  <div className="text-muted">
                    Current:
                    <pre className="mt-1 overflow-auto whitespace-pre-wrap break-all text-[11px]">
                      {JSON.stringify(change.beforeSummary, null, 2)}
                    </pre>
                  </div>
                )}
                {change.afterSummary != null && (
                  <div className="text-muted">
                    Will be:
                    <pre className="mt-1 overflow-auto whitespace-pre-wrap break-all text-[11px]">
                      {JSON.stringify(change.afterSummary, null, 2)}
                    </pre>
                  </div>
                )}
                {change.warnings.map((warning) => (
                  <p key={warning} className="text-amber-600">
                    {warning}
                  </p>
                ))}
              </div>
            ))}
            <button
              className="app-button-primary mt-3"
              disabled={busy}
              onClick={() => void apply()}
            >
              Apply reviewed changes
            </button>
            <button className="app-button-secondary ml-2 mt-3" onClick={() => setPreview(null)}>
              Cancel
            </button>
          </div>
        )}
        <details className="border-t border-border-faint pt-3">
          <summary className="cursor-pointer text-[12px] font-medium">
            Import from this agent or browse the catalog
          </summary>
          <div className="mt-3 space-y-3">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-[12px] text-muted">If the name exists:</span>
              <select
                className="app-input py-1"
                value={conflict}
                // SAFETY: every <option> below is a McpConflict value.
                onChange={(e) => setConflict(e.target.value as typeof conflict)}
              >
                <option value="keep">Keep existing</option>
                <option value="replace">Replace existing</option>
                <option value="rename">Import as another name</option>
              </select>
              {conflict === "rename" && (
                <input
                  className="app-input"
                  placeholder="New name"
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                />
              )}
            </div>
            <div className="space-y-1">
              {found.map((entry) => (
                <div
                  key={entry.name}
                  className="flex items-center gap-2 rounded border border-border-faint px-2 py-1.5"
                >
                  <span className="min-w-0 flex-1 truncate text-[12px]">
                    {entry.name} · {entry.status}
                  </span>
                  <button
                    className="text-[11px] text-accent"
                    disabled={busy || !deploymentSupported}
                    onClick={() => void importFound(entry.name)}
                  >
                    Review import
                  </button>
                </div>
              ))}
              {found.length === 0 && (
                <p className="text-[11px] text-muted">
                  No existing connections found for this agent.
                </p>
              )}
            </div>
            <div className="flex gap-2">
              <input
                className={inputClass}
                placeholder="Search public catalog"
                value={catalogQuery}
                onChange={(e) => setCatalogQuery(e.target.value)}
              />
              <button
                className="app-button-secondary"
                disabled={busy}
                onClick={() => {
                  setCatalog([]);
                  setCatalogCursor(null);
                  void browseCatalog();
                }}
              >
                Search
              </button>
            </div>
            {catalog.map((server) => (
              <div
                key={server.name}
                className="flex items-start gap-3 border-t border-border-faint py-2"
              >
                <div className="min-w-0 flex-1">
                  <p className="text-[12px] font-medium">{server.title || server.name}</p>
                  <p className="text-[11px] text-muted">
                    {server.description || server.repository || server.name}
                  </p>
                </div>
                <button
                  className="text-[11px] text-accent"
                  disabled={busy}
                  onClick={() => void selectCatalogServer(server.name)}
                >
                  Use as starting point
                </button>
              </div>
            ))}
            {catalogCursor && (
              <button
                className="app-button-secondary mt-2"
                disabled={busy}
                onClick={() => void browseCatalog(catalogCursor)}
              >
                Load more catalog entries
              </button>
            )}
          </div>
        </details>
      </div>
      {catalogMetadata != null && (
        <details className="rounded-lg border border-border-faint p-3 text-[12px] lg:col-span-2">
          <summary>Registry connection metadata for manual setup</summary>
          <pre className="mt-2 max-h-60 overflow-auto whitespace-pre-wrap break-all">
            {JSON.stringify(catalogMetadata, null, 2)}
          </pre>
        </details>
      )}
      {catalogDrafts.length > 1 && (
        <label className="block space-y-1 text-[12px] text-muted">
          Setup option
          <select
            className={inputClass}
            value={catalogDraftIndex}
            onChange={(event) => selectCatalogDraft(Number(event.target.value))}
          >
            {catalogDrafts.map((draft, index) => (
              <option key={`${draft.label}-${index}`} value={index}>
                {draft.label}
              </option>
            ))}
          </select>
        </label>
      )}
      {catalogDrafts[catalogDraftIndex]?.requirements.map((requirement) => (
        <p
          key={requirement}
          className="rounded border border-border-faint px-3 py-2 text-[11px] text-muted"
        >
          {requirement}
        </p>
      ))}
    </section>
  );
}

function InstructionsLibrary({
  initialId,
  create,
  hostId,
  scope,
  target,
  unavailable,
}: {
  initialId: string;
  create: boolean;
  hostId: null | string;
  scope: WorkspaceScope;
  target: McpTarget;
  unavailable: boolean;
}) {
  type Item = {
    id: string;
    name: string;
    description?: string;
    revision: string;
    updated_at: string;
    files: { path: string; size?: number }[];
  };

  type FileItem = {
    path: string;
    kind: "native" | "nested" | "override" | "root";
    exists: boolean;
    content?: string;
    managed: boolean;
    applicable?: boolean;
    symlink_target?: string;
    revision?: string;
    conflict?: boolean;
  };

  type Reference = {
    source_path: string;
    target_path: string;
    kind: string;
    line: number;
    loading?: "eager" | "on_demand";
  };

  type Change = {
    path: string;
    status: "conflict" | "create" | "replace" | "unchanged";
    content?: string;
    conflict?: string;
    previous?: string;
  };

  const agentKey = target.agentKey;
  const [items, setItems] = useState<Item[]>([]);
  const [selected, setSelected] = useState("");
  const [files, setFiles] = useState<Record<string, string>>({});
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [path, setPath] = useState("AGENTS.md");
  const [fileMode, setFileMode] = useState<"agent" | "shared">("shared");

  const [recoveries, setRecoveries] = useState<
    {
      transaction_id: string;
      kind: string;
      status: string;
      files?: { path: string }[];
      failed?: { path: string; error: string }[];
      remaining?: { path: string; error: string }[];
    }[]
  >([]);

  const [scopeDir, setScopeDir] = useState("");
  const [deploymentDir, setDeploymentDir] = useState("");
  const [deploymentRefresh, setDeploymentRefresh] = useState(0);
  const [editContent, setEditContent] = useState("");
  const [loadedRevision, setLoadedRevision] = useState<null | string>(null);
  const [diskFiles, setDiskFiles] = useState<FileItem[]>([]);
  const [references, setReferences] = useState<Reference[]>([]);

  const [excluded, setExcluded] = useState<{ path: string; reason: string }[]>([]);

  const [warnings, setWarnings] = useState<string[]>([]);

  const [preview, setPreview] = useState<{
    preview_id: string;
    changes: Change[];
    warnings: string[];
  } | null>(null);

  const [resolutions, setResolutions] = useState<Record<string, "keep_local" | "take_library">>({});

  const [notice, setNotice] = useState("");
  const [operationIssues, setOperationIssues] = useState<{ path: string; error: string }[]>([]);
  const [busy, setBusy] = useState(false);

  const [libraryTab, setLibraryTab] = useState<"files" | "library">(
    create || initialId || scope.kind === "library" ? "library" : "files",
  );

  const [worktrees, setWorktrees] = useState<
    { name: string; path: string; branch?: string; is_main: boolean }[]
  >([]);

  const [worktree, setWorktree] = useState("");
  const { tools } = useApp();
  const tool = tools.find((candidate) => candidate.key === agentKey);

  const scopeRestriction =
    agentKey === "cursor" && !target.projectId
      ? "Cursor global instructions are configured in Cursor settings."
      : null;

  const blockedReason = scope.kind === "library" ? null : scopeRestriction;

  const instructionTarget: InstructionTarget = { agent_key: agentKey };

  if (target.projectId) instructionTarget.project_id = target.projectId;
  // Worktrees scope file browsing and editing; bundle updates stay on the main project.
  const fileTarget: InstructionTarget = { ...instructionTarget };

  if (worktree) fileTarget.worktree = worktree;

  const nativeInstructionPath = () =>
    ({
      claude_code: "CLAUDE.md",
      codex: "AGENTS.override.md",
      antigravity: "GEMINI.md",
      hermes: target.projectId ? ".hermes.md" : "SOUL.md",
      cursor: ".cursor/rules/instructions.mdc",
    })[agentKey] ?? "AGENTS.md";

  const request = <T,>(action: InstructionsRequest) =>
    invokeHost<T>(hostId, "instructions_request", { request: action });

  const refreshItems = async () => {
    try {
      const result = await request<{ items: Item[] }>({ action: "list" });
      setItems(result.items);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    }
  };

  const scan = async (directory = scopeDir) => {
    if (unavailable || blockedReason || scope.kind === "library") return;
    setBusy(true);

    try {
      const scanRequest: InstructionsRequest = { action: "scan", target: fileTarget };

      if (directory) scanRequest.include_dirs = [directory];

      const result = await request<{
        files: FileItem[];
        references: Reference[];
        excluded: { path: string; reason: string }[];
        warnings: string[];
      }>(scanRequest);

      setDiskFiles(result.files);
      setReferences(result.references ?? []);
      setExcluded(result.excluded ?? []);
      setWarnings(result.warnings ?? []);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    let current = true;
    void invokeHost<{ items: Item[] }>(hostId, "instructions_request", {
      request: { action: "list" },
    })
      .then((result) => {
        if (current) setItems(result.items);
      })
      .catch((error) => {
        if (current) setNotice(error instanceof Error ? error.message : String(error));
      });

    if (!unavailable && !blockedReason && scope.kind !== "library") {
      const scanTarget: InstructionTarget = { agent_key: agentKey };

      if (target.projectId) scanTarget.project_id = target.projectId;

      if (worktree) scanTarget.worktree = worktree;
      const scanRequest: InstructionsRequest = { action: "scan", target: scanTarget };

      if (scopeDir) scanRequest.include_dirs = [scopeDir];
      void invokeHost<{
        files: FileItem[];
        references: Reference[];
        excluded: { path: string; reason: string }[];
        warnings: string[];
      }>(hostId, "instructions_request", { request: scanRequest })
        .then((result) => {
          if (current) {
            setDiskFiles(result.files);
            setReferences(result.references ?? []);
            setExcluded(result.excluded ?? []);
            setWarnings(result.warnings ?? []);
          }
        })
        .catch((error) => {
          if (current) setNotice(error instanceof Error ? error.message : String(error));
        });
    }

    return () => {
      current = false;
    };
  }, [
    hostId,
    scope.kind,
    scope.projectId,
    agentKey,
    target.projectId,
    scopeDir,
    worktree,
    unavailable,
    blockedReason,
  ]);
  useEffect(() => {
    if (!target.projectId) return;
    let current = true;
    void invokeHost<{ items: typeof worktrees }>(hostId, "instructions_request", {
      request: { action: "worktrees", project_id: target.projectId },
    })
      .then((result) => {
        if (current) setWorktrees(result.items);
      })
      .catch((error) => {
        if (current) setNotice(error instanceof Error ? error.message : String(error));
      });

    return () => {
      current = false;
    };
  }, [hostId, target.projectId]);

  const newBundle = () => {
    setPreview(null);
    setSelected("");
    setName("");
    setDescription("");

    const initialPath = fileMode === "shared" ? "AGENTS.md" : nativeInstructionPath();

    setPath(initialPath);
    setFiles({ [initialPath]: "" });
  };

  const editBundle = async (id: string) => {
    setPreview(null);
    setBusy(true);
    setNotice("");

    try {
      const result = await request<{
        item: { files: Record<string, string> } & Item;
      }>({ action: "get", id });

      setSelected(id);
      setName(result.item.name);
      setDescription(result.item.description ?? "");
      setFiles(result.item.files);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const saveBundle = async () => {
    setPreview(null);
    setBusy(true);
    setNotice("");

    try {
      const current = items.find((item) => item.id === selected);

      const saveRequest: InstructionsRequest = { action: "save", name: name.trim(), files };

      if (selected) saveRequest.id = selected;

      if (description.trim()) saveRequest.description = description.trim();

      if (current) saveRequest.expected_revision = current.revision;
      const result = await request<{ item: Item }>(saveRequest);

      await refreshItems();
      setSelected(result.item.id);
      setNotice("Bundle saved.");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const openDiskFile = async (file: FileItem) => {
    setPath(file.path);
    setEditContent("");
    setLoadedRevision(null);
    setBusy(true);

    try {
      const result = await request<{
        path: string;
        content: string;
        revision: string;
      }>({
        action: "read",
        target: fileTarget,
        path: file.path,
      });

      setEditContent(result.content);
      setLoadedRevision(result.revision);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const saveDiskFile = async () => {
    if (!loadedRevision) return;
    setBusy(true);

    try {
      const result = await request<{ path: string; revision: string }>({
        action: "write",
        target: fileTarget,
        path,
        content: editContent,
        expected_revision: loadedRevision,
      });

      setLoadedRevision(result.revision);
      await scan();
      setNotice("File saved.");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const previewBundle = async () => {
    if (!selected || unavailable || blockedReason || scopeRestriction) return;
    setBusy(true);
    setNotice("");

    try {
      const previewTarget: InstructionTarget = { ...instructionTarget };

      if (deploymentDir) previewTarget.relative_dir = deploymentDir;

      const result = await request<{
        preview_id: string;
        changes: Change[];
        warnings: string[];
      }>({ action: "preview", target: previewTarget, instruction_id: selected });

      setPreview(result);
      setResolutions({});
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const applyBundle = async () => {
    if (!preview) return;
    setBusy(true);

    try {
      const result = await request<{
        applied: string[];
        failed: { path: string; error: string }[];
        partial: boolean;
      }>({ action: "apply", preview_id: preview.preview_id, resolutions });

      setOperationIssues(result.failed);
      setNotice(
        result.failed.length
          ? `Applied ${result.applied.length}; ${result.failed.length} file(s) need attention.`
          : result.partial
            ? "The operation was interrupted. Recover it before retrying."
            : `Applied ${result.applied.length} file(s).`,
      );
      setPreview(null);
      setDeploymentRefresh((value) => value + 1);
      await scan();
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const recover = async () => {
    setBusy(true);
    setNotice("");

    try {
      const result = await request<{ items: typeof recoveries }>({
        action: "recover",
      });

      setRecoveries(result.items);
      setOperationIssues(result.items.flatMap((item) => item.remaining ?? []));
      setNotice("Recovery finished. Review any remaining issues below.");
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const removeBundle = async () => {
    if (!selected) return;
    setBusy(true);

    try {
      await request({ action: "remove", id: selected, detach: true });
      newBundle();
      setDeploymentRefresh((value) => value + 1);
      await refreshItems();
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  const capabilities = new Map([
    ["claude_code", "Claude Code reads instruction files from the selected directory."],
    ["codex", "Codex reads AGENTS.md files in the selected directory."],
    [
      "antigravity",
      "Antigravity reads workspace instruction files; secret references are unavailable.",
    ],
    ["hermes", "Hermes supports global instruction files."],
    ["cursor", "Cursor project instructions are managed as files."],
  ]);

  useEffect(() => {
    if (!initialId) return;
    let current = true;
    void invokeHost<{
      item: { name: string; description?: string; files: Record<string, string> };
    }>(hostId, "instructions_request", { request: { action: "get", id: initialId } })
      .then((result) => {
        if (!current) return;
        setSelected(initialId);
        setName(result.item.name);
        setDescription(result.item.description ?? "");
        setFiles(result.item.files);
      })
      .catch((error) => {
        if (current) setNotice(String(error));
      });

    return () => {
      current = false;
    };
  }, [hostId, initialId]);

  return (
    <section className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-[15px] font-semibold">Instructions</h2>
          <p className="mt-1 text-[12px] text-muted">
            Save reusable instruction bundles, review changes, or edit files in this workspace.
          </p>
        </div>
        <button
          className="app-button-secondary"
          onClick={() => void scan()}
          disabled={busy || unavailable}
        >
          Refresh workspace
        </button>
      </div>
      {tool && (
        <p className="text-[12px] text-muted">
          {tool.display_name}:{" "}
          {scopeRestriction ??
            capabilities.get(agentKey) ??
            "Instruction files are available for this agent."}
        </p>
      )}
      {scope.kind === "library" && (
        <div className="space-y-2 rounded-lg border border-border-faint px-3 py-2 text-[11px] text-muted">
          <p>
            Bundles contain complete files. Choose shared AGENTS.md guidance or an agent-specific
            entry file. Claude Code checks CLAUDE.md first and uses AGENTS.md as a fallback.
          </p>
        </div>
      )}
      <div className="space-y-2 text-[12px] text-muted">
        <p>Interrupted changes can be recovered while preserving newer edits.</p>
        <button className="app-button-secondary" disabled={busy} onClick={() => void recover()}>
          Recover interrupted changes
        </button>
        {recoveries.map((item) => (
          <div key={item.transaction_id}>
            <p>
              {item.kind}: {item.status}
            </p>
            {item.failed?.map((issue, index) => (
              <p key={`${issue.path}:${index}`} className="break-all">
                {issue.path}: {issue.error}
              </p>
            ))}
          </div>
        ))}
      </div>
      {blockedReason && <div className="app-panel p-3 text-[12px] text-muted">{blockedReason}</div>}
      {!blockedReason && worktrees.length > 1 && (
        <div
          role="tablist"
          aria-label="Worktrees"
          className="flex flex-wrap items-center gap-2 border-b border-border-faint pb-2"
        >
          {worktrees.map((entry) => {
            const value = entry.is_main ? "" : entry.path;

            return (
              <button
                key={entry.path}
                role="tab"
                aria-selected={worktree === value}
                title={entry.path}
                className={cn(
                  "rounded-lg px-3 py-1.5 text-[12px]",
                  worktree === value
                    ? "bg-surface-active text-primary"
                    : "text-muted hover:bg-surface-hover hover:text-primary",
                )}
                onClick={() => {
                  setWorktree(value);
                  setPath("");
                  setEditContent("");
                  setLoadedRevision(null);
                }}
              >
                {entry.is_main ? "main" : entry.name}
                {entry.branch && <span className="text-muted">{` (${entry.branch})`}</span>}
              </button>
            );
          })}
        </div>
      )}
      {!blockedReason && (
        <div className="flex flex-wrap items-center gap-2 border-b border-border-faint pb-2">
          <button
            className={cn(
              "rounded-lg px-3 py-1.5 text-[12px]",
              libraryTab === "library" ? "bg-surface-active text-primary" : "text-muted",
            )}
            onClick={() => setLibraryTab("library")}
          >
            Bundle library
          </button>
          {scope.kind !== "library" && (
            <button
              className={cn(
                "rounded-lg px-3 py-1.5 text-[12px]",
                libraryTab === "files" ? "bg-surface-active text-primary" : "text-muted",
              )}
              onClick={() => setLibraryTab("files")}
            >
              Workspace files
            </button>
          )}
          {libraryTab === "files" && (
            <label className="ml-auto flex items-center gap-2 text-[12px] text-muted">
              Include directory
              <input
                className="app-input py-1"
                aria-label="Include directory"
                placeholder="(workspace root)"
                value={scopeDir}
                onChange={(event) => setScopeDir(event.target.value)}
                onBlur={() => {
                  setPath("");
                  setEditContent("");
                  void scan();
                }}
              />
            </label>
          )}
        </div>
      )}
      {!blockedReason && libraryTab === "library" && (
        <div className="grid gap-4 lg:grid-cols-[230px_minmax(0,1fr)]">
          <div className="app-panel overflow-hidden">
            <div className="flex items-center justify-between border-b border-border-faint px-3 py-2">
              <h3 className="text-[13px] font-semibold">Saved bundles</h3>
              <button className="text-[12px] text-accent" disabled={busy} onClick={newBundle}>
                New
              </button>
            </div>
            {items.map((item) => (
              <button
                key={item.id}
                className={cn(
                  "block w-full border-b border-border-faint px-3 py-2 text-left",
                  selected === item.id ? "bg-surface-active" : "hover:bg-surface-hover",
                )}
                disabled={busy}
                onClick={() => void editBundle(item.id)}
              >
                <span className="block text-[13px] font-medium">{item.name}</span>
                <span className="text-[11px] text-muted">{item.files.length} file(s)</span>
              </button>
            ))}
          </div>
          <div className="app-panel space-y-3 p-4">
            <h3 className="text-[14px] font-semibold">
              {selected ? "Edit instruction bundle" : "New instruction bundle"}
            </h3>
            {target.projectId && (
              <label className="block space-y-1 text-[12px] text-muted">
                Deployment subdirectory
                <input
                  className={inputClass}
                  value={deploymentDir}
                  placeholder="Project root"
                  onChange={(event) => {
                    setDeploymentDir(event.target.value);
                    setPreview(null);
                  }}
                />
              </label>
            )}
            <label className="block space-y-1 text-[12px] text-muted">
              Name
              <input
                className={inputClass}
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            <label className="block space-y-1 text-[12px] text-muted">
              Description
              <input
                className={inputClass}
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </label>
            {!selected && (
              <label className="block space-y-1 text-[12px] text-muted">
                Bundle style
                <select
                  className={inputClass}
                  value={fileMode}
                  onChange={(event) => {
                    const mode = event.target.value === "agent" ? "agent" : "shared";
                    setFileMode(mode);

                    const nextPath = mode === "shared" ? "AGENTS.md" : nativeInstructionPath();

                    setPath(nextPath);
                    setFiles({ [nextPath]: "" });
                  }}
                >
                  {" "}
                  <option value="shared">Shared instructions (AGENTS.md)</option>
                  <option value="agent">Agent-specific entry file</option>
                </select>
              </label>
            )}
            {agentKey === "hermes" && fileMode === "agent" && (
              <p className="text-[11px] text-muted">
                SOUL.md defines Hermes identity and behavior, rather than general project
                instructions.
              </p>
            )}
            <div className="space-y-2">
              {Object.entries(files).map(([filePath, content]) => (
                <div key={filePath} className="rounded-lg border border-border-faint p-2">
                  <div className="flex items-center gap-2">
                    <span className="min-w-0 flex-1 truncate text-[12px] font-medium">
                      {filePath}
                    </span>
                    <button
                      className="text-[11px] text-danger"
                      onClick={() =>
                        setFiles((old) => {
                          const next = { ...old };
                          delete next[filePath];

                          return next;
                        })
                      }
                    >
                      Remove
                    </button>
                  </div>
                  <MarkdownEditor
                    ariaLabel={`${filePath} content`}
                    rows={7}
                    value={content}
                    onChange={(next) => setFiles((old) => ({ ...old, [filePath]: next }))}
                  />
                </div>
              ))}
            </div>
            <div className="flex gap-2">
              <input
                aria-label="New file path"
                className={inputClass}
                placeholder="docs/guide.md"
                value={path}
                onChange={(e) => setPath(e.target.value)}
              />
              <button
                className="app-button-secondary"
                onClick={() => {
                  if (path.trim() && !files[path.trim()])
                    setFiles((old) => ({ ...old, [path.trim()]: "" }));
                }}
              >
                Add file
              </button>
            </div>
            <div className="flex flex-wrap gap-2">
              <button
                className="app-button-primary"
                disabled={busy || !name.trim() || !Object.keys(files).length}
                onClick={() => void saveBundle()}
              >
                Save bundle
              </button>
              {selected && (
                <>
                  <button
                    className="app-button-secondary"
                    disabled={busy || !!scopeRestriction || !!worktree}
                    title={
                      worktree
                        ? "Bundle updates apply to the main project. Switch to the main worktree."
                        : undefined
                    }
                    onClick={() => void previewBundle()}
                  >
                    Review updates
                  </button>
                  <LibraryRemoveButton key={selected} busy={busy} onRemove={removeBundle} />
                </>
              )}
            </div>
          </div>
        </div>
      )}
      {!blockedReason && scope.kind !== "library" && libraryTab === "files" && (
        <div className="grid gap-4 lg:grid-cols-[260px_minmax(0,1fr)]">
          <div className="app-panel overflow-hidden">
            <h3 className="border-b border-border-faint px-3 py-2 text-[13px] font-semibold">
              Files in this scope
            </h3>
            <InstructionFileTree
              files={diskFiles}
              selectedPath={path}
              onOpen={(file) => void openDiskFile(file)}
            />
            {excluded.length > 0 && (
              <details className="border-t border-border-faint px-3 py-2 text-[11px] text-muted">
                <summary className="cursor-pointer">Skipped ({excluded.length})</summary>
                {excluded.map((entry) => (
                  <p key={entry.path} title={entry.reason} className="mt-1 break-all">
                    {entry.path}: {entry.reason}
                  </p>
                ))}
              </details>
            )}
          </div>
          <div className="app-panel space-y-3 p-4">
            <div className="flex items-center justify-between">
              <h3 className="text-[13px] font-semibold">{path}</h3>
              <div className="flex gap-2">
                <button
                  className="app-button-primary"
                  disabled={busy || !loadedRevision}
                  onClick={() => void saveDiskFile()}
                >
                  Save file
                </button>
                <button
                  className="app-button-secondary"
                  disabled={busy || !path || !loadedRevision}
                  onClick={() => {
                    setSelected("");
                    setName(
                      path
                        .split("/")
                        .pop()
                        ?.replace(/\.mdc?$/, "") ?? "Instructions",
                    );
                    setDescription("");
                    setFiles({ [path]: editContent });
                    setPreview(null);
                    setLibraryTab("library");
                    setNotice(
                      "Review this new bundle and save it. The workspace file stays independent.",
                    );
                  }}
                >
                  Use as new bundle
                </button>
              </div>
            </div>
            {path && (
              <MarkdownEditor
                key={`${worktree}:${path}`}
                ariaLabel="Instruction file content"
                rows={18}
                value={editContent}
                onChange={setEditContent}
              />
            )}
            {references.length > 0 && (
              <div>
                <h4 className="text-[12px] font-semibold">Referenced documents</h4>
                {references.map((reference, i) => (
                  <p
                    key={`${reference.source_path}-${reference.line}-${i}`}
                    className="mt-1 break-all text-[11px] text-muted"
                  >
                    {reference.source_path}:{reference.line} → {reference.target_path} (
                    {reference.kind}
                    {reference.loading
                      ? ` · ${reference.loading === "eager" ? "native import" : "on-demand reference"}`
                      : ""}
                    )
                  </p>
                ))}
              </div>
            )}
            {warnings.map((warning) => (
              <p key={warning} className="text-[11px] text-amber-600">
                {warning}
              </p>
            ))}
          </div>
        </div>
      )}
      {!unavailable && !scopeRestriction && (
        <InstructionDeployments
          key={`${target.agentKey}:${target.projectId ?? "global"}:${deploymentRefresh}`}
          hostId={hostId}
          target={target}
          names={Object.fromEntries(items.map((item) => [item.id, item.name]))}
          onChanged={() => void scan()}
        />
      )}
      {notice && (
        <p role="status" className="text-[12px] text-muted">
          {notice}
        </p>
      )}
      {operationIssues.map((issue, index) => (
        <p
          key={`${issue.path}:${index}`}
          role="alert"
          className="break-all text-[12px] text-danger"
        >
          {issue.path}: {issue.error}
        </p>
      ))}
      {preview && (
        <div className="app-panel space-y-3 p-4">
          <h3 className="text-[14px] font-semibold">Review bundle changes</h3>
          {preview.warnings.map((warning) => (
            <p key={warning} className="text-[12px] text-amber-600">
              {warning}
            </p>
          ))}
          {preview.changes.map((change) => (
            <div key={change.path} className="border-t border-border-faint pt-3">
              <p className="text-[12px] font-medium">
                {change.status}: {change.path}
              </p>
              {change.conflict && (
                <p className="mt-1 text-[11px] text-amber-600">{change.conflict}</p>
              )}
              {change.status === "conflict" && (
                <div className="mt-2 flex gap-3 text-[11px]">
                  <label>
                    <input
                      type="radio"
                      name={`conflict-${change.path}`}
                      checked={resolutions[change.path] === "keep_local"}
                      onChange={() =>
                        setResolutions((old) => ({
                          ...old,
                          [change.path]: "keep_local",
                        }))
                      }
                    />{" "}
                    Keep my file
                  </label>
                  <label>
                    <input
                      type="radio"
                      name={`conflict-${change.path}`}
                      checked={resolutions[change.path] === "take_library"}
                      onChange={() =>
                        setResolutions((old) => ({
                          ...old,
                          [change.path]: "take_library",
                        }))
                      }
                    />{" "}
                    Use bundle version
                  </label>
                </div>
              )}
              {change.previous && (
                <details className="mt-2 text-[11px]">
                  <summary>Current content</summary>
                  <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded bg-background p-2">
                    {change.previous}
                  </pre>
                </details>
              )}
              {change.content && (
                <details className="mt-2 text-[11px]">
                  <summary>Incoming content</summary>
                  <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded bg-background p-2">
                    {change.content}
                  </pre>
                </details>
              )}
            </div>
          ))}
          <button
            className="app-button-primary"
            disabled={
              busy ||
              preview.changes.some(
                (change) => change.status === "conflict" && !resolutions[change.path],
              )
            }
            onClick={() => void applyBundle()}
          >
            Apply reviewed changes
          </button>
          <button className="app-button-secondary ml-2" onClick={() => setPreview(null)}>
            Cancel
          </button>
        </div>
      )}
    </section>
  );
}

/** Keep confirmation inside the desktop webview; native confirm is unreliable. */
function LibraryRemoveButton({ busy, onRemove }: { busy: boolean; onRemove: () => Promise<void> }) {
  const [confirming, setConfirming] = useState(false);

  if (!confirming)
    return (
      <button className="app-button-secondary" disabled={busy} onClick={() => setConfirming(true)}>
        Remove from library
      </button>
    );

  return (
    <div className="rounded-lg border border-border-faint p-3 text-[12px]">
      <p>
        Remove this library item and detach its saved deployments? Existing agent files and
        configuration will stay in place.
      </p>
      <div className="mt-2 flex gap-2">
        <button
          className="app-button-secondary text-danger"
          disabled={busy}
          onClick={() => void onRemove()}
        >
          Remove and detach
        </button>
        <button
          className="app-button-secondary"
          disabled={busy}
          onClick={() => setConfirming(false)}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

function InstructionDeployments({
  hostId,
  target,
  names,
  onChanged,
}: {
  hostId: null | string;
  target: McpTarget;
  names: Record<string, string>;
  onChanged: () => void;
}) {
  type Deployment = {
    id: string;
    instruction_id: string;
    target: {
      agent_key: string;
      project_id?: null | string;
      relative_dir?: null | string;
    };
    files: { path: string }[];
  };

  const [items, setItems] = useState<Deployment[]>([]);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let current = true;
    void invokeHost<{ items: Deployment[] }>(hostId, "instructions_request", {
      request: { action: "deployments" },
    })
      .then((result) => {
        if (current)
          setItems(
            result.items.filter(
              (item) =>
                item.target.agent_key === target.agentKey &&
                (item.target.project_id ?? null) === (target.projectId ?? null),
            ),
          );
      })
      .catch((error) => {
        if (current) setMessage(String(error));
      });

    return () => {
      current = false;
    };
  }, [hostId, target.agentKey, target.projectId]);

  const undeploy = async (id: string) => {
    setBusy(true);

    try {
      const result = await invokeHost<{ removed: string[] }>(hostId, "instructions_request", {
        request: { action: "undeploy", deployment_id: id },
      });

      setItems((previous) => previous.filter((item) => item.id !== id));
      setMessage(
        `Detached deployment and removed ${result.removed.length} unchanged file(s). Locally edited files were preserved.`,
      );
      onChanged();
    } catch (error) {
      setMessage(String(error));
    } finally {
      setBusy(false);
    }
  };

  if (!items.length && !message) return null;

  return (
    <div className="app-panel space-y-2 p-3 text-[12px]">
      <h3 className="font-semibold">Managed instruction deployments</h3>
      {items.map((item) => (
        <div key={item.id} className="flex flex-wrap items-center gap-2">
          <span className="flex-1">
            {names[item.instruction_id] ?? item.instruction_id} ·{" "}
            {item.target.relative_dir || "Scope root"} · {item.files.length} file(s)
          </span>
          <button
            className="app-button-secondary"
            disabled={busy}
            onClick={() => void undeploy(item.id)}
          >
            Remove unchanged files and detach
          </button>
        </div>
      ))}
      {message && <p role="status">{message}</p>}
    </div>
  );
}
