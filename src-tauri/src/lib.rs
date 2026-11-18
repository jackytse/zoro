//! 应用入口：装配托盘、窗口生命周期与命令。
//!
//! 关键行为：
//! - 主窗口点击关闭按钮只隐藏，进程驻留托盘继续运行；
//! - 托盘弹窗（label = tray）失焦自动隐藏；
//! - macOS Dock 图标点击（Reopen）重新拉起主窗口。

mod commands;
mod config;
mod importer;
mod opener;
mod shortcut;
mod tray;

use config::AppState;
// RunEvent 仅 macOS/Windows 的 Reopen 分支消费，Linux 上必须裁掉导入
#[cfg(any(target_os = "macos", windows))]
use tauri::RunEvent;
use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      // 诊断模式：--debug-monitors 打印显示器原始数据后退出，
      // 用于排查多显示器下托盘弹窗定位问题
      if std::env::args().any(|a| a == "--debug-monitors") {
        let monitors = app
          .available_monitors()
          .unwrap_or_default();
        println!("显示器数量: {}", monitors.len());
        for m in &monitors {
          println!(
            "  name={:?} scale={} position={:?} size={:?}",
            m.name(),
            m.scale_factor(),
            m.position(),
            m.size()
          );
        }
        let primary = app.primary_monitor().ok().flatten();
        if let Some(m) = primary {
          println!(
            "主屏: scale={} position={:?} size={:?}",
            m.scale_factor(),
            m.position(),
            m.size()
          );
        }
        app.handle().exit(0);
      }

      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      // 全局快捷键：触发时呼出/收起托盘弹窗（注册动作在 setup 里按配置执行）
      app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
          .with_handler(|app, _shortcut, event| {
            // 只在按下瞬间响应一次，抬起不重复触发
            if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
              shortcut::on_trigger(app);
            }
          })
          .build(),
      )?;

      // 开机自启：自启动时携带 --hidden，登录后静默驻留托盘不弹窗口
      app.handle().plugin(
        tauri_plugin_autostart::init(
          tauri_plugin_autostart::MacosLauncher::LaunchAgent,
          Some(vec!["--hidden"]),
        ),
      )?;

      // 原生目录选择对话框（添加项目时浏览目录用）
      app.handle().plugin(tauri_plugin_dialog::init())?;

      // 加载/初始化配置并注册为全局状态，命令与托盘逻辑都从这里取数据
      let state = config::init(app.handle())?;
      app.manage(state);

      // 托盘与弹窗都在启动时创建好，之后只做显示/隐藏切换
      tray::create_tray(app.handle())?;
      tray::create_popup_window(app.handle())?;

      // 纯托盘应用：不显示 Dock 图标（macOS 的 Accessory 模式）。
      // 必须在 setup 里调用——事件循环启动后切换会无效或不生效。
      // 隐藏 Dock 后 RunEvent::Reopen 不会再触发，主窗口改由
      // 托盘菜单「显示主窗口」与弹窗「设置」入口唤起。
      #[cfg(target_os = "macos")]
      app.set_activation_policy(tauri::ActivationPolicy::Accessory);

      // 按配置注册全局快捷键；占用/非法组合只记日志，不阻塞启动
      if let Err(err) = shortcut::apply(app.handle()) {
        log::warn!("全局快捷键注册失败：{err}");
      }

      // 自启动场景（--hidden）：历史行为，与 visible:false 一致保持静默
      if std::env::args().any(|a| a == "--hidden") {
        if let Some(main) = app.get_webview_window(tray::MAIN_LABEL) {
          let _ = main.hide();
        }
      }

      // 按配置同步系统自启动状态（enable 幂等，反复调用无副作用）
      let want_autostart = app.state::<AppState>().config.lock().unwrap().autostart;
      let autolaunch = app.autolaunch();
      let result = if want_autostart {
        autolaunch.enable()
      } else {
        autolaunch.disable()
      };
      if let Err(err) = result {
        log::warn!("同步开机自启状态失败：{err}");
      }
      Ok(())
    })
    .on_window_event(|window, event| match event {
      // 点关闭按钮（红绿灯 / Alt+F4 / Cmd+W）：不真正关闭，只隐藏，
      // 让应用驻留托盘后台运行
      WindowEvent::CloseRequested { api, .. } => {
        api.prevent_close();
        let _ = window.hide();
        if window.label() == tray::POPUP_LABEL {
          *window
            .app_handle()
            .state::<AppState>()
            .popup_last_hidden
            .lock()
            .unwrap() = Some(std::time::Instant::now());
        }
      }
      // 弹窗失焦即收起，符合 macOS 菜单栏弹层的交互习惯
      WindowEvent::Focused(false) if window.label() == tray::POPUP_LABEL => {
        tray::on_popup_blur(window.app_handle());
      }
      _ => {}
    })
    .invoke_handler(tauri::generate_handler![
      commands::get_config,
      commands::scan_recent_projects,
      commands::add_project,
      commands::set_global_shortcut,
      commands::set_autostart,
      commands::remove_project,
      commands::set_project_tool,
      commands::open_project,
      commands::open_in_terminal,
      commands::scan_terminal_tools,
      commands::set_terminal_mode,
      commands::set_terminal_tool,
      commands::set_locale,
      commands::set_theme,
      commands::show_main_window,
      commands::hide_tray_popup,
      commands::quit_app,
    ])
    .build(tauri::generate_context!())
    .expect("error while building tauri application")
    .run(|app, event| {
      // Reopen 变体只有 macOS 才有（点 Dock 图标唤回窗口，对应
      // NSApplicationDelegate 的 applicationShouldHandleReopen）。
      // 注意：Windows 与 Linux 的 RunEvent 都没有这个变体——Linux 之前能
      // 编译过只是因为旧 cfg 条件恰好为 false，实际语义一直是「仅 macOS」。
      #[cfg(target_os = "macos")]
      {
        if let RunEvent::Reopen { .. } = event {
          tray::show_main_window(app);
        }
      }
      // 其他平台没有需要处理的运行期事件，显式消费参数避免未使用告警
      #[cfg(not(target_os = "macos"))]
      {
        let _ = (app, event);
      }
    });
}
