# zoro

Vite+ + Vue 3 + Nuxt UI + VueUse + Tauri 的桌面应用起步项目。

## 技术栈

| 层 | 选型 | 说明 |
| --- | --- | --- |
| 工具链 | [Vite+](https://viteplus.dev) 1.7 | dev / build / lint / fmt / test / staged 全部收敛到一份 `vite.config.ts` |
| 框架 | Vue 3.5 + vue-router 5 | `<script setup>` 组合式 API |
| UI | [Nuxt UI](https://ui.nuxt.com) 4.12 | Tailwind CSS 4，组件与组合式函数自动引入 |
| 工具库 | VueUse 15 | `useColorMode`、`useNow`、`useDateFormat` 等 |
| 桌面壳 | Tauri 2.12 | Rust 后端 + 系统 WebView，产物在 `src-tauri/` |

## 环境要求

- Node.js ≥ 22.18（建议 22 LTS）
- pnpm 12（`devEngines` 会自动下载匹配版本）
- [vp CLI](https://viteplus.dev)：`curl -fsSL https://viteplus.dev/install.sh | bash`
- Tauri：Rust 1.90+ 与 Xcode Command Line Tools（macOS）

## 常用命令

```sh
pnpm dev                  # Vite+ 开发服务器（浏览器，端口 5173）
pnpm run desktop:dev      # Tauri 桌面窗口（自动拉起前端 dev server）
pnpm build                # 类型检查 + 生产构建（Rolldown），输出 dist/
pnpm run desktop:build    # 打包桌面安装包
vp check                  # 格式化 + lint + 类型检查（--fix 自动修复）
vp test                   # Vitest 单测
```

> 注意：`vp dev` / `vp build` 是 Vite+ 内置命令；要执行 package.json 里的脚本请用 `pnpm run xxx` 或 `vp run xxx`。

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
- `src-tauri/tauri.conf.json` 中 `build.beforeDevCommand` 为 `vp dev`，Windows 下用 `vp.cmd` 或改走 `pnpm` 脚本均可。
- 图标使用 `@iconify-json/lucide` 本地集合，桌面端离线可用。
