<script setup lang="ts">
import { useColorMode } from "@vueuse/core";

/**
 * 明暗主题：Nuxt UI 的组件样式跟着 html 上的 .dark 类走，
 * 而 @vueuse/core 的 useColorMode 默认就是往 <html> 写 class 并持久化到 localStorage，
 * 两者天然对齐，所以这里直接用 vueuse 的实现而不是 Nuxt UI 的 colorMode 插件。
 */
const colorMode = useColorMode();

/** 在明暗之间直接翻转，不做 system 态的回退，避免点击后看起来“没反应” */
function toggleColorMode() {
  colorMode.value = colorMode.value === "dark" ? "light" : "dark";
}
</script>

<template>
  <!-- UApp 提供 toast / tooltip 等全局浮层的挂载点，必须包在最外层；toast 固定在窗口顶部居中 -->
  <UApp :toaster="{ position: 'top-center' }">
    <div class="flex min-h-screen flex-col bg-default text-default">
      <!--
        macOS 用 Overlay 标题栏（原生红绿灯悬浮在内容上，无标题文字）。
        仿 Tiny RDM 的紧凑标题栏：高 40px，标题水平居中，红绿灯经
        trafficLightPosition 精确落在同一水平线上。
        data-tauri-drag-region 只作用于挂载元素本身，按钮不挂以免吞掉点击；
        居中标题块用 pointer-events-none 让事件落到 header 上，整条都能拖。
      -->
      <header
        class="sticky top-0 z-10 h-10 border-b border-default bg-default/80 backdrop-blur"
        data-tauri-drag-region
      >
        <div
          class="pointer-events-none absolute inset-0 flex items-center justify-center gap-1.5 text-sm font-semibold select-none"
          data-tauri-drag-region
        >
          <UIcon name="i-lucide-hexagon" class="size-4 text-primary" />
          zoro
        </div>

        <div class="absolute inset-y-0 right-2 flex items-center">
          <UButton
            :icon="colorMode === 'dark' ? 'i-lucide-sun' : 'i-lucide-moon'"
            color="neutral"
            variant="ghost"
            size="xs"
            :aria-label="colorMode === 'dark' ? '切换到浅色模式' : '切换到深色模式'"
            @click="toggleColorMode"
          />
        </div>
      </header>

      <main class="mx-auto w-full max-w-5xl flex-1 px-4 py-10">
        <RouterView />
      </main>

      <footer class="border-t border-default py-4 text-center text-xs text-muted">
        Vite+ · Vue 3 · Nuxt UI · VueUse · Tauri
      </footer>
    </div>
  </UApp>
</template>
