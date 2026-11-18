<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useColorMode } from "@vueuse/core";
import { useI18n } from "vue-i18n";
import { desktop, type DesktopConfig } from "./api/desktop";
import { useAppTheme } from "./composables/useAppTheme";

/**
 * 明暗主题：Nuxt UI 的组件样式跟着 html 上的 .dark 类走，
 * 而 @vueuse/core 的 useColorMode 默认就是往 <html> 写 class，
 * 两者天然对齐。这里只负责应用逻辑，偏好来源是后端配置
 * （settings.theme），由 useAppTheme 统一watch并应用。
 */
const colorMode = useColorMode();
const { t } = useI18n();

/** 配置快照：App 层只需要 theme（语言等由页面组件各自消费） */
const config = ref<DesktopConfig | null>(null);
useAppTheme(config);

/**
 * 头部明暗切换按钮：按当前实际生效的主题取反（而非配置值取反——
 * "跟随系统"时取反也要落在一个确定的主题上），并持久化到配置。
 */
function toggleColorMode() {
  const dark = colorMode.value === "dark";
  void desktop.setTheme(dark ? "light" : "dark").then((c) => (config.value = c));
}

/** 配置变更监听的取消函数；卸载时解绑 */
let offConfigChanged: (() => void) | null = null;

onMounted(() => {
  void desktop
    .getConfig()
    .then((c) => (config.value = c))
    .catch(() => {});
  offConfigChanged = desktop.onConfigChanged(() => {
    void desktop
      .getConfig()
      .then((c) => (config.value = c))
      .catch(() => {});
  });
});
</script>

<template>
  <!-- UApp 提供 toast / tooltip 等全局浮层的挂载点，必须包在最外层；
       toast 固定在窗口顶部居中；progress: false 关掉 toast 底部的倒计时进度条 -->
  <UApp :toaster="{ position: 'top-center', progress: false }">
    <div class="flex min-h-screen flex-col bg-default text-default">
      <!--
        macOS 用 Overlay 标题栏（原生红绿灯悬浮在内容上，无标题文字）。
        标题水平居中：absolute + inset-x-0 + 居中布局，与 macOS 原生窗口
        标题习惯一致；红绿灯悬浮在左上角，不占布局空间。
        data-tauri-drag-region 只作用于挂载元素本身，按钮不挂以免吞掉点击。
      -->
      <header
        class="sticky top-0 z-10 h-10 border-b border-default bg-default/80 backdrop-blur"
        data-tauri-drag-region
      >
        <div
          class="pointer-events-none absolute inset-x-0 inset-y-0 flex items-center justify-center gap-1.5 text-sm font-semibold select-none"
          data-tauri-drag-region
        >
          <UIcon name="i-lucide-hexagon" class="size-4 text-primary" />
          Zoro
        </div>

        <div class="absolute inset-y-0 right-2 flex items-center">
          <UButton
            :icon="colorMode === 'dark' ? 'i-lucide-sun' : 'i-lucide-moon'"
            color="neutral"
            variant="ghost"
            size="xs"
            :aria-label="colorMode === 'dark' ? t('app.toLightMode') : t('app.toDarkMode')"
            @click="toggleColorMode"
          />
        </div>
      </header>

      <main class="mx-auto w-full max-w-5xl flex-1 px-4 py-10">
        <RouterView />
      </main>
    </div>
  </UApp>
</template>
