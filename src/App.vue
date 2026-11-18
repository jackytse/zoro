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
  <!-- UApp 提供 toast / tooltip 等全局浮层的挂载点，必须包在最外层 -->
  <UApp>
    <div class="flex min-h-screen flex-col bg-default text-default">
      <header class="sticky top-0 z-10 border-b border-default bg-default/80 backdrop-blur">
        <div class="mx-auto flex h-14 w-full max-w-5xl items-center gap-6 px-4">
          <span class="flex items-center gap-2 text-base font-semibold">
            <UIcon name="i-lucide-hexagon" class="size-5 text-primary" />
            zoro
          </span>

          <div class="ml-auto">
            <UButton
              :icon="colorMode === 'dark' ? 'i-lucide-sun' : 'i-lucide-moon'"
              color="neutral"
              variant="ghost"
              :aria-label="colorMode === 'dark' ? '切换到浅色模式' : '切换到深色模式'"
              @click="toggleColorMode"
            />
          </div>
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
