# zoro

[English](./README_EN.md) · 简体中文

**菜单栏里的「最近项目」快速启动器。** zoro 常驻系统托盘，左键唤出最近打开的项目列表，一键用 VS Code、Cursor、JetBrains 系列、Zed 等开发工具打开；也可以从这些 IDE 的历史记录里批量导入项目。

当前版本 `0.1.9`，以 [MIT](./LICENSE) 协议开源。

## 功能特性

- **菜单栏托盘**：macOS 模板图标随明暗主题自动着色；左键切换弹窗，右键呼出系统菜单。
- **最近项目弹窗**：按最近打开时间倒序展示（品牌图标 / 名称 / 路径 / 相对时间），点击即用绑定的工具打开；悬停可用系统终端打开目录。
- **工具自动同步**：内置 18 种常用工具（VS Code、Cursor、Windsurf、JetBrains 全系、Android Studio、Xcode、Sublime Text、Zed、iTerm、Terminal 等），启动时按本机安装状态自动增删。
- **一键导入**：只读扫描 VS Code / Cursor、JetBrains 全系、Android Studio、Zed 的最近项目记录，勾选后批量导入，可逐条改选打开工具。
- **全局快捷键**：默认 `⌘/Win + Shift + P` 唤出弹窗，可在设置里自定义或禁用。
- **开机自启**：默认开启，登录后静默驻留托盘（ `--hidden` ），不弹窗口。
- **多语言与主题**：简体中文 / 繁體中文 / English 三语界面，默认跟随系统，切换即时生效且托盘菜单同步；主题支持浅色 / 深色 / 跟随系统。
- **应用内更新**：正式版 / 预发布双通道，更新包经 minisign 签名校验。
- **可靠的打开策略**：依次尝试 CLI → 登录 shell → macOS `open -a`，三级都失败时给出明确错误。

## 技术栈

| 层     | 选型                                | 说明                                                                     |
| ------ | ----------------------------------- | ------------------------------------------------------------------------ |
| 工具链 | [Vite+](https://viteplus.dev) 1.7   | dev / build / lint / fmt / test / staged 全部收敛到一份 `vite.config.ts` |
| 框架   | Vue 3.5 + vue-router 5              | `<script setup>` 组合式 API                                              |
| UI     | [Nuxt UI](https://ui.nuxt.com) 4.12 | Tailwind CSS 4，组件与组合式函数自动引入                                 |
| 工具库 | VueUse 15、vue-i18n 11              | 主题 / 时间 / 通知组合式函数与三语国际化                                 |
| 桌面壳 | Tauri 2.12                          | Rust 后端（edition 2024，rust-version 1.90）+ 系统 WebView                |

## 环境要求

- **Node.js**：`^22.18.0 || >=24.12.0`（见 `package.json` 的 `engines`）
- **pnpm 12**（CI 固定 12.21.0；版本与 lockfile 格式绑定）
- [vp CLI](https://viteplus.dev)：`curl -fsSL https://viteplus.dev/install.sh | bash`
- **Tauri**：Rust 1.90+ 与 Xcode Command Line Tools（macOS）；Windows 需 WebView2 与 MSVC 构建工具

## 安装与开发

```sh
vp install            # 推荐：Vite+ 统一管理依赖（匹配 pnpm 12 与 catalog）
# 或 pnpm install

pnpm dev              # 浏览器开发，端口 5173
pnpm run desktop:dev  # Tauri 桌面窗口，端口 1420（自动拉起前端 dev server）
vp check              # 格式化 + lint + 类型检查（--fix 自动修复）
vp test               # Vitest 单测
cargo test --locked --manifest-path src-tauri/Cargo.toml   # Rust 单测（CI 门禁同款）
```

> 注意：`vp dev` / `vp build` 是 Vite+ **内置命令**；要执行 `package.json` 里的脚本请用 `pnpm run xxx` 或 `vp run xxx`，两者不能互换。

### 两个 dev server 分别占什么端口

| 命令                   | 端口 | 说明                                          |
| ---------------------- | ---- | --------------------------------------------- |
| `pnpm dev`             | 5173 | 浏览器调试用                                  |
| `pnpm run desktop:dev` | 1420 | Tauri 壳加载的地址，由 `dev:desktop` 脚本指定 |

两者使用**不同端口**，可同时运行而互不抢占。不要手动再起一个占用 1420 的 server，否则 `beforeDevCommand` 会因 `--strictPort` 直接失败。

## 打包

```sh
pnpm build             # 类型检查 + 生产构建（Rolldown），输出 dist/
pnpm run desktop:build # Tauri 打包，产物在 src-tauri/target/release/bundle/
```

- 前端是**多入口**构建：`index.html` 是主窗口，`tray.html` 是托盘弹窗（独立小入口，不加载主应用路由）。
- `bundle.targets = all`，CI 分别产出 Linux `deb`、macOS `dmg`、Windows `nsis` / `msi`，命名为 `zoro-<版本>-<系统>-<架构>` 并附 `.sha256`。
- 注入更新签名私钥（`TAURI_SIGNING_PRIVATE_KEY`）时，构建会额外产出供应用内更新校验的 `.sig` 文件。

## 托盘功能

应用启动时在菜单栏创建托盘图标（macOS 模板图标，随明暗主题自动着色）。

- **左键**：在图标下方弹出「最近项目」窗口；再次点击、点击别处（失焦）或按 `Esc` 均可收起。
- **右键**：系统菜单，按界面语言显示「设置 / 退出应用」（zh-TW：設定 / 結束應用；en：Settings / Quit App）。
- **弹窗内容**：按最近打开时间排序的项目列表，每项含品牌图标、名称、路径与相对时间；点击整行用绑定工具打开，悬停时点终端图标可在系统终端打开该目录。底部有「设置」与「退出」。
- **打开主窗口**：macOS 以 Accessory 模式运行，**不显示 Dock 图标**；主窗口通过托盘右键菜单「设置」或弹窗底部「设置」唤起，也可用 `⌘/Ctrl + ,` 从弹窗直达。
- **关闭窗口 ≠ 退出**：点击主窗口关闭按钮只是隐藏窗口，进程驻留后台；需通过托盘菜单或弹窗的「退出」真正结束进程。
- **设置页**（主窗口 `/`）：三个标签页——「通用」（主题、语言、开机自启、全局快捷键、终端行为、关于与更新）、「项目」（增删与导入项目）、「工具」（本机可用工具清单，只读展示）。
- **导入最近项目**：「项目 → 扫描最近项目」只读扫描本机记录，弹窗勾选后批量导入，可逐条改选打开工具。数据源包括 VS Code / Cursor（`state.vscdb`、`storage.json`、`workspaceStorage`）、JetBrains 全系与 Android Studio（`recentProjects.xml`）、Zed（`db.sqlite`）。自动过滤已删除目录与已配置路径；iTerm 无可读记录源，不参与扫描。
- **打开策略**：直接执行 CLI → 登录 shell 解析（兼容 nvm / homebrew 的 PATH）→ macOS `open -a` 兜底；终端类项目走 AppleScript 明确 `cd` 到目录（支持 iTerm2 / Terminal，新窗口 / 新选项卡）。
- **多显示器定位**：macOS 通过 `NSStatusItem` 取托盘图标的权威位置，避免混合缩放下弹窗跑到另一块屏；定位异常可用 `zoro --debug-monitors` 打印显示器信息排查。

## 配置位置

配置持久化为 JSON，损坏时自动回退默认值：

- macOS：`~/Library/Application Support/me.jackytse.zoro/config.json`
- Windows：`%APPDATA%\me.jackytse.zoro\config.json`（Tauri 默认应用配置目录）

> 早期版本使用 `dev.zoro.desktop` 作为 identifier。若新目录没有配置而旧目录存在，`config.json` 会在首次启动时自动从 `~/Library/Application Support/dev.zoro.desktop/config.json` 迁移过来，工具、项目与偏好不会丢失。

托盘图标由 [`scripts/gen-tray-icon.py`](./scripts/gen-tray-icon.py) 生成（纯标准库手写 PNG，无需图片库），输出为 `src-tauri/icons/tray.png`。

## 目录结构

```text
src/                          Vue 前端（主窗口 + 托盘弹窗两个入口）
  main.ts / App.vue           主窗口入口与根组件
  tray/                       托盘弹窗（main.ts、TrayPopup.vue、tray.css）
  views/SettingsView.vue      设置页（通用 / 项目 / 工具）
  router/index.ts             路由表（主窗口仅 / → 设置页）
  api/desktop.ts              Tauri 命令封装与共享类型
  i18n/                       国际化（index.ts + locales/{en,zh-CN,zh-TW}.ts）
  composables/                useAppTheme / useNotify / useUpdater
  components/                 ToolIcon / OverlayScrollbar
  icons/brands.ts             IDE 品牌图标映射
  utils/time.ts               相对时间格式化
  assets/css/main.css         Tailwind 与 Nuxt UI 样式入口
src-tauri/                    Rust 桌面壳
  src/main.rs / lib.rs        程序入口与应用装配、事件与命令注册
  src/config.rs               配置模型、持久化、工具目录与安装同步
  src/commands.rs             供前端调用的 Tauri 命令
  src/importer.rs             最近项目扫描（VS Code / JetBrains / Zed 等）
  src/opener.rs               项目与终端打开策略
  src/shortcut.rs             全局快捷键注册与触发
  src/tray.rs                 托盘图标、弹窗定位与右键菜单
  src/updater.rs              应用内更新（双通道）
  capabilities/default.json   权限声明
  tauri.conf.json             窗口、打包与更新配置
ci/                           CI 脚本（setup / test / build / release / upload）
.cnb.yml                      CNB 流水线（门禁 + 三平台发布）
.github/workflows/release.yml GitHub Actions 发布流水线
vite.config.ts                Vite+ 统一配置（插件 / lint / fmt / staged）
```

## 常见问题

- **关闭窗口后应用去哪了？** 只是隐藏到托盘，并未退出。macOS 上它以后台 Accessory 模式运行，不显示 Dock 图标。
- **怎么重新打开主窗口？** 托盘右键菜单「设置」、弹窗底部「设置」，或在弹窗中按 `⌘/Ctrl + ,`。
- **某个 IDE 为什么不在工具列表里？** 工具清单会按本机安装状态自动同步；未安装且未被任何项目引用的内置工具会被移除，装好后重新出现。设置页「工具」标签可查看当前可用项。
- **打开项目失败怎么办？** 确认目标目录仍存在、工具已安装；若 CLI 不在 PATH（nvm / homebrew 常见），把其加入 PATH 或依赖登录 shell / `open -a` 兜底。错误信息会说明具体原因。
- **配置文件在哪？损坏了怎么办？** 见「配置位置」。文件解析失败时会回退默认配置，不会导致托盘功能不可用。
- **导入会改动我的 IDE 数据吗？** 不会。扫描全程只读，不会写入 VS Code / JetBrains / Zed 的任何文件。
- **终端打开没反应或目录不对？** macOS 需允许 Zoro 控制 iTerm / Terminal（系统设置 → 隐私与安全性 → 自动化）；从受沙箱限制的终端启动应用时 Apple Events 可能被拦截，建议从普通终端启动。
- **弹窗跑到另一块屏？** 用 `zoro --debug-monitors` 打印显示器原始数据，便于定位混合缩放多屏问题。
- **`auto-imports.d.ts` / `components.d.ts` 是什么？** Nuxt UI 插件首次 `pnpm dev` 或 `pnpm build` 后自动生成，已 gitignore，`vue-tsc` 依赖它们做类型检查。

## 参与贡献

欢迎提交 issue 与 PR。开发流程、提交规范与检查清单见 [CONTRIBUTING.md](./CONTRIBUTING.md)。

## License

[MIT](./LICENSE) © 2026 jackytse
