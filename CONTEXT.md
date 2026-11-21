# Agents Manager

Manages the skills, instructions and MCP definitions that coding agents load, on this computer and on SSH hosts.

## Hosts and Sync

**Host**:
A machine whose agents Agents Manager manages: this computer (Local) or an SSH host.
_Avoid_: Remote, server, machine, device

**Sync**:
The shared library that every host follows through one git remote, so each host has the same skills, presets and active preset.
_Avoid_: Backup, Git Backup, Git Sync

**Sync remote**:
The GitHub repository that every host pulls from and pushes to directly, each with its own deploy key.
_Avoid_: Backup repo, backup remote

**Sync state**:
Where a host's Sync stands: Up to date, Syncing, Paused (automatic sync off), Offline (can't reach GitHub), Needs review, Update required (this host must update to keep syncing) or Update pending (the app will update this host's CLI when it can reach it).

**Join**:
Bringing a host into Sync. Items match by name; Sync's version wins and the host's extras are added. A pre-join copy makes it undoable.
_Avoid_: Clone, restore, adopt

**Sync format**:
The version of the files and merge rules in the Sync remote. It changes only when they change, not with every app release.
_Avoid_: Protocol, schema version

## Skills on agents

**Install**:
Adding a skill to the library, from the marketplace or a source.
_Avoid_: using it for agents

**Create**:
Writing a new skill straight into the library. It has no source, so there is nothing to update it from.
_Avoid_: Scaffold, author, using install for this

**Add to agent**:
Putting a library skill into an agent's skills folder, as a link or a copy. A skill is "On agent" or "Not on agent"; the opposite action is "Remove from agent".
_Avoid_: Sync, deploy, install

**Preset**:
A named set of library skills with per-agent toggles.
_Avoid_: Scenario

**Active preset**:
The preset whose skills are on agents. Every host in Sync has the same active preset; a click on a preset switches it on all hosts.

**Toggle**:
A per-agent on/off choice for a skill in a preset. Sync stores only the choices you set; a missing toggle means on.

**Added directly**:
A skill added to an agent outside the active preset. It stays on that host only, and the next switch removes it.
_Avoid_: Direct deploy

**Edited here**:
A copy on an agent or in a project that someone changed there. Sync never overwrites it; the user chooses "Use Sync version" or "Keep my edit".
_Avoid_: Drift, diverged, project newer

**Project status**:
How a project's copy of a library skill compares with the library: Up to date, Update available, Edited here or Both changed.
_Avoid_: In sync, center newer, diverged

## Project Workspaces

**Project setup**:
The saved selection of Library Skills, Instruction bundles, and MCP connections for a project, with the agents each item targets.
