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
use tauri::{AppHandle, Emitter, Manager, Rect, WebviewUrl, WebviewWindowBuilder};
// 物理坐标定位只在非 macOS 平台用（macOS 走 CG 点空间）
#[cfg(not(target_os = "macos"))]
use tauri::PhysicalPosition;

/// 弹窗窗口的 label，前端也用它区分窗口身份
pub const POPUP_LABEL: &str = "tray";
/// 主窗口 label，与 tauri.conf.json 中 windows[0] 一致
pub const MAIN_LABEL: &str = "main";
/// 托盘图标 id（tray_by_id 检索用）
pub const TRAY_ID: &str = "main-tray";
/// 失焦隐藏后多少毫秒内的托盘点击视为「关闭」意图
const RECLICK_GRACE: Duration = Duration::from_millis(300);
/// 显示后多少毫秒内的失焦视为假失焦
const FOCUS_GRACE: Duration = Duration::from_millis(400);
/// 弹窗与托盘图标 / 屏幕边缘的间距（物理像素）
const GAP: i32 = 8;

/// 按语言构建托盘右键菜单文案。locale 取自 config（Rust 侧已归一化为
/// "zh-CN" / "zh-TW" / "en"），与前端语言包的 key 保持一致。
fn build_menu(app: &AppHandle, locale: &str) -> tauri::Result<Menu<tauri::Wry>> {
  let (settings_label, quit_label) = match locale {
    "zh-TW" => ("設定", "結束應用"),
    "en" => ("Settings", "Quit App"),
    _ => ("设置", "退出应用"),
  };
  Menu::with_items(
    app,
    &[
      // 右键菜单第一项就是设置：菜单文案与弹窗底部入口保持一致
      &MenuItem::with_id(app, "show-main", settings_label, true, None::<&str>)?,
      &MenuItem::with_id(app, "quit", quit_label, true, None::<&str>)?,
    ],
  )
}

/// 各语言对应的托盘 tooltip 文案
fn tray_tooltip(locale: &str) -> &'static str {
  match locale {
    "zh-TW" => "zoro - 最近專案",
    "en" => "zoro - Recent Projects",
    _ => "zoro - 最近项目",
  }
}

/// Cmd+Q 二次确认的窗口期：首次按下只提示，窗口期内再按一次才真正退出。
/// 3 秒是常见「双击确认」交互的体验上限，太短用户来不及反应，太长像失效。
#[cfg(target_os = "macos")]
const QUIT_CONFIRM_WINDOW: Duration = Duration::from_secs(3);

/// 退出快捷键（Cmd+Q）二次确认状态机。
///
/// 首次按下：布防（记录时间）+ 向所有窗口广播 quit-confirm-hint，
/// 前端 toast「再按一次确认退出」；窗口期内再次按下：拆防并退出。
/// 超过窗口期的按下视为新一轮「首次按下」，重新布防。
///
/// 为什么只拦应用菜单的退出项而不动托盘菜单「退出应用」：快捷键误触是
/// 误退出的主因，显式点菜单是有意识的操作，保持立即退出。
#[cfg(target_os = "macos")]
fn handle_quit_shortcut(app: &AppHandle) {
  let state = app.state::<AppState>();
  // 布防/拆防一次完成：读出「是否在窗口期内」，再写入新状态
  let confirmed = {
    let mut guard = state.quit_armed.lock().unwrap();
    let confirmed = guard
      .map(|t| t.elapsed() < QUIT_CONFIRM_WINDOW)
      .unwrap_or(false);
    *guard = if confirmed { None } else { Some(Instant::now()) };
    confirmed
  };
  if confirmed {
    log::info!("退出快捷键二次确认命中，退出应用");
    app.exit(0);
  } else {
    log::info!("退出快捷键首次按下，提示二次确认");
    let _ = app.emit("quit-confirm-hint", ());
  }
}

/// macOS 应用菜单：替换 Tauri 默认菜单，把「Cmd+Q 立即退出」换成二次确认。
///
/// 必须自带完整菜单的原因：一旦 set_menu，默认菜单整个消失，系统标准
/// 编辑快捷键（Cmd+C/V/X/Z）也会跟着失效——所以补一个预定义 Edit 子菜单。
/// 第一个子菜单即 macOS 应用菜单（屏幕左上角的应用名下），把「设置 /
/// 退出应用」放这里，与托盘菜单文案保持一致。
///
/// 仅 macOS 构建：Windows/Linux 没有应用菜单概念，Alt+F4 等由系统窗口
/// 关闭链路处理（走 CloseRequested → 隐藏，不涉及本状态机）。
#[cfg(target_os = "macos")]
pub fn create_app_menu(app: &AppHandle) -> tauri::Result<()> {
  use tauri::menu::{IsMenuItem, PredefinedMenuItem, Submenu};

  let locale = app
    .try_state::<AppState>()
    .map(|s| s.config.lock().unwrap().locale().to_string())
    .unwrap_or_default();
  let (settings_label, quit_label, close_label) = match locale.as_str() {
    "zh-TW" => ("設定", "結束應用", "關閉視窗"),
    "en" => ("Settings", "Quit App", "Close Window"),
    _ => ("设置", "退出应用", "关闭窗口"),
  };

  // 应用子菜单：设置 / 关闭窗口(Cmd+W) / 退出(Cmd+Q，二次确认)。
  // 关闭窗口用系统预定义项：自带 Cmd+W 快捷键，走原生 performClose，
  // 最终落入主窗口的 CloseRequested（只隐藏、进程驻留托盘）。
  // 数组元素是 MenuItem 与 PredefinedMenuItem 的混合类型，必须显式声明成
  // &dyn IsMenuItem 才能放进同一个切片
  let app_submenu = Submenu::with_items(
    app,
    "Zoro",
    true,
    &[
      &MenuItem::with_id(app, "show-main", settings_label, true, None::<&str>)?
        as &dyn IsMenuItem<tauri::Wry>,
      &PredefinedMenuItem::separator(app)? as &dyn IsMenuItem<tauri::Wry>,
      &PredefinedMenuItem::close_window(app, Some(close_label))? as &dyn IsMenuItem<tauri::Wry>,
      &PredefinedMenuItem::separator(app)? as &dyn IsMenuItem<tauri::Wry>,
      &MenuItem::with_id(app, "app-quit", quit_label, true, Some("CmdOrCtrl+Q"))?
        as &dyn IsMenuItem<tauri::Wry>,
    ],
  )?;
  // 编辑子菜单：系统预定义项，保证主窗口输入框的编辑快捷键继续可用
  let edit_submenu = Submenu::with_items(
    app,
    "Edit",
    true,
    &[
      &PredefinedMenuItem::undo(app, None)? as &dyn IsMenuItem<tauri::Wry>,
      &PredefinedMenuItem::redo(app, None)? as &dyn IsMenuItem<tauri::Wry>,
      &PredefinedMenuItem::separator(app)? as &dyn IsMenuItem<tauri::Wry>,
      &PredefinedMenuItem::cut(app, None)? as &dyn IsMenuItem<tauri::Wry>,
      &PredefinedMenuItem::copy(app, None)? as &dyn IsMenuItem<tauri::Wry>,
      &PredefinedMenuItem::paste(app, None)? as &dyn IsMenuItem<tauri::Wry>,
      &PredefinedMenuItem::select_all(app, None)? as &dyn IsMenuItem<tauri::Wry>,
    ],
  )?;
  let menu = Menu::with_items(app, &[&app_submenu, &edit_submenu])?;
  app.set_menu(menu)?;

  // 应用菜单事件走全局 handler（托盘菜单有自己的 handler，互不影响；
  // id 用独立命名空间，避免与托盘的 quit 混淆）
  app.on_menu_event(|app, event| match event.id().as_ref() {
    "show-main" => show_main_window(app),
    "app-quit" => handle_quit_shortcut(app),
    _ => {}
  });
  Ok(())
}

/// 创建托盘图标：左键 = 弹出最近项目，右键 = 系统菜单（显示主窗口 / 退出）。
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
  // 菜单语言跟随配置；此时 AppState 已 manage（lib.rs setup 顺序保证）
  let locale = app
    .try_state::<AppState>()
    .map(|s| s.config.lock().unwrap().locale().to_string())
    .unwrap_or_default();
  let menu = build_menu(app, &locale)?;

  TrayIconBuilder::with_id(TRAY_ID)
    .icon(tauri::image::Image::from_bytes(include_bytes!(
      "../icons/tray.png"
    ))?)
    // macOS：按模板图标渲染，随明暗主题自动反色
    .icon_as_template(true)
    .menu(&menu)
    // 左键留给弹窗逻辑，菜单只在右键出现
    .show_menu_on_left_click(false)
    .tooltip(tray_tooltip(&locale))
    .on_menu_event(|app, event| match event.id().as_ref() {
      "show-main" => show_main_window(app),
      "quit" => app.exit(0),
      _ => {}
    })
    // 左键抬起时切换弹窗；`tray` 引用会传给定位逻辑，
    // macOS 上用它访问内部 NSStatusItem 取权威位置
    .on_tray_icon_event(|tray, event| {
      // 只响应左键抬起，避免按下/抬起各触发一次
      if let TrayIconEvent::Click {
        rect,
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
      } = event
      {
        toggle_popup(tray.app_handle(), tray, rect);
      }
    })
    .build(app)?;
  Ok(())
}

/// 语言切换后重建托盘菜单与 tooltip（set_locale 命令调用）。
/// 失败只记日志：菜单语言滞后一拍不影响任何功能。
pub fn refresh_tray_locale(app: &AppHandle) {
  let locale = app
    .state::<AppState>()
    .config
    .lock()
    .unwrap()
    .locale()
    .to_string();
  let Some(tray) = app.tray_by_id(TRAY_ID) else {
    log::warn!("托盘图标不存在，菜单语言未更新");
    return;
  };
  match build_menu(app, &locale) {
    Ok(menu) => {
      if let Err(err) = tray.set_menu(Some(menu)) {
        log::warn!("托盘菜单语言更新失败：{err}");
      }
      let _ = tray.set_tooltip(Some(tray_tooltip(&locale)));
    }
    Err(err) => log::warn!("重建托盘菜单失败：{err}"),
  }
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
///
/// `tray` 是触发本次事件的托盘图标引用：macOS 上通过它访问内部
/// NSStatusItem 拿权威位置（多显示器混合缩放下几何推算不可靠）。
pub fn toggle_popup(app: &AppHandle, tray: &tauri::tray::TrayIcon, rect: Rect) {
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

  if let Err(err) = position_near_tray(app, tray, &window, rect) {
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

/// 托盘图标与目标屏边界的统一几何描述。
///
/// 坐标系约定：
/// - macOS：全局「点」空间（CG 点空间），上左为原点——与
///   `set_position(LogicalPosition)` 的输入空间一致；
/// - 其他平台：统一的全局物理像素空间，上左为原点。
struct TrayPlacement {
  /// 图标水平中心
  icon_center_x: f64,
  /// 图标上边缘
  icon_top: f64,
  /// 图标下边缘
  icon_bottom: f64,
  /// 可用于放置弹窗的屏幕边界（macOS 用 visibleFrame，已排除菜单栏/Dock）
  screen_left: f64,
  screen_top: f64,
  screen_right: f64,
  screen_bottom: f64,
}

/// 把弹窗贴着托盘图标放置，并夹紧在图标所在显示器的可视范围内。
///
/// - macOS：托盘在顶部菜单栏 → 弹窗放图标下方；
/// - Windows：托盘在右下角 → 放上方；
/// - 多显示器：以托盘图标真实所在的显示器为准，避免弹窗跑到另一块屏上。
///
/// 多显示器坑（macOS 特有）：`TrayIconEvent` 的 rect 是**物理坐标**（已乘
/// 托盘所在屏的 backingScaleFactor），而多屏混合缩放时（如 1512×982@2x 与
/// 1920×1080@1x 拼接）两块屏的物理区间在全局坐标里会**重叠**——
/// 「按几何区间匹配显示器」没有唯一解，这就是「点 B 屏托盘、弹窗出现在
/// A 屏」的根因。
///
/// 解法：绕开事件 rect，直接问系统要权威数据——tray-icon 内部持有
/// `NSStatusItem`，其按钮所属 `NSWindow` 自带 `NSScreen` 归属，且窗口
/// frame 就在统一的「点空间」里，无缩放歧义。几何推算只作为拿不到
/// NSStatusItem 时的兜底。
fn position_near_tray(
  app: &AppHandle,
  tray: &tauri::tray::TrayIcon,
  window: &tauri::WebviewWindow,
  rect: Rect,
) -> tauri::Result<()> {
  // 非 macOS 走几何推算，不需要读托盘内部对象；显式丢弃参数避免未使用告警
  #[cfg(not(target_os = "macos"))]
  let _ = tray;

  // 窗口逻辑尺寸（CSS px）：跨屏移动时不变，用它参与坐标运算才能保证
  // 在不同缩放的屏上都得到正确的视觉大小
  let window_size = window.outer_size().unwrap_or_else(|_| (372, 460).into());
  let win_scale = window.scale_factor().unwrap_or(1.0);
  let win_w = window_size.width as f64 / win_scale;
  let win_h = window_size.height as f64 / win_scale;

  // ---------- 1. 取托盘几何 ----------
  // 优先 NSStatusItem 权威数据（macOS）；失败则退回各平台的几何推算
  #[cfg(target_os = "macos")]
  let placement = status_item_geometry(tray).unwrap_or_else(|| {
    log::warn!("未能从 NSStatusItem 获取托盘位置，退回几何推算");
    fallback_geometry_macos(app, rect)
  });

  #[cfg(not(target_os = "macos"))]
  let placement = fallback_geometry_other(app, rect);

  let gap = GAP as f64;

  // ---------- 2. 计算贴图标的位置 ----------
  // 优先放图标下方；下方放不下（屏幕边缘）则放到图标上方
  let below_y = placement.icon_bottom + gap;
  let above_y = placement.icon_top - gap - win_h;
  let mut y = if below_y + win_h <= placement.screen_bottom {
    below_y
  } else {
    above_y
  };

  // 水平方向以图标中心对齐，再夹进屏幕左右边缘
  // 用 max→min 而不是 clamp：极小屏上 clamp 的 min>max 会 panic
  let mut x = placement.icon_center_x - win_w / 2.0;
  x = x.max(placement.screen_left + gap).min(placement.screen_right - gap - win_w);
  y = y.max(placement.screen_top + gap).min(placement.screen_bottom - gap - win_h);

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

/// macOS：从 NSStatusItem 提取托盘图标的权威几何（点空间）。
///
/// 死锁说明：本函数在托盘点击回调（主线程、事件循环派发中）内调用
/// `with_inner_tray_icon`。tauri-runtime-wry 的 `send_user_message`
/// 检测到「当前已在主线程」时会**同步执行**闭包而非经事件循环转发，
/// 因此阻塞等待的 recv 立即返回，不会死锁。（来自其他线程调用时走
/// proxy 转发，事件循环空闲，同样安全。）
///
/// 闭包内完成全部计算、只返回纯数字（f64 是 Send）——objc2 的
/// Retained<NSStatusItem> 等非 Send 类型不允许跨闭包边界带出。
#[cfg(target_os = "macos")]
fn status_item_geometry(tray: &tauri::tray::TrayIcon) -> Option<TrayPlacement> {
  use objc2::rc::Retained;
  use objc2_app_kit::{NSStatusItem, NSWindow};
  use objc2_foundation::MainThreadMarker;
  // 与 tao 的翻转逻辑保持一致：主屏 CG 高度 = 点空间总高度
  use objc2_core_graphics::{CGDisplayPixelsHigh, CGMainDisplayID};

  tray
    .with_inner_tray_icon(|inner| -> Option<TrayPlacement> {
      // 闭包必在主线程执行（见上方死锁说明），MainThreadMarker 一定可用
      let mtm = MainThreadMarker::new()?;
      // 1. tray-icon 内部持有的 NSStatusItem（托盘图标本体）
      let item: Retained<NSStatusItem> = inner.ns_status_item()?;
      // 2. 状态栏按钮 → 按钮所在的 NSWindow；
      //    NSStatusBarButton → NSButton → NSControl → NSView 逐级上溯
      //    才能调到 NSView::window()（objc2 继承方法只声明在父类上）
      let button = item.button(mtm)?;
      let view = button
        .into_super() // NSButton
        .into_super() // NSControl
        .into_super(); // NSView
      let win: Retained<NSWindow> = view.window()?;
      // 3. 窗口所在屏幕（权威归属，混合缩放下也不歧义）
      let screen = win.screen()?;

      // 4. 换算到点空间（上左原点）：
      //    Cocoa 全局坐标是「下左原点」，翻转常数 = 主屏 CG 高度（点）
      let main_h = CGDisplayPixelsHigh(CGMainDisplayID()) as f64;
      let frame = win.frame(); // 图标全局矩形（下左原点，点）
      let vf = screen.visibleFrame(); // 可视区（排除菜单栏/Dock）

      Some(TrayPlacement {
        icon_center_x: frame.origin.x + frame.size.width / 2.0,
        icon_top: main_h - (frame.origin.y + frame.size.height),
        icon_bottom: main_h - frame.origin.y,
        screen_left: vf.origin.x,
        screen_right: vf.origin.x + vf.size.width,
        screen_top: main_h - (vf.origin.y + vf.size.height),
        screen_bottom: main_h - vf.origin.y,
      })
    })
    .ok()
    .flatten()
}

/// macOS 兜底：拿不到 NSStatusItem 时按事件 rect 做几何推算（点空间）。
///
/// 单屏或纯 Retina 场景下此路径依旧正确；混合缩放多屏可能匹配歧义，
/// 但 NSStatusItem 路径正常时不会走到这里。
#[cfg(target_os = "macos")]
fn fallback_geometry_macos(app: &AppHandle, rect: Rect) -> TrayPlacement {
  // 事件 rect 是物理坐标，对 Physical 变体 to_physical 无损透传
  let icon_pos = rect.position.to_physical::<f64>(1.0);
  let icon_size = rect.size.to_physical::<f64>(1.0);
  let icon_center_x = icon_pos.x + icon_size.width / 2.0;

  // 对每块屏用它自己的 scale 还原 CG 点边界，与图标坐标（÷各屏 scale）
  // 匹配；单屏时主屏命中，多屏取第一个命中的屏。
  // 注意：存入 TrayPlacement 的所有值都要除以该屏 scale（物理 → 点）
  let monitors = app.available_monitors().unwrap_or_default();
  let matched = monitors.iter().find_map(|m| {
    let s = m.scale_factor();
    let p = m.position();
    let sz = m.size();
    let (left, top) = (p.x as f64 / s, p.y as f64 / s);
    let (right, bottom) = (left + sz.width as f64 / s, top + sz.height as f64 / s);
    let cx = icon_center_x / s;
    let top_edge = icon_pos.y / s;
    (cx >= left && cx < right && top_edge >= top && top_edge < bottom).then(|| TrayPlacement {
      icon_center_x: cx,
      icon_top: top_edge,
      icon_bottom: top_edge + icon_size.height as f64 / s,
      screen_left: left,
      screen_top: top,
      screen_right: right,
      screen_bottom: bottom,
    })
  });

  // 主屏 scale 兜底换算（拿不到主屏时按 1x，至少不越界太离谱）
  let fb_scale = app
    .primary_monitor()
    .ok()
    .flatten()
    .map(|m| m.scale_factor())
    .unwrap_or(1.0);
  matched.unwrap_or_else(|| TrayPlacement {
    icon_center_x: icon_center_x / fb_scale,
    icon_top: icon_pos.y / fb_scale,
    icon_bottom: icon_pos.y / fb_scale + icon_size.height as f64 / fb_scale,
    screen_left: 0.0,
    screen_top: 0.0,
    screen_right: f64::MAX,
    screen_bottom: f64::MAX,
  })
}

/// 非 macOS 兜底：物理空间统一，直接按图标中心匹配所在显示器。
#[cfg(not(target_os = "macos"))]
fn fallback_geometry_other(app: &AppHandle, rect: Rect) -> TrayPlacement {
  let icon_pos = rect.position.to_physical::<f64>(1.0);
  let icon_size = rect.size.to_physical::<f64>(1.0);
  let icon_center_x = icon_pos.x + icon_size.width / 2.0;
  let icon_top = icon_pos.y;
  let icon_bottom = icon_pos.y + icon_size.height;

  // 图标中心点落在哪块屏（物理空间），失败退回主屏
  let matched = app
    .monitor_from_point(icon_center_x, icon_top)
    .ok()
    .flatten()
    .or_else(|| app.primary_monitor().ok().flatten());

  matched
    .map(|m| {
      let p = m.position();
      let sz = m.size();
      TrayPlacement {
        icon_center_x,
        icon_top,
        icon_bottom,
        screen_left: p.x as f64,
        screen_top: p.y as f64,
        screen_right: (p.x + sz.width as i32) as f64,
        screen_bottom: (p.y + sz.height as i32) as f64,
      }
    })
    .unwrap_or_else(|| TrayPlacement {
      // 连主屏都拿不到：不夹紧，至少弹窗能显示出来
      icon_center_x,
      icon_top,
      icon_bottom,
      screen_left: 0.0,
      screen_top: 0.0,
      screen_right: f64::MAX,
      screen_bottom: f64::MAX,
    })
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
