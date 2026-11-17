<script setup lang="ts">
import { computed } from "vue";
import { useDateFormat, useNow } from "@vueuse/core";
import { isTauri } from "@tauri-apps/api/core";

/** 实时时钟：useNow 默认每秒刷新一次，组件卸载时自动停止定时器 */
const now = useNow();
const clock = useDateFormat(now, "YYYY-MM-DD HH:mm:ss");

/** 判断当前跑在 Tauri 壳里还是普通浏览器里，用于展示运行环境 */
const runtime = computed(() => (isTauri() ? "Tauri 桌面" : "浏览器"));

/** 技术栈卡片，与项目实际依赖保持一致 */
const stack = [
  {
    icon: "i-lucide-zap",
    title: "Vite+",
    version: "1.7.0",
    desc: "统一 dev / build / check / test 工具链，配置收敛在一份 vite.config.ts。",
  },
  {
    icon: "i-lucide-component",
    title: "Vue 3",
    version: "3.5",
    desc: "Composition API + <script setup>，搭配 vue-router 做路由组织。",
  },
  {
    icon: "i-lucide-palette",
    title: "Nuxt UI",
    version: "4.12",
    desc: "Tailwind 4 之上的组件库，组件与组合式函数自动按需引入。",
  },
  {
    icon: "i-lucide-link",
    title: "VueUse",
    version: "15.0",
    desc: "useColorMode / useNow / useDateFormat 等组合式工具集。",
  },
  {
    icon: "i-lucide-monitor-smartphone",
    title: "Tauri",
    version: "2.12",
    desc: "Rust 后端的桌面壳，复用系统 WebView，产物体积远小于 Electron。",
  },
];
</script>

<template>
  <section class="space-y-10">
    <!-- 首屏：确认脚手架可用 -->
    <div class="space-y-5">
      <div class="flex flex-wrap items-center gap-2">
        <UBadge color="primary" variant="subtle">脚手架就绪</UBadge>
        <UBadge color="neutral" variant="outline">运行于 {{ runtime }}</UBadge>
      </div>

      <h1 class="text-3xl font-semibold tracking-tight sm:text-4xl">zoro</h1>

      <p class="max-w-2xl text-base text-muted">
        由 Vite+ 脚手架、Vue 3、Nuxt UI、VueUse 与 Tauri 组成的起步项目。开发时用
        <code class="rounded bg-elevated px-1.5 py-0.5 text-sm">vp dev</code>
        跑浏览器，用
        <code class="rounded bg-elevated px-1.5 py-0.5 text-sm">vp run desktop:dev</code>
        拉起桌面窗口。
      </p>

      <div class="flex flex-wrap gap-3">
        <UButton icon="i-lucide-rocket" to="/stack">查看技术栈</UButton>
        <UButton
          icon="i-lucide-book-open"
          color="neutral"
          variant="outline"
          to="https://ui.nuxt.com/components"
          target="_blank"
        >
          Nuxt UI 组件
        </UButton>
      </div>

      <!-- 托盘行为说明：只在桌面端展示，浏览器里没有托盘概念 -->
      <UAlert
        v-if="isTauri()"
        color="info"
        variant="subtle"
        icon="i-lucide-panel-top"
        title="应用已驻留菜单栏托盘"
        description="点击窗口关闭按钮只是隐藏主窗口，应用仍在后台运行；点击菜单栏托盘图标可快速打开最近项目，关闭应用请使用托盘菜单里的「退出」。"
      />
    </div>

    <!-- VueUse 实时能力演示 -->
    <UCard>
      <div class="flex flex-wrap items-center justify-between gap-4">
        <div class="space-y-1">
          <p class="text-sm font-medium">当前时间</p>
          <p class="text-xs text-muted">由 useNow + useDateFormat 驱动，每秒响应式更新</p>
        </div>
        <p class="font-mono text-2xl tabular-nums">{{ clock }}</p>
      </div>
    </UCard>

    <!-- 技术栈卡片 -->
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <UCard v-for="item in stack" :key="item.title">
        <div class="space-y-3">
          <div class="flex items-center gap-2">
            <UIcon :name="item.icon" class="size-5 text-primary" />
            <span class="font-medium">{{ item.title }}</span>
            <UBadge class="ml-auto" color="neutral" variant="subtle" size="sm">
              {{ item.version }}
            </UBadge>
          </div>
          <p class="text-sm leading-relaxed text-muted">{{ item.desc }}</p>
        </div>
      </UCard>
    </div>
  </section>
</template>
