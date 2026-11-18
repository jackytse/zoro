import { defineAppConfig } from "@nuxt/ui";

/**
 * Nuxt UI 全局定制（Vite 模式下该文件由 @nuxt/ui/vite 插件自动读取）。
 *
 * 卡片内边距收紧一档：设置窗口尺寸缩小后，Nuxt UI 默认的
 * p-4 sm:p-6 让文字与边框之间的留白显得过宽，这里统一压到
 * p-3 sm:p-4，信息密度更接近 macOS 系统设置的风格。
 */
export default defineAppConfig({
  ui: {
    card: {
      slots: {
        header: "p-3 sm:px-4",
        body: "p-3 sm:p-4",
        footer: "p-3 sm:px-4",
      },
    },
  },
});
