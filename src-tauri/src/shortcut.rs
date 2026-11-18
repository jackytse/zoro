//! 全局快捷键：注册、重注册与触发处理。
//!
//! 设计：
//! - 配置存 `global_shortcut` 字符串（global-hotkey 解析格式，如 "Super+Shift+P"）；
//!   `None` 表示使用平台默认（macOS 上 Super = ⌘，Windows 上 Super = Win 键），
//!   `Some("")` 表示禁用；
//! - 注册统一走 `apply`：先注销当前生效的组合再注册新的，失败时回滚旧组合；
//! - 触发动作 = 在托盘图标附近切换「最近项目」弹窗（与左键点击同一逻辑）。
//!
//! 插件本身在 lib.rs 的 Builder 里装配（with_handler），这里只负责
//! 「按配置增删注册」和「触发后的业务动作」。

use crate::config::AppState;
use crate::tray;
use std::str::FromStr;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Rect};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// 平台默认组合：macOS = ⌘+Shift+P，Windows = Win+Shift+P（Super 随平台解析）。
pub const DEFAULT_SHORTCUT: &str = "Super+Shift+P";

/// 把配置里的原始值解析成要注册的目标组合。
/// `None` → 平台默认；`Some("")` → 禁用（返回 None）；`Some(s)` → 原样使用。
fn resolve_target(custom: Option<&str>) -> Result<Option<String>, String> {
  match custom {
    None => Ok(Some(DEFAULT_SHORTCUT.into())),
    Some("") => Ok(None),
    Some(s) => Ok(Some(s.to_string())),
  }
}

/// 按当前配置注册全局快捷键（幂等：先注销旧的，再注册新的）。
///
/// 失败语义：新组合注册失败时会把旧组合恢复回来，应用状态与配置保持一致。
pub fn apply(app: &AppHandle) -> Result<(), String> {
  let state = app.state::<AppState>();
  let custom = state.config.lock().unwrap().global_shortcut.clone();
  let target = resolve_target(custom.as_deref())?;

  let gs = app.global_shortcut();

  // 取出并注销当前生效的组合
  let previous = state.active_shortcut.lock().unwrap().take();
  if let Some(prev) = &previous {
    if let Ok(prev_sc) = Shortcut::from_str(prev) {
      if let Err(err) = gs.unregister(prev_sc) {
        log::warn!("注销旧快捷键 {prev} 失败：{err}");
      }
    }
  }

  let Some(target) = target else {
    // 禁用态：已注销完毕，直接成功返回
    return Ok(());
  };

  let sc = Shortcut::from_str(&target).map_err(|e| format!("无法解析快捷键「{target}」：{e}"))?;
  if let Err(err) = gs.register(sc) {
    // 注册失败：尽量把旧组合恢复，避免用户「改了个冲突组合反而把好用的丢了」
    if let Some(prev) = &previous {
      if let Ok(prev_sc) = Shortcut::from_str(prev) {
        let _ = gs.register(prev_sc);
        *state.active_shortcut.lock().unwrap() = Some(prev.clone());
      }
    }
    return Err(format!("快捷键「{target}」注册失败，可能已被系统或其他应用占用：{err}"));
  }

  *state.active_shortcut.lock().unwrap() = Some(target);
  Ok(())
}

/// 快捷键触发：在托盘图标附近切换弹窗。
/// 托盘 rect 拿不到（极端时序）时退化为系统默认屏顶部居中放置的虚拟热区。
pub fn on_trigger(app: &AppHandle) {
  let rect = app
    .tray_by_id("main-tray")
    .and_then(|t| t.rect().ok())
    .flatten()
    .or_else(|| fallback_rect(app));
  match rect {
    Some(rect) => tray::toggle_popup(app, rect),
    None => log::error!("无法确定弹窗位置：托盘与显示器信息均不可用"),
  }
}

/// 兜底热区：默认显示器顶部中央，32×32 逻辑像素。
fn fallback_rect(app: &AppHandle) -> Option<Rect> {
  let monitor = app.primary_monitor().ok().flatten()?;
  let center_x = monitor.position().x as f64 + monitor.size().width as f64 / 2.0;
  Some(Rect {
    position: LogicalPosition::new(center_x - 16.0, monitor.position().y as f64 + 8.0).into(),
    size: LogicalSize::new(32.0, 32.0).into(),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  /// 平台默认与自定义组合都能被插件解析；禁用态返回 None
  #[test]
  fn resolves_targets() {
    assert_eq!(resolve_target(None).unwrap(), Some(DEFAULT_SHORTCUT.into()));
    assert_eq!(resolve_target(Some("")).unwrap(), None);
    assert_eq!(
      resolve_target(Some("Ctrl+Alt+O")).unwrap(),
      Some("Ctrl+Alt+O".into())
    );
  }

  /// 默认组合字符串必须能被 global-hotkey 解析（防止改默认值时引入非法 token）
  #[test]
  fn default_shortcut_parses() {
    assert!(Shortcut::from_str(DEFAULT_SHORTCUT).is_ok());
  }
}
