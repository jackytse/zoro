//! 托盘图标与「最近项目」弹窗窗口的管理。
//!
//! 生命周期设计：
//! - 弹窗窗口在应用启动时创建一次（visible=false），之后只在 显示/隐藏 间切换，
//!   绝不销毁重建，避免重复窗口与闪烁；
//! - 点击托盘图标 = 切换（已显示则隐藏）；刚因失焦隐藏的 300ms 内再次点击
//!   视为「关闭」意图，直接忽略，防止 blur-hide → click-show 造成的抖动；
//! - show 之后 400ms 内到达的失焦事件视为假失焦（macOS 激活窗口的竞态），
//!   予以忽略。

use crate::config::AppState;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindowBuilder};

/// 弹窗窗口的 label，前端也用它区分窗口身份
pub const POPUP_LABEL: &str = "tray";
/// 主窗口 label，与 tauri.conf.json 中 windows[0] 一致
pub const MAIN_LABEL: &str = "main";
/// 失焦隐藏后多少毫秒内的托盘点击视为「关闭」意图
const RECLICK_GRACE: Duration = Duration::from_millis(300);
/// 显示后多少毫秒内的失焦视为假失焦
const FOCUS_GRACE: Duration = Duration::from_millis(400);
/// 弹窗与托盘图标 / 屏幕边缘的间距（物理像素）
const GAP: i32 = 8;

/// 创建托盘图标：左键 = 弹出最近项目，右键 = 系统菜单（显示主窗口 / 退出）。
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
  let menu = Menu::with_items(
    app,
    &[
      &MenuItem::with_id(app, "show-main", "显示主窗口", true, None::<&str>)?,
      &MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?,
    ],
  )?;

  TrayIconBuilder::with_id("main-tray")
    .icon(tauri::image::Image::from_bytes(include_bytes!(
      "../icons/tray.png"
    ))?)
    // macOS：按模板图标渲染，随明暗主题自动反色
    .icon_as_template(true)
    .menu(&menu)
    // 左键留给弹窗逻辑，菜单只在右键出现
    .show_menu_on_left_click(false)
    .tooltip("zoro - 最近项目")
    .on_menu_event(|app, event| match event.id().as_ref() {
      "show-main" => show_main_window(app),
      "quit" => app.exit(0),
      _ => {}
    })
    .on_tray_icon_event(|tray, event| {
      // 只响应左键抬起，避免按下/抬起各触发一次
      if let TrayIconEvent::Click {
        rect,
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
      } = event
      {
        toggle_popup(tray.app_handle(), rect);
      }
    })
    .build(app)?;
  Ok(())
}

/// 启动时创建一次弹窗窗口：无边框、置顶、不进任务栏、初始不可见。
pub fn create_popup_window(app: &AppHandle) -> tauri::Result<()> {
  WebviewWindowBuilder::new(app, POPUP_LABEL, WebviewUrl::App("tray.html".into()))
    .title("最近项目")
    .inner_size(372.0, 460.0)
    .resizable(false)
    .decorations(false)
    // 透明窗口 + 前端圆角，做出悬浮卡片观感
    .transparent(true)
    .shadow(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .build()?;
  Ok(())
}

/// 托盘左键点击：显示→隐藏，隐藏→定位并显示。
pub fn toggle_popup(app: &AppHandle, rect: Rect) {
  let Some(window) = app.get_webview_window(POPUP_LABEL) else {
    log::error!("弹窗窗口不存在，可能被销毁");
    return;
  };

  if window.is_visible().unwrap_or(false) {
    let _ = window.hide();
    return;
  }

  // 刚刚因为失焦被隐藏（用户点了托盘想关掉它），这次点击不重复打开
  if let Ok(state) = app.state::<AppState>().popup_last_hidden.lock() {
    if let Some(t) = *state {
      if t.elapsed() < RECLICK_GRACE {
        return;
      }
    }
  }

  if let Err(err) = position_near_tray(app, &window, rect) {
    log::warn!("弹窗定位失败，使用上次位置：{err}");
  }
  if let Err(err) = window.show() {
    log::error!("显示弹窗失败：{err}");
    return;
  }
  let _ = window.set_focus();
  *app.state::<AppState>().popup_last_shown.lock().unwrap() = Some(Instant::now());
}

/// 弹窗失焦时隐藏并记录时间（供 toggle 做意图判断）。
pub fn on_popup_blur(app: &AppHandle) {
  // 刚 show 完就到的 blur 是激活竞态，忽略，否则弹窗会一闪而过
  if let Ok(shown) = app.state::<AppState>().popup_last_shown.lock() {
    if let Some(t) = *shown {
      if t.elapsed() < FOCUS_GRACE {
        return;
      }
    }
  }
  if let Some(window) = app.get_webview_window(POPUP_LABEL) {
    let _ = window.hide();
    *app.state::<AppState>().popup_last_hidden.lock().unwrap() = Some(Instant::now());
  }
}

/// 把弹窗贴着托盘图标放置，并夹紧在图标所在显示器的可视范围内。
///
/// - macOS：托盘在顶部菜单栏 → 弹窗放图标下方；
/// - Windows：托盘在右下角 → 放上方；
/// - 多显示器：以图标所在显示器为准，避免弹窗跑到另一块屏上。
fn position_near_tray(app: &AppHandle, window: &tauri::WebviewWindow, rect: Rect) -> tauri::Result<()> {
  let scale = window.scale_factor().unwrap_or(1.0);
  let icon_pos = rect.position.to_physical::<f64>(scale);
  let icon_size = rect.size.to_physical::<f64>(scale);
  let window_size = window.outer_size().unwrap_or_else(|_| (372, 460).into());

  // 图标几何（物理像素）
  let icon_top = icon_pos.y;
  let icon_bottom = icon_pos.y + icon_size.height;
  let icon_center_x = icon_pos.x + icon_size.width / 2.0;

  // 图标中心点落在哪块屏幕，就用哪块屏幕的边界做夹紧
  let monitor = app
    .monitor_from_point(icon_center_x, icon_top.max(icon_pos.y))
    .ok()
    .flatten()
    .or_else(|| app.primary_monitor().ok().flatten());

  let (m_left, m_top, m_right, m_bottom) = match &monitor {
    Some(m) => {
      let p = m.position();
      let s = m.size();
      (p.x, p.y, p.x + s.width as i32, p.y + s.height as i32)
    }
    // 拿不到显示器信息时退化为「不做边缘夹紧」，只保证不放在负数区域
    None => (0, 0, i32::MAX, i32::MAX),
  };

  let win_w = window_size.width as i32;
  let win_h = window_size.height as i32;

  // 优先放图标下方；下方放不下（屏幕边缘）则放到图标上方
  let below_y = icon_bottom as i32 + GAP;
  let above_y = icon_top as i32 - GAP - win_h;
  let mut y = if below_y + win_h <= m_bottom { below_y } else { above_y };

  // 水平方向以图标中心对齐，再夹进屏幕左右边缘
  let mut x = icon_center_x as i32 - win_w / 2;
  if m_right != i32::MAX {
    x = x.clamp(m_left + GAP, m_right - GAP - win_w);
    y = y.clamp(m_top + GAP, m_bottom - GAP - win_h);
  }

  window.set_position(PhysicalPosition::new(x, y))
}

/// 显示并聚焦主窗口（托盘菜单 / 弹窗「设置」入口共用）。
pub fn show_main_window(app: &AppHandle) {
  if let Some(window) = app.get_webview_window(MAIN_LABEL) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
  } else {
    log::error!("主窗口不存在");
  }
  if let Some(popup) = app.get_webview_window(POPUP_LABEL) {
    let _ = popup.hide();
  }
}
