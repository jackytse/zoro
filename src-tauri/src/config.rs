//! 应用配置：常用开发工具（Tool）与最近项目（Project）的定义、持久化与默认值。
//!
//! 配置以 JSON 形式存放在系统配置目录（macOS: ~/Library/Application Support/dev.zoro.desktop），
//! 读取失败时回退到默认配置，避免一个坏文件让托盘功能整个不可用。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use tauri::Manager;

/// 一个可用来打开项目的开发工具。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
  /// 稳定标识，项目通过 tool_id 引用它
  pub id: String,
  /// 展示名，如 "VS Code"
  pub name: String,
  /// CLI 可执行文件名或绝对路径，如 "code"
  pub command: String,
  /// 固定附加参数，如 ["-n"]（VS Code 新窗口）
  #[serde(default)]
  pub args: Vec<String>,
  /// macOS 兜底用的应用名：CLI 找不到时退回 `open -a <app_name> <path>`
  #[serde(default)]
  pub app_name: Option<String>,
  /// 前端展示用的 lucide 图标名，如 "i-lucide-code-2"
  pub icon: String,
}

/// 一条「最近打开的项目」记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
  pub id: String,
  /// 展示名，默认取目录名
  pub name: String,
  /// 项目绝对路径（支持 ~ 前缀，加载时会展开）
  pub path: String,
  /// 使用哪个工具打开
  pub tool_id: String,
  /// 最近一次打开的毫秒时间戳，用于排序；None 表示从未打开过
  #[serde(default)]
  pub last_opened_at: Option<i64>,
}

/// 持久化的整体配置。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
  pub tools: Vec<Tool>,
  pub projects: Vec<Project>,
}

/// 全局共享状态：配置本体 + 文件路径 + 托盘弹窗的显示时序记录。
pub struct AppState {
  pub config: Mutex<Config>,
  pub config_path: PathBuf,
  /// 最近一次弹窗因「失焦」被隐藏的时间，用于区分托盘图标的打开/关闭两种点击意图
  pub popup_last_hidden: Mutex<Option<Instant>>,
  /// 最近一次弹窗被显示的时间，用于过滤 show 后瞬间出现的假失焦
  pub popup_last_shown: Mutex<Option<Instant>>,
}

impl Config {
  /// 出厂默认工具集：均为常见 IDE/终端，用户可在设置页增删项目。
  fn with_default_tools() -> Self {
    Self {
      tools: vec![
        Tool {
          id: "vscode".into(),
          name: "VS Code".into(),
          command: "code".into(),
          args: vec!["-n".into()],
          app_name: Some("Visual Studio Code".into()),
          icon: "i-lucide-file-code-2".into(),
        },
        Tool {
          id: "cursor".into(),
          name: "Cursor".into(),
          command: "cursor".into(),
          args: vec!["-n".into()],
          app_name: Some("Cursor".into()),
          icon: "i-lucide-mouse-pointer-2".into(),
        },
        Tool {
          id: "idea".into(),
          name: "IntelliJ IDEA".into(),
          command: "idea".into(),
          args: vec![],
          app_name: Some("IntelliJ IDEA".into()),
          icon: "i-lucide-coffee".into(),
        },
        Tool {
          id: "iterm".into(),
          name: "iTerm".into(),
          // iTerm 没有统一 CLI，直接走 macOS 的 open -a
          command: "open".into(),
          args: vec!["-a".into(), "iTerm".into()],
          app_name: None,
          icon: "i-lucide-terminal".into(),
        },
      ],
      projects: Vec::new(),
    }
  }
}

/// 展开路径里的 ~ 与 ~user 前缀（只处理当前用户的 ~）。
pub fn expand_tilde(path: &str) -> String {
  if path == "~" {
    if let Some(home) = std::env::var_os("HOME") {
      return home.to_string_lossy().into_owned();
    }
  } else if let Some(rest) = path.strip_prefix("~/") {
    if let Some(home) = std::env::var_os("HOME") {
      let mut joined = PathBuf::from(home);
      joined.push(rest);
      return joined.to_string_lossy().into_owned();
    }
  }
  path.to_string()
}

/// 从磁盘加载配置并构建 AppState；文件不存在或解析失败时落回默认值。
pub fn init(app: &tauri::AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
  let dir = app.path().app_config_dir()?;
  fs::create_dir_all(&dir)?;
  let config_path = dir.join("config.json");

  let config = if config_path.exists() {
    match serde_json::from_str::<Config>(&fs::read_to_string(&config_path)?) {
      // 旧配置里的 ~ 前缀在此处统一展开
      Ok(mut c) => {
        for p in &mut c.projects {
          p.path = expand_tilde(&p.path);
        }
        c
      }
      Err(err) => {
        log::warn!("配置文件损坏（{err}），使用默认配置");
        Config::with_default_tools()
      }
    }
  } else {
    Config::with_default_tools()
  };

  let state = AppState {
    config: Mutex::new(config),
    config_path,
    popup_last_hidden: Mutex::new(None),
    popup_last_shown: Mutex::new(None),
  };
  // 首次运行时把默认配置写盘，让设置页与托盘读到同一份来源
  save(&state)?;
  Ok(state)
}

/// 原子化写盘：先写临时文件再 rename，避免写一半留下坏 JSON。
pub fn save(state: &AppState) -> Result<(), String> {
  let config = state.config.lock().unwrap().clone();
  let tmp_path = state.config_path.with_extension("json.tmp");
  let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
  fs::write(&tmp_path, json).map_err(|e| e.to_string())?;
  fs::rename(&tmp_path, &state.config_path).map_err(|e| e.to_string())?;
  Ok(())
}

/// 生成不与现有条目冲突的短 ID。
pub fn next_id(existing: &[Project]) -> String {
  let base = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis();
  let mut id = format!("p{base}");
  let mut counter = 0u32;
  while existing.iter().any(|p| p.id == id) {
    counter += 1;
    id = format!("p{base}-{counter}");
  }
  id
}
