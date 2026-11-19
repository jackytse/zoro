//! 应用内更新（多通道）。
//!
//! 更新通道分「正式版 stable」与「预发布 beta」，两个通道各对应固定 tag
//! 「updater-manifest」Release 上的一份清单附件（CI 每次发版覆盖上传同名
//! 附件即完成清单更新，客户端端点永不变化）：
//!   - stable → https://cnb.cool/jackytse/zoro/-/releases/download/updater-manifest/latest.json
//!   - beta   → https://cnb.cool/jackytse/zoro/-/releases/download/updater-manifest/latest-beta.json
//! （releases/download 附件地址匿名可下载，与参考实现 wocnb 的热更新链路一致）
//!
//! 为什么不用 JS 端 @tauri-apps/plugin-updater 的 check()：它只能用
//! tauri.conf.json 里配置的固定端点，无法按用户选择的通道切换清单文件。
//! 所以检测与下载安装都在本模块用 updater_builder() 覆盖端点实现，
//! 检测结果（Update 对象）留在 Rust 全局状态里，前端只拿到只读摘要；
//! 下载进度经 emit 事件（updater://progress）推回前端状态机。

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// 待安装的更新：updater_check 命中时写入，updater_download_and_install 消费。
/// Update 对象内部记录了目标版本、下载地址与签名，检测/下载必须复用同一个
/// 实例（相当于 JS 插件把 Update 传回给 downloadAndInstall 的角色）。
#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

/// 检测结果摘要：前端只需要「有没有新版本、是什么版本」，不持有连接细节
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
  /// 新版本号（来自清单顶层 version）
  version: String,
  /// 当前运行的应用版本
  current_version: String,
  /// 更新说明（清单 notes 字段，可能缺省）
  notes: Option<String>,
}

/// 更新通道 → 清单文件名。stable 缺省兜底，非法值也回落到 stable，
/// 前端传错参数不会 500，只是查不到 beta 内容
fn channel_endpoint(channel: &str) -> Result<url::Url, String> {
  let file = if channel == "beta" {
    "latest-beta.json"
  } else {
    "latest.json"
  };
  // 清单托管在固定 tag「updater-manifest」的 Release 附件上：
  // CI 发版时覆盖上传同名附件，端点 URL 永不变化，且匿名可下载
  format!("https://cnb.cool/jackytse/zoro/-/releases/download/updater-manifest/{file}")
    .parse()
    .map_err(|e| format!("更新端点 URL 无效: {e}"))
}

/// 检测更新：按通道请求对应清单，与当前版本比对（插件内部按 semver 比较，
/// 预发布版本号如 0.1.6-beta.1 与正式版比较的语义符合 semver 规范）。
/// 返回 Some 表示发现新版本，None 表示已是最新。
#[tauri::command]
pub async fn updater_check(
  app: AppHandle,
  pending: State<'_, PendingUpdate>,
  channel: Option<String>,
) -> Result<Option<UpdateInfo>, String> {
  let channel = channel.unwrap_or_else(|| "stable".into());
  let url = channel_endpoint(&channel)?;

  // endpoints() 覆盖 tauri.conf.json 里的默认端点；公钥仍从配置读取。
  // build() 把配置固化为 Updater 实例（会先解析并校验公钥），再由它检测
  let update = app
    .updater_builder()
    .endpoints(vec![url])
    .map_err(|e| e.to_string())?
    .build()
    .map_err(|e| e.to_string())?
    .check()
    .await
    .map_err(|e| e.to_string())?;

  Ok(update.map(|u| {
    let info = UpdateInfo {
      version: u.version.clone(),
      current_version: u.current_version.clone(),
      notes: u.body.clone(),
    };
    // 同一时间只保留一个待安装更新：重复检测直接覆盖旧实例
    *pending.0.lock().unwrap() = Some(u);
    info
  }))
}

/// 下载并安装待更新包：Windows NSIS 下载完静默安装，macOS 替换 .app，
/// Linux AppImage 原地替换。安装完成后前端走 process 插件 relaunch 重启。
/// 分块进度经 updater://progress 事件推给前端（payload = [已下载字节增量, 总大小]）。
#[tauri::command]
pub async fn updater_download_and_install(
  app: AppHandle,
  pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
  let update = pending
    .0
    .lock()
    .unwrap()
    .take()
    .ok_or_else(|| "没有待安装的更新（请先检测更新）".to_string())?;

  // emit 的 AppHandle 克隆一份进 FnMut 闭包（原 app 在命令结束后释放）
  let app_for_progress = app.clone();
  update
    .download_and_install(
      move |chunk: usize, total: Option<u64>| {
        let _ = app_for_progress.emit("updater://progress", (chunk as u64, total));
      },
      || {},
    )
    .await
    .map_err(|e| e.to_string())
}
