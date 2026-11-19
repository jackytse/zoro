/**
 * 应用自动更新（多通道）。
 *
 * 更新通道分「正式版 stable」与「预发布 beta」，由用户在设置页选择，
 * 选择持久化在 localStorage（key: zoro.update-channel）。检测与下载安装
 * 走自定义 Tauri 命令（src-tauri/src/updater.rs）——JS 端 plugin-updater
 * 的 check() 只能用 tauri.conf.json 的固定端点，无法按通道切换清单文件：
 *   - stable → /-/raw/updater/latest.json
 *   - beta   → /-/raw/updater/latest-beta.json
 * 更新包带 minisign 签名（公钥在 tauri.conf.json），前端只负责
 * 「检测 → 确认 → 下载安装 → 重启」这条状态机；下载进度由 Rust 侧经
 * updater://progress 事件推送（payload = [分块字节增量, 总大小]）。
 *
 * 阶段流转：
 *   idle → checking → latest（已是最新）| available（发现新版本）
 *   available →（用户确认）→ downloading → ready →（用户点重启）→ relaunch
 *   任意阶段出错 → error（保留错误信息，可重新检测）
 */
import { computed, ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";

/** 更新通道：stable 正式版 / beta 预发布（各对应一份更新清单） */
export type UpdateChannel = "stable" | "beta";

/** 通道选择的 localStorage 键（跨会话记住用户的通道偏好） */
const CHANNEL_KEY = "zoro.update-channel";

/** Rust updater_check 命令返回的检测摘要（字段名跟 serde camelCase 对齐） */
interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
}

/** 更新流程阶段（供 UI 状态机渲染） */
export type UpdatePhase =
  | "idle" // 初始态，还没检测过
  | "checking" // 正在请求更新清单比对版本
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

/** 读取用户上次选择的更新通道；没有记录或值非法都回落到正式版 */
function loadChannel(): UpdateChannel {
  const stored = localStorage.getItem(CHANNEL_KEY);
  return stored === "beta" ? "beta" : "stable";
}

export function useUpdater() {
  const phase = ref<UpdatePhase>("idle");
  /** 当前更新的通道（切换后旧检测结果作废，需重新检测） */
  const channel = ref<UpdateChannel>(loadChannel());
  /** 当前应用版本（来自 tauri.conf.json 的 version，经核心 app 插件读取） */
  const currentVersion = ref("");
  /** 新版本号（检测命中时回填） */
  const newVersion = ref("");
  /** 检测或下载阶段的错误信息（String(error)） */
  const errorMessage = ref("");
  /** 下载进度（downloading 阶段有效） */
  const progress = ref<DownloadProgress>({ downloaded: 0, total: null });

  const isChecking = computed(() => phase.value === "checking");
  const isDownloading = computed(() => phase.value === "downloading");

  /**
   * 检测命中标记：available 阶段才允许进入 downloading。
   * Update 实例本体留在 Rust 的 PendingUpdate 状态里，前端只记「有没有」
   */
  let hasPendingUpdate = false;

  /** 读取当前版本号；失败不阻塞页面，版本位留空即可 */
  async function loadCurrentVersion() {
    try {
      currentVersion.value = await getVersion();
    } catch (error) {
      // dev 模式或权限缺失时可能失败，只降级展示，不打扰用户
      console.warn("读取应用版本号失败", error);
    }
  }

  /** 切换更新通道：清空旧检测结果回到初始态，用户重新检测生效 */
  function setChannel(next: UpdateChannel) {
    if (next === channel.value) return;
    channel.value = next;
    localStorage.setItem(CHANNEL_KEY, next);
    phase.value = "idle";
    hasPendingUpdate = false;
    newVersion.value = "";
    errorMessage.value = "";
    progress.value = { downloaded: 0, total: null };
  }

  /** 检测更新：请求当前通道的更新清单与当前版本比对 */
  async function checkForUpdate() {
    phase.value = "checking";
    errorMessage.value = "";
    try {
      const info = await invoke<UpdateInfo | null>("updater_check", {
        channel: channel.value,
      });
      if (info) {
        hasPendingUpdate = true;
        newVersion.value = info.version;
        phase.value = "available";
      } else {
        hasPendingUpdate = false;
        newVersion.value = "";
        phase.value = "latest";
      }
    } catch (error) {
      // 常见原因：无网络、清单 404（该通道还没有任何发布）
      errorMessage.value = String(error);
      phase.value = "error";
    }
  }

  /** 确认后下载并安装更新；Windows NSIS 走 passive 静默装，macOS 替换 .app */
  async function downloadAndInstall() {
    if (!hasPendingUpdate) return;
    phase.value = "downloading";
    progress.value = { downloaded: 0, total: null };
    // 事件订阅先于下载启动，避免早期分块丢失；finally 里必须退订
    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listen<[number, number | null]>("updater://progress", (event) => {
        const [chunk, total] = event.payload;
        progress.value = {
          // Rust 侧发的是「本次分块字节增量」，逐块累加得到已下载量
          downloaded: progress.value.downloaded + chunk,
          // 总大小可能拿不到（服务端没回 Content-Length），拿到才更新
          total: total ?? progress.value.total,
        };
      });
      await invoke("updater_download_and_install");
      phase.value = "ready";
    } catch (error) {
      errorMessage.value = String(error);
      phase.value = "error";
    } finally {
      unlisten?.();
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
    channel,
    currentVersion,
    newVersion,
    errorMessage,
    progress,
    isChecking,
    isDownloading,
    loadCurrentVersion,
    setChannel,
    checkForUpdate,
    downloadAndInstall,
    restartApp,
    formatMB,
  };
}
