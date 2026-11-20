<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useNotify } from "../composables/useNotify";
import { desktop, type DesktopConfig, type Project } from "../api/desktop";
import { applyLocale, resolveLocale } from "../i18n";
import { useAppTheme } from "../composables/useAppTheme";
import { useUpdater } from "../composables/useUpdater";
import ToolIcon from "../components/ToolIcon.vue";
import OverlayScrollbar from "../components/OverlayScrollbar.vue";
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

/**
 * 弹窗内的更新检测状态机（复用 useUpdater，与设置页各自独立的实例）。
 * 弹窗是独立 webview，与主窗口同 origin，localStorage 里的通道选择
 * （zoro.update-channel）天然共享——用户在设置页切到预发布通道后，
 * 这里检测的就是预发布清单。
 */
const {
  phase: updatePhase,
  newVersion,
  progress: downloadProgress,
  checkForUpdate,
  downloadAndInstall,
  restartApp,
  formatMB,
} = useUpdater();

/** 左下角「检测更新」：结果不用 toast（弹窗是独立 webview，toast 表现
 *  不稳定），直接在按钮旁内联显示状态文字，颜色按语义区分 */
async function checkUpdateFromPopup() {
  await checkForUpdate();
}

/** 内联「立即更新」回调：下载安装完成后按钮旁换成「重启应用」入口。
 *  即便用户中途收起弹窗（窗口只是隐藏），Rust 侧下载仍会继续完成 */
async function installFromPopup() {
  await downloadAndInstall();
}

/** 下载中的内联进度文字：已下载 / 总大小（总大小拿不到时只显示已下载） */
const downloadLabel = computed(() => {
  const p = downloadProgress.value;
  const total = p.total ? ` / ${formatMB(p.total)} MB` : "";
  return t("settings.downloading", { progress: `${formatMB(p.downloaded)} MB${total}` });
});

/** 在系统终端里打开项目目录（图标按钮触发）。
 *  成功不打扰：终端窗口弹起本身就是反馈，失败才提示原因 */
async function openInTerminal(project: Project) {
  try {
    await desktop.openInTerminal(project.path);
  } catch (error) {
    notify("error", t("tray.terminalOpenFailed"), { description: String(error) });
  }
}

/** 用系统文件管理器打开项目目录（图标按钮触发）。
 *  与终端入口同理：成功不打扰，失败才提示原因 */
async function openInFolder(project: Project) {
  try {
    await desktop.openInFolder(project.path);
  } catch (error) {
    notify("error", t("tray.folderOpenFailed"), { description: String(error) });
  }
}

/** 配置变更监听的取消函数；卸载时解绑，避免重复订阅 */
let offConfigChanged: (() => void) | null = null;
/** Cmd+Q 二次确认提示监听的取消函数 */
let offQuitConfirmHint: (() => void) | null = null;

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    void desktop.hideTrayPopup();
    return;
  }
  // Cmd/Ctrl + W：与主窗口的「关闭窗口」快捷键对齐——弹窗是无边框窗口，
  // 收不到应用菜单预定义项的 performClose，这里手动等价于 Esc 隐藏
  if (event.key === "w" && (event.metaKey || event.ctrlKey)) {
    event.preventDefault();
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
  // Cmd+Q 首次按下：与主窗口一致的 toast 提示（用户可能在弹窗聚焦时按快捷键）
  offQuitConfirmHint = desktop.onQuitConfirmHint(() => {
    notify("info", t("tray.quitConfirmHint"), { duration: 3000 });
  });
  window.addEventListener("keydown", onKeydown);
});

onBeforeUnmount(() => {
  offConfigChanged?.();
  offQuitConfirmHint?.();
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

      <!-- 列表滚动区：覆盖式滚动条，默认隐藏、悬停/滚动时半透明淡入 -->
      <OverlayScrollbar class="min-h-0 flex-1">
        <main class="p-2">
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
              <!-- 悬停出现的文件管理器入口：用系统文件管理器打开项目目录；
                   click.stop 避免触发整行的「用 IDE 打开」 -->
              <UButton
                icon="i-lucide-folder-open"
                color="neutral"
                variant="ghost"
                size="xs"
                class="shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
                :aria-label="t('tray.openInFolderNamed', { name: project.name })"
                :title="t('tray.openInFolder')"
                @click.stop="openInFolder(project)"
              />
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
      </OverlayScrollbar>

      <footer class="flex items-center gap-1 border-t border-default px-3 py-2">
        <!-- 左下角检测更新 + 内联结果：不用 toast（独立 webview 里不可靠），
             检测完成后状态文字直接显示在按钮右侧；文字颜色按语义区分——
             常态 muted、新版本 primary、失败 error，均随明暗主题自动适配 -->
        <div class="mr-auto flex min-w-0 items-center gap-2">
          <UButton
            size="sm"
            color="neutral"
            variant="ghost"
            icon="i-lucide-refresh-cw"
            class="shrink-0"
            :loading="updatePhase === 'checking'"
            :aria-label="t('tray.checkUpdate')"
            @click="checkUpdateFromPopup"
          >
            {{ t("tray.checkUpdate") }}
          </UButton>
          <span v-if="updatePhase === 'latest'" class="truncate text-xs text-muted">
            {{ t("settings.upToDate") }}
          </span>
          <template v-else-if="updatePhase === 'available'">
            <span class="truncate text-xs text-primary">
              {{ t("settings.newVersionFound", { version: newVersion }) }}
            </span>
            <UButton
              size="xs"
              color="primary"
              variant="soft"
              class="shrink-0"
              @click="installFromPopup"
            >
              {{ t("settings.updateNow") }}
            </UButton>
          </template>
          <span v-else-if="updatePhase === 'downloading'" class="truncate text-xs text-muted">
            {{ downloadLabel }}
          </span>
          <UButton
            v-else-if="updatePhase === 'ready'"
            size="xs"
            color="primary"
            variant="soft"
            class="shrink-0"
            @click="restartApp"
          >
            {{ t("settings.restart") }}
          </UButton>
          <span v-else-if="updatePhase === 'error'" class="truncate text-xs text-error">
            {{ t("settings.updateCheckFailed") }}
          </span>
        </div>
        <UButton
          size="sm"
          color="neutral"
          variant="ghost"
          icon="i-lucide-settings-2"
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
