<p align="center">
  <img src="assets/icon.png" width="80" />
</p>

<h1 align="center">Skills Manager</h1>

<p align="center">
  一个应用，统一管理所有 AI 编码工具的 Skills。
</p>

<p align="center">
  <a href="./README.md">English</a>
</p>

<p align="center">
  <img src="assets/demo-zh.gif" width="800" alt="Skills Manager 演示" />
</p>

## 功能

- **统一技能库** — 从 Git 仓库、本地目录、`.zip` / `.skill` 文件或 [skills.sh](https://skills.sh) 市场安装技能，统一存放在 `~/.skills-manager`。
- **多工具同步** — 一键将技能同步到任意支持的工具，支持软链接和复制两种模式。
- **场景管理** — 将技能分组为场景（Scenario），随时切换。
- **更新检查** — 为 Git 类技能检查远端更新；本地技能支持重新导入。
- **文档预览** — 直接在应用内查看 `SKILL.md` / `README.md`。

## 支持的工具

Cursor · Claude Code · Codex · OpenCode · Amp · Kilo Code · Roo Code · Goose · Gemini CLI · GitHub Copilot · Windsurf · TRAE IDE · Antigravity · Clawdbot · Droid

## 技术栈

| 层 | 技术 |
|----|------|
| 前端 | React 19、TypeScript、Vite、Tailwind CSS |
| 桌面 | Tauri 2 |
| 后端 | Rust |
| 存储 | SQLite（`rusqlite`） |
| 国际化 | react-i18next |

## 快速开始

### 前置依赖

- Node.js 18+
- Rust 工具链
- 当前系统的 [Tauri 依赖](https://v2.tauri.app/start/prerequisites/)

### 开发

```bash
npm install
npm run tauri:dev
```

### 构建

```bash
npm run tauri:build
```

## 常见问题

### macOS 提示"应用已损坏，无法打开"

下载应用后如果出现此提示，在终端执行以下命令后重新打开即可：

```bash
xattr -cr /Applications/skills-manager.app
```

如果 `.app` 不在 `/Applications`，请替换为实际路径。

## License

MIT
