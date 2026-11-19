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
  /** 全局快捷键；null = 平台默认，"" = 禁用，其余为组合字符串（如 "Super+Shift+P"） */
  globalShortcut: string | null;
  /** 开机自动启动（默认开启，自启动时静默驻留托盘） */
  autostart: boolean;
  /** 终端打开方式："window" = 新窗口（默认），"tab" = 新选项卡（旧配置的 "existing" 同义） */
  terminalOpenMode: string | null;
  /** 默认终端工具："iterm" / "terminal"；null = 自动推荐 */
  terminalTool: string | null;
  /** 界面语言："zh-CN" | "zh-TW" | "en"；null = 跟随系统语言 */
  locale: string | null;
  /** 界面主题："light" | "dark"；null = 跟随系统 */
  theme: string | null;
}

/** 本机终端工具扫描结果（设置页「默认终端」下拉框的单条记录） */
export interface TerminalTool {
  id: string;
  name: string;
  icon: string;
  installed: boolean;
}

/** 从本机开发工具记录里探测到的候选项目 */
export interface DetectedProject {
  name: string;
  path: string;
  /** 建议使用的工具 id（来源工具），导入时可改选 */
  toolId: string;
}

export const desktop = {
  getConfig: () => invoke<DesktopConfig>("get_config"),
  /** 扫描 VS Code / Cursor / JetBrains 的最近打开目录记录（只读，已过滤已配置项） */
  scanRecentProjects: () => invoke<DetectedProject[]>("scan_recent_projects"),
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

  /** 在系统终端打开一个目录（最近项目悬停的终端图标）；方式读自配置 */
  openInTerminal: (path: string) => invoke<void>("open_in_terminal", { path }),
  /** 更换已配置项目的打开工具（设置页项目行下拉框）；返回更新后的配置 */
  setProjectTool: (projectId: string, toolId: string) =>
    invoke<DesktopConfig>("set_project_tool", { projectId, toolId }),

  /** 扫描本机已安装的终端工具（设置页「默认终端」下拉框数据源） */
  scanTerminalTools: () => invoke<TerminalTool[]>("scan_terminal_tools"),

  /** 设置默认终端工具："iterm" / "terminal"，null = 自动推荐（装了 iTerm 用 iTerm） */
  setTerminalTool: (tool: string | null) =>
    invoke<DesktopConfig>("set_terminal_tool", { tool }),

  /** 设置终端打开方式："window"（新窗口，默认）或 "tab"（新选项卡） */
  setTerminalMode: (mode: "window" | "tab") =>
    invoke<DesktopConfig>("set_terminal_mode", { mode }),
  /**
   * 设置界面语言；null = 跟随系统。
   * 后端会广播 config-changed 并按新语言重建托盘菜单。
   */
  setLocale: (locale: "zh-CN" | "zh-TW" | "en" | null) =>
    invoke<DesktopConfig>("set_locale", { locale }),
  /** 设置界面主题："light" / "dark"，null = 跟随系统（广播后两个窗口同步生效） */
  setTheme: (theme: "light" | "dark" | null) => invoke<DesktopConfig>("set_theme", { theme }),
  /**
   * 设置全局快捷键；null = 恢复平台默认，"" = 禁用。
   * 失败时 reject 中文错误（组合冲突 / 无法解析）。
   */
  setGlobalShortcut: (shortcut: string | null) =>
    invoke<DesktopConfig>("set_global_shortcut", { shortcut }),
  /** 设置开机自动启动；失败时 reject 中文错误 */
  setAutostart: (enabled: boolean) => invoke<DesktopConfig>("set_autostart", { enabled }),
  showMainWindow: () => invoke<void>("show_main_window"),
  hideTrayPopup: () => invoke<void>("hide_tray_popup"),
  quitApp: () => invoke<void>("quit_app"),
  /** 用系统默认浏览器打开 http(s) 链接（设置页的源码仓库入口）；后端校验 scheme 白名单 */
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  /** 订阅配置变更（任一窗口增删/打开项目后触发） */
  onConfigChanged: (handler: () => void) => {
    // listen 返回 Promise<UnlistenFn>；卸载函数本身是同步的，这里只负责转发
    const unlisten = listen("config-changed", handler);
    return () => {
      void unlisten.then((off) => off());
    };
  },
};
