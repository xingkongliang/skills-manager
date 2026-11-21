# Make Agents Manager agent-first through MCP

One request should let an agent find the right target, manage Skills, Instructions, and MCP connections, then verify the result without opening the desktop app.

## Agreed

- Cover the whole Agents Manager, including its Library, projects, and hosts.
- A clear request authorizes routine changes. Ask when scope is unclear, edits conflict, or new access is required.
- First end-to-end test is **“Set up this project.”** Include target discovery, deployment, and verification. Decide the fixture and completion evidence on the map.

## Decision sequence

1. Define what project setup includes and which requests the first release must finish. Research Codex and Claude Code setup in parallel.
2. Decide how requests identify their project, agent, and host. Use client research to decide first-run setup.
3. Define what a clear request authorizes, how to handle exceptions, and how to verify and retry changes.
4. Decide how to report effects across hosts without changing Sync behavior. Draft the implementation plan for Yi’s explicit approval.

## Performance bound

Aim for one MCP call after setup for a fully specified routine change. One validation pass over relevant state, one apply and verification per affected host, and only requested inputs, changed config, and compact results cross boundaries. Test at 10× current project and Library size. At 100×, host fan-out and result size set the limit. Page large reads and bound remote work.

[Canonical Wayfinder map in Linear](https://linear.app/abrian/issue/SKI-27/make-agents-manager-agent-first-through-mcp)
