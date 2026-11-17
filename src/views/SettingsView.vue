<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useToast } from "@nuxt/ui/composables";
import { desktop, type DesktopConfig } from "../api/desktop";

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
  </section>
</template>
