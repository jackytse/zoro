import { fileURLToPath, URL } from "node:url";

import { defineConfig, lazyPlugins } from "vite-plus";
import vue from "@vitejs/plugin-vue";
import ui from "@nuxt/ui/vite";

// Tauri 在移动端调试时会通过该变量下发局域网地址
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig({
  staged: {
    "*": "vp check --fix",
  },
  fmt: {},
  lint: {
    jsPlugins: [{ name: "vite-plus", specifier: "vite-plus/oxlint-plugin" }],
    rules: { "vite-plus/prefer-vite-plus-imports": "error" },
    options: { typeAware: true, typeCheck: true },
  },
  // Nuxt UI 的 Vite 插件内部注册了 unplugin-auto-import / unplugin-vue-components，
  // 会生成 auto-imports.d.ts、components.d.ts，因此必须和其他插件一起交给 lazyPlugins。
  // icon.clientBundle.scan：构建期扫描源码里用到的 i-lucide-* 图标并内联数据——
  // 否则 UIcon 运行时从 api.iconify.design 拉取，被 tauri CSP 的 connect-src
  // 'self' 拦截，打包版图标全部不显示（v0.1.12 实测）。
  // globInclude 追加 Rust 源码：工具的默认图标名（ToolIcon 的 fallback-icon
  // 来源）定义在 src-tauri/src/config.rs，不在前端扫描范围内
  plugins: lazyPlugins(() => [
    vue(),
    ui({
      icon: {
        clientBundle: {
          scan: {
            globInclude: [
              "**/*.{vue,jsx,tsx,md,mdc,mdx}",
              "src-tauri/src/**/*.rs",
            ],
          },
        },
      },
    }),
  ]),
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  // 允许在业务代码里读取 TAURI_ENV_* 注入的平台/调试信息
  envPrefix: ["VITE_", "TAURI_ENV_"],
  // Tauri 的 CLI 输出需要保留，不能被 Vite 清屏覆盖
  clearScreen: false,
  server: {
    // 浏览器开发用 5173；桌面端由 `dev:desktop` 脚本用 --port 1420 覆盖，
    // 两边端口分开才能同时开着而不抢占（Tauri 的 devUrl 写死 1420）
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Rust 侧代码由 cargo 监听，Vite 不必重复监听
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // 多入口：index.html 是主窗口，tray.html 是托盘弹窗（独立小入口，不加载主应用路由）
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        tray: fileURLToPath(new URL("./tray.html", import.meta.url)),
      },
    },
    // Tauri 使用系统 WebView：Windows 是 WebView2，其余是 WebKit。
    // safari16 是 Tailwind CSS 4 的低线，比 Tauri 模板默认的 safari13 更稳妥。
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari16",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
