/**
 * 应用自动更新。
 *
 * 底层是 Tauri updater 插件（@tauri-apps/plugin-updater）：更新元数据从
 * GitHub Releases 的 latest.json 拉取（端点配置在 tauri.conf.json 的
 * plugins.updater），更新包带 minisign 签名，前端只负责「检测 → 确认 →
 * 下载安装 → 重启」这条状态机；进度通过 downloadAndInstall 的事件回调累计。
 *
 * 阶段流转：
 *   idle → checking → latest（已是最新）| available（发现新版本）
 *   available →（用户确认）→ downloading → ready →（用户点重启）→ relaunch
 *   任意阶段出错 → error（保留错误信息，可重新检测）
 */
import { computed, ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** 更新流程阶段（供 UI 状态机渲染） */
export type UpdatePhase =
  | "idle" // 初始态，还没检测过
  | "checking" // 正在请求 latest.json 比对版本
  | "latest" // 已是最新版本
  | "available" // 发现新版本，等用户确认
  | "downloading" // 下载（Windows 上下载完会顺带静默安装）
  | "ready" // 安装完成，等用户重启
  | "error"; // 出错（检测或下载安装）

/** 下载进度：已累计字节数与总大小（服务端未给 Content-Length 时为 null） */
export interface DownloadProgress {
  downloaded: number;
  total: number | null;
}

export function useUpdater() {
  const phase = ref<UpdatePhase>("idle");
  /** 当前应用版本（来自 tauri.conf.json 的 version，经核心 app 插件读取） */
  const currentVersion = ref("");
  /** 新版本号（check() 命中时回填） */
  const newVersion = ref("");
  /** 检测或下载阶段的错误信息（String(error)） */
  const errorMessage = ref("");
  /** 下载进度（downloading 阶段有效） */
  const progress = ref<DownloadProgress>({ downloaded: 0, total: null });

  const isChecking = computed(() => phase.value === "checking");
  const isDownloading = computed(() => phase.value === "downloading");

  /**
   * check() 返回的 Update 对象要跨阶段持有：检测、确认、下载安装是
   * 同一次会话里的同一个实例（内部记录了目标版本与下载地址），不能丢
   */
  let pendingUpdate: Update | null = null;

  /** 读取当前版本号；失败不阻塞页面，版本位留空即可 */
  async function loadCurrentVersion() {
    try {
      currentVersion.value = await getVersion();
    } catch (error) {
      // dev 模式或权限缺失时可能失败，只降级展示，不打扰用户
      console.warn("读取应用版本号失败", error);
    }
  }

  /** 检测更新：请求端点的 latest.json 与当前版本比对 */
  async function checkForUpdate() {
    phase.value = "checking";
    errorMessage.value = "";
    try {
      const update = await check();
      if (update) {
        pendingUpdate = update;
        newVersion.value = update.version;
        phase.value = "available";
      } else {
        pendingUpdate = null;
        phase.value = "latest";
      }
    } catch (error) {
      // 常见原因：无网络、端点 404（还没有带 latest.json 的 release）
      errorMessage.value = String(error);
      phase.value = "error";
    }
  }

  /** 确认后下载并安装更新；Windows NSIS 走 passive 静默装，macOS 替换 .app */
  async function downloadAndInstall() {
    if (!pendingUpdate) return;
    phase.value = "downloading";
    progress.value = { downloaded: 0, total: null };
    try {
      await pendingUpdate.downloadAndInstall((event) => {
        switch (event.event) {
          // 内容总长度（可能拿不到，比如服务端没回 Content-Length）
          case "Started":
            progress.value = { ...progress.value, total: event.data.contentLength ?? null };
            break;
          // 分块累计：updater 按流式下载，逐块累加得到已下载字节数
          case "Progress":
            progress.value = {
              ...progress.value,
              downloaded: progress.value.downloaded + event.data.chunkLength,
            };
            break;
          // Finished 不用单独处理，downloadAndInstall resolve 即安装完成
          case "Finished":
            break;
        }
      });
      phase.value = "ready";
    } catch (error) {
      errorMessage.value = String(error);
      phase.value = "error";
    }
  }

  /** 重启应用加载新版本；relaunch 会以新二进制重新拉起进程 */
  async function restartApp() {
    await relaunch();
  }

  /** 字节数 → MB 展示（一位小数） */
  function formatMB(bytes: number) {
    return (bytes / 1024 / 1024).toFixed(1);
  }

  return {
    phase,
    currentVersion,
    newVersion,
    errorMessage,
    progress,
    isChecking,
    isDownloading,
    loadCurrentVersion,
    checkForUpdate,
    downloadAndInstall,
    restartApp,
    formatMB,
  };
}
