<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useToast } from "@nuxt/ui/composables";
import { desktop, type DesktopConfig, type DetectedProject } from "../api/desktop";

const toast = useToast();

const config = ref<DesktopConfig | null>(null);
const isLoading = ref(true);

/** 新增项目表单状态 */
const form = ref({
  name: "",
  path: "",
  toolId: "vscode",
});
const isSubmitting = ref(false);

/** 表单校验：路径必填、必须选择工具 */
const formError = computed(() => {
  if (!form.value.path.trim()) return "请填写项目路径";
  if (!form.value.toolId) return "请选择开发工具";
  return null;
});

const toolOptions = computed(() =>
  (config.value?.tools ?? []).map((tool) => ({
    label: `${tool.name}（${tool.command}）`,
    value: tool.id,
  })),
);

async function loadConfig() {
  try {
    config.value = await desktop.getConfig();
  } catch (error) {
    toast.add({ title: "读取配置失败", description: String(error), color: "error" });
  } finally {
    isLoading.value = false;
  }
}

async function addProject() {
  if (formError.value) {
    toast.add({ title: formError.value, color: "warning" });
    return;
  }
  isSubmitting.value = true;
  try {
    config.value = await desktop.addProject({
      name: form.value.name,
      path: form.value.path,
      toolId: form.value.toolId,
    });
    toast.add({ title: "已添加项目", color: "success" });
    // 只清空输入，保留上次选择的工具方便连续添加
    form.value.name = "";
    form.value.path = "";
  } catch (error) {
    toast.add({ title: "添加失败", description: String(error), color: "error" });
  } finally {
    isSubmitting.value = false;
  }
}

async function removeProject(id: string, name: string) {
  try {
    config.value = await desktop.removeProject(id);
    toast.add({ title: `已移除「${name}」`, color: "success" });
  } catch (error) {
    toast.add({ title: "移除失败", description: String(error), color: "error" });
  }
}

/** 在设置页也能直接试一下打开是否成功，避免等到托盘里才发现配置错误 */
async function openProject(id: string, name: string) {
  try {
    config.value = await desktop.openProject(id);
    toast.add({ title: `已用工具打开「${name}」`, color: "success" });
  } catch (error) {
    toast.add({
      title: `打开「${name}」失败`,
      description: String(error),
      color: "error",
    });
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
  return [...counts.entries()].map(([id, n]) => `${toolLabel(id)} ${n} 条`).join(" · ");
});

/** 勾选数量，导入按钮据此禁用 */
const selectedCount = computed(() => Object.values(importSelection.value).filter(Boolean).length);

/** 工具 id → 展示名 */
function toolLabel(id: string) {
  return config.value?.tools.find((t) => t.id === id)?.name ?? id;
}

async function scanProjects() {
  isScanning.value = true;
  try {
    const items = await desktop.scanRecentProjects();
    if (items.length === 0) {
      toast.add({
        title: "没有扫描到新的最近项目",
        description: "本机工具的记录都已配置过，或这些工具尚未在本机使用。",
        color: "info",
      });
      return;
    }
    scanItems.value = items;
    // 默认全选、来源工具作为打开工具
    importSelection.value = Object.fromEntries(items.map((i) => [i.path, true]));
    importTool.value = Object.fromEntries(items.map((i) => [i.path, i.toolId]));
    importOpen.value = true;
  } catch (error) {
    toast.add({ title: "扫描失败", description: String(error), color: "error" });
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
    toast.add({
      title: `已导入 ${ok} 个项目`,
      description: failures.length ? `${failures.length} 条失败，详情见后续提示` : undefined,
      color: "success",
    });
  }
  for (const message of failures.slice(0, 3)) {
    toast.add({ title: message, color: "error", duration: 8000 });
  }
}

let offConfigChanged: (() => void) | null = null;

onMounted(async () => {
  await loadConfig();
  offConfigChanged = desktop.onConfigChanged(() => void loadConfig());
});
</script>

<script lang="ts">
export default { name: "SettingsView" };
</script>

<template>
  <section class="space-y-8">
    <div class="space-y-2">
      <h1 class="text-2xl font-semibold tracking-tight">设置</h1>
      <p class="text-sm text-muted">
        配置常用开发工具与项目路径，配置完成后可在托盘弹窗中快速打开。
      </p>
    </div>

    <!-- 新增项目 -->
    <UCard>
      <template #header>
        <p class="font-medium">添加项目</p>
      </template>
      <form class="space-y-4" @submit.prevent="addProject">
        <div class="grid gap-4 sm:grid-cols-2">
          <UFormField label="项目名称" hint="留空则取目录名">
            <UInput v-model="form.name" placeholder="我的项目" class="w-full" />
          </UFormField>
          <UFormField label="开发工具">
            <USelect v-model="form.toolId" :items="toolOptions" class="w-full" />
          </UFormField>
        </div>
        <UFormField label="项目路径" hint="支持 ~ 前缀，如 ~/Projects/zoro">
          <UInput v-model="form.path" placeholder="/Users/you/Projects/demo" class="w-full" />
        </UFormField>
        <div class="flex justify-end">
          <UButton type="submit" icon="i-lucide-plus" :loading="isSubmitting"> 添加到托盘 </UButton>
        </div>
      </form>
    </UCard>

    <!-- 从本机工具导入最近项目 -->
    <UCard>
      <template #header>
        <div class="flex items-center justify-between gap-3">
          <p class="font-medium">从开发工具导入</p>
          <UButton
            size="sm"
            variant="soft"
            icon="i-lucide-download"
            :loading="isScanning"
            @click="scanProjects"
          >
            扫描最近项目
          </UButton>
        </div>
      </template>
      <p class="text-sm text-muted">
        读取 VS Code / Cursor 的最近工作区与 JetBrains
        系列产品的最近项目记录（只读，不会写入这些工具的数据）。
        已自动过滤已删除目录和配置过的路径；iTerm 没有可读取的记录，未参与扫描。
      </p>
    </UCard>

    <!-- 已配置项目 -->
    <UCard>
      <template #header>
        <p class="font-medium">已配置项目（{{ config?.projects.length ?? 0 }}）</p>
      </template>
      <div v-if="isLoading" class="flex flex-col gap-2">
        <USkeleton v-for="i in 3" :key="i" class="h-10 rounded-lg" />
      </div>
      <p
        v-else-if="(config?.projects.length ?? 0) === 0"
        class="py-6 text-center text-sm text-muted"
      >
        还没有项目，用上面的表单添加第一个吧。
      </p>
      <ul v-else class="divide-y divide-default">
        <li
          v-for="project in config!.projects"
          :key="project.id"
          class="flex items-center gap-3 py-3 first:pt-0 last:pb-0"
        >
          <UIcon name="i-lucide-folder-git-2" class="size-5 shrink-0 text-primary" />
          <span class="min-w-0 flex-1">
            <span class="block truncate text-sm font-medium">{{ project.name }}</span>
            <span class="block truncate text-xs text-muted">{{ project.path }}</span>
          </span>
          <UBadge color="neutral" variant="subtle" size="sm">
            {{ config!.tools.find((t) => t.id === project.toolId)?.name ?? "未知工具" }}
          </UBadge>
          <UButton
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-external-link"
            aria-label="试运行打开"
            @click="openProject(project.id, project.name)"
          />
          <UButton
            size="xs"
            color="error"
            variant="ghost"
            icon="i-lucide-trash-2"
            aria-label="移除项目"
            @click="removeProject(project.id, project.name)"
          />
        </li>
      </ul>
    </UCard>

    <!-- 工具清单（内置，只读展示） -->
    <UCard>
      <template #header>
        <p class="font-medium">可用工具</p>
      </template>
      <p class="mb-3 text-xs text-muted">
        打开顺序：优先执行 CLI，找不到时通过登录 shell 解析，最后回退到 open -a 应用名。
      </p>
      <ul class="divide-y divide-default">
        <li
          v-for="tool in config?.tools ?? []"
          :key="tool.id"
          class="flex items-center gap-3 py-3 first:pt-0 last:pb-0"
        >
          <UIcon :name="tool.icon" class="size-5 shrink-0 text-primary" />
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
      title="导入最近项目"
      :description="scanSummary || '选择要导入的项目'"
    >
      <template #body>
        <div class="max-h-80 space-y-1 overflow-y-auto">
          <label
            v-for="item in scanItems"
            :key="item.path"
            class="flex items-center gap-3 rounded-lg px-2 py-2 hover:bg-elevated"
          >
            <UCheckbox v-model="importSelection[item.path]" />
            <UIcon
              :name="config?.tools.find((t) => t.id === item.toolId)?.icon ?? 'i-lucide-folder'"
              class="size-5 shrink-0 text-primary"
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
              class="w-40 shrink-0"
            />
          </label>
        </div>
      </template>
      <template #footer>
        <div class="flex w-full items-center justify-end gap-3">
          <span class="mr-auto text-xs text-muted">已选 {{ selectedCount }} 项</span>
          <UButton color="neutral" variant="ghost" @click="importOpen = false"> 取消 </UButton>
          <UButton icon="i-lucide-check" :disabled="selectedCount === 0" @click="importSelected">
            导入 {{ selectedCount }} 项
          </UButton>
        </div>
      </template>
    </UModal>
  </section>
</template>
