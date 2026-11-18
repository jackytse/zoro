<script setup lang="ts">
/**
 * 开发工具图标：优先渲染内置的品牌图标（VS Code / Cursor / IDEA / iTerm），
 * 未知工具回退到配置里的 iconify 图标名（UIcon）。
 *
 * 为什么用内联 SVG 而不是 UIcon：品牌图标是彩色多路径图形，经 iconify
 * 集合注册需要额外依赖与在线集合；内联渲染零依赖、离线可用，且能保留
 * 官方彩色渐变（彩色图标自带 fill，单色图标用 currentColor 跟随明暗主题）。
 */
import { computed } from "vue";
import { getBrandIcon } from "../icons/brands";

const props = withDefaults(
  defineProps<{
    /** 工具 id（vscode / cursor / idea / iterm…），决定用哪个品牌图标 */
    toolId: string;
    /** 没有对应品牌图标时的 iconify 回退名（一般取配置里的 tool.icon） */
    fallbackIcon?: string;
    /** 尺寸 / 颜色等样式类，默认与 UIcon 的 size-5 对齐 */
    iconClass?: string;
  }>(),
  { fallbackIcon: "i-lucide-folder-git-2", iconClass: "size-5" },
);

/**
 * 拼出完整的 <svg> 字符串交给 v-html。
 *
 * 注意：v-html 必须落在普通 HTML 元素（span）上，不能直接落在 <svg> 上——
 * 后者依赖 SVGElement.innerHTML，WebKit 上历史行为不一致（可能按 HTML
 * 命名空间解析导致整块不渲染）。放进 span 则由 HTML 解析器的 foreign
 * content 规则正确处理，各浏览器一致。
 */
const markup = computed(() => {
  const icon = getBrandIcon(props.toolId);
  if (!icon) return null;
  // 不设 fill="none"：IDEA 图标里有一块无 fill 的黑色底方（依赖默认填充），
  // 设成 none 会把品牌底色调丢
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${icon.viewBox}" ` +
    `width="100%" height="100%" aria-hidden="true" focusable="false">` +
    icon.body +
    `</svg>`
  );
});
</script>

<template>
  <!-- 品牌图标：外层 span 承载尺寸类，内层 svg 铺满，视觉大小与 UIcon 的 size-5 对齐 -->
  <span
    v-if="markup"
    class="inline-flex shrink-0 items-center justify-center [&>svg]:block"
    :class="iconClass"
    v-html="markup"
  />
  <UIcon v-else :name="fallbackIcon" :class="iconClass" />
</template>
