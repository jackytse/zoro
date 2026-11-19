<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { open } from "@tauri-apps/plugin-dialog";
import { desktop, type DesktopConfig, type DetectedProject, type Project } from "../api/desktop";
import { applyLocale, resolveLocale, LOCALE_LABELS, type AppLocale } from "../i18n";
import ToolIcon from "../components/ToolIcon.vue";
import OverlayScrollbar from "../components/OverlayScrollbar.vue";
import { useNotify } from "../composables/useNotify";
import { useUpdater, type UpdateChannel } from "../composables/useUpdater";
import { formatRelativeTime } from "../utils/time";

const notify = useNotify();
const { t } = useI18n();

const config = ref<DesktopConfig | null>(null);
const isLoading = ref(true);

/* ---------- 设置页布局：Tabs 分组 ---------- */
/**
 * 设置项增多后单页滚动已经很难找，按使用场景拆成三组：
 * - 通用：外观（主题/语言）、启动（自启/快捷键）、终端行为；
 * - 项目：增删项目、从本机工具导入；
 * - 工具：本机可用工具清单（只读展示）。
 * UTabs 的 items 用显式 value + slot，内容放各自命名插槽里。
 */
const activeTab = ref("general");
const tabItems = computed(() => [
  { label: t("settings.tabGeneral"), icon: "i-lucide-settings-2", value: "general", slot: "general" },
  { label: t("settings.tabProjects"), icon: "i-lucide-folder-open", value: "projects", slot: "projects" },
  { label: t("settings.tabTools"), icon: "i-lucide-wrench", value: "tools", slot: "tools" },
]);

/** 新增项目表单状态 */
const form = ref({
  name: "",
  path: "",
  toolId: "vscode",
});
const isSubmitting = ref(false);

/** 表单校验：路径必填、必须选择工具 */
const formError = computed(() => {
  if (!form.value.path.trim()) return t("settings.pathRequired");
  if (!form.value.toolId) return t("settings.toolRequired");
  return null;
});

/** 打开原生目录选择器；选中后回填路径，名称留空则保存时自动取目录名 */
async function browseFolder() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t("settings.pickFolder"),
    });
    if (typeof selected === "string" && selected) {
      form.value.path = selected;
    }
  } catch (error) {
    notify("error", t("settings.browseFailed"), { description: String(error) });
  }
}

/**
 * 终端类工具 id：不进入「可用工具」清单与「打开工具」下拉——
 * 终端的形态与选择由「通用 → 终端」设置（默认终端 / 打开方式）控制，
 * 项目行的终端图标也独立于工具选择存在，两套入口互不混淆。
 */
const TERMINAL_TOOL_IDS = ["iterm", "terminal"];
const ideTools = computed(() =>
  (config.value?.tools ?? []).filter((tool) => !TERMINAL_TOOL_IDS.includes(tool.id)),
);

const toolOptions = computed(() =>
  ideTools.value.map((tool) => ({
    label: `${tool.name}（${tool.command}）`,
    value: tool.id,
  })),
);

async function loadConfig() {
  try {
    config.value = await desktop.getConfig();
    // 语言跟随配置热切换（弹窗改语言 / 其他窗口同步都会走到这里）
    applyLocale(resolveLocale(config.value.locale));
  } catch (error) {
    notify("error", t("tray.loadConfigFailed"), { description: String(error) });
  } finally {
    isLoading.value = false;
  }
}

/* ---------- 通用设置：外观（主题 / 语言） ---------- */

/** 主题：跟随系统（默认）/ 浅色 / 深色；"system" 映射为后端的 null */
const themeValue = computed(() => config.value?.theme ?? "system");
const themeOptions = [
  { label: t("settings.themeSystem"), value: "system", icon: "i-lucide-monitor" },
  { label: t("settings.themeLight"), value: "light", icon: "i-lucide-sun" },
  { label: t("settings.themeDark"), value: "dark", icon: "i-lucide-moon" },
];

/** 切换主题；广播后两个窗口同步生效。失败时重新拉取配置以回弹 */
async function setTheme(value: string) {
  try {
    config.value = await desktop.setTheme(value === "system" ? null : (value as "light" | "dark"));
  } catch (error) {
    notify("error", t("settings.setFailed"), { description: String(error) });
    config.value = await desktop.getConfig();
  }
}

/** 跟随系统 + 三个显式语言选项；"follow" 映射为后端的 null */
const languageValue = computed(() => config.value?.locale ?? "follow");
const languageOptions = [
  { label: t("settings.followSystem"), value: "follow" },
  ...(Object.keys(LOCALE_LABELS) as AppLocale[]).map((l) => ({
    label: LOCALE_LABELS[l],
    value: l,
  })),
];

/** 切换界面语言；后端广播后两个窗口都会通过 loadConfig 同步生效 */
async function setLanguage(value: string) {
  const locale = value === "follow" ? null : (value as AppLocale);
  try {
    config.value = await desktop.setLocale(locale);
  } catch (error) {
    notify("error", t("settings.setFailed"), { description: String(error) });
    config.value = await desktop.getConfig();
  }
}

async function addProject() {
  if (formError.value) {
    notify("warning", formError.value);
    return;
  }
  isSubmitting.value = true;
  try {
    config.value = await desktop.addProject({
      name: form.value.name,
      path: form.value.path,
      toolId: form.value.toolId,
    });
    notify("success", t("settings.added"));
    // 只清空输入，保留上次选择的工具方便连续添加
    form.value.name = "";
    form.value.path = "";
  } catch (error) {
    notify("error", t("settings.addFailed"), { description: String(error) });
  } finally {
    isSubmitting.value = false;
  }
}

async function removeProject(id: string, name: string) {
  try {
    config.value = await desktop.removeProject(id);
    notify("success", t("settings.removed", { name }));
  } catch (error) {
    notify("error", t("settings.removeFailed"), { description: String(error) });
  }
}

/** 在设置页也能直接试一下打开是否成功，避免等到托盘里才发现配置错误。
 *  成功不打扰：IDE 窗口弹起本身就是反馈，失败才提示原因 */
async function openProject(id: string, name: string) {
  try {
    await desktop.openProject(id);
  } catch (error) {
    notify("error", t("settings.openTryFailed", { name }), { description: String(error) });
  }
}

/* ---------- 从本机开发工具导入最近项目 ---------- */

const isScanning = ref(false);
const importOpen = ref(false);
/** 扫描结果（后端已过滤不存在目录与已配置项） */
const scanItems = ref<DetectedProject[]>([]);
/** 每条候选是否勾选导入，key 为路径 */
const importSelection = ref<Record<string, boolean>>({});
/** 每条候选导入时使用的工具，key 为路径，默认取来源工具 */
const importTool = ref<Record<string, string>>({});

/** 按来源工具汇总扫描结果，展示在弹窗描述里 */
const scanSummary = computed(() => {
  const counts = new Map<string, number>();
  for (const item of scanItems.value) {
    counts.set(item.toolId, (counts.get(item.toolId) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([id, n]) => `${toolLabel(id)} ${t("settings.scanCount", { n })}`)
    .join(" · ");
});

/** 勾选数量，导入按钮据此禁用 */
const selectedCount = computed(() => Object.values(importSelection.value).filter(Boolean).length);

/**
 * 全选框的状态，三态：
 * - true：全部候选已勾选；
 * - false：一个都没勾（或没有候选）；
 * - "indeterminate"：部分勾选（横杠图标），提示「还差几条」。
 * reka-ui 的 CheckboxRoot 用 modelValue === "indeterminate" 表达第三态。
 */
const selectAllState = computed<boolean | "indeterminate">(() => {
  const total = scanItems.value.length;
  if (total === 0 || selectedCount.value === 0) return false;
  return selectedCount.value >= total ? true : "indeterminate";
});

/**
 * 全选 / 取消全选。
 *
 * 点击语义按「当前是否已全选」取反，而不是直接反转传进来的值：
 * 第三态（部分选中）时点一下应当变成「全选」，这是列表批量选择的通用预期。
 */
function toggleSelectAll() {
  const next = selectAllState.value !== true;
  // 逐条写回，保持 importSelection 的 key 集合与候选列表一致（导入时按 key 取值）
  importSelection.value = Object.fromEntries(scanItems.value.map((i) => [i.path, next]));
}

/** 工具 id → 展示名 */
function toolLabel(id: string) {
  return config.value?.tools.find((t) => t.id === id)?.name ?? id;
}

async function scanProjects() {
  isScanning.value = true;
  try {
    const items = await desktop.scanRecentProjects();
    if (items.length === 0) {
      notify("info", t("settings.scanEmpty"), {
        description: t("settings.scanEmptyDesc"),
      });
      return;
    }
    scanItems.value = items;
    // 默认全选、来源工具作为打开工具
    importSelection.value = Object.fromEntries(items.map((i) => [i.path, true]));
    importTool.value = Object.fromEntries(items.map((i) => [i.path, i.toolId]));
    importOpen.value = true;
  } catch (error) {
    notify("error", t("settings.scanFailed"), { description: String(error) });
  } finally {
    isScanning.value = false;
  }
}

/** 逐条导入勾选项；单条失败不中断，最后汇总报告 */
async function importSelected() {
  const targets = scanItems.value.filter((i) => importSelection.value[i.path]);
  let ok = 0;
  const failures: string[] = [];
  for (const item of targets) {
    try {
      // addProject 每次返回全量配置，循环结束即拿到最新快照
      config.value = await desktop.addProject({
        name: item.name,
        path: item.path,
        toolId: importTool.value[item.path] ?? item.toolId,
      });
      ok += 1;
    } catch (error) {
      failures.push(`${item.name}：${String(error)}`);
    }
  }
  importOpen.value = false;

  if (ok > 0) {
    notify("success", t("settings.imported", { n: ok }), {
      description: failures.length
        ? t("settings.importFailures", { n: failures.length })
        : undefined,
    });
  }
  for (const message of failures.slice(0, 3)) {
    notify("error", message, { duration: 8000 });
  }
}

/* ---------- 全局快捷键 ---------- */

const isRecording = ref(false);
const recorderRef = ref<HTMLElement | null>(null);
/** 录入中的组合预览（global-hotkey 格式），null 表示还没按到有效组合 */
const shortcutPreview = ref<string | null>(null);

/** 平台判定：Super 在 macOS 是 ⌘，Windows 是 Win 键 */
const isMac = navigator.platform.toUpperCase().includes("MAC");

/** 把 global-hotkey 格式转成平台化展示文案 */
function formatShortcut(raw: string) {
  return raw
    .split("+")
    .map((t) => (t.toLowerCase() === "super" ? (isMac ? "Cmd" : "Win") : t))
    .join(" + ");
}

/** 当前生效组合的展示（null = 默认，"" = 禁用） */
const shortcutDisplay = computed(() => {
  const custom = config.value?.globalShortcut;
  if (custom === "") return t("settings.shortcutDisabled");
  return formatShortcut(custom ?? "Super+Shift+P");
});

async function startRecording() {
  isRecording.value = true;
  shortcutPreview.value = null;
  // 等弹层渲染完再聚焦，否则按键会落到页面上
  await nextTick();
  recorderRef.value?.focus();
}

function stopRecording() {
  isRecording.value = false;
  shortcutPreview.value = null;
}

/** 录入：捕获按键组合转成 global-hotkey 格式；Esc 取消，退格清空重按 */
function onRecorderKeydown(e: KeyboardEvent) {
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape") {
    stopRecording();
    return;
  }
  if (e.key === "Backspace" || e.key === "Delete") {
    shortcutPreview.value = null;
    return;
  }
  // 主键只接受字母 / 数字 / F1-F12，避免录到方向键等不易解析的键
  let key: string | null = null;
  if (/^[a-z0-9]$/i.test(e.key)) key = e.key.toUpperCase();
  else if (/^F([1-9]|1[0-2])$/.test(e.key)) key = e.key.toUpperCase();
  if (!key) return;
  // 修饰键按 Super > Ctrl > Alt > Shift 固定顺序输出，保证字符串可复现
  const mods: string[] = [];
  if (e.metaKey) mods.push("Super");
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  // 不带修饰键的组合会抢占裸键，明确拒绝
  if (mods.length === 0) {
    shortcutPreview.value = null;
    return;
  }
  shortcutPreview.value = [...mods, key].join("+");
}

/** 保存录入的组合；失败（冲突/非法）由后端回滚并 toast 原因 */
async function saveShortcut() {
  const preview = shortcutPreview.value;
  if (!preview) return;
  try {
    config.value = await desktop.setGlobalShortcut(preview);
    notify("success", t("settings.shortcutUpdated"));
    stopRecording();
  } catch (error) {
    notify("error", t("settings.shortcutSetFailed"), { description: String(error) });
  }
}

/** 恢复平台默认组合 */
async function resetShortcut() {
  try {
    config.value = await desktop.setGlobalShortcut(null);
    notify("success", t("settings.shortcutReset"));
    stopRecording();
  } catch (error) {
    notify("error", t("settings.actionFailed"), { description: String(error) });
  }
}

/** 禁用全局快捷键（仍可点托盘图标呼出） */
async function disableShortcut() {
  try {
    config.value = await desktop.setGlobalShortcut("");
    notify("success", t("settings.shortcutDisabledToast"));
    stopRecording();
  } catch (error) {
    notify("error", t("settings.actionFailed"), { description: String(error) });
  }
}

/* ---------- 项目行操作 ---------- */

/** 更换项目的打开工具；失败时重新拉取配置以回弹下拉框选中态 */
async function changeTool(projectId: string, toolId: string) {
  try {
    config.value = await desktop.setProjectTool(projectId, toolId);
  } catch (error) {
    notify("error", t("settings.changeToolFailed"), { description: String(error) });
    config.value = await desktop.getConfig();
  }
}

/** 在默认终端打开项目目录（设置页项目行的终端图标）。
 *  与托盘弹窗同一后端链路：成功不打扰，失败才提示原因 */
async function openInTerminalFromSettings(project: Project) {
  try {
    await desktop.openInTerminal(project.path);
  } catch (error) {
    notify("error", t("tray.terminalOpenFailed"), { description: String(error) });
  }
}

/** 展示用归一化：旧配置里的 "existing" 等价于 "tab"，避免 RadioGroup 匹配不到 */
const terminalModeDisplay = computed<"window" | "tab">(
  () => (config.value?.terminalOpenMode === "tab" || config.value?.terminalOpenMode === "existing"
    ? "tab"
    : "window"),
);

/** 设置终端打开方式；失败时重新拉取配置以回弹选项状态。
 *  RadioGroup 回调给的是宽泛 string，这里归一化成两个合法值 */
async function setTerminalMode(mode: string) {
  const value: "window" | "tab" = mode === "tab" ? "tab" : "window";
  try {
    config.value = await desktop.setTerminalMode(value);
  } catch (error) {
    notify("error", t("settings.terminalModeFailed"), { description: String(error) });
    config.value = await desktop.getConfig();
  }
}

/* ---------- 通用设置：开机自启 ---------- */

/** 切换开机自启；失败时重新拉取配置以回弹开关状态 */
async function toggleAutostart(enabled: boolean) {
  try {
    config.value = await desktop.setAutostart(enabled);
    notify("success", enabled ? t("settings.autostartOn") : t("settings.autostartOff"));
  } catch (error) {
    notify("error", t("settings.setFailed"), { description: String(error) });
    config.value = await desktop.getConfig();
  }
}

/* ---------- 通用设置：关于与更新 ---------- */

/**
 * 解构出来的是 ref，在模板里自动解包；
 * checkForUpdate / downloadAndInstall 返回 Promise，用包装函数统一弹错误 toast，
 * 状态行本身只展示阶段语义（已是最新 / 发现新版本 / 下载进度 / 待重启）
 */
const {
  phase: updatePhase,
  channel: updateChannel,
  currentVersion,
  newVersion: updateNewVersion,
  progress: updateProgress,
  errorMessage: updateError,
  isChecking,
  loadCurrentVersion,
  setChannel,
  checkForUpdate,
  downloadAndInstall,
  restartApp,
  formatMB,
} = useUpdater();

/** 更新通道选项：正式版随发版节奏，预发布抢鲜但有稳定性风险 */
const updateChannelItems = computed(() => [
  { label: t("settings.updateChannelStable"), value: "stable" },
  { label: t("settings.updateChannelBeta"), value: "beta" },
]);

/** 检测更新；失败时状态行给结论、toast 给原因 */
async function handleCheckUpdate() {
  await checkForUpdate();
  if (updatePhase.value === "error") {
    notify("error", t("settings.updateCheckFailed"), { description: updateError.value });
  }
}

/** 确认后下载安装；失败时同样状态行 + toast 双通道 */
async function handleInstallUpdate() {
  await downloadAndInstall();
  if (updatePhase.value === "error") {
    notify("error", t("settings.updateInstallFailed"), { description: updateError.value });
  }
}

/** 两个镜像仓库的地址：CNB 是主发布渠道（Release 托管安装包与更新清单），
 *  GitHub 仅作源码镜像与 tag 备份 */
const REPO_CNB = "https://cnb.cool/jackytse/zoro";
const REPO_GITHUB = "https://github.com/jackytse/zoro";

/** 打开源码仓库外链（系统默认浏览器）；失败给明确提示而非静默 */
async function openRepo(url: string) {
  try {
    await desktop.openUrl(url);
  } catch (error) {
    notify("error", t("settings.openRepoFailed"), { description: String(error) });
  }
}

/**
 * 状态行文案；null 表示该阶段（初始态）不展示状态行。
 * 下载中拼 MB 进度：总大小拿不到时只显示已下载量
 */
const updateStatusText = computed<string | null>(() => {
  switch (updatePhase.value) {
    case "latest":
      return t("settings.upToDate");
    case "available":
      return t("settings.newVersionFound", { version: updateNewVersion.value });
    case "downloading": {
      const done = formatMB(updateProgress.value.downloaded);
      const total = updateProgress.value.total;
      return t("settings.downloading", {
        progress: total ? `${done} / ${formatMB(total)} MB` : `${done} MB`,
      });
    }
    case "ready":
      return t("settings.installedRestart");
    case "error":
      // updater 原始报错很技术化；最常见的「端点 404 / 没有 release」
      // （Could not fetch a valid release JSON）翻译成用户能懂的说法，
      // 其余错误原样展示，完整信息仍在 toast 的 description 里
      return updateError.value.includes("Could not fetch")
        ? t("settings.updateSourceUnavailable")
        : updateError.value;
    default:
      return null;
  }
});

/** 状态行图标：随阶段变化，颜色区分正常/新版本/出错 */
const updateStatusIcon = computed(() => {
  switch (updatePhase.value) {
    case "latest":
      return "i-lucide-circle-check";
    case "available":
    case "downloading":
    case "ready":
      return "i-lucide-arrow-up-circle";
    default:
      return "i-lucide-circle-alert";
  }
});

/** 图标配色：最新绿色、有更新/下载/待重启主色、出错红色 */
const updateStatusClass = computed(() =>
  updatePhase.value === "error"
    ? "text-error"
    : updatePhase.value === "latest"
      ? "text-success"
      : "text-primary",
);

let offConfigChanged: (() => void) | null = null;

/* ---------- 通用设置：默认终端工具 ---------- */

/** 本机终端扫描结果（iTerm2 / Terminal 等，含安装状态） */
const terminalTools = ref<import("../api/desktop").TerminalTool[]>([]);

/** 「默认终端」下拉选项：自动推荐 + 已装/未装的终端列表（未装的禁用） */
const terminalToolItems = computed(() => [
  { label: t("settings.terminalAuto"), value: "auto" },
  ...terminalTools.value.map((tool) => ({
    label: tool.installed ? tool.name : `${tool.name}（${t("settings.notInstalled")}）`,
    value: tool.id,
    disabled: !tool.installed,
  })),
]);

/** 展示值：用户配置过的终端优先；未配置时默认选中扫描推荐的常用终端。
 *  推荐结果只影响展示与默认态，写入配置后才会持久化 */
const terminalToolDisplay = computed(
  () =>
    config.value?.terminalTool ??
    (terminalTools.value.find((tool) => tool.installed && tool.id === "iterm")
      ? "iterm"
      : terminalTools.value.some((tool) => tool.installed)
        ? "terminal"
        : "auto"),
);

/** 更换默认终端；"auto" = 恢复自动推荐（配置存 null）。失败时回拉配置回弹 */
async function setDefaultTerminal(value: string) {
  try {
    config.value = await desktop.setTerminalTool(value === "auto" ? null : value);
  } catch (error) {
    notify("error", t("settings.defaultTerminalFailed"), { description: String(error) });
    config.value = await desktop.getConfig();
  }
}

onMounted(async () => {
  await loadConfig();
  // 当前版本号用于「关于与更新」卡片展示；失败只降级为占位符
  void loadCurrentVersion();
  // 扫描终端工具用于「默认终端」下拉；失败不阻塞页面，下拉退化为仅自动推荐
  try {
    terminalTools.value = await desktop.scanTerminalTools();
  } catch (error) {
    console.warn("终端工具扫描失败", error);
  }
  offConfigChanged = desktop.onConfigChanged(() => void loadConfig());
});
</script>

<script lang="ts">
export default { name: "SettingsView" };
</script>

<template>
  <section class="space-y-6">
    <div class="space-y-2">
      <h1 class="text-2xl font-semibold tracking-tight">{{ t("settings.title") }}</h1>
      <p class="text-sm text-muted">
        {{ t("settings.subtitle") }}
      </p>
    </div>

    <!-- 顶部 Tabs：通用 / 项目 / 工具 三组，设置项多时比单页滚动更好找 -->
    <UTabs v-model="activeTab" :items="tabItems" size="md" class="w-fit" />

    <!-- ============ 通用 ============ -->
    <template v-if="activeTab === 'general'">
      <!-- 外观：主题 / 语言 -->
      <UCard>
        <template #header>
          <p class="font-medium">{{ t("settings.appearance") }}</p>
        </template>
        <div class="flex items-center justify-between gap-4">
          <div>
            <p class="text-sm font-medium">{{ t("settings.theme") }}</p>
            <p class="mt-0.5 text-xs text-muted">{{ t("settings.themeDesc") }}</p>
          </div>
          <USelect
            :model-value="themeValue"
            :items="themeOptions"
            size="sm"
            class="w-44 shrink-0"
            :aria-label="t('settings.theme')"
            @update:model-value="setTheme"
          />
        </div>
        <!-- 语言：跟随系统 / 简体中文 / 繁體中文 / English，切换后全局热生效 -->
        <div class="mt-6 flex items-center justify-between gap-4 border-t border-default pt-6">
          <div>
            <p class="text-sm font-medium">{{ t("settings.language") }}</p>
            <p class="mt-0.5 text-xs text-muted">{{ t("settings.languageDesc") }}</p>
          </div>
          <USelect
            :model-value="languageValue"
            :items="languageOptions"
            size="sm"
            class="w-44 shrink-0"
            :aria-label="t('settings.language')"
            @update:model-value="setLanguage"
          />
        </div>
      </UCard>

      <!-- 启动：开机自启 / 全局快捷键 -->
      <UCard>
        <template #header>
          <p class="font-medium">{{ t("settings.launch") }}</p>
        </template>
        <div class="flex items-center justify-between gap-4">
          <div>
            <p class="text-sm font-medium">{{ t("settings.autostart") }}</p>
            <p class="mt-0.5 text-xs text-muted">{{ t("settings.autostartDesc") }}</p>
          </div>
          <USwitch
            :model-value="config?.autostart ?? false"
            :aria-label="t('settings.autostart')"
            @update:model-value="toggleAutostart"
          />
        </div>
        <!-- 全局快捷键 -->
        <div class="mt-6 border-t border-default pt-6">
          <div class="flex items-center justify-between gap-4">
            <div>
              <p class="text-sm font-medium">{{ t("settings.shortcut") }}</p>
              <p class="mt-0.5 text-xs text-muted">{{ t("settings.shortcutHint") }}</p>
            </div>
            <div class="flex items-center gap-2">
              <UKbd v-if="!isRecording" size="md">{{ shortcutDisplay }}</UKbd>
              <!-- 录入态：聚焦的方块捕获按键组合 -->
              <div
                v-else
                ref="recorderRef"
                tabindex="0"
                class="flex h-8 min-w-40 items-center justify-center rounded-md border border-primary px-2 text-sm outline-none"
                @keydown="onRecorderKeydown"
                @blur="stopRecording"
              >
                <span v-if="shortcutPreview" class="font-medium">{{
                  formatShortcut(shortcutPreview)
                }}</span>
                <span v-else class="text-xs text-muted">{{ t("settings.pressKeys") }}</span>
              </div>
              <template v-if="isRecording">
                <UButton
                  size="sm"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-x"
                  @click="stopRecording"
                >
                  {{ t("settings.cancel") }}
                </UButton>
                <UButton
                  size="sm"
                  icon="i-lucide-check"
                  :disabled="!shortcutPreview"
                  @click="saveShortcut"
                >
                  {{ t("settings.save") }}
                </UButton>
              </template>
              <template v-else>
                <!-- 降级类操作用 neutral，与右侧主动作「修改」区分主次 -->
                <UButton
                  v-if="config?.globalShortcut === ''"
                  size="sm"
                  color="neutral"
                  variant="soft"
                  icon="i-lucide-rotate-ccw"
                  @click="resetShortcut"
                >
                  {{ t("settings.reset") }}
                </UButton>
                <UButton
                  v-else
                  size="sm"
                  color="error"
                  variant="soft"
                  icon="i-lucide-circle-off"
                  @click="disableShortcut"
                >
                  {{ t("settings.disable") }}
                </UButton>
                <UButton
                  size="sm"
                  variant="soft"
                  icon="i-lucide-pencil-line"
                  @click="startRecording"
                >
                  {{ t("settings.edit") }}
                </UButton>
              </template>
            </div>
          </div>
          <p class="mt-2 text-xs text-muted">
            {{ t("settings.shortcutDesc", { default: formatShortcut("Super+Shift+P") }) }}
          </p>
        </div>
      </UCard>

      <!-- 终端：打开方式 / 默认终端 -->
      <UCard>
        <template #header>
          <p class="font-medium">{{ t("settings.terminalSection") }}</p>
        </template>
        <div class="flex items-center justify-between gap-4">
          <div>
            <p class="text-sm font-medium">{{ t("settings.terminalMode") }}</p>
            <p class="mt-0.5 text-xs text-muted">
              {{ t("settings.terminalModeDesc") }}
            </p>
          </div>
          <USelect
            :model-value="terminalModeDisplay"
            :items="[
              { label: t('settings.terminalWindow'), value: 'window' },
              { label: t('settings.terminalTab'), value: 'tab' },
            ]"
            size="sm"
            class="w-44 shrink-0"
            :aria-label="t('settings.terminalMode')"
            @update:model-value="setTerminalMode"
          />
        </div>
        <!-- 默认终端工具：扫描本机已装终端，推荐项默认选中 -->
        <div class="mt-6 flex items-center justify-between gap-4 border-t border-default pt-6">
          <div>
            <p class="text-sm font-medium">{{ t("settings.defaultTerminal") }}</p>
            <p class="mt-0.5 text-xs text-muted">
              {{ t("settings.defaultTerminalDesc") }}
            </p>
          </div>
          <USelect
            :model-value="terminalToolDisplay"
            :items="terminalToolItems"
            size="sm"
            class="w-44 shrink-0"
            :aria-label="t('settings.defaultTerminal')"
            @update:model-value="setDefaultTerminal"
          />
        </div>
      </UCard>

      <!-- 关于与更新：当前版本 + 手动检测更新（应用内自动升级入口） -->
      <UCard>
        <template #header>
          <p class="font-medium">{{ t("settings.aboutSection") }}</p>
        </template>
        <!-- 当前行：左侧标题说明 + 右侧版本徽章与检测按钮，与其他设置行同构 -->
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <p class="text-sm font-medium">{{ t("settings.currentVersion") }}</p>
            <p class="mt-0.5 text-xs text-muted">{{ t("settings.currentVersionDesc") }}</p>
          </div>
          <div class="flex shrink-0 items-center gap-2.5">
            <!-- 版本徽章：等宽字体 + 浅底圆角，替代裸文本（v 前缀更易读） -->
            <span
              class="rounded-md bg-elevated px-2 py-1 font-mono text-xs text-dimmed"
              :title="t('settings.currentVersion')"
            >
              v{{ currentVersion || "—" }}
            </span>
            <!-- whitespace-nowrap：英文文案较长，禁止折行成两行按钮 -->
            <UButton
              size="sm"
              variant="soft"
              icon="i-lucide-refresh-cw"
              class="shrink-0 whitespace-nowrap"
              :loading="isChecking"
              :aria-label="t('settings.checkUpdate')"
              @click="handleCheckUpdate"
            >
              {{ t("settings.checkUpdate") }}
            </UButton>
          </div>
        </div>
        <!-- 动态状态行：紧跟版本行（不放到卡片最底部），检测/下载/出错后的结果与后续动作 -->
        <div v-if="updateStatusText" class="mt-4 flex items-center justify-between gap-4">
          <div class="flex min-w-0 items-center gap-2">
            <UIcon :name="updateStatusIcon" class="size-4 shrink-0" :class="updateStatusClass" />
            <!-- 错误信息可能很长：截断展示，完整原因在 toast 里 -->
            <p class="truncate text-sm">{{ updateStatusText }}</p>
          </div>
          <UButton
            v-if="updatePhase === 'available'"
            size="sm"
            icon="i-lucide-arrow-down-to-line"
            @click="handleInstallUpdate"
          >
            {{ t("settings.updateNow") }}
          </UButton>
          <UButton
            v-else-if="updatePhase === 'ready'"
            size="sm"
            icon="i-lucide-rotate-cw"
            @click="restartApp"
          >
            {{ t("settings.restart") }}
          </UButton>
        </div>
        <!-- 通道行：独立成行，切换即清空旧检测结果，需重新检测 -->
        <div class="mt-6 flex items-center justify-between gap-4 border-t border-default pt-6">
          <div class="min-w-0">
            <p class="text-sm font-medium">{{ t("settings.updateChannel") }}</p>
            <p class="mt-0.5 text-xs text-muted">{{ t("settings.updateChannelDesc") }}</p>
          </div>
          <USelect
            :model-value="updateChannel"
            :items="updateChannelItems"
            size="sm"
            class="w-32 shrink-0"
            :aria-label="t('settings.updateChannel')"
            @update:model-value="setChannel($event as UpdateChannel)"
          />
        </div>
        <!-- 源码仓库行：两个镜像仓库的外链入口，点击用系统默认浏览器打开 -->
        <div class="mt-6 flex items-center justify-between gap-4 border-t border-default pt-6">
          <div class="min-w-0">
            <p class="text-sm font-medium">{{ t("settings.sourceRepo") }}</p>
            <p class="mt-0.5 text-xs text-muted">{{ t("settings.sourceRepoDesc") }}</p>
          </div>
          <div class="flex shrink-0 items-center gap-2">
            <UButton
              size="sm"
              color="neutral"
              variant="soft"
              icon="i-lucide-cloud"
              @click="openRepo(REPO_CNB)"
            >
              CNB
            </UButton>
            <UButton
              size="sm"
              color="neutral"
              variant="soft"
              icon="i-lucide-github"
              @click="openRepo(REPO_GITHUB)"
            >
              GitHub
            </UButton>
          </div>
        </div>
      </UCard>
    </template>

    <!-- ============ 项目 ============ -->
    <template v-else-if="activeTab === 'projects'">
      <!-- 新增项目 -->
      <UCard>
        <template #header>
          <p class="font-medium">{{ t("settings.addProject") }}</p>
        </template>
        <form class="space-y-4" @submit.prevent="addProject">
          <div class="grid gap-4 sm:grid-cols-2">
            <UFormField :label="t('settings.projectName')" :hint="t('settings.nameHint')">
              <UInput v-model="form.name" :placeholder="t('settings.namePlaceholder')" class="w-full" />
            </UFormField>
            <UFormField :label="t('settings.tool')">
              <USelect v-model="form.toolId" :items="toolOptions" class="w-full" />
            </UFormField>
          </div>
          <UFormField :label="t('settings.projectPath')" :hint="t('settings.pathHint')">
            <div class="flex w-full gap-2">
              <UInput v-model="form.path" placeholder="/Users/you/Projects/demo" class="flex-1" />
              <UButton
                icon="i-lucide-folder-open"
                color="neutral"
                variant="soft"
                :aria-label="t('settings.browse')"
                @click="browseFolder"
              />
            </div>
          </UFormField>
          <div class="flex justify-end">
            <UButton type="submit" icon="i-lucide-plus" :loading="isSubmitting">
              {{ t("settings.addToTray") }}
            </UButton>
          </div>
        </form>
      </UCard>

      <!-- 从本机工具导入最近项目 -->
      <UCard>
        <template #header>
          <div class="flex items-center justify-between gap-3">
            <p class="font-medium">{{ t("settings.import") }}</p>
            <UButton
              size="sm"
              variant="soft"
              icon="i-lucide-folder-search"
              :loading="isScanning"
              @click="scanProjects"
            >
              {{ t("settings.scan") }}
            </UButton>
          </div>
        </template>
        <p class="text-sm text-muted">
          {{ t("settings.importDesc") }}
        </p>
      </UCard>

      <!-- 已配置项目 -->
      <UCard>
        <template #header>
          <p class="font-medium">{{ t("settings.configuredProjects", { n: config?.projects.length ?? 0 }) }}</p>
        </template>
        <div v-if="isLoading" class="flex flex-col gap-2">
          <USkeleton v-for="i in 3" :key="i" class="h-10 rounded-lg" />
        </div>
        <p
          v-else-if="(config?.projects.length ?? 0) === 0"
          class="py-6 text-center text-sm text-muted"
        >
          {{ t("settings.emptyProjects") }}
        </p>
        <ul v-else class="divide-y divide-default">
          <li
            v-for="project in config!.projects"
            :key="project.id"
            class="flex items-center gap-3 py-3 first:pt-0 last:pb-0"
          >
            <!-- 图标随项目绑定的开发工具变化：直接显示对应 IDE 的品牌图标 -->
            <ToolIcon
              :tool-id="project.toolId"
              :fallback-icon="config!.tools.find((t) => t.id === project.toolId)?.icon"
              icon-class="size-5 shrink-0"
            />
            <span class="min-w-0 flex-1">
              <span class="block truncate text-sm font-medium">{{ project.name }}</span>
              <span class="block truncate text-xs text-muted">{{ project.path }}</span>
            </span>
            <!-- 最近打开时间：与托盘弹窗同一套相对时间文案 -->
            <span class="shrink-0 text-xs text-dimmed">
              {{ formatRelativeTime(project.lastOpenedAt) }}
            </span>
            <!-- 打开工具：行内下拉可直接更换，图标随之联动 -->
            <USelect
              :model-value="project.toolId"
              :items="toolOptions"
              size="sm"
              class="w-44 shrink-0"
              :aria-label="$t('settings.projectTool')"
              @update:model-value="(v: string) => changeTool(project.id, v)"
            />
            <!-- 终端入口：与托盘弹窗一致，在默认终端打开项目目录 -->
            <UButton
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-square-terminal"
              :aria-label="$t('tray.openInTerminal')"
              :title="$t('tray.openInTerminal')"
              @click="openInTerminalFromSettings(project)"
            />
            <UButton
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-external-link"
              :aria-label="$t('settings.tryOpen')"
              @click="openProject(project.id, project.name)"
            />
            <UButton
              size="xs"
              color="error"
              variant="ghost"
              icon="i-lucide-trash-2"
              :aria-label="$t('settings.removeProject')"
              @click="removeProject(project.id, project.name)"
            />
          </li>
        </ul>
      </UCard>
    </template>

    <!-- ============ 工具 ============ -->
    <UCard v-else>
      <template #header>
        <p class="font-medium">{{ t("settings.tools") }}</p>
      </template>
      <p class="mb-3 text-xs text-muted">
        {{ t("settings.toolsDesc") }}
      </p>
      <ul class="divide-y divide-default">
        <li
          v-for="tool in ideTools"
          :key="tool.id"
          class="flex items-center gap-3 py-3 first:pt-0 last:pb-0"
        >
          <ToolIcon :tool-id="tool.id" :fallback-icon="tool.icon" icon-class="size-5 shrink-0" />
          <span class="min-w-0 flex-1">
            <span class="block text-sm font-medium">{{ tool.name }}</span>
            <span class="block truncate font-mono text-xs text-muted">
              {{ tool.command }} {{ tool.args.join(" ") }}
            </span>
          </span>
        </li>
      </ul>
    </UCard>

    <!-- 导入确认弹窗：勾选要导入的项目并确认打开工具 -->
    <UModal
      v-model:open="importOpen"
      :title="t('settings.importTitle')"
      :description="scanSummary || t('settings.importPickDesc')"
      :ui="{
        // Nuxt UI 的 modal body 默认自带 overflow-y-auto：内容（全选栏 +
        // max-h-80 列表）超高时 body 自己出一条原生大滚动条，和列表的
        // 覆盖式滑块叠成两条。这里关掉 body 的滚动，让滚动只发生在
        // OverlayScrollbar 视口内（弹窗总高 < 视口高，body 不需要滚）
        body: 'overflow-y-visible',
      }"
    >
      <template #body>
        <!-- 全选栏固定在滚动区外，列表很长时也能随时全选/取消全选 -->
        <div class="mb-2 flex items-center gap-2 border-b border-default pb-2">
          <UCheckbox
            :model-value="selectAllState"
            :label="t('settings.selectAll')"
            @update:model-value="toggleSelectAll"
          />
          <span class="ml-auto text-xs text-muted">
            {{ selectedCount }} / {{ scanItems.length }}
          </span>
        </div>
        <OverlayScrollbar class="max-h-80">
          <div class="space-y-1">
          <label
            v-for="item in scanItems"
            :key="item.path"
            class="flex items-center gap-3 rounded-lg px-2 py-2 hover:bg-elevated"
          >
            <UCheckbox v-model="importSelection[item.path]" />
            <ToolIcon
              :tool-id="item.toolId"
              :fallback-icon="config?.tools.find((t) => t.id === item.toolId)?.icon"
              icon-class="size-5 shrink-0"
            />
            <span class="min-w-0 flex-1">
              <span class="block truncate text-sm font-medium">{{ item.name }}</span>
              <span class="block truncate text-xs text-muted">{{ item.path }}</span>
            </span>
            <!-- 允许改选打开工具：比如 JetBrains 里记的目录想用 Cursor 打开 -->
            <USelect
              v-model="importTool[item.path]"
              :items="toolOptions"
              size="sm"
              class="w-44 shrink-0"
            />
          </label>
          </div>
        </OverlayScrollbar>
      </template>
      <template #footer>
        <div class="flex w-full items-center justify-end gap-3">
          <!-- 数量已在 body 顶部的全选栏展示，这里只留操作按钮 -->
          <UButton color="neutral" variant="ghost" icon="i-lucide-x" @click="importOpen = false">
            {{ t("settings.cancel") }}
          </UButton>
          <UButton icon="i-lucide-check" :disabled="selectedCount === 0" @click="importSelected">
            {{ t("settings.importN", { n: selectedCount }) }}
          </UButton>
        </div>
      </template>
    </UModal>
  </section>
</template>
