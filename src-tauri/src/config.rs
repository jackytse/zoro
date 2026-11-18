//! 应用配置：常用开发工具（Tool）与最近项目（Project）的定义、持久化与默认值。
//!
//! 配置以 JSON 形式存放在系统配置目录（macOS: ~/Library/Application Support/dev.zoro.desktop），
//! 读取失败时回退到默认配置，避免一个坏文件让托盘功能整个不可用。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
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
  /// 全局快捷键（global-hotkey 格式，如 "Super+Shift+P"）。
  /// None = 平台默认（⌘+Shift+P / Win+Shift+P）；Some("") = 禁用。
  #[serde(default)]
  pub global_shortcut: Option<String>,
  /// 开机自动启动，默认开启；自启动时带 --hidden 参数最小化到托盘。
  #[serde(default = "default_true")]
  pub autostart: bool,
  /// 终端打开方式："window" = 新窗口（默认），"existing" = 现有窗口（新标签页）。
  /// 旧配置文件没有该字段时按默认值反序列化，不需要迁移。
  #[serde(default)]
  pub terminal_open_mode: Option<String>,
}

/// serde 默认值辅助：true
fn default_true() -> bool {
  true
}

/// 终端打开方式的缺省值：新窗口。
/// Option<String> + None 处理为 window，而不是用 serde default 字符串，
/// 这样「用户显式存过 existing」与「从未设置」在语义上都归一到 window/existing。
impl Config {
  /// 归一化后的终端打开方式：只可能是 "window" 或 "existing"
  pub fn terminal_mode(&self) -> &str {
    match self.terminal_open_mode.as_deref() {
      Some("existing") => "existing",
      _ => "window",
    }
  }
}

/// 全局共享状态：配置本体 + 文件路径 + 托盘弹窗的显示时序记录。
pub struct AppState {
  pub config: Mutex<Config>,
  pub config_path: PathBuf,
  /// 最近一次弹窗因「失焦」被隐藏的时间，用于区分托盘图标的打开/关闭两种点击意图
  pub popup_last_hidden: Mutex<Option<Instant>>,
  /// 最近一次弹窗被显示的时间，用于过滤 show 后瞬间出现的假失焦
  pub popup_last_shown: Mutex<Option<Instant>>,
  /// 当前实际注册成功的全局快捷键（global-hotkey 格式），None 表示未注册（禁用态）
  pub active_shortcut: Mutex<Option<String>>,
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
      global_shortcut: None,
      autostart: true,
      // 出厂默认：终端在新窗口打开（旧配置文件缺字段时同样按 window 处理）
      terminal_open_mode: None,
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
    active_shortcut: Mutex::new(None),
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

/// 路径不存在时猜测「用户真正想输入的目录」，用于报错信息里给出可采纳的建议。
///
/// 处理两类常见手滑：
/// 1. 漏了开头的 `/`（如 `Users/me/proj`）→ 补上后存在即返回；
/// 2. 中间某级目录名拼错（如 `Projecjs` vs `Projects`）→ 找到最近的
///    真实存在的祖先目录，在它的子项里找编辑距离最近的名字，再拼回剩余路径。
pub fn suggest_similar_dir(path: &str) -> Option<String> {
  // 情况 1：漏了前导斜杠
  if !path.starts_with('/') {
    let with_slash = format!("/{path}");
    if Path::new(&with_slash).is_dir() {
      return Some(with_slash);
    }
  }

  let p = Path::new(path);
  // 从最深一级向上找第一个真实存在的祖先目录
  let mut ancestor = p.parent()?;
  while !ancestor.as_os_str().is_empty() && !ancestor.is_dir() {
    ancestor = ancestor.parent()?;
  }
  if ancestor.as_os_str().is_empty() {
    return None;
  }

  // tail 是祖先之后缺失的部分，至少一级；拿第一级去和现有子项比对
  let mut tail = p.strip_prefix(ancestor).ok()?.components();
  let missing = tail.next()?.as_os_str().to_string_lossy().into_owned();
  let rest: Vec<_> = tail.collect();

  // 在祖先的子目录里找编辑距离最小的名字
  let best = std::fs::read_dir(ancestor)
    .ok()?
    .filter_map(|e| e.ok())
    .filter(|e| e.path().is_dir())
    .map(|e| e.file_name().to_string_lossy().into_owned())
    .min_by_key(|n| levenshtein(n, &missing))?;

  // 阈值：距离 ≤ 3 且不超过较长串长度的一半，防止把完全不相关的目录拉进来
  let distance = levenshtein(&best, &missing);
  if distance > 3 || distance * 2 > best.len().max(missing.len()) {
    return None;
  }

  // 拼回建议路径：祖先/相近名/剩余部分
  let mut suggestion = ancestor.join(&best);
  for part in rest {
    suggestion.push(part);
  }
  Some(suggestion.to_string_lossy().into_owned())
}

/// 经典动态规划编辑距离，按字符比较（正确处理中文等多字节字符）。
fn levenshtein(a: &str, b: &str) -> usize {
  let a: Vec<char> = a.chars().collect();
  let b: Vec<char> = b.chars().collect();
  // 滚动数组：只保留上一行，空间 O(min(m,n))
  let mut prev: Vec<usize> = (0..=b.len()).collect();
  for (i, ca) in a.iter().enumerate() {
    let mut cur = vec![i + 1; b.len() + 1];
    for (j, cb) in b.iter().enumerate() {
      cur[j + 1] = (prev[j] + usize::from(ca != cb))
        .min(prev[j + 1] + 1)
        .min(cur[j] + 1);
    }
    prev = cur;
  }
  prev[b.len()]
}

#[cfg(test)]
mod tests {
  use super::*;

  /// 编辑距离基本性质：相同为 0，插入/替换各计 1
  #[test]
  fn levenshtein_basics() {
    assert_eq!(levenshtein("Projects", "Projects"), 0);
    assert_eq!(levenshtein("Projecjs", "Projects"), 1); // 一次替换：j -> t
    assert_eq!(levenshtein("", "abc"), 3);
    assert_eq!(levenshtein("项目", "项止"), 1);
  }

  /// 端到端：拼错的目录名能推荐出真实目录；不相关的则返回 None
  #[test]
  fn suggests_close_sibling_dir() {
    let tmp = std::env::temp_dir().join(format!("zoro-suggest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("Projects/zoro")).unwrap();
    std::fs::create_dir_all(tmp.join("Other")).unwrap();

    let home = tmp.to_string_lossy();

    // 拼写错误（中间目录）→ 建议真实目录并拼回尾部
    let typo = format!("{home}/Projecjs/zoro");
    assert_eq!(
      suggest_similar_dir(&typo),
      Some(format!("{home}/Projects/zoro"))
    );

    // 完全不存在的名字且无相近项 → None
    let nonsense = format!("{home}/qqqqqqqqqq");
    assert_eq!(suggest_similar_dir(&nonsense), None);

    std::fs::remove_dir_all(&tmp).unwrap();
  }
}
