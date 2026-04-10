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

| 我的 Skills | 项目 Skills |
|:-----------:|:----------:|
| <img src="assets/CleanShot_20260312_234539@2x.png" width="400" alt="我的 Skills" /> | <img src="assets/CleanShot_20260312_234613@2x.png" width="400" alt="项目 Skills" /> |

## 功能

- **统一技能库** — 从 Git 仓库、本地目录、`.zip` / `.skill` 文件或 [skills.sh](https://skills.sh) 市场安装技能，统一存放在 `~/.skills-manager`。
- **多工具同步** — 一键将技能同步到任意支持的工具，支持软链接和复制两种模式。
- **项目 Skills** — 查看并管理任意项目的 `.claude/skills/` 目录，支持与中央库双向同步。
- **场景管理** — 将技能分组为场景（Scenario），支持按场景配置 Agent 开关，随时切换。
- **批量操作** — 多选技能后批量启用/禁用、导出或删除。
- **技能标签** — 为技能添加标签并按标签筛选，快速定位。
- **更新检查** — 为 Git 类技能检查远端更新；本地技能支持重新导入。
- **文档预览** — 直接在应用内查看 `SKILL.md` / `README.md`。
- **自定义工具** — 添加自定义 Agent/工具并指定 Skills 目录，也可覆盖内置工具的默认路径。
- **Git 备份** — 用 Git 管理技能库，支持版本控制和多机同步。

## 快速上手

1. 先创建或切换到一个适合当前工作的场景。
2. 从本地目录、Git 仓库、压缩包或市场安装 Skills。
3. 打开 **我的 Skills**，决定哪些 Skill 属于当前场景。
4. 将已启用的 Skill 同步到已检测到的工具；如果是项目内本地 Skills，则使用 **项目工作区** 管理。
5. 在 **设置** 中配置 Agent 路径、自定义工具、代理和 Git 偏好。
6. 如果需要历史版本或多机同步，先在 **设置** 保存 Git 远程地址，再到 **我的 Skills** 执行 **开始备份** 或 **同步到 Git**。

## Git 备份

将 `~/.skills-manager/skills/` 备份到 Git 仓库，用于版本管理和多机同步。

### 快速配置

1. 创建一个私有仓库（推荐）。
2. 打开 **设置 → Git 同步配置**，保存远程仓库地址。
3. 打开 **我的 Skills** 页面。
4. 二选一：
- 已有远程仓库：点击 **开始备份**，按已配置地址克隆。
- 首次本地初始化：点击 **开始备份** 初始化本地仓库，再使用 **同步到 Git**。
5. 在我的 Skills 顶部工具栏点击 **同步到 Git**。

`同步到 Git` 会根据仓库状态自动处理拉取/提交/推送。
每次同步成功会自动创建一个快照版本标签。你可以在我的 Skills 中打开 **版本历史**，并将任意快照恢复为一条新的提交。

### 认证说明

- SSH 地址（`git@github.com:...`）：需要先在本机配置 SSH Key，并将公钥添加到 GitHub。
- HTTPS 地址（`https://github.com/...`）：推送通常需要 Personal Access Token（PAT）。

> **注意：** SQLite 数据库（`~/.skills-manager/skills-manager.db`）不纳入 Git 管理，它存储的元数据可通过扫描技能文件重建。

## 支持的工具

Cursor · Claude Code · Codex · OpenCode · Amp · Kilo Code · Roo Code · Goose · Gemini CLI · GitHub Copilot · Windsurf · TRAE IDE · Antigravity · Clawdbot · Droid

你也可以在**设置**中添加自定义工具，以相同方式管理其 Skills。

## 应用内帮助

设置页中的 **帮助** 按钮会展示与上面一致的快速流程，方便用户不离开应用也能快速理解使用方式。

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
