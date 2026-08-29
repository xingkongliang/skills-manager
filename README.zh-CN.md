<p align="center">
  <img src="assets/icon.png" width="80" />
</p>

<h1 align="center">Skills Manager</h1>

<p align="center">
  一个应用，统一管理所有 AI 编码工具的 Skills。
</p>

<p align="center">
  <strong><a href="https://skillsmanager.dev/zh">skillsmanager.dev</a></strong>
</p>

<p align="center">
  🎬 <a href="https://www.bilibili.com/video/BV1845F6REUu/">视频介绍（Bilibili）</a>
  &nbsp;·&nbsp;
  <a href="https://www.youtube.com/watch?v=wfbCrfNASVU">Video intro (YouTube)</a>
</p>

<p align="center">
  <a href="./README.md">English</a>
</p>

<p align="center">
  <a href="https://trendshift.io/repositories/23290?utm_source=repository-badge&amp;utm_medium=badge&amp;utm_campaign=badge-repository-23290" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/repositories/23290" alt="xingkongliang%2Fskills-manager | Trendshift" width="250" height="55"/></a>
</p>

<p align="center">
  <a href="https://skills.sh/xingkongliang/skills-manager"><img src="https://skills.sh/b/xingkongliang/skills-manager" alt="skills.sh 上的 manage-skills" /></a>
</p>

<p align="center">
  <img src="assets/demo/library.png" width="800" alt="Skills Manager 技能库" />
</p>

<p align="center"><strong>安装 Skills</strong></p>
<p align="center"><img src="assets/demo/install-skills.png" width="800" alt="安装 Skills" /></p>

<p align="center"><strong>全局工作区</strong></p>
<p align="center"><img src="assets/demo/global-workspace.png" width="800" alt="全局工作区" /></p>

<p align="center"><strong>Agent 工作区</strong></p>
<p align="center"><img src="assets/demo/agent-workspace.png" width="800" alt="Agent 工作区" /></p>

<p align="center"><strong>项目工作区</strong></p>
<p align="center"><img src="assets/demo/project-workspace.png" width="800" alt="项目工作区" /></p>

<p align="center"><strong>备份与多设备同步</strong></p>
<p align="center"><img src="assets/demo/backup.png" width="800" alt="备份与多设备同步" /></p>

<p align="center"><strong>设置</strong></p>
<p align="center"><img src="assets/demo/settings.png" width="800" alt="设置" /></p>

## 功能

<p align="center">
  <img src="assets/diagram-concept-map.png" width="640" alt="概念图：技能库、Preset、全局工作区、项目工作区、Agent" />
</p>

- **统一技能库** — 从 Git 仓库、本地目录、`.zip` / `.skill` 文件或 [skills.sh](https://skills.sh) 市场安装技能，统一存放在 `~/.skills-manager`。
- **技能市场** — 浏览市场上的热门 Skills，用关键词搜索找到需要的那个。
- **让你的 Agent 管理 Skills** —— Claude Code、Codex、Cursor 等可以替你装一个 skill、部署到另一个 agent、或报告哪个 agent 有什么，走的是驱动 Skills Manager 而不是绕过它直接写 agent 目录——来源、preset、更新追踪和各 agent 的状态都不会丢。Dashboard 上一键完成设置，详见[让你的 Agent 来管理 Skills](#让你的-agent-来管理-skills)。
- **Preset（预设）** — 将技能分组为命名 Preset。在任意工作区点击 Preset 标签，即可一键为当前 Agent 范围激活或停用其全部技能，激活的 Preset 显示 ✓，部分安装显示数量。应用 Preset 是一次性复制，不是实时同步。
- **全局工作区** — 每个 Agent 都有自己的页面，列出其全局目录里的所有 Skills（包括不是通过 Skills Manager 安装的），始终反映 Agent 实际看到的内容。可按 Agent 添加或移除 Skills，也可通过「全部 Agents」总览跨所有已安装 Agent 统一管理。
- **项目工作区** — 查看并管理任意项目的本地 Skills 目录，支持与中央库双向同步。支持嵌套 Skill 目录和导出时按 Agent 分配。
- **关联工作区** — 将任意目录指定为 Skills 根目录，适合管理不在默认 Agent 路径下的 Skills。作为独立工作区管理，不参与全局 Preset 同步。
- **多工具同步** — 一键将技能同步到任意支持的工具，支持软链接和复制两种模式。每张 Skill 卡片会为每个已启用 Agent 显示一个图标角标，点击角标即可直接在卡片上为该 Agent 安装或移除这个 Skill，角标会实时反映同步状态。
- **「添加 Skills」弹层** — 任意工作区点击 **+ 添加 Skills** 即可打开统一的挑选弹层：搜索中央库，用始终可见的 Agent 标签切换目标（含一键全选/清空），一次提交批量添加多个 Skills。
- **批量操作** — 多选技能后批量启用/禁用、导出或删除。项目工作区中的项目 Skills 也支持批量启用/禁用。
- **技能标签** — 为技能添加标签，用于归类同类技能，并按来源或标签筛选；新增的 **未标签** 过滤项可快速定位漏打标签的 Skills。
- **更新检查** — 为 Git 类技能检查远端更新；本地技能支持重新导入。
- **文档预览** — 直接在应用内查看 `SKILL.md` / `README.md`。
- **自定义工具** — 添加自定义 Agent/工具并指定 Skills 目录，也可覆盖内置工具的默认路径。
- **备份与多设备同步** — 一次 GitHub 登录（或任意 Git 远端）接入私有备份仓库，之后自动备份、多台设备自动保持一致。合并以技能为单位——一台改名、另一台改内容会自动组合；真冲突不阻塞不覆盖，本机版本保留待你三选一处理。快照版本随时可恢复。
- **活动日志 & 导出日志** — 应用会记录本地的安装/移除/更新/同步操作。在 **设置 → 导出日志** 可把最近日志和活动记录打包成压缩文件，方便提交 Issue 时附上。
- **灵活的应用设置** — 在一个页面里配置仓库路径、同步模式、主题、字号、语言、托盘行为、代理、Git 远程、更新检查，以及 Agent 在全应用中的显示顺序。
- **应用内更新** — 有新版本时应用会主动提醒，并在 macOS 和 Windows 上直接完成安装。不会自行下载或安装：检查只负责告知，安装和重启各需一次点击。

## 安装

### macOS

使用 [Homebrew](https://brew.sh) 安装：

```bash
brew install --cask skills-manager
```

也可以从 [最新 Release](https://github.com/xingkongliang/skills-manager/releases/latest) 直接下载 `.dmg`。

### Windows 和 Linux

从 [最新 Release](https://github.com/xingkongliang/skills-manager/releases/latest) 下载对应平台的安装包：Windows 为 `.exe` 或 `.msi`，Linux 为 `.AppImage`、`.deb` 或 `.rpm`（提供 x64 和 arm64）。

## 快速上手

1. 从本地目录、Git 仓库、压缩包或市场安装 Skills。
2. 从侧边栏进入 **全局工作区**，选择一个 Agent（如 Claude Code）。
3. 点击 **Preset** 标签为该 Agent 一键激活对应 Skills，或点 **+ 添加 Skills** 从技能库挑选并即时切换目标 Agent。激活的 Preset 显示 ✓，部分安装显示计数角标。
4. 如需管理项目本地 Skills，打开 **项目工作区**，同样使用 Preset 标签，或通过 **+ 添加 Skills** 弹层用多 Agent 目标选择器挑选。
5. 在 **设置** 中配置 Agent 路径、自定义工具、主题、语言、代理和 Git 偏好。
6. 如果需要历史版本或多机同步，从侧边栏打开 **备份** 页，点击 **使用 GitHub 登录**——之后备份和跨设备同步都会自动进行。

## 让你的 Agent 来管理 Skills

Claude Code、Codex、Cursor 等可以替你装一个 Skill、把它部署到另一个 Agent、或者报告哪个 Agent 有什么——做法是驱动 Skills Manager，而不是绕过它直接往 Agent 目录里写。这正是来源元数据、Preset 归属、更新追踪和跨 Agent 部署状态得以保全的原因。

Dashboard 上有一个一次性入口：勾选哪些 Agent 可以这么做，应用就把 [`manage-skills`](skills/manage-skills/SKILL.md) 装进技能库并只部署给这些 Agent。之后它就是一个普通的库内 Skill——增减 Agent 走它自己卡片上的 Agent 图标行。全程不需要配置 PATH：应用会把自己那份 CLI 发布到 Agent 会去找的位置。

它同时也是一个正常发布的 Skill，不装应用也能直接装：

```bash
npx skills add xingkongliang/skills-manager
```

## 备份与多设备同步

侧边栏的 **备份** 页把技能库托管在一个 Git 仓库里：单台设备是带版本历史、可恢复快照的备份；多台设备连接同一仓库时会自动保持一致。远端始终是纯 Git 仓库——随时可以 `git clone` 走，没有锁定。

### 连接

- **使用 GitHub 登录**（推荐）：输入 8 位码完成授权，应用会自动创建私有仓库 `skills-manager-backup`。令牌只存在系统钥匙串里，绝不落入文件或仓库配置。
- **高级方式**：在 **设置 → Git 同步配置** 粘贴任意 Git 地址（HTTPS + PAT、SSH、自建服务均可）。
- 新机器上技能库为空时，首次启动会询问：**全新开始，还是从备份恢复？**

### 同步如何工作

- **全自动**：本地改动停止编辑约两分钟后自动提交并上传；其他设备推送的更新会自动合并进来并推送回去。随时可点 **立即备份**，备份历史会显示每一条来自哪台设备。
- **按技能合并**：同步以技能为单位而非文本行——一台设备改名、另一台改内容，会自动正确组合。
- **冲突不阻塞、不覆盖**：同一技能在两台设备被同时修改时，其余技能照常同步，该技能保留本机版本并进入 **需要处理** 列表（技能卡上也有徽章）。三选一：**保留本机 / 使用远端 / 两个都保留**——应用任一选择前都会先建安全快照，每个决定都可撤销。
- **快照与恢复**：手动备份会创建快照版本，在备份页历史中可恢复任意一个；恢复前会先把当前状态存为新快照。

### 备份包含什么

技能文件、标签、Preset 及每个 Agent 的技能开关会被备份。机密信息（API Key、令牌、代理配置）和本机接线永不上传。超过 100 MB 的技能自动留在本机、不进备份（备份页会标注）。SQLite 数据库不进 Git——其中的元数据可从技能文件重建。

### 断开连接

备份页提供三档：**断开本机**（其他设备与远端数据不受影响）、**撤销 GitHub 授权**、以及 **删除远端备份**（经 GitHub 原生的输入仓库名确认流程）。

## 支持的工具

开箱支持 53 个 Agent，包括：

Claude Code · Codex · Cursor · GitHub Copilot · Gemini CLI · OpenCode · OpenClaw · Hermes Agent · OpenHands · Cline · Goose · Windsurf · Continue · Grok · Antigravity · Qwen Code · ZCode · Crush · Kilo Code · Roo Code · Amp · Kiro CLI · Droid · TRAE IDE · Warp · Qoder · CodeBuddy

**设置**页会列出全部，并优先展示在你机器上检测到的那些。你也可以在那里添加自定义工具，以相同方式管理其 Skills。

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

- Node.js 20.19+ 或 22.12+（Vite 7 的要求）
- Rust 1.77.2 或更高
- 当前系统的 [Tauri 依赖](https://v2.tauri.app/start/prerequisites/)

### 开发

```bash
npm install
npm run tauri:dev
```

### CLI

仓库包含一个面向 agent 的 CLI，和桌面应用共用同一套 Rust core——同一个 SQLite 数据库、同一个中央技能库、同一套同步引擎。

```bash
# 看一眼现状
npm run cli -- skills list
npm run cli -- skills show db

# 装进技能库（默认不会自动部署到任何 Agent）
npm run cli -- skills install ./my-skill
npm run cli -- skills install https://github.com/foo/bar/tree/main/skills/baz
npm run cli -- skills install vercel-labs/agent-skills@react-best-practices

# 部署给该有它的 Agent，然后核对
npm run cli -- skills deploy react-best-practices --agent claude_code --agent codex
npm run cli -- skills status react-best-practices

# 拉上游更新；把 Agent 里已有的技能纳管进来
npm run cli -- skills check --all
npm run cli -- skills update --all
npm run cli -- skills adopt ~/.claude/skills --dry-run
```

任何命令组或子命令加 `--help` 都会打印完整用法——下面几个组能做的远不止上面这些例子，破坏性命令都支持 `--dry-run`（`remove` 还强制要求 `--yes`）。

可用命令分组：
- `repo`：查看或修改当前 base directory
- `agents`（兼容别名 `tools`）：列出 Agent，并全局启用或禁用 Agent
- `skills`：管理中央库、标签，以及 skill 在各 Agent 中的真实部署
- `presets`：创建、修改、删除、整理、部署或撤下 Preset
- `git`：操作 git 管理的 `skills/` 仓库（`clone`、`pull`、`push`、`commit`、`versions`、`restore`）

额外参数：
- `--skills-root <path>`：直接针对某个已 clone / 已导出的 skills repo 操作，而不是本机 app 默认目录。manager 的状态（DB、scenarios、cache、logs）会落在 `~/.skills-manager/external/<name>-<hash>/`，按 skills root 的规范化路径分目录隔离，外部仓库本身保持干净。
- `--json`：给脚本 / agent 使用的机器可读输出。失败时在 stderr 打印 `{"ok": false, "code": …, "message": …}` 并以非零码退出。因目标不属于我们而被拒绝的部署会把路径作为数据带出来（`code: "TARGET_CONFLICT"`、`details.conflicts[].path`），调用方可以直接指出是哪个目录挡路，而不是转述一句话。

```bash
npm run -s cli -- --skills-root /path/to/my-skills --json skills list
```

#### 二进制放在哪

应用启动时会把自己那份 CLI 复制到 `~/.skills-manager/bin/skills-manager-cli`，版本永远与正在运行的应用一致，Agent 不需要你改 PATH 就能找到它。旁边的 `.version` 标记只在副本校验通过后才写、每次重新发布前先删——所以一次失败的复制（比如 Windows 上二进制正被占用）绝不会被当成可用的。

把 CLI 放到**你自己**的 PATH 上手敲命令，是另一件事：

```bash
npm run cli:install
# 等价于：
# cargo install --path src-tauri --bin skills-manager-cli --locked --force
```

二进制会装到 `~/.cargo/bin/skills-manager-cli`。代码更新后再跑一次即可刷新。

正式 Release 也会提供 macOS arm64/x64、Windows x64、Linux x64 的独立 CLI 文件。下载对应的 `skills-manager-cli-*`，在 macOS/Linux 添加可执行权限后放入 PATH 即可。

#### 与桌面应用并发使用

CLI 和桌面应用共享同一个 SQLite 数据库及仓库锁。CLI 修改 metadata 或 Agent 部署后，桌面应用通常会通过文件监听自动刷新；如果应用当时处于休眠状态，手动刷新一次即可。

### 构建

```bash
npm run tauri:build
npm run cli:build
```

## 常见问题

**macOS 打不开应用。** 从 **v1.29.0** 起，发布版本都经过 Apple Developer ID 签名与公证，可以正常打开。如果看到「Apple 无法验证…」或「应用已损坏」，说明你用的是 v1.28.5 或更早的版本，升级即可解决。（升级会改变代码签名，macOS 可能会再问一次 `skills-manager-git-backup` 钥匙串权限，点「始终允许」。）

其它问题——[提个 issue](https://github.com/xingkongliang/skills-manager/issues)，并附上 **设置 → 导出日志** 打出来的压缩包。

## Star 增长

<p align="center">
  <a href="https://github.com/xingkongliang/star-history-svg">
    <img src="assets/star-history.svg" width="800" alt="xingkongliang/skills-manager 的 Star History 图" />
  </a>
</p>

## License

MIT
