<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useNotify } from "../composables/useNotify";
import { desktop, type DesktopConfig, type Project } from "../api/desktop";
import { applyLocale, resolveLocale } from "../i18n";
import { useAppTheme } from "../composables/useAppTheme";
import ToolIcon from "../components/ToolIcon.vue";
import { formatRelativeTime } from "../utils/time";

const notify = useNotify();
const { t } = useI18n();

/** 配置快照；null 表示还在加载首屏 */
const config = ref<DesktopConfig | null>(null);
/** 弹窗窗口的主题跟随配置（useAppTheme 监听并写 <html> 的 .dark 类） */
useAppTheme(config);

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
    // 语言跟随配置热切换：设置页改语言后，广播会让弹窗同步生效
    applyLocale(resolveLocale(config.value.locale));
  } catch (error) {
    notify("error", t("tray.loadConfigFailed"), { description: String(error) });
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
    notify(
      "error",
      t("tray.openFailed", {
        tool: tool?.name ?? t("tray.unknownTool"),
        name: project.name,
      }),
      { description: String(error) },
    );
  }
}

/** 底部「设置」：显示并聚焦主窗口 */
async function goSettings() {
  try {
    await desktop.showMainWindow();
  } catch (error) {
    notify("error", t("tray.openSettingsFailed"), { description: String(error) });
  }
}

/** 底部「退出」：窗口关闭只是隐藏，这里提供真正的进程出口 */
async function quitApp() {
  try {
    await desktop.quitApp();
  } catch (error) {
    notify("error", t("tray.quitFailed"), { description: String(error) });
  }
}

/** 在系统终端里打开项目目录（图标按钮触发）。
 *  成功不打扰：终端窗口弹起本身就是反馈，失败才提示原因 */
async function openInTerminal(project: Project) {
  try {
    await desktop.openInTerminal(project.path);
  } catch (error) {
    notify("error", t("tray.terminalOpenFailed"), { description: String(error) });
  }
}

/** 配置变更监听的取消函数；卸载时解绑，避免重复订阅 */
let offConfigChanged: (() => void) | null = null;

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    void desktop.hideTrayPopup();
    return;
  }
  // Cmd/Ctrl + ,：macOS 通用「打开设置」快捷键，弹窗打开时也能直达设置窗口
  if (event.key === "," && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
    void goSettings();
  }
}

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
  <!-- 窗口本体是透明的，这里负责画出圆角卡片；阴影由系统窗口阴影提供。
       toaster 配置与主窗口 App.vue 保持一致：顶部居中 + 不显示倒计时进度条 -->
  <UApp :toaster="{ position: 'top-center', progress: false }">
    <div class="flex h-screen flex-col overflow-hidden rounded-xl bg-default text-default">
      <header class="flex items-center gap-2 border-b border-default px-4 py-3">
        <UIcon name="i-lucide-clock-arrow-up" class="size-4 text-primary" />
        <span class="text-sm font-medium">{{ t("tray.recentProjects") }}</span>
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
          <p class="text-sm font-medium">{{ t("tray.noProjectTitle") }}</p>
          <!-- w-full 很关键：flex 容器里不定宽的文本会按 max-content 撑开，
               长句会溢出窗口而不是换行 -->
          <p class="w-full text-xs leading-relaxed text-muted">
            {{ t("tray.noProjectHint") }}
          </p>
          <UButton size="sm" icon="i-lucide-settings-2" @click="goSettings">
            {{ t("tray.goSettings") }}
          </UButton>
        </div>

        <!-- 项目列表 -->
        <ul v-else class="flex flex-col gap-0.5">
          <li v-for="project in recentProjects" :key="project.id">
            <button
              type="button"
              class="group flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition-colors hover:bg-elevated focus-visible:bg-elevated focus-visible:outline-none"
              @click="openProject(project)"
            >
              <!-- IDE 品牌图标：一眼区分项目用哪个工具打开 -->
              <ToolIcon
                :tool-id="project.toolId"
                :fallback-icon="toolMap.get(project.toolId)?.icon"
                icon-class="size-5 shrink-0"
              />
              <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-medium">{{ project.name }}</span>
                <span class="block truncate text-xs text-muted">
                  {{ project.path }}
                </span>
              </span>
              <span class="shrink-0 text-xs text-dimmed">
                {{ formatRelativeTime(project.lastOpenedAt) }}
              </span>
              <!-- 悬停出现的终端入口：在系统终端打开项目目录；
                   click.stop 避免触发整行的「用 IDE 打开」 -->
              <UButton
                icon="i-lucide-square-terminal"
                color="neutral"
                variant="ghost"
                size="xs"
                class="shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
                :aria-label="t('tray.openInTerminalNamed', { name: project.name })"
                :title="t('tray.openInTerminal')"
                @click.stop="openInTerminal(project)"
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
          {{ t("tray.settings") }}
        </UButton>
        <UButton
          size="sm"
          color="neutral"
          variant="ghost"
          icon="i-lucide-power"
          :aria-label="t('tray.quitApp')"
          @click="quitApp"
        />
      </footer>
    </div>
  </UApp>
</template>
