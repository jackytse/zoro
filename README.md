# zoro

Vite+ + Vue 3 + Nuxt UI + VueUse + Tauri 的桌面应用起步项目。

## 技术栈

| 层     | 选型                                | 说明                                                                     |
| ------ | ----------------------------------- | ------------------------------------------------------------------------ |
| 工具链 | [Vite+](https://viteplus.dev) 1.7   | dev / build / lint / fmt / test / staged 全部收敛到一份 `vite.config.ts` |
| 框架   | Vue 3.5 + vue-router 5              | `<script setup>` 组合式 API                                              |
| UI     | [Nuxt UI](https://ui.nuxt.com) 4.12 | Tailwind CSS 4，组件与组合式函数自动引入                                 |
| 工具库 | VueUse 15                           | `useColorMode`、`useNow`、`useDateFormat` 等                             |
| 桌面壳 | Tauri 2.12                          | Rust 后端 + 系统 WebView，产物在 `src-tauri/`                            |

## 环境要求

- Node.js ≥ 22.18（建议 22 LTS）
- pnpm 12（`devEngines` 会自动下载匹配版本）
- [vp CLI](https://viteplus.dev)：`curl -fsSL https://viteplus.dev/install.sh | bash`
- Tauri：Rust 1.90+ 与 Xcode Command Line Tools（macOS）

## 常用命令

```sh
pnpm dev                  # 浏览器开发，端口 5173
pnpm run desktop:dev      # Tauri 桌面窗口，端口 1420（自动拉起前端 dev server）
pnpm build                # 类型检查 + 生产构建（Rolldown），输出 dist/
pnpm run desktop:build    # 打包桌面安装包
vp check                  # 格式化 + lint + 类型检查（--fix 自动修复）
vp test                   # Vitest 单测
```

> 注意：`vp dev` / `vp build` 是 Vite+ 内置命令；要执行 package.json 里的脚本请用 `pnpm run xxx` 或 `vp run xxx`。

### 两个 dev server 分别占什么端口

| 命令                   | 端口 | 说明                                          |
| ---------------------- | ---- | --------------------------------------------- |
| `pnpm dev`             | 5173 | 浏览器调试用                                  |
| `pnpm run desktop:dev` | 1420 | Tauri 壳加载的地址，由 `dev:desktop` 脚本指定 |

两者使用**不同端口**，所以可以在开着一个的情况下再跑另一个，不会互相抢占。不要手动再起一个占 1420 的 server，否则 `beforeDevCommand` 会因 `--strictPort` 直接失败。

## 托盘功能

应用启动时会在菜单栏创建托盘图标（macOS 模板图标，随明暗主题自动着色）：

- **关闭窗口 ≠ 退出**：点击主窗口关闭按钮只是隐藏窗口，进程驻留后台；Dock 图标点击可重新唤起。
- **托盘左键**：在图标下方弹出「最近项目」窗口（再次点击 / 点击别处 / 按 Esc 均可收起）。
- **托盘右键**：系统菜单（显示主窗口 / 退出）。
- **弹窗内容**：按最近打开时间排序的项目列表（项目名、工具名、路径、相对时间），底部有「设置」与「退出」。
- **设置页**（主窗口 `/settings`）：添加 / 移除 / 试打开项目，工具清单为内置只读。
- **导入最近项目**：设置页可一键扫描本机 VS Code / Cursor 的最近工作区（`storage.json`）与 JetBrains 系列的最近项目（`recentProjects.xml`），弹窗勾选后批量导入，可改选打开工具；只读扫描，自动过滤已删除目录与已配置路径（iTerm 无记录源，不参与）。
- **打开策略**：直接执行 CLI → 登录 shell 解析（兼容 nvm/homebrew 的 PATH）→ macOS `open -a` 兜底；三级都失败时弹窗会显示明确错误。

配置持久化在 `~/Library/Application Support/dev.zoro.desktop/config.json`，文件损坏时自动回退默认值。

托盘图标由 `scripts/gen-tray-icon.py` 生成（纯 stdlib 画 PNG，无需图片库）。

## 目录结构

```
src/
  assets/css/main.css   # Tailwind 与 Nuxt UI 样式入口
  router/index.ts       # 路由表
  views/                # 页面组件（与路由一一对应）
src-tauri/              # Rust 桌面壳：窗口配置、能力声明、打包设置
vite.config.ts          # Vite+ 统一配置（插件 / lint / fmt / staged）
```

## 备注

- Nuxt UI 会在项目根目录生成 `auto-imports.d.ts` 与 `components.d.ts`（已 gitignore），首次 `pnpm dev` 或 `pnpm build` 后产生，`vue-tsc` 依赖它们做类型检查。
- `src-tauri/tauri.conf.json` 的 `build.beforeDevCommand` 用的是 `pnpm dev:desktop` 而非 `vp dev`：一是即使全局 `vp` 不在 PATH（IDE 内置终端常见）也能靠 `node_modules/.bin/vp` 兜底，二是端口与浏览器版 dev server 隔离开。
- 图标使用 `@iconify-json/lucide` 本地集合，桌面端离线可用。
