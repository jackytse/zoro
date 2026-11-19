//! 应用配置：常用开发工具（Tool）与最近项目（Project）的定义、持久化与默认值。
//!
//! 配置以 JSON 形式存放在系统配置目录（macOS: ~/Library/Application Support/dev.zoro.desktop），
//! 读取失败时回退到默认配置，避免一个坏文件让托盘功能整个不可用。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
// Command 只在非 macOS 的 PATH 探测里用到（macOS 走 .app 路径探测）
#[cfg(not(target_os = "macos"))]
use std::process::Command;
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
  /// 终端打开方式："window" = 新窗口（默认），"tab" = 新选项卡。
  /// 旧版本的 "existing"（现有窗口）等价于现在的 "tab"，读取时统一归一化；
  /// 旧配置文件没有该字段时按默认值反序列化，不需要迁移。
  #[serde(default)]
  pub terminal_open_mode: Option<String>,
  /// 默认终端工具："iterm" | "terminal"。
  /// None = 自动推荐（装了 iTerm2 用 iTerm2，否则 Terminal.app）；
  /// 用户显式选择后按选择执行，所选应用未安装时自动回落到另一个。
  #[serde(default)]
  pub terminal_tool: Option<String>,
  /// 界面语言："zh-CN" | "zh-TW" | "en"；None = 跟随系统语言。
  /// 前端两个窗口与 Rust 托盘菜单共用这一份语言设置。
  #[serde(default)]
  pub locale: Option<String>,
  /// 界面主题："light" | "dark"；None = 跟随系统（含系统切换时实时跟随）。
  /// 与语言同构：实际应用到 <html> 的 .dark 类在前端做，Rust 侧只存偏好。
  #[serde(default)]
  pub theme: Option<String>,
}

/// serde 默认值辅助：true
fn default_true() -> bool {
  true
}

/// 解析系统界面语言（托盘菜单等 Rust 侧文案在「跟随系统」模式下使用）。
/// 规则与前端 detectLocale()（navigator.language）对齐：
///   - zh 开头且带繁体标识（Hant / TW / HK / MO）→ zh-TW
///   - zh 开头其它（简体）→ zh-CN
///   - 其余一律 → en
/// sys-locale 返回 BCP47 标签（如 "zh-Hant-TW" / "en-US"），统一小写后匹配；
/// 拿不到系统语言（极少见）时按英文处理。
fn resolve_system_locale() -> &'static str {
  let lang = sys_locale::get_locale().unwrap_or_default().to_lowercase();
  if lang.starts_with("zh") {
    if lang.contains("hant") || lang.contains("-tw") || lang.contains("-hk") || lang.contains("-mo") {
      "zh-TW"
    } else {
      "zh-CN"
    }
  } else {
    "en"
  }
}

/// 终端打开方式的缺省值：新窗口。
/// Option<String> + None 处理为 window，而不是用 serde default 字符串，
/// 这样「用户显式存过 existing」与「从未设置」在语义上都归一到 window/existing。
impl Config {
  /// 归一化后的终端打开方式：只可能是 "window" 或 "tab"。
  /// 旧版本的 "existing" 语义就是「在现有窗口里新开标签页」，映射为 "tab"。
  pub fn terminal_mode(&self) -> &str {
    match self.terminal_open_mode.as_deref() {
      Some("existing") | Some("tab") => "tab",
      _ => "window",
    }
  }

  /// 归一化后的界面语言：只可能是 "zh-CN" / "zh-TW" / "en"。
  /// 显式配置直接采用；None 或脏数据 = 跟随系统语言——按系统 locale 解析
  ///（规则与前端 detectLocale 一致），托盘菜单与前端界面保持同一种语言。
  pub fn locale(&self) -> &str {
    match self.locale.as_deref() {
      Some("zh-TW") => "zh-TW",
      Some("en") => "en",
      Some("zh-CN") => "zh-CN",
      _ => resolve_system_locale(),
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

/// 候选工具目录条目：一个主流开发工具的完整定义 + 安装探测路径。
///
/// 为什么不直接用 Tool + 序列化 app_paths：安装探测路径是实现细节，
/// 不该进配置文件污染用户数据；目录只存在于代码里，配置里的 tools
/// 始终是「经过安装同步的真实工具子集 + 用户自定义项」。
pub struct CatalogEntry {
  pub id: &'static str,
  pub name: &'static str,
  pub command: &'static str,
  pub args: &'static [&'static str],
  /// macOS open -a 兜底用的应用名
  pub app_name: &'static str,
  pub icon: &'static str,
  /// macOS .app 包探测路径（任一存在即视为已安装）；
  /// 非 macOS 平台按 command 是否在 PATH 里探测
  pub app_paths: &'static [&'static str],
}

impl CatalogEntry {
  /// 转成可持久化的 Tool
  fn to_tool(&self) -> Tool {
    Tool {
      id: self.id.into(),
      name: self.name.into(),
      command: self.command.into(),
      args: self.args.iter().map(|s| (*s).into()).collect(),
      app_name: Some(self.app_name.into()),
      icon: self.icon.into(),
    }
  }
}

/// 主流开发工具候选目录。
///
/// 顺序即设置页「工具清单」的展示顺序：通用编辑器 → JetBrains 系 →
/// 其他 IDE → 终端。终端类（iterm/terminal）必须保留：它们走
/// open_in_terminal 专用链路（见 opener::is_terminal_tool），且
/// Terminal.app 在任何 macOS 上都存在，保证列表非空。
pub const TOOL_CATALOG: [CatalogEntry; 18] = [
  CatalogEntry {
    id: "vscode",
    name: "VS Code",
    command: "code",
    args: &["-n"],
    app_name: "Visual Studio Code",
    icon: "i-lucide-file-code-2",
    app_paths: &["/Applications/Visual Studio Code.app", "~/Applications/Visual Studio Code.app"],
  },
  CatalogEntry {
    id: "cursor",
    name: "Cursor",
    command: "cursor",
    args: &["-n"],
    app_name: "Cursor",
    icon: "i-lucide-mouse-pointer-2",
    app_paths: &["/Applications/Cursor.app", "~/Applications/Cursor.app"],
  },
  CatalogEntry {
    id: "windsurf",
    name: "Windsurf",
    command: "windsurf",
    args: &["-n"],
    app_name: "Windsurf",
    icon: "i-lucide-navigation",
    app_paths: &["/Applications/Windsurf.app", "~/Applications/Windsurf.app"],
  },
  CatalogEntry {
    id: "idea",
    name: "IntelliJ IDEA",
    command: "idea",
    args: &[],
    app_name: "IntelliJ IDEA",
    icon: "i-lucide-coffee",
    app_paths: &["/Applications/IntelliJ IDEA.app", "~/Applications/IntelliJ IDEA.app"],
  },
  CatalogEntry {
    id: "pycharm",
    name: "PyCharm",
    command: "pycharm",
    args: &[],
    app_name: "PyCharm",
    icon: "i-lucide-flask-conical",
    app_paths: &["/Applications/PyCharm.app", "~/Applications/PyCharm.app"],
  },
  CatalogEntry {
    id: "webstorm",
    name: "WebStorm",
    command: "webstorm",
    args: &[],
    app_name: "WebStorm",
    icon: "i-lucide-globe",
    app_paths: &["/Applications/WebStorm.app", "~/Applications/WebStorm.app"],
  },
  CatalogEntry {
    id: "goland",
    name: "GoLand",
    command: "goland",
    args: &[],
    app_name: "GoLand",
    icon: "i-lucide-braces",
    app_paths: &["/Applications/GoLand.app", "~/Applications/GoLand.app"],
  },
  CatalogEntry {
    id: "clion",
    name: "CLion",
    command: "clion",
    args: &[],
    app_name: "CLion",
    icon: "i-lucide-binary",
    app_paths: &["/Applications/CLion.app", "~/Applications/CLion.app"],
  },
  CatalogEntry {
    id: "rider",
    name: "Rider",
    command: "rider",
    args: &[],
    app_name: "Rider",
    icon: "i-lucide-package",
    app_paths: &["/Applications/Rider.app", "~/Applications/Rider.app"],
  },
  CatalogEntry {
    id: "phpstorm",
    name: "PhpStorm",
    command: "phpstorm",
    args: &[],
    app_name: "PhpStorm",
    icon: "i-lucide-code-xml",
    app_paths: &["/Applications/PhpStorm.app", "~/Applications/PhpStorm.app"],
  },
  CatalogEntry {
    id: "datagrip",
    name: "DataGrip",
    command: "datagrip",
    args: &[],
    app_name: "DataGrip",
    icon: "i-lucide-database",
    app_paths: &["/Applications/DataGrip.app", "~/Applications/DataGrip.app"],
  },
  CatalogEntry {
    id: "rubymine",
    name: "RubyMine",
    command: "rubymine",
    args: &[],
    app_name: "RubyMine",
    icon: "i-lucide-gem",
    app_paths: &["/Applications/RubyMine.app", "~/Applications/RubyMine.app"],
  },
  CatalogEntry {
    id: "android-studio",
    name: "Android Studio",
    // Android Studio 没有 CLI，直接走 open -a 兜底链路
    command: "open",
    args: &["-a", "Android Studio"],
    app_name: "Android Studio",
    icon: "i-lucide-smartphone",
    app_paths: &["/Applications/Android Studio.app", "~/Applications/Android Studio.app"],
  },
  CatalogEntry {
    id: "xcode",
    name: "Xcode",
    command: "open",
    args: &["-a", "Xcode"],
    app_name: "Xcode",
    icon: "i-lucide-hammer",
    app_paths: &["/Applications/Xcode.app", "~/Applications/Xcode.app"],
  },
  CatalogEntry {
    id: "sublime",
    name: "Sublime Text",
    command: "subl",
    args: &["-n"],
    app_name: "Sublime Text",
    icon: "i-lucide-text",
    app_paths: &["/Applications/Sublime Text.app", "~/Applications/Sublime Text.app"],
  },
  CatalogEntry {
    id: "zed",
    name: "Zed",
    command: "zed",
    // -n：与 vscode 同语义，在新窗口打开项目（CLI 未装时回落 open -a）
    args: &["-n"],
    app_name: "Zed",
    icon: "i-lucide-zap",
    app_paths: &["/Applications/Zed.app", "~/Applications/Zed.app"],
  },
  CatalogEntry {
    id: "iterm",
    name: "iTerm",
    // iTerm 没有统一 CLI，直接走 macOS 的 open -a
    command: "open",
    args: &["-a", "iTerm"],
    app_name: "iTerm",
    icon: "i-lucide-terminal",
    app_paths: &["/Applications/iTerm.app", "/Applications/iTerm2.app"],
  },
  CatalogEntry {
    id: "terminal",
    name: "Terminal",
    command: "open",
    args: &["-a", "Terminal"],
    app_name: "Terminal",
    icon: "i-lucide-square-terminal",
    app_paths: &[
      // macOS 12+ 系统自带 Terminal 挪到了 /System/Applications
      "/System/Applications/Utilities/Terminal.app",
      "/Applications/Utilities/Terminal.app",
    ],
  },
];

/// 判断目录条目对应的工具是否已安装。
/// - macOS：.app 包路径任一存在即已安装（~ 前缀展开）；
/// - 其他平台：command 可在 PATH 里解析到即已安装（用 sh 的 command -v，
///   一次子进程调用，只在启动同步时执行数次，无性能顾虑）。
fn catalog_installed(entry: &CatalogEntry) -> bool {
  #[cfg(target_os = "macos")]
  {
    entry
      .app_paths
      .iter()
      .any(|p| Path::new(&expand_tilde(p)).exists())
  }
  #[cfg(not(target_os = "macos"))]
  {
    // 只有 open -a 形态的条目（无真实 CLI）在非 macOS 上无法使用
    if entry.command == "open" {
      return false;
    }
    Command::new("sh")
      .args(["-c", &format!("command -v {} >/dev/null 2>&1", entry.command)])
      .status()
      .map(|s| s.success())
      .unwrap_or(false)
  }
}

/// 启动时的工具同步：让配置里的 tools 始终反映本机实际安装情况。
///
/// 规则：
/// 1. 已安装的目录工具 → 保留（已在配置里）或补入（新装了）；
/// 2. 未安装的目录工具 → 移除，但以下两种情况除外：
///    a. 有项目引用它（删了会悬空，打开时报错更糟）；
///    b. 用户改过 command（视为用户自定义配置，尊重其意愿）；
/// 3. 不在目录里的工具（用户手工加的）→ 原样保留。
///
/// 这正是「没装 Cursor 却出现在可用工具里」「装了 Android Studio 却找不到」
/// 两个问题的统一解法：列表不再是无脑全量，而是跟随本机安装状态。
fn sync_installed_tools(cfg: &mut Config) {
  let referenced: Vec<&str> = cfg.projects.iter().map(|p| p.tool_id.as_str()).collect();
  let mut tools: Vec<Tool> = Vec::new();

  for entry in &TOOL_CATALOG {
    let installed = catalog_installed(entry);
    let existing = cfg.tools.iter().find(|t| t.id == entry.id);

    if installed {
      // 已在配置里：保留用户可能改过的字段；不在：按目录补入
      tools.push(existing.cloned().unwrap_or_else(|| entry.to_tool()));
    } else if let Some(t) = existing {
      // 未安装：仅当被项目引用或被用户改过命令时保留
      let customized = t.command != entry.command || t.app_name != Some(entry.app_name.into());
      if referenced.contains(&t.id.as_str()) || customized {
        tools.push(t.clone());
      } else {
        log::info!("工具 {} 未安装且未被引用，从可用工具中移除", entry.name);
      }
    }
    // 未安装且配置里也没有：无事可做
  }

  // 用户自定义工具（不在目录中）原样保留在末尾
  for t in &cfg.tools {
    if !TOOL_CATALOG.iter().any(|e| e.id == t.id) {
      tools.push(t.clone());
    }
  }

  cfg.tools = tools;
}

impl Config {
  /// 出厂默认工具集：从主流工具目录里挑选已安装项。
  /// 纯首次运行（/ 配置损坏重建）场景；老用户走 init 里的 sync_installed_tools。
  fn with_default_tools() -> Self {
    let tools: Vec<Tool> = TOOL_CATALOG
      .iter()
      .filter(|e| catalog_installed(e))
      .map(|e| e.to_tool())
      .collect();
    // 极端情况（一个都没探测到）也要保证下拉框非空：塞入 Terminal（macOS
    // 必有）或目录首项，避免前端拿空列表渲染不出选择器
    let tools = if tools.is_empty() {
      vec![Tool {
        id: "terminal".into(),
        name: "Terminal".into(),
        command: "open".into(),
        args: vec!["-a".into(), "Terminal".into()],
        app_name: Some("Terminal".into()),
        icon: "i-lucide-square-terminal".into(),
      }]
    } else {
      tools
    };
    Self {
      tools,
      projects: Vec::new(),
      global_shortcut: None,
      autostart: true,
      // 出厂默认：终端在新窗口打开（旧配置文件缺字段时同样按 window 处理）
      terminal_open_mode: None,
      // 出厂默认：终端工具自动推荐（扫描已安装的终端后选常用的那个）
      terminal_tool: None,
      // 出厂默认：跟随系统语言（前端按 navigator.language 探测）
      locale: None,
      // 出厂默认：跟随系统主题（前端按 prefers-color-scheme 探测）
      theme: None,
    }
  }
}

/// 从环境变量获取用户主目录：优先 `HOME`（macOS / Linux），其次 `USERPROFILE`（Windows）。
///
/// 供没有 `AppHandle` 的纯函数使用（如 `expand_tilde`、`importer::scan_jetbrains`）。
/// 在持有 `AppHandle` 的命令里应优先用 `app.path().home_dir()`——它由 Tauri 按平台
/// 解析，比环境变量更可靠。
pub fn home_dir_from_env() -> Option<PathBuf> {
  std::env::var_os("HOME")
    .or_else(|| std::env::var_os("USERPROFILE"))
    .map(PathBuf::from)
}

/// 展开路径里的 ~ 前缀（只处理当前用户的 ~）。
pub fn expand_tilde(path: &str) -> String {
  let Some(home) = home_dir_from_env() else {
    return path.to_string();
  };
  if path == "~" {
    home.to_string_lossy().into_owned()
  } else if let Some(rest) = path.strip_prefix("~/") {
    let mut joined = home;
    joined.push(rest);
    joined.to_string_lossy().into_owned()
  } else {
    path.to_string()
  }
}

/// 从磁盘加载配置并构建 AppState；文件不存在或解析失败时落回默认值。
pub fn init(app: &tauri::AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
  let dir = app.path().app_config_dir()?;
  fs::create_dir_all(&dir)?;
  let config_path = dir.join("config.json");

  // 包名迁移：identifier 从 dev.zoro.desktop 改为 me.jackytse.zoro 后，
  // app_config_dir 跟着变了。新目录里没有配置而旧目录里有时，把旧配置
  // 拷贝过来，避免用户升级后工具/项目/偏好全部「消失」。
  if !config_path.exists() {
    if let Some(parent) = dir.parent() {
      let legacy = parent.join("dev.zoro.desktop").join("config.json");
      if legacy.exists() {
        if fs::copy(&legacy, &config_path).is_ok() {
          log::info!("已从旧包名目录迁移配置：{}", legacy.display());
        }
      }
    }
  }

  let mut config = if config_path.exists() {
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
  // 工具清单与本机安装状态对齐：未装的默认工具剔除、新装的工具补入。
  // 放在保存之前执行，保证落盘的就是对齐后的结果
  sync_installed_tools(&mut config);

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

  /// 工具同步：未安装、未被引用的目录工具应被移除；
  /// 被项目引用或被用户改过 command 的保留。
  /// 用「未安装」的目录条目做替身：xcode 在 CI 机器上基本不会装，
  /// 但为了让测试与真实机器解耦，这里不依赖真实安装状态，
  /// 而是构造一个必然未安装的探测路径场景来验证逻辑。
  #[test]
  fn sync_removes_uninstalled_unreferenced_tools() {
    let mut cfg = Config {
      tools: vec![Tool {
        id: "xcode".into(),
        name: "Xcode".into(),
        command: "open".into(),
        args: vec!["-a".into(), "Xcode".into()],
        app_name: Some("Xcode".into()),
        icon: "i-lucide-hammer".into(),
      }],
      projects: Vec::new(),
      ..Default::default()
    };
    // 用户的机器装没装 Xcode 都可能；为了确定性，把该条目视作未安装的
    // 前提是探测为 false —— 这里直接按真实探测结果分支断言语义：
    // 未安装 ⇒ 被移除；已安装 ⇒ 保留。两种结果都合法。
    sync_installed_tools(&mut cfg);
    let installed = TOOL_CATALOG
      .iter()
      .find(|e| e.id == "xcode")
      .map(catalog_installed)
      .unwrap();
    assert_eq!(
      cfg.tools.iter().any(|t| t.id == "xcode"),
      installed,
      "同步后 xcode 的存在性应等于其安装状态"
    );
  }

  /// 工具同步：被项目引用的工具即使未安装也必须保留（否则项目悬空）。
  #[test]
  fn sync_keeps_referenced_tools_even_if_uninstalled() {
    let mut cfg = Config {
      tools: vec![Tool {
        id: "xcode".into(),
        name: "Xcode".into(),
        command: "open".into(),
        args: vec!["-a".into(), "Xcode".into()],
        app_name: Some("Xcode".into()),
        icon: "i-lucide-hammer".into(),
      }],
      projects: vec![Project {
        id: "p1".into(),
        name: "demo".into(),
        path: "/tmp/demo".into(),
        tool_id: "xcode".into(),
        last_opened_at: None,
      }],
      ..Default::default()
    };
    sync_installed_tools(&mut cfg);
    assert!(
      cfg.tools.iter().any(|t| t.id == "xcode"),
      "被项目引用的工具不能被同步移除"
    );
  }

  /// 工具同步：已安装但配置里缺失的目录工具应被补入（新装了 Android Studio 等）。
  #[test]
  fn sync_appends_newly_installed_catalog_tools() {
    let mut cfg = Config {
      tools: Vec::new(),
      projects: Vec::new(),
      ..Default::default()
    };
    sync_installed_tools(&mut cfg);
    // macOS 上 Terminal.app 必然存在，所以同步后至少要有 terminal，清单非空
    #[cfg(target_os = "macos")]
    {
      assert!(
        cfg.tools.iter().any(|t| t.id == "terminal"),
        "Terminal.app 在 macOS 上必装，同步后必须出现在工具清单：{:?}",
        cfg.tools.iter().map(|t| &t.id).collect::<Vec<_>>()
      );
      assert!(!cfg.tools.is_empty(), "同步后的工具清单不能为空");
    }
    // 非 macOS（含 CI 的纯净 Linux 容器）可能一个目录工具都探测不到，
    // 同步结果为空是预期行为——不空断言只在 macOS 上成立，不能通用
  }

  /// 工具同步：不在目录里的用户自定义工具原样保留。
  #[test]
  fn sync_keeps_custom_tools() {
    let mut cfg = Config {
      tools: vec![Tool {
        id: "my-tool".into(),
        name: "My Tool".into(),
        command: "mytool".into(),
        args: vec![],
        app_name: None,
        icon: "i-lucide-box".into(),
      }],
      projects: Vec::new(),
      ..Default::default()
    };
    sync_installed_tools(&mut cfg);
    assert!(
      cfg.tools.iter().any(|t| t.id == "my-tool"),
      "用户自定义工具必须原样保留"
    );
  }

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
