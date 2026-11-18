/**
 * 界面主题：把配置里的 theme 偏好应用到当前窗口。
 *
 * 为什么不用 Nuxt UI 的 colorMode 插件：Nuxt UI 组件样式跟着 <html> 上的
 * .dark 类走，@vueuse/core 的 useColorMode 默认就是往 <html> 写 class，
 * 两者天然对齐（与 App.vue 此前的实现一致）。
 *
 * 为什么由配置驱动而不是 localStorage：主窗口与托盘弹窗是两个独立 WebView，
 * localStorage 不保证互通；配置走后端广播（config-changed），两个窗口都能
 * 可靠同步。vueuse 的持久化存储保留但仅作兜底。
 *
 * "auto"（跟随系统）时 vueuse 会监听 prefers-color-scheme，
 * 系统切换明暗时窗口实时跟随，无需重启。
 */
import { watch, type Ref } from "vue";
import { useColorMode } from "@vueuse/core";
import type { DesktopConfig } from "../api/desktop";

export function useAppTheme(config: Ref<DesktopConfig | null>) {
  const colorMode = useColorMode();

  watch(
    () => config.value?.theme,
    (theme) => {
      // None = 跟随系统 → vueuse 的 "auto"；显式 light/dark 直接应用
      colorMode.value = theme === "light" || theme === "dark" ? theme : "auto";
    },
    // immediate：窗口刚打开、配置首屏加载完就应用，避免闪一下默认主题
    { immediate: true },
  );
}
