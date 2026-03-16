# AGENTS.md

## Cursor Cloud specific instructions

### Overview

Skills Manager v2 is a Tauri 2 desktop app (Rust backend + React/TypeScript frontend). See `CLAUDE.md` for project overview and `README.md` for getting-started docs.

### System dependencies (Linux)

The following system packages are required and pre-installed in the VM snapshot:

```
libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
```

### Rust toolchain

Rust stable **must be >= 1.85** (some transitive crates require `edition2024`). The pre-installed Rust 1.83 is too old — `rustup default stable` after `rustup update stable` resolves this. The update script handles this automatically.

### Running the app

```bash
npm run tauri:dev    # starts Vite dev server (port 1420) + compiles & runs the Rust backend
```

First run compiles ~678 Rust crates (~2-3 min). Subsequent runs use incremental compilation and are much faster.

EGL warnings (`libEGL warning: DRI3 error`) are expected in the VM and do not affect functionality.

### Key commands

| Task | Command |
|------|---------|
| Install deps | `npm install` |
| Lint | `npm run lint` |
| Build frontend | `npm run build` |
| Check Rust backend | `cd src-tauri && cargo check` |
| Run dev mode | `npm run tauri:dev` |

### Data paths

The app stores data at `~/.skills-manager/` (SQLite DB + skills directory). This is created automatically on first run.
