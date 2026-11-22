---
name: agentation
description: Add Agentation visual feedback toolbar to a Next.js project
---

# Agentation Setup

Set up the toolbar and, when requested, connect it to the user's coding agent.
A visible toolbar and a healthy MCP server do not prove that browser feedback
reaches the agent. Verify that final step before calling setup complete.

## Steps

1. **Inspect the project**
   - Read the project's instructions and detect its package manager.
   - Look for `agentation` and an existing `<Agentation>` mount before installing.
   - If already mounted, inspect its `endpoint` and current MCP setup. Do not
     exit just because the component exists, and do not add a duplicate mount.
   - Install `agentation` if missing, using the existing lockfile's package manager.

2. **Choose the connection mode from the request**
   - For MCP or live agent feedback, use the same HTTP server URL as the agent.
     The default local server is `http://localhost:4747`; preserve an existing
     custom port or proxy URL. Never assume the browser and agent use the same
     machine when the project is remote.
   - For manual copy/paste only, omit `endpoint` and explain that feedback remains
     in this browser. Do not silently enable a server connection.
   - If the intended mode is unclear, ask one brief question while inspecting
     the existing project. Keep working on independent setup checks.

3. **Add or update the component**

   For a Next.js App Router project, render in the root layout's body after
   children. For Pages Router, render after `Component` in `pages/_app`.
   Preserve any existing callbacks and options.

   ```tsx
   import { Agentation } from "agentation";

   // Use the actual server URL selected above for MCP sync:
   {process.env.NODE_ENV === "development" && (
     <Agentation endpoint="http://localhost:4747" />
   )}
   ```

   The package provides its own client boundary. Keep the application's root
   layout server-rendered unless the project has another reason to change it.

4. **Configure the agent when MCP was requested**
   - Use the project's existing MCP client and configuration conventions.
   - The launch command must include the `server` subcommand:
     `npx -y agentation-mcp server`.
   - For supported agents, `npx add-mcp "npx -y agentation-mcp server"` is one
     setup option. For Claude Code, use
     `claude mcp add agentation -- npx -y agentation-mcp server` or the init wizard.
   - Apply the same custom port to the server command and component endpoint.
   - Restart or reconnect the agent when its configuration requires it. Do not
     create a second HTTP server on a port already owned by the working server.

5. **Verify the complete connection**
   - Run `npx agentation-mcp doctor` for service diagnostics. For a custom URL,
     use `doctor --http-url <url>` when supported by the installed version.
   - Open the app and create one clearly identified test annotation through the
     toolbar. Read it with `agentation_get_all_pending` or session-scoped tools.
   - Check the exact test comment and page, not just a nonzero count or health
     response. Leave unrelated annotations alone.
   - If annotations are empty, check the component endpoint, browser network
     failures/CORS, and whether the agent and browser point to the same server.
   - If browser or MCP tools are unavailable, say exactly which part remains
     unverified and give the user this final check. Do not claim live sync works.

## Notes

- The development guard prevents the toolbar from appearing in production.
- Agentation requires React 18 or newer.
- Installing the toolbar does not install, configure or connect an MCP server.
- Manual copy mode is supported and does not need a running server.
