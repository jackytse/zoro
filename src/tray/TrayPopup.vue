<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useToast } from "@nuxt/ui/composables";
import { desktop, type DesktopConfig, type Project } from "../api/desktop";

const toast = useToast();

/** 配置快照；null 表示还在加载首屏 */
const config = ref<DesktopConfig | null>(null);

/** 工具 id -> Tool 的索引，避免渲染时反复 find */
const toolMap = computed(() => {
  const map = new Map<string, DesktopConfig["tools"][number]>();
  for (const tool of config.value?.tools ?? []) {
    map.set(tool.id, tool);
  }
  return map;
});

/** 按最近打开时间倒序的项目列表（从未打开过的排在最后） */
const recentProjects = computed(() => {
  const projects = [...(config.value?.projects ?? [])];
  return projects.sort((a, b) => (b.lastOpenedAt ?? 0) - (a.lastOpenedAt ?? 0));
});

const isLoading = ref(true);

async function loadConfig() {
  try {
    config.value = await desktop.getConfig();
  } catch (error) {
    toast.add({
      title: "读取配置失败",
      description: String(error),
      color: "error",
    });
  } finally {
    isLoading.value = false;
  }
}

/** 点击项目：交给后端用对应工具打开；后端成功后会自动隐藏弹窗 */
async function openProject(project: Project) {
  const tool = toolMap.value.get(project.toolId);
  try {
    await desktop.openProject(project.id);
  } catch (error) {
    // 打开失败必须给用户明确反馈：显示工具名 + 后端给出的原因
    toast.add({
      title: `无法用 ${tool?.name ?? "所选工具"} 打开「${project.name}」`,
      description: String(error),
      color: "error",
    });
  }
}

/** 底部「设置」：显示并聚焦主窗口 */
async function goSettings() {
  try {
    await desktop.showMainWindow();
  } catch (error) {
    toast.add({ title: "打开设置失败", description: String(error), color: "error" });
  }
}

/** 底部「退出」：窗口关闭只是隐藏，这里提供真正的进程出口 */
async function quitApp() {
  try {
    await desktop.quitApp();
  } catch (error) {
    toast.add({ title: "退出失败", description: String(error), color: "error" });
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    void desktop.hideTrayPopup();
  }
}

/** 相对时间展示，让「最近」有排序的直观感受 */
function formatTime(ts: number | null): string {
  if (!ts) return "未打开过";
  const diff = Date.now() - ts;
  const minute = 60_000;
  if (diff < minute) return "刚刚";
  if (diff < 60 * minute) return `${Math.floor(diff / minute)} 分钟前`;
  if (diff < 24 * 60 * minute) return `${Math.floor(diff / (60 * minute))} 小时前`;
  return new Date(ts).toLocaleDateString("zh-CN");
}

let offConfigChanged: (() => void) | null = null;

onMounted(async () => {
  await loadConfig();
  // 设置页增删项目后，弹窗里的列表要跟着刷新
  offConfigChanged = desktop.onConfigChanged(() => void loadConfig());
  window.addEventListener("keydown", onKeydown);
});

onBeforeUnmount(() => {
  offConfigChanged?.();
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <!-- 窗口本体是透明的，这里负责画出圆角卡片；阴影由系统窗口阴影提供 -->
  <UApp>
    <div class="flex h-screen flex-col overflow-hidden rounded-xl bg-default text-default">
      <header class="flex items-center gap-2 border-b border-default px-4 py-3">
        <UIcon name="i-lucide-clock-arrow-up" class="size-4 text-primary" />
        <span class="text-sm font-medium">最近项目</span>
        <UBadge v-if="config" color="neutral" variant="subtle" size="sm" class="ml-auto">
          {{ recentProjects.length }}
        </UBadge>
      </header>

      <main class="min-h-0 flex-1 overflow-y-auto p-2">
        <!-- 加载中 -->
        <div v-if="isLoading" class="flex h-full flex-col gap-2 p-2">
          <USkeleton v-for="i in 4" :key="i" class="h-12 rounded-lg" />
        </div>

        <!-- 空状态：没有任何已配置项目时的引导 -->
        <div
          v-else-if="recentProjects.length === 0"
          class="flex h-full flex-col items-center justify-center gap-3 px-6 text-center"
        >
          <UIcon name="i-lucide-folder-search" class="size-10 text-dimmed" />
          <p class="text-sm font-medium">还没有配置项目</p>
          <!-- w-full 很关键：flex 容器里不定宽的文本会按 max-content 撑开，
               长句会溢出窗口而不是换行 -->
          <p class="w-full text-xs leading-relaxed text-muted">
            在主窗口的「设置」里添加常用项目路径并选择开发工具，就会出现在这里。
          </p>
          <UButton size="sm" icon="i-lucide-settings-2" @click="goSettings">去设置</UButton>
        </div>

        <!-- 项目列表 -->
        <ul v-else class="flex flex-col gap-0.5">
          <li v-for="project in recentProjects" :key="project.id">
            <button
              type="button"
              class="group flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors hover:bg-elevated focus-visible:bg-elevated focus-visible:outline-none"
              @click="openProject(project)"
            >
              <UIcon
                :name="toolMap.get(project.toolId)?.icon ?? 'i-lucide-box'"
                class="size-5 shrink-0 text-primary"
              />
              <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-medium">{{ project.name }}</span>
                <span class="block truncate text-xs text-muted">
                  {{ toolMap.get(project.toolId)?.name ?? "未知工具" }} · {{ project.path }}
                </span>
              </span>
              <span class="shrink-0 text-xs text-dimmed">
                {{ formatTime(project.lastOpenedAt) }}
              </span>
              <UIcon
                name="i-lucide-arrow-up-right"
                class="size-4 shrink-0 text-dimmed opacity-0 transition-opacity group-hover:opacity-100"
              />
            </button>
          </li>
        </ul>
      </main>

      <footer class="flex items-center gap-1 border-t border-default px-3 py-2">
        <UButton
          size="sm"
          color="neutral"
          variant="ghost"
          icon="i-lucide-settings-2"
          class="ml-auto"
          @click="goSettings"
        >
          设置
        </UButton>
        <UButton
          size="sm"
          color="neutral"
          variant="ghost"
          icon="i-lucide-power"
          aria-label="退出应用"
          @click="quitApp"
        />
      </footer>
    </div>
  </UApp>
</template>
