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
use tauri::{AppHandle, Manager, Rect, WebviewUrl, WebviewWindowBuilder};
// 物理坐标定位只在非 macOS 平台用（macOS 走 CG 点空间）
#[cfg(not(target_os = "macos"))]
use tauri::PhysicalPosition;

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
///
/// 多显示器坑（macOS 特有）：`TrayIconEvent` 的 rect 是**物理坐标**
/// （已乘托盘所在屏的 backingScaleFactor），而 tao 的 `monitor_from_point`
/// 按 `CGDisplayBounds`（全局**点**空间）匹配。托盘所在屏缩放 > 1 时，
/// 物理坐标在点空间里必然越界 → 匹配失败 → 回退主屏，表现为「点击 B 屏
/// 托盘，弹窗却在 A 屏出现」。因此 macOS 上必须先还原出点空间再匹配与定位。
fn position_near_tray(
  app: &AppHandle,
  window: &tauri::WebviewWindow,
  rect: Rect,
) -> tauri::Result<()> {
  // TrayIconEvent 的 rect.position / rect.size 都是 Physical（文档明确标注），
  // to_physical 对 Physical 变体是无损透传，传 1.0 只是为了满足签名
  let icon_pos = rect.position.to_physical::<f64>(1.0);
  let icon_size = rect.size.to_physical::<f64>(1.0);
  let window_size = window.outer_size().unwrap_or_else(|_| (372, 460).into());

  // 窗口逻辑尺寸（CSS px）：跨屏移动时不变，用它参与坐标运算才能保证
  // 在不同缩放的屏上都得到正确的视觉大小
  let win_scale = window.scale_factor().unwrap_or(1.0);
  let win_w = window_size.width as f64 / win_scale;
  let win_h = window_size.height as f64 / win_scale;

  // 图标中心（物理）与图标上/下边缘（物理）
  let icon_center_x = icon_pos.x + icon_size.width / 2.0;
  let icon_top = icon_pos.y;
  let icon_bottom = icon_pos.y + icon_size.height;

  // ---------- 1. 找出托盘图标所在的显示器 ----------
  //
  // macOS：tao 的 Monitor::position()/size() 实为「CG 点 × 本屏缩放」，
  // 混合缩放的多屏没有统一的物理全局空间；但「点空间」是统一的。
  // 所以对每块屏用**它自己的 scale** 把图标物理坐标还原成点坐标，
  // 能落进该屏点边界的那块就是托盘所在屏（各屏点边界互不重叠，唯一命中）。
  //
  // 其他平台（Windows 等）：物理全局坐标空间是统一的，
  // 直接用 monitor_from_point 按物理坐标匹配即可。
  #[cfg(target_os = "macos")]
  let matched = {
    let monitors = app.available_monitors().unwrap_or_default();
    monitors.iter().find_map(|m| {
      let s = m.scale_factor();
      let p = m.position();
      let sz = m.size();
      // 还原这块屏的 CG 点边界
      let (pl, pt) = (p.x as f64 / s, p.y as f64 / s);
      let (pr, pb) = (
        pl + sz.width as f64 / s,
        pt + sz.height as f64 / s,
      );
      // 图标物理坐标 ÷ 本屏 scale = 点坐标；落在边界内即命中
      let cx_pts = icon_center_x / s;
      let top_pts = icon_top / s;
      (cx_pts >= pl && cx_pts < pr && top_pts >= pt && top_pts < pb)
        .then(|| (s, pl, pt, pr, pb))
    })
  };

  #[cfg(not(target_os = "macos"))]
  let matched = {
    // 图标中心点落在哪块屏（物理空间）
    app
      .monitor_from_point(icon_center_x, icon_top)
      .ok()
      .flatten()
      .map(|m| {
        let p = m.position();
        let s = m.size();
        (
          m.scale_factor(),
          p.x as f64,
          p.y as f64,
          (p.x + s.width as i32) as f64,
          (p.y + s.height as i32) as f64,
        )
      })
  };

  // 没匹配上时退回主屏（按主屏 scale 还原坐标），至少保证弹窗不跑到屏幕外
  let (tray_scale, m_left, m_top, m_right, m_bottom) = matched.unwrap_or_else(|| {
    log::warn!("未能定位托盘所在显示器，退回主屏");
    app
      .primary_monitor()
      .ok()
      .flatten()
      .map(|m| {
        let s = m.scale_factor();
        let p = m.position();
        let sz = m.size();
        #[cfg(target_os = "macos")]
        let bounds = (
          p.x as f64 / s,
          p.y as f64 / s,
          (p.x as f64 + sz.width as f64) / s,
          (p.y as f64 + sz.height as f64) / s,
        );
        #[cfg(not(target_os = "macos"))]
        let bounds = (
          p.x as f64,
          p.y as f64,
          (p.x + sz.width as i32) as f64,
          (p.y + sz.height as i32) as f64,
        );
        (s, bounds.0, bounds.1, bounds.2, bounds.3)
      })
      // 连主屏都拿不到时用一个大到不会夹紧的假想边界
      .unwrap_or((1.0, 0.0, 0.0, f64::MAX, f64::MAX))
  });

  // ---------- 2. 在目标空间内计算贴图标的位置 ----------
  // 图标几何换算到目标空间（macOS = 点，其余 = 物理像素）
  let icon_left = icon_pos.x / tray_scale;
  let icon_center = icon_left + (icon_size.width / tray_scale) / 2.0;
  let icon_top_s = icon_top / tray_scale;
  let icon_bottom_s = icon_bottom / tray_scale;

  let gap = GAP as f64;

  // 优先放图标下方；下方放不下（屏幕边缘）则放到图标上方
  let below_y = icon_bottom_s + gap;
  let above_y = icon_top_s - gap - win_h;
  let mut y = if below_y + win_h <= m_bottom { below_y } else { above_y };

  // 水平方向以图标中心对齐，再夹进屏幕左右边缘
  // 用 max→min 而不是 clamp：极小屏上 clamp 的 min>max 会 panic
  let mut x = icon_center - win_w / 2.0;
  x = x.max(m_left + gap).min(m_right - gap - win_w);
  y = y.max(m_top + gap).min(m_bottom - gap - win_h);

  // ---------- 3. 落位 ----------
  // macOS：传 LogicalPosition（点坐标）——tao 对 Logical 变体原样透传给
  // NSWindow，不受弹窗当前所在屏缩放的影响；传 Physical 反而会被弹窗
  // 现在所在屏的 scale 二次换算而出错。
  // 其他平台：物理空间统一，直接传 PhysicalPosition。
  #[cfg(target_os = "macos")]
  let target_pos = tauri::LogicalPosition::new(x, y);
  #[cfg(not(target_os = "macos"))]
  let target_pos = PhysicalPosition::new(x as i32, y as i32);
  window.set_position(target_pos)
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
