//! 供前端调用的 Tauri 命令。
//!
//! 约定：所有会修改配置的命令都返回**完整 Config**，前端直接整体替换本地状态，
//! 免去增删改差量的同步逻辑；同时每次变更后向所有窗口广播 `config-changed`，
//! 让弹窗和设置页保持一致。

use crate::config::{self, AppState, Config, Project};
use crate::importer::{self, DetectedProject};
use crate::{opener, tray};
use tauri::{AppHandle, Emitter, Manager, State};

/// 读取当前配置（工具 + 项目）。
#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
  Ok(state.config.lock().unwrap().clone())
}

/// 扫描本机开发工具（VS Code / Cursor / JetBrains 系列）的最近打开目录记录，
/// 返回过滤掉不存在目录与已配置项之后的候选列表。
///
/// 只做文件读取，不写任何外部数据；扫描放在命令线程上，
/// 避免阻塞 UI 主线程。
#[tauri::command]
pub fn scan_recent_projects(
  app: AppHandle,
  state: State<'_, AppState>,
) -> Result<Vec<DetectedProject>, String> {
  // importer 的数据源（~/Library/...）全部是 macOS 路径，Windows / Linux 上
  // 即使拿到主目录也扫不到东西。这些平台返回空列表而不是 Err——「没有可导入项」
  // 是正常的，不该让整个扫描命令失败并弹「扫描失败」。
  if cfg!(not(target_os = "macos")) {
    return Ok(Vec::new());
  }

  // 快照已配置的（路径, 工具）对，供 importer 过滤重复
  let existing = {
    let cfg = state.config.lock().unwrap();
    cfg
      .projects
      .iter()
      .map(|p| (p.path.clone(), p.tool_id.clone()))
      .collect::<Vec<_>>()
  };
  // 主目录交给 Tauri 按平台解析，避免硬编码依赖 HOME 环境变量。
  let home = app
    .path()
    .home_dir()
    .map_err(|e| format!("无法获取用户主目录：{e}"))?;
  Ok(importer::scan(&home, &existing))
}

/// 新增一条项目记录。
#[tauri::command]
pub fn add_project(
  app: AppHandle,
  state: State<'_, AppState>,
  name: String,
  path: String,
  tool_id: String,
) -> Result<Config, String> {
  let path = config::expand_tilde(path.trim());
  if path.is_empty() {
    return Err("项目路径不能为空".into());
  }
  // 只接受目录：这些工具的语义就是打开一个工作区
  let p = std::path::Path::new(&path);
  if !p.is_dir() {
    // 拼写错误时给出最接近的真实路径，省得用户对着报错逐字符排查
    let mut message = format!("路径不存在或不是目录：{path}");
    if let Some(suggestion) = config::suggest_similar_dir(&path) {
      message.push_str(&format!("，是不是想输入：{suggestion}？"));
    }
    return Err(message);
  }

  let mut cfg = state.config.lock().unwrap();
  let Some(tool) = cfg.tools.iter().find(|t| t.id == tool_id) else {
    return Err(format!("未找到工具：{tool_id}"));
  };

  // 同一路径 + 同一工具只允许一条记录，避免列表重复
  if cfg
    .projects
    .iter()
    .any(|p| p.path == path && p.tool_id == tool_id)
  {
    return Err(format!("该路径已配置在 {} 下", tool.name));
  }

  // 未填名称时取目录名兜底
  let name = if name.trim().is_empty() {
    p.file_name()
      .map(|n| n.to_string_lossy().into_owned())
      .unwrap_or_else(|| path.clone())
  } else {
    name.trim().to_string()
  };

  let project = Project {
    id: config::next_id(&cfg.projects),
    name,
    path,
    tool_id: tool_id.clone(),
    last_opened_at: None,
  };
  cfg.projects.push(project);
  let snapshot = cfg.clone();
  drop(cfg); // 释放锁后再做 IO 与广播

  config::save(&state)?;
  broadcast(&app);
  Ok(snapshot)
}

/// 删除一条项目记录。
#[tauri::command]
pub fn remove_project(
  app: AppHandle,
  state: State<'_, AppState>,
  id: String,
) -> Result<Config, String> {
  let mut cfg = state.config.lock().unwrap();
  let before = cfg.projects.len();
  cfg.projects.retain(|p| p.id != id);
  if cfg.projects.len() == before {
    return Err(format!("项目不存在：{id}"));
  }
  let snapshot = cfg.clone();
  drop(cfg);

  config::save(&state)?;
  broadcast(&app);
  Ok(snapshot)
}

/// 更换已配置项目的打开工具（设置页项目行的下拉框）。
/// 只改 tool_id，其余字段（路径 / 名称 / last_opened_at）原样保留；
/// 工具或项目不存在时拒绝，避免配置里出现悬空引用。
#[tauri::command]
pub fn set_project_tool(
  app: AppHandle,
  state: State<'_, AppState>,
  project_id: String,
  tool_id: String,
) -> Result<Config, String> {
  {
    let mut cfg = state.config.lock().unwrap();
    if !cfg.tools.iter().any(|t| t.id == tool_id) {
      return Err(format!("未找到工具：{tool_id}"));
    }
    let Some(project) = cfg.projects.iter_mut().find(|p| p.id == project_id) else {
      return Err(format!("项目不存在：{project_id}"));
    };
    // 重复设置同一个工具时不落盘，直接返回当前快照（前端用于回弹）
    if project.tool_id == tool_id {
      return Ok(cfg.clone());
    }
    project.tool_id = tool_id;
  }
  config::save(&state)?;
  broadcast(&app);
  Ok(state.config.lock().unwrap().clone())
}

/// 用项目所属工具打开它；成功后刷新 last_opened_at 并隐藏弹窗。
#[tauri::command]
pub fn open_project(
  app: AppHandle,
  state: State<'_, AppState>,
  id: String,
) -> Result<Config, String> {
  // 先把需要的数据克隆出来，避免持锁期间做进程启动这类慢操作
  let (project, tool) = {
    let cfg = state.config.lock().unwrap();
    let project = cfg
      .projects
      .iter()
      .find(|p| p.id == id)
      .cloned()
      .ok_or_else(|| format!("项目不存在：{id}"))?;
    let tool = cfg
      .tools
      .iter()
      .find(|t| t.id == project.tool_id)
      .cloned()
      .ok_or_else(|| format!("项目 {} 的工具已被删除", project.name))?;
    (project, tool)
  };

  // 打开失败直接把错误文本抛给前端做 toast
  opener::open_with_tool(&tool, &project.path)?;

  let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis() as i64;

  let mut cfg = state.config.lock().unwrap();
  if let Some(p) = cfg.projects.iter_mut().find(|p| p.id == id) {
    p.last_opened_at = Some(now);
  }
  let snapshot = cfg.clone();
  drop(cfg);

  config::save(&state)?;
  broadcast(&app);
  // 打开成功后弹窗收起，符合「点完即走」的预期
  if let Some(popup) = app.get_webview_window(tray::POPUP_LABEL) {
    let _ = popup.hide();
  }
  Ok(snapshot)
}

/// 设置开机自动启动。enable() / disable() 均为幂等操作。
///
/// 事务顺序：单次持锁读出旧值并写入新值、clone 出快照后立即释放锁 →
/// 锁外调用系统 API → 落盘。系统 API 失败或落盘失败时都把内存回滚到旧值，
/// 并尽量把系统自启状态也还原，保证「内存 / 磁盘 / 系统」三者一致。
#[tauri::command]
pub fn set_autostart(
  app: AppHandle,
  state: State<'_, AppState>,
  enabled: bool,
) -> Result<Config, String> {
  use tauri_plugin_autostart::ManagerExt as _;

  // 单次持锁完成读-改-写并 clone 快照；锁内不含任何系统 API / IO
  let (previous, snapshot) = {
    let mut cfg = state.config.lock().unwrap();
    let previous = cfg.autostart;
    cfg.autostart = enabled;
    (previous, cfg.clone())
  };

  let result = if enabled {
    app.autolaunch().enable()
  } else {
    app.autolaunch().disable()
  };
  if let Err(err) = result {
    state.config.lock().unwrap().autostart = previous;
    return Err(format!("设置开机自启失败：{err}"));
  }

  if let Err(err) = config::save(&state) {
    // 落盘失败：先回滚内存，再尽量还原系统自启状态（幂等，best-effort）
    state.config.lock().unwrap().autostart = previous;
    let revert = if previous {
      app.autolaunch().enable()
    } else {
      app.autolaunch().disable()
    };
    if let Err(e) = revert {
      log::warn!("开机自启落盘失败后还原系统状态也失败：{e}");
    }
    return Err(err);
  }
  broadcast(&app);
  Ok(snapshot)
}

/// 设置全局快捷键。
/// - `None`：恢复平台默认（macOS ⌘+Shift+P / Windows Win+Shift+P）；
/// - `Some("")`：禁用；
/// - `Some(组合)`：自定义，如 "Super+Shift+P"（global-hotkey 解析格式）。
///
/// 注册成功才算生效；系统注册失败或落盘失败时都把配置回滚到旧值，
/// 并把原因抛给前端 toast。
#[tauri::command]
pub fn set_global_shortcut(
  app: AppHandle,
  state: State<'_, AppState>,
  shortcut: Option<String>,
) -> Result<Config, String> {
  // 单次持锁完成读-改-写并 clone 快照；apply() 会读 config，必须在锁外调用
  let (previous, snapshot) = {
    let mut cfg = state.config.lock().unwrap();
    let previous = cfg.global_shortcut.clone();
    cfg.global_shortcut = shortcut;
    (previous, cfg.clone())
  };

  if let Err(err) = crate::shortcut::apply(&app) {
    // apply 内部已尽量恢复旧注册，这里回滚配置保持两者一致
    state.config.lock().unwrap().global_shortcut = previous;
    return Err(err);
  }

  if let Err(err) = config::save(&state) {
    // 落盘失败：回滚内存并重新注册旧组合（best-effort，apply 读回滚后的 config）
    state.config.lock().unwrap().global_shortcut = previous;
    if let Err(e) = crate::shortcut::apply(&app) {
      log::warn!("快捷键落盘失败后恢复旧注册也失败：{e}");
    }
    return Err(err);
  }
  broadcast(&app);
  Ok(snapshot)
}

/// 显示并聚焦主窗口（弹窗底部的「设置」入口）。
#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), String> {
  tray::show_main_window(&app);
  Ok(())
}

/// 在系统终端中打开指定目录（最近项目悬停出现的终端图标按钮）。
/// 打开方式（新窗口/新选项卡）与默认终端工具均读自配置。
#[tauri::command]
pub fn open_in_terminal(state: State<'_, AppState>, path: String) -> Result<(), String> {
  let (mode, tool) = {
    let cfg = state.config.lock().unwrap();
    (cfg.terminal_mode().to_string(), cfg.terminal_tool.clone())
  };
  opener::open_in_terminal(&path, &mode, tool.as_deref())
}

/// 用系统文件管理器打开指定目录（最近项目悬停出现的文件夹图标按钮）。
/// 打开方式（新窗口/新选项卡）读自配置；选项卡仅 macOS Finder 支持。
#[tauri::command]
pub fn open_in_folder(state: State<'_, AppState>, path: String) -> Result<(), String> {
  let mode = {
    let cfg = state.config.lock().unwrap();
    cfg.folder_mode().to_string()
  };
  opener::open_in_file_manager(&path, &mode)
}

/// 设置文件管理器打开方式："window"（新窗口，默认）或 "tab"（新选项卡）。
/// 选项卡目前仅 macOS Finder 支持；其他平台选 tab 时行为与 window 相同。
#[tauri::command]
pub fn set_folder_mode(
  app: AppHandle,
  state: State<'_, AppState>,
  mode: String,
) -> Result<Config, String> {
  // 只接受合法值，其余直接拒绝，避免把垃圾数据写进配置文件
  let normalized = match mode.as_str() {
    "window" => "window",
    "tab" => "tab",
    _ => return Err(format!("不支持的文件管理器打开方式：{mode}")),
  };
  set_config_field(
    &app,
    &state,
    |cfg| std::mem::replace(&mut cfg.folder_open_mode, Some(normalized.to_string())),
    |cfg, old| cfg.folder_open_mode = old,
  )
}

/// 扫描本机已安装的终端工具，供设置页「默认终端」下拉框渲染。
/// 返回 [{id, name, icon, installed}]；前端据此推荐默认项（iTerm2 优先）。
#[tauri::command]
pub fn scan_terminal_tools() -> Vec<crate::opener::TerminalToolInfo> {
  crate::opener::detect_terminal_tools()
    .into_iter()
    .map(|(id, name, icon, installed)| crate::opener::TerminalToolInfo {
      id: id.to_string(),
      name: name.to_string(),
      icon: icon.to_string(),
      installed,
    })
    .collect()
}

/// 设置默认终端工具："iterm" / "terminal"，None = 自动推荐。
/// 推荐策略：装了 iTerm2 用 iTerm2，否则系统 Terminal。
#[tauri::command]
pub fn set_terminal_tool(
  app: AppHandle,
  state: State<'_, AppState>,
  tool: Option<String>,
) -> Result<Config, String> {
  // 只接受合法值（或 None），避免把垃圾数据写进配置文件
  if let Some(t) = &tool {
    if t != "iterm" && t != "terminal" {
      return Err(format!("不支持的终端工具：{t}"));
    }
  }
  set_config_field(
    &app,
    &state,
    |cfg| std::mem::replace(&mut cfg.terminal_tool, tool),
    |cfg, old| cfg.terminal_tool = old,
  )
}

/// 设置终端打开方式："window"（新窗口，默认）或 "tab"（新选项卡）。
/// 兼容旧版本的 "existing"（语义等价于 "tab"），收到时自动归一化。
#[tauri::command]
pub fn set_terminal_mode(
  app: AppHandle,
  state: State<'_, AppState>,
  mode: String,
) -> Result<Config, String> {
  // 只接受合法值，其余直接拒绝，避免把垃圾数据写进配置文件；
  // 旧值 "existing" 在这里就转成 "tab"，保证配置文件里只有新语义
  let normalized = match mode.as_str() {
    "window" => "window",
    "tab" | "existing" => "tab",
    _ => return Err(format!("不支持的终端打开方式：{mode}")),
  };
  set_config_field(
    &app,
    &state,
    |cfg| std::mem::replace(&mut cfg.terminal_open_mode, Some(normalized.to_string())),
    |cfg, old| cfg.terminal_open_mode = old,
  )
}

/// 设置界面主题："light" / "dark"，None = 跟随系统。
/// 保存后广播 config-changed（前端两个窗口同步应用到 <html>）。
#[tauri::command]
pub fn set_theme(
  app: AppHandle,
  state: State<'_, AppState>,
  theme: Option<String>,
) -> Result<Config, String> {
  // 只接受合法值（或 None），避免把垃圾数据写进配置文件
  if let Some(t) = &theme {
    if t != "light" && t != "dark" {
      return Err(format!("不支持的主题：{t}"));
    }
  }
  set_config_field(
    &app,
    &state,
    |cfg| std::mem::replace(&mut cfg.theme, theme),
    |cfg, old| cfg.theme = old,
  )
}

/// 设置自动检测更新开关（每小时静默检测一次；默认开）。
/// 开关只影响前端的定时检测节奏，已亮起的托盘更新角标不随之摘除——
/// 「有新版本」是客观事实，用户关掉自动检测仍可在设置页手动检测。
#[tauri::command]
pub fn set_auto_check_update(
  app: AppHandle,
  state: State<'_, AppState>,
  enabled: bool,
) -> Result<Config, String> {
  set_config_field(
    &app,
    &state,
    |cfg| std::mem::replace(&mut cfg.auto_check_update, enabled),
    |cfg, old| cfg.auto_check_update = old,
  )
}

/// 切换托盘图标的「有更新」角标（前端自动/手动检测到结果后调用）。
/// 只切图标不动配置，失败仅记日志——角标是纯提示，不影响主流程。
#[tauri::command]
pub fn set_tray_update_badge(app: AppHandle, has_update: bool) {
  crate::tray::set_update_badge(&app, has_update);
}

/// 设置界面语言："zh-CN" / "zh-TW" / "en"，None = 跟随系统语言。
/// 保存后广播 config-changed（前端两窗口热切换），并按新语言重建托盘菜单。
#[tauri::command]
pub fn set_locale(
  app: AppHandle,
  state: State<'_, AppState>,
  locale: Option<String>,
) -> Result<Config, String> {
  // 只接受合法值（或 None），避免把垃圾数据写进配置文件
  if let Some(l) = &locale {
    if l != "zh-CN" && l != "zh-TW" && l != "en" {
      return Err(format!("不支持的语言：{l}"));
    }
  }
  // 单次持锁完成读-改-写并 clone 快照
  let (previous, snapshot) = {
    let mut cfg = state.config.lock().unwrap();
    let previous = cfg.locale.clone();
    cfg.locale = locale;
    (previous, cfg.clone())
  };
  if let Err(err) = config::save(&state) {
    state.config.lock().unwrap().locale = previous;
    return Err(err);
  }
  // 托盘菜单文案独立于前端，落盘成功后按新语言同步重建
  tray::refresh_tray_locale(&app);
  broadcast(&app);
  Ok(snapshot)
}

/// 隐藏托盘弹窗（Esc 键 / 设置跳转时使用）。
#[tauri::command]
pub fn hide_tray_popup(app: AppHandle) -> Result<(), String> {
  if let Some(popup) = app.get_webview_window(tray::POPUP_LABEL) {
    let _ = popup.hide();
  }
  Ok(())
}

/// 完全退出应用（托盘菜单与弹窗共用）。窗口只是隐藏，必须提供显式出口。
#[tauri::command]
pub fn quit_app(app: AppHandle) -> Result<(), String> {
  app.exit(0);
  Ok(())
}

/// 用系统默认浏览器打开一个 http(s) 链接（设置页的源码仓库入口）。
///
/// 为什么不用 tauri-plugin-opener：只为这一个外链入口引入一个插件 +
/// capability 权限配置不划算，系统 open 链路三个平台各一行即可覆盖。
///
/// 安全约束：只放行 http/https scheme。前端传入的虽然是固定常量，
/// 但 open/start/xdg-open 理论上能处理任意 scheme（file://、smb:// 等），
/// 边界处做白名单校验，避免未来有人把动态输入接进来时踩坑。
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
  if !(url.starts_with("https://") || url.starts_with("http://")) {
    return Err(format!("不允许打开非 http(s) 链接：{url}"));
  }

  // 三平台各用系统默认方式拉起浏览器；spawn 只负责拉起进程，浏览器自行接管
  #[cfg(target_os = "macos")]
  let result = std::process::Command::new("open").arg(&url).spawn();

  #[cfg(target_os = "windows")]
  // start 后第一个空参数是窗口标题占位符，防止 URL 被误当标题吞掉
  let result = std::process::Command::new("cmd")
    .args(["/C", "start", ""])
    .arg(&url)
    .spawn();

  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  let result = std::process::Command::new("xdg-open").arg(&url).spawn();

  result
    .map(|_| ())
    .map_err(|err| format!("打开浏览器失败：{err}"))
}

/// 提交一次纯配置字段变更。
///
/// 在**单次持锁**内完成「读旧值 → 写新值 → clone 落盘快照」，随后立即释放锁；
/// `mutate` 返回被替换的旧值，落盘失败时 `rollback` 用它把字段恢复，避免
/// 「内存已改、磁盘未写」的不一致。锁内不调用系统 API 或做 IO，杜绝死锁。
fn set_config_field<T, F, R>(
  app: &AppHandle,
  state: &AppState,
  mutate: F,
  rollback: R,
) -> Result<Config, String>
where
  F: FnOnce(&mut Config) -> T,
  R: FnOnce(&mut Config, T),
{
  let (old, snapshot) = {
    let mut cfg = state.config.lock().unwrap();
    let old = mutate(&mut cfg);
    (old, cfg.clone())
  };
  if let Err(err) = config::save(state) {
    rollback(&mut state.config.lock().unwrap(), old);
    return Err(err);
  }
  broadcast(app);
  Ok(snapshot)
}

/// 向所有窗口广播配置变更事件。
fn broadcast(app: &AppHandle) {
  if let Err(err) = app.emit("config-changed", ()) {
    log::warn!("广播 config-changed 失败：{err}");
  }
}
