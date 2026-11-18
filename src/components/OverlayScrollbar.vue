<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

/**
 * 覆盖式滚动条容器（macOS 原生风格）。
 *
 * 为什么要自绘：Tauri (WKWebView) 里自定义 ::-webkit-scrollbar 会把
 * 原生 overlay 滚动条降级成「占内容宽度的经典滚动条」，系统「始终显示
 * 滚动条」设置下会永久占着一条灰色大滚动条。这里反过来：
 *   1. 视口隐藏原生滚动条（scrollbar-width: none + ::-webkit-scrollbar
 *      display:none）——零布局宽度，内容布局与窗口尺寸不受影响；
 *   2. 右侧叠一个绝对定位的半透明圆角滑块（不占布局空间），
 *      鼠标移入容器或正在滚动时淡入，停止滚动且鼠标离开后淡出。
 *
 * 滚动行为完全交给原生视口（滚轮 / 触控板 / 键盘均正常），本组件只
 * 读取 scrollTop/scrollHeight 计算滑块的位置与长度。
 */
const viewport = ref<HTMLElement | null>(null);
/** 内容包裹层：用 ResizeObserver 监听内容高度变化（列表增删、窗口缩放） */
const content = ref<HTMLElement | null>(null);

/** 滑块几何：top 为距视口顶部的像素，height 为滑块长度 */
const thumbTop = ref(0);
const thumbHeight = ref(0);
/** 内容是否真的溢出（不溢出时连滑块带留白都不渲染） */
const hasOverflow = ref(false);

/** 鼠标是否悬停在容器上 */
const hovering = ref(false);
/** 是否正在滚动（滚动开始置 true，停止 700ms 后复位） */
const scrolling = ref(false);
let scrollTimer: ReturnType<typeof setTimeout> | null = null;

/** 滑块是否可见：悬停或滚动中任一成立即显示，其余时间淡出隐藏 */
const visible = computed(() => hovering.value || scrolling.value);

/** 由当前滚动状态重算滑块几何；滑块最短 32px，接近原生 macOS 观感 */
function updateThumb() {
  const el = viewport.value;
  if (!el) return;
  hasOverflow.value = el.scrollHeight > el.clientHeight + 1;
  const track = el.clientHeight;
  thumbHeight.value = Math.max(32, (track / el.scrollHeight) * track);
  const maxScroll = el.scrollHeight - el.clientHeight;
  const ratio = maxScroll > 0 ? el.scrollTop / maxScroll : 0;
  thumbTop.value = ratio * (track - thumbHeight.value);
}

/** 滚动事件：刷新滑块 + 标记「滚动中」，停止后延迟淡出 */
function onScroll() {
  updateThumb();
  scrolling.value = true;
  if (scrollTimer) clearTimeout(scrollTimer);
  scrollTimer = setTimeout(() => (scrolling.value = false), 700);
}

/** 鼠标离开容器：等一小段（可能正要滚轮）再淡出，观感更接近原生 */
function onLeave() {
  hovering.value = false;
}

let ro: ResizeObserver | null = null;

onMounted(() => {
  const el = viewport.value;
  if (!el) return;
  // 滚动与几何初始化
  el.addEventListener("scroll", onScroll, { passive: true });
  updateThumb();
  // 内容尺寸变化（折叠/展开、列表增删）与视口尺寸变化都要重算
  ro = new ResizeObserver(() => updateThumb());
  if (content.value) ro.observe(content.value);
  ro.observe(el);
});

onBeforeUnmount(() => {
  viewport.value?.removeEventListener("scroll", onScroll);
  ro?.disconnect();
  if (scrollTimer) clearTimeout(scrollTimer);
});
</script>

<template>
  <!-- 外层负责悬停检测与叠放滑块；尺寸由使用方决定（flex-1 / h-full 等） -->
  <div
    class="relative min-h-0"
    @mouseenter="hovering = true"
    @mouseleave="onLeave"
  >
    <!-- 视口：滚动行为的真正载体；osb-none 在 <style> 里隐藏原生滚动条。
         max-h-[inherit]：外层根元素用 max-h-80 限高时视口跟着继承，
         组件既能在定高 flex 容器里用，也能包一段限高的列表 -->
    <div ref="viewport" class="osb-viewport h-full max-h-[inherit]">
      <div ref="content">
        <slot />
      </div>
    </div>

    <!-- 覆盖式滑块：绝对定位不占布局宽度；pointer-events-none 不挡点击 -->
    <div
      v-if="hasOverflow"
      class="pointer-events-none absolute inset-y-0 right-0 w-2"
    >
      <Transition name="osb-fade">
        <div
          v-show="visible"
          class="absolute right-[3px] w-[5px] rounded-full bg-neutral-400/45 dark:bg-neutral-500/45"
          :style="{ top: `${thumbTop}px`, height: `${thumbHeight}px` }"
        />
      </Transition>
    </div>
  </div>
</template>

<style>
/*
 * 隐藏原生滚动条——必须用非 scoped 全局样式。
 * 实测 WKWebView（Tauri macOS）里 scoped 属性选择器对
 * ::-webkit-scrollbar 伪元素不生效，且仅 display:none 也不够，
 * 需要 width/height 同时归零才能真正隐藏并释放占位。
 * Firefox 走 scrollbar-width: none。
 */
.osb-viewport {
  scrollbar-width: none;
}
.osb-viewport::-webkit-scrollbar {
  display: none;
  width: 0;
  height: 0;
  background: transparent;
}
</style>

<style scoped>
/* 淡入淡出：200ms 出 / 400ms 收，与 macOS 原生滚动条的节奏接近 */
.osb-fade-enter-active {
  transition: opacity 200ms ease-out;
}
.osb-fade-leave-active {
  transition: opacity 400ms ease-in;
}
.osb-fade-enter-from,
.osb-fade-leave-to {
  opacity: 0;
}
</style>
