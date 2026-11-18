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
mod tray;

use config::AppState;
use tauri::{Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      // 加载/初始化配置并注册为全局状态，命令与托盘逻辑都从这里取数据
      let state = config::init(app.handle())?;
      app.manage(state);

      // 托盘与弹窗都在启动时创建好，之后只做显示/隐藏切换
      tray::create_tray(app.handle())?;
      tray::create_popup_window(app.handle())?;
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
      commands::remove_project,
      commands::open_project,
      commands::show_main_window,
      commands::hide_tray_popup,
      commands::quit_app,
    ])
    .build(tauri::generate_context!())
    .expect("error while building tauri application")
    .run(|app, event| {
      // macOS：点击 Dock 图标时重新显示主窗口（此时它可能处于隐藏状态）
      if let RunEvent::Reopen { .. } = event {
        tray::show_main_window(app);
      }
    });
}
