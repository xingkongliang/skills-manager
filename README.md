<p align="center">
  <img src="assets/icon.png" width="80" />
</p>

<h1 align="center">Skills Manager</h1>

<p align="center">
  One app to manage AI agent skills across all your coding tools.
</p>

<p align="center">
  <a href="./README.zh-CN.md">中文说明</a>
  &nbsp;·&nbsp;
  <a href="https://x.com/JayTL00">@JayTL00 on X</a>
  &nbsp;·&nbsp;
  <a href="https://buymeacoffee.com/jaytl">Buy me a coffee</a>
</p>

<p align="center">
  <img src="assets/demo.gif" width="800" alt="Skills Manager Demo" />
</p>

<p align="center"><strong>My Skills</strong></p>
<p align="center"><img src="assets/CleanShot_20260316_231111@2x.png" width="800" alt="My Skills" /></p>

<p align="center"><strong>Install Skills — Marketplace</strong></p>
<p align="center"><img src="assets/CleanShot_20260316_231142@2x.png" width="800" alt="Install Skills Marketplace" /></p>

<p align="center"><strong>Projects</strong></p>
<p align="center"><img src="assets/CleanShot_20260316_231202@2x.png" width="800" alt="Projects" /></p>

<p align="center"><strong>Settings</strong></p>
<p align="center"><img src="assets/CleanShot_20260316_231216@2x.png" width="800" alt="Settings" /></p>

## Features

- **Unified skill library** — Install skills from Git repos, local folders, `.zip` / `.skill` archives, or the [skills.sh](https://skills.sh) marketplace. Everything goes into one central repo at `~/.skills-manager`.
- **Multi-tool sync** — Sync skills to any supported tool via symlink or copy with a single click.
- **Project Skills** — View and manage skills inside any project's `.claude/skills/` directory, with bidirectional sync to your central library.
- **Scenarios** — Group skills into scenarios, configure per-scenario agent toggles, and switch between them instantly.
- **Batch operations** — Multi-select skills for bulk enable/disable, export, or delete.
- **Skill tagging** — Tag skills and filter by tag for quick lookup.
- **Update tracking** — Check for upstream updates on Git-based skills; re-import local ones.
- **Skill preview** — Read `SKILL.md` / `README.md` docs right inside the app.
- **Custom tools** — Add your own agents/tools with custom skills directories, or override the default path for any built-in tool.
- **Git backup** — Version-control your skill library with Git for backup and multi-machine sync.

## Quick Start

1. Create or switch to a scenario for the workflow you want to use.
2. Install skills from local folders, Git repositories, archives, or the marketplace.
3. Open **My Skills** to decide which skills belong to the current scenario.
4. Sync enabled skills to detected tools, or use **Project Workspaces** for project-local skill folders.
5. Configure agent paths, custom tools, proxy, and Git preferences in **Settings**.
6. If you want history or multi-machine sync, set a Git remote in **Settings** and run **Start Backup** or **Sync to Git** from **My Skills**.

## Git Backup

Back up `~/.skills-manager/skills/` to a Git repo for version history and multi-machine sync.

### Quick setup

1. Create a private repository (recommended).
2. Open **Settings → Git Sync Configuration** and save your remote URL.
3. Open **My Skills**.
4. Choose one:
- Existing remote: click **Start Backup** to clone from the configured remote.
- New local repo: click **Start Backup** to initialize locally, then use **Sync to Git**.
5. Use **Sync to Git** from the My Skills toolbar.

`Sync to Git` automatically handles pull/commit/push based on current repo status.
Each successful sync now creates a snapshot version tag. You can open **Version History** in My Skills and restore any snapshot as a new commit.

### Authentication

- SSH URL (`git@github.com:...`): requires SSH key configured on your machine and added to GitHub.
- HTTPS URL (`https://github.com/...`): push usually requires a Personal Access Token (PAT).

> **Note:** The SQLite database (`~/.skills-manager/skills-manager.db`) is not included in Git — it stores metadata that can be rebuilt by scanning the skill files.

## Supported Tools

Cursor · Claude Code · Codex · OpenCode · Amp · Kilo Code · Roo Code · Goose · Gemini CLI · GitHub Copilot · Windsurf · TRAE IDE · Antigravity · Clawdbot · Droid

You can also add custom tools in **Settings** and manage their skills the same way.

## In-App Help

The **Help** button in **Settings** summarizes the same workflow as above, so new users can get oriented without leaving the app.

## Tech Stack

| Layer | Tech |
|-------|------|
| Frontend | React 19, TypeScript, Vite, Tailwind CSS |
| Desktop | Tauri 2 |
| Backend | Rust |
| Storage | SQLite (`rusqlite`) |
| i18n | react-i18next |

## Getting Started

### Prerequisites

- Node.js 18+
- Rust toolchain
- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS

### Development

```bash
npm install
npm run tauri:dev
```

### Build

```bash
npm run tauri:build
```

## Troubleshooting

### macOS: "App is damaged and can't be opened"

If you see this error after downloading the app, run the following command in Terminal and then open the app again:

```bash
xattr -cr /Applications/skills-manager.app
```

Replace the path with wherever you placed the `.app` file if it's not in `/Applications`.

## License

MIT
