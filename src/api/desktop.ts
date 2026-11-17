/**
 * 托盘弹窗与主窗口共用的类型与 API 封装。
 *
 * 字段命名与 Rust 侧 serde(rename_all = "camelCase") 对齐；
 * 所有命令都通过一层薄封装调用，避免业务组件里散落裸 invoke 字符串。
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** 可用来打开项目的开发工具 */
export interface Tool {
  id: string;
  name: string;
  command: string;
  args: string[];
  appName: string | null;
  icon: string;
}

/** 一条最近项目记录 */
export interface Project {
  id: string;
  name: string;
  path: string;
  toolId: string;
  lastOpenedAt: number | null;
}

/** 完整配置快照 */
export interface DesktopConfig {
  tools: Tool[];
  projects: Project[];
}

export const desktop = {
  getConfig: () => invoke<DesktopConfig>("get_config"),
  /** 新增项目；失败时 reject 一个可直接展示的中文错误信息 */
  addProject: (input: { name: string; path: string; toolId: string }) =>
    invoke<DesktopConfig>("add_project", {
      name: input.name,
      path: input.path,
      toolId: input.toolId,
    }),
  removeProject: (id: string) => invoke<DesktopConfig>("remove_project", { id }),
  /** 打开项目；成功后后端会隐藏弹窗并广播 config-changed */
  openProject: (id: string) => invoke<DesktopConfig>("open_project", { id }),
  showMainWindow: () => invoke<void>("show_main_window"),
  hideTrayPopup: () => invoke<void>("hide_tray_popup"),
  quitApp: () => invoke<void>("quit_app"),
  /** 订阅配置变更（任一窗口增删/打开项目后触发） */
  onConfigChanged: (handler: () => void) => {
    // listen 返回 Promise<UnlistenFn>；卸载函数本身是同步的，这里只负责转发
    const unlisten = listen("config-changed", handler);
    return () => {
      void unlisten.then((off) => off());
    };
  },
};
