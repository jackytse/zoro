<script setup lang="ts">
/** 常用命令，按使用频率排序 */
const commands = [
  { cmd: "vp dev", desc: "启动 Vite+ 开发服务器（端口 5173）" },
  { cmd: "vp run desktop:dev", desc: "启动 Tauri 桌面窗口，自动拉起前端 dev server" },
  { cmd: "vp check", desc: "一次性跑格式化 + lint + 类型检查，加 --fix 自动修复" },
  { cmd: "vp build", desc: "Rolldown 生产构建，产物输出到 dist/" },
  { cmd: "vp run desktop:build", desc: "打包桌面安装包，产物在 src-tauri/target/release/bundle" },
  { cmd: "vp test", desc: "跑 Vitest 单测（默认执行一次，watch 需显式加 watch）" },
];

/** 目录职责说明 */
const layout = [
  { path: "src/views", desc: "页面级组件，与路由一一对应" },
  { path: "src/router", desc: "vue-router 路由表" },
  { path: "src/assets/css", desc: "Tailwind 与 Nuxt UI 样式入口" },
  { path: "src-tauri", desc: "Rust 桌面壳：tauri.conf.json、窗口能力、打包配置" },
  { path: "vite.config.ts", desc: "Vite+ 统一配置：插件、lint、fmt、staged" },
];
</script>

<template>
  <section class="space-y-8">
    <div class="space-y-2">
      <h1 class="text-2xl font-semibold tracking-tight">技术栈与命令</h1>
      <p class="text-sm text-muted">初始化后可直接使用的命令与目录约定。</p>
    </div>

    <UCard>
      <template #header>
        <p class="font-medium">常用命令</p>
      </template>
      <ul class="divide-y divide-default">
        <li
          v-for="item in commands"
          :key="item.cmd"
          class="flex flex-col gap-1 py-3 first:pt-0 last:pb-0 sm:flex-row sm:items-center sm:gap-4"
        >
          <code class="w-fit rounded bg-elevated px-2 py-1 font-mono text-sm">{{ item.cmd }}</code>
          <span class="text-sm text-muted">{{ item.desc }}</span>
        </li>
      </ul>
    </UCard>

    <UCard>
      <template #header>
        <p class="font-medium">目录职责</p>
      </template>
      <ul class="divide-y divide-default">
        <li
          v-for="item in layout"
          :key="item.path"
          class="flex flex-col gap-1 py-3 first:pt-0 last:pb-0 sm:flex-row sm:items-center sm:gap-4"
        >
          <code class="w-fit font-mono text-sm text-primary">{{ item.path }}</code>
          <span class="text-sm text-muted">{{ item.desc }}</span>
        </li>
      </ul>
    </UCard>
  </section>
</template>
