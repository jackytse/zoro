//! 从本机开发工具的历史记录里扫描「最近打开的项目目录」。
//!
//! 支持的数据源（macOS）：
//! - VS Code / Cursor（同构存储）：
//!   - 新版（1.64+）：`<app>/User/globalStorage/state.vscdb`（SQLite），
//!     `ItemTable` 表的 `history.recentlyOpenedPathsList` 键
//!   - 旧版兜底：`<app>/User/globalStorage/storage.json` 的同名 JSON 结构
//!   - 最新版（1.14x+，实测 1.146 已移除上述 key）：`<app>/User/workspaceStorage/<hash>/workspace.json`
//!     ——按文件 mtime 近似「最近打开」排序
//! - JetBrains 系列产品（IDEA / PyCharm / GoLand 等）：
//!   `~/Library/Application Support/JetBrains/<产品版本>/options/recentProjects.xml`
//!   - 新版：`additionalInfo` map 的 `<entry key="路径">`
//!   - 旧版：`recentPaths` 列表的 `<option value="路径">`
//! - Android Studio：数据文件格式与 JetBrains 完全一致，只是根目录不同——
//!   `~/Library/Application Support/Google/AndroidStudio<版本>/options/recentProjects.xml`
//! - Zed：状态持久化在 `~/Library/Application Support/Zed/db/<db>/db.sqlite`
//!   的 `kv_store` 表里，value 为序列化 blob，路径以明文嵌在字节流中——
//!   按字节流扫描提取（见 scan_zed），不依赖 SQLite 解析库
//!
//! 设计约定：
//! - 扫描是**只读**的，不会写入任何工具的数据文件；
//! - 结果按路径全局去重（同一路径只保留最先命中的来源）；
//! - 过滤掉不存在的目录与「路径 + 工具」均已配置过的条目，
//!   保证设置页看到的导入候选全是新增项。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 一条从本机工具记录里探测到的候选项目。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProject {
  /// 展示名，取目录名
  pub name: String,
  /// 项目绝对路径
  pub path: String,
  /// 建议使用的工具 id（即来源工具），导入时可在弹窗里改选
  pub tool_id: String,
}

/// 每个来源最多返回的条数：覆盖 IDE 的完整最近记录。
const MAX_PER_SOURCE: usize = 100;

/// 扫描入口。`home` 传用户主目录（独立成参数方便单元测试注入临时目录），
/// `existing` 是已配置的（路径, 工具id）列表，用于过滤重复项。
pub fn scan(home: &Path, existing: &[(String, String)]) -> Vec<DetectedProject> {
  let support = home.join("Library/Application Support");
  let mut found: Vec<DetectedProject> = Vec::new();
  // 跨来源按路径去重：VS Code 和 JetBrains 都打开过的目录只保留一条
  let mut seen: HashSet<String> = HashSet::new();

  // VS Code 与 Cursor 共用同一套 Code-OSS 存储格式，逐个扫描
  for (dir, tool_id) in [("Code", "vscode"), ("Cursor", "cursor")] {
    for p in scan_vscode_like(&support.join(dir), tool_id, existing) {
      if seen.insert(p.path.clone()) {
        found.push(p);
      }
    }
  }
  // JetBrains 各产品的记录统一归到 idea 工具（导入时可改选）
  for p in scan_jetbrains(&support.join("JetBrains"), existing, "idea") {
    if seen.insert(p.path.clone()) {
      found.push(p);
    }
  }
  // Android Studio 与 JetBrains 同构，但根目录在 Google/ 下，归到 android-studio
  for p in scan_jetbrains(&support.join("Google"), existing, "android-studio") {
    if seen.insert(p.path.clone()) {
      found.push(p);
    }
  }
  // Zed：状态持久化在 SQLite kv_store 的 value blob 里（见 scan_zed 注释）
  for p in scan_zed(&support.join("Zed"), home, existing) {
    if seen.insert(p.path.clone()) {
      found.push(p);
    }
  }
  found
}

/// 扫描 VS Code / Cursor。
/// 最近记录的三个来源按可靠性依次尝试：
/// 1. `state.vscdb` 的 `history.recentlyOpenedPathsList`（1.64 ~ 1.14x）；
/// 2. `storage.json` 的同名结构（更老版本）；
/// 3. `workspaceStorage/*/workspace.json`（**新版唯一可用来源**）。
///
/// 第 3 条是必需项而非兜底：实测 VS Code 1.146 的 ItemTable 里已经没有
/// `history.recentlyOpenedPathsList` 这个 key，最近列表只体现为每个曾打开
/// 过的窗口留下的 workspaceStorage 目录。
fn scan_vscode_like(
  base: &Path,
  tool_id: &str,
  existing: &[(String, String)],
) -> Vec<DetectedProject> {
  let global = base.join("User/globalStorage");

  // 按上述优先级拼接，后面按路径去重（保留最先生成的来源顺序）
  let mut uris = read_uris_from_vscdb(&global.join("state.vscdb"));
  uris.extend(read_uris_from_storage_json(&global.join("storage.json")));
  uris.extend(read_uris_from_workspace_storage(base));

  let mut out = Vec::new();
  let mut seen: HashSet<String> = HashSet::new();
  for uri in uris {
    if out.len() >= MAX_PER_SOURCE {
      break;
    }
    // vscode-remote:// 等远程 URI 在这里返回 None，被自然过滤
    let Some(mut path) = file_uri_to_path(&uri) else {
      continue;
    };
    // .code-workspace 多根工作区：记录的是文件而非目录，
    // 取它所在目录当作项目目录（通常就是仓库根）更符合「导入项目」的直觉
    if path.ends_with(".code-workspace") {
      match Path::new(&path).parent() {
        Some(parent) if parent.is_dir() => path = parent.to_string_lossy().into_owned(),
        _ => continue,
      }
    }
    if !seen.insert(path.clone()) {
      continue;
    }
    push_if_valid(&mut out, path, tool_id, existing);
  }
  out
}

/// 从 `User/workspaceStorage` 还原最近打开的目录。
///
/// 每个打开过的窗口都会在 `workspaceStorage/<hash>/` 留下 workspace.json：
/// - 单目录窗口：`{"folder": "file:///path"}`
/// - `.code-workspace` 多根窗口：`{"workspace": "file:///path/x.code-workspace"}`
///
/// 排序依据：workspace.json 的 mtime（每次打开窗口都会刷新），
/// 用它近似「最近打开」的先后，保证最常用的项目排在前面。
fn read_uris_from_workspace_storage(base: &Path) -> Vec<String> {
  let dir = base.join("User/workspaceStorage");
  let Ok(entries) = std::fs::read_dir(&dir) else {
    return Vec::new();
  };

  // (mtime 毫秒, uri)；mtime 拿不到时按 0 处理，排在最后
  let mut items: Vec<(u64, String)> = Vec::new();
  for entry in entries.flatten() {
    let dir_path = entry.path();
    if !dir_path.is_dir() {
      continue;
    }
    let meta_path = dir_path.join("workspace.json");
    let Ok(raw) = std::fs::read_to_string(&meta_path) else {
      continue;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
      continue;
    };
    // folder 优先（普通项目），没有再取 workspace（多根工作区文件）
    let Some(uri) = json
      .get("folder")
      .and_then(|v| v.as_str())
      .or_else(|| json.get("workspace").and_then(|v| v.as_str()))
    else {
      continue;
    };
    let mtime = std::fs::metadata(&meta_path)
      .ok()
      .and_then(|m| m.modified().ok())
      .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
      .map(|d| d.as_millis() as u64)
      .unwrap_or(0);
    items.push((mtime, uri.to_string()));
  }

  // 最近的在前
  items.sort_by(|a, b| b.0.cmp(&a.0));
  items.into_iter().map(|(_, uri)| uri).collect()
}

/// 从 state.vscdb（SQLite）读 `history.recentlyOpenedPathsList` 的 folderUri 列表。
///
/// VS Code 运行时持有该库的写锁，因此先把主库文件与 -wal / -shm 一并
/// 复制到临时目录再打开副本：不影响正在运行的 VS Code，也避免读取被拒。
fn read_uris_from_vscdb(db_path: &Path) -> Vec<String> {
  if !db_path.exists() {
    return Vec::new();
  }
  let tmp_base = std::env::temp_dir().join(format!("zoro-vscdb-{}.sqlite", std::process::id()));
  // 主库 + WAL/SHM 一起复制：最新写入可能尚未 checkpoint 进主库
  let base_str = db_path.to_string_lossy().into_owned();
  let tmp_str = tmp_base.to_string_lossy().into_owned();
  for suffix in ["", "-wal", "-shm"] {
    // 先绑定 String 再借用：Path::new 不能直接借用临时 format! 值
    let src_str = format!("{base_str}{suffix}");
    let dst_str = format!("{tmp_str}{suffix}");
    let src = Path::new(&src_str);
    if src.exists() {
      let dst = Path::new(&dst_str);
      if std::fs::copy(src, dst).is_err() {
        return Vec::new();
      }
    }
  }

  let uris = (|| -> Option<Vec<String>> {
    let conn = rusqlite::Connection::open(&tmp_base).ok()?;
    let value: String = conn
      .query_row(
        "SELECT value FROM ItemTable WHERE key = 'history.recentlyOpenedPathsList'",
        [],
        |row| row.get(0),
      )
      .ok()?;
    let json: serde_json::Value = serde_json::from_str(&value).ok()?;
    let entries = json.get("entries")?.as_array()?;
    Some(
      entries
        .iter()
        .filter_map(|e| e.get("folderUri").and_then(|v| v.as_str()).map(String::from))
        .collect(),
    )
  })()
  .unwrap_or_default();

  // 清理临时副本（-wal/-shm 一并删）
  for suffix in ["", "-wal", "-shm"] {
    let path_str = format!("{tmp_str}{suffix}");
    let _ = std::fs::remove_file(Path::new(&path_str));
  }
  uris
}

/// 从旧版 storage.json 读最近记录（结构与 vscdb 里存的 JSON 相同）。
fn read_uris_from_storage_json(path: &Path) -> Vec<String> {
  // 文件不存在 / 不可读 / JSON 损坏都静默返回空：旧文件只是兜底来源
  let Ok(raw) = std::fs::read_to_string(path) else {
    return Vec::new();
  };
  let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
    return Vec::new();
  };

  let mut uris: Vec<String> = Vec::new();
  let entries = json
    .pointer("/history/recentlyOpenedPathsList/entries")
    .and_then(|v| v.as_array())
    .cloned()
    .unwrap_or_default();
  for entry in &entries {
    if let Some(uri) = entry.get("folderUri").and_then(|v| v.as_str()) {
      uris.push(uri.to_string());
    }
  }
  // 兜底：极旧版本可能只有 windowsState 里记录了最近活动窗口的目录
  if uris.is_empty() {
    if let Some(uri) = json
      .pointer("/windowsState/lastActiveWindow/folder")
      .and_then(|v| v.as_str())
    {
      uris.push(uri.to_string());
    }
  }
  uris
}

/// 扫描 JetBrains 系（含 Android Studio，二者数据格式一致）的 recentProjects.xml。
/// 扫出的项目按产品目录名归类到对应工具（IntelliJIdea2026.3 → idea、
/// Rider2026.1 → rider 等）；识别不了的产品回落到 idea。
fn scan_jetbrains(base: &Path, existing: &[(String, String)], fallback_tool_id: &str) -> Vec<DetectedProject> {
  // $USER_HOME$ 宏展开需要主目录；统一走配置模块的跨平台辅助函数
  // （HOME → USERPROFILE），Windows 上不再因缺少 HOME 而直接放弃扫描。
  let Some(home) = crate::config::home_dir_from_env() else {
    return Vec::new();
  };
  let home = home.to_string_lossy().into_owned();
  let Ok(entries) = std::fs::read_dir(base) else {
    return Vec::new();
  };

  // 目录名形如 IntelliJIdea2026.3 / PyCharm2023.3，排序让输出顺序稳定；
  // 多版本并存（如 2026.2 与 2026.3）时都扫，重复由上层去重
  let mut products: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
  products.sort();

  let mut out = Vec::new();
  for product in products {
    let xml_path = product.join("options/recentProjects.xml");
    let Ok(xml) = std::fs::read_to_string(&xml_path) else {
      continue;
    };
    // 产品目录名 = 产品名 + 版本号，取前缀小写后按映射归类
    let dir_name = product
      .file_name()
      .and_then(|n| n.to_str())
      .unwrap_or_default()
      .to_lowercase();
    let tool_id = jetbrains_tool_id(&dir_name).unwrap_or(fallback_tool_id);
    for path in extract_recent_paths(&xml, &home) {
      if out.len() >= MAX_PER_SOURCE {
        return out;
      }
      push_if_valid(&mut out, path, tool_id, existing);
    }
  }
  out
}

/// JetBrains 产品配置目录名前缀 → zoro 工具 id。
/// 目录名形如 `Rider2026.1`，用小写前缀匹配而非精确名，避免版本号干扰。
fn jetbrains_tool_id(dir_name_lower: &str) -> Option<&'static str> {
  const PREFIXES: &[(&str, &str)] = &[
    ("intellijidea", "idea"),
    ("androidstudio", "android-studio"),
    ("rider", "rider"),
    ("pycharm", "pycharm"),
    ("webstorm", "webstorm"),
    ("goland", "goland"),
    ("clion", "clion"),
    ("phpstorm", "phpstorm"),
    ("datagrip", "datagrip"),
    ("rubymine", "rubymine"),
  ];
  PREFIXES
    .iter()
    .find(|(prefix, _)| dir_name_lower.starts_with(prefix))
    .map(|(_, id)| *id)
}

/// 扫描 Zed 的最近项目。
///
/// Zed（0.19x 起从 Sled 迁移）把全部状态持久化在
/// `~/Library/Application Support/Zed/db/<db>/db.sqlite`（及 `-wal`）的
/// `kv_store` 表里，value 是序列化后的 blob，项目路径以明文直接出现在
/// 字节流中。这里刻意不做完整的 SQLite 解析（避免为此引入 rusqlite
/// 依赖，且 key 布局随版本变动），而是按字节流扫描：
/// 1. 切出可打印 ASCII 连续段；
/// 2. 在段内定位 home 前缀的出现位置；
/// 3. 从该处向短处裁剪，第一个「真实存在的目录」且不是 home 本身的
///    前缀即视为项目路径（blob 里路径后紧跟其他字段，串尾不可信）。
///
/// 只接受真实存在的目录，因此 Zed 里已删除项目的旧记录会被自动过滤；
/// Zed 刚安装还没有打开过项目时，扫不出内容属正常现象。
fn scan_zed(base: &Path, home: &Path, existing: &[(String, String)]) -> Vec<DetectedProject> {
  let mut out = Vec::new();
  let home_str = home.to_string_lossy().into_owned();

  // 遍历 db/ 下的每个库目录（0-global、0-stable，未来可能新增），两个文件
  // 都要扫：-wal 是预写日志，新写入的记录在 checkpoint 前只存在于这里
  let Ok(dbs) = std::fs::read_dir(base.join("db")) else {
    return out;
  };
  let mut files: Vec<PathBuf> = dbs
    .filter_map(|e| e.ok())
    .flat_map(|e| {
      let p = e.path();
      [p.join("db.sqlite"), p.join("db.sqlite-wal")]
    })
    .filter(|f| f.exists())
    .collect();
  // 排序保证跨文件的产出顺序稳定（同样的路径重复出现在多个文件里）
  files.sort();

  for file in files {
    let Ok(bytes) = std::fs::read(&file) else {
      continue;
    };
    for path in extract_home_dir_paths(&bytes, &home_str) {
      if out.len() >= MAX_PER_SOURCE {
        return out;
      }
      push_if_valid(&mut out, path, "zed", existing);
    }
  }
  out
}

/// 从字节流里提取「home 前缀开头 + 真实存在的目录」路径，按出现顺序返回（去重）。
fn extract_home_dir_paths(bytes: &[u8], home_str: &str) -> Vec<String> {
  let mut out = Vec::new();
  // 1. 切可打印 ASCII 段（0x20~0x7E）：blob 的二进制部分天然形成边界
  let mut run_start = None;
  for (i, b) in bytes.iter().chain(std::iter::once(&0u8)).enumerate() {
    let printable = (0x20..=0x7e).contains(b);
    if printable && run_start.is_none() {
      run_start = Some(i);
    } else if !printable {
      if let Some(start) = run_start.take() {
        extract_from_run(&bytes[start..i], home_str, &mut out);
      }
    }
  }
  out
}

/// 在一段可打印文本里查找 home 前缀并裁出存在的目录路径。
///
/// 直接对整段做 `find(home_str)` 循环：find 本身是 O(n)，while 循环会遍历段内
/// 所有 home 出现位置，无需按长度切分。此前按 `\n\r\t` 递归切分是无效的——
/// 入参只含 0x20..=0x7E 可打印字符，永远切不出更短的段，导致无限递归直至
/// 栈溢出（进程 abort）；改用定长 `chunks` 又可能在边界处切断 `<home>/<dir>`
/// 前缀而漏提取。因此这里彻底去掉长度分支，从根上消除递归。
fn extract_from_run(run: &[u8], home_str: &str, out: &mut Vec<String>) {
  let run_str = String::from_utf8_lossy(run);
  let home = Path::new(home_str);
  let mut search_from = 0usize;
  while let Some(off) = run_str[search_from..].find(home_str) {
    let start = search_from + off;
    let tail = &run_str[start..];
    if let Some(path) = longest_existing_dir_prefix(tail, home) {
      if !out.contains(&path) {
        out.push(path);
      }
    }
    // 同一段里可能有多条路径，从下一个字符继续找
    search_from = start + home_str.len();
  }
}

/// 从字符串开头向短处裁剪，返回最长的「真实存在的目录」前缀。
///
/// 规则：
/// - home 本身不算项目（blob 里任何含 home 的串裁到底都会命中 home）；
/// - home 之后至少要有一级目录，避免把 home 的父目录误当项目；
/// - 优先按 '/' 边界裁剪，找不到再逐字符裁（应对路径后紧跟非路径文本）。
fn longest_existing_dir_prefix(tail: &str, home: &Path) -> Option<String> {
  let bytes = tail.as_bytes();
  let min_len = home.to_string_lossy().len();
  // 生成候选前缀长度（从长到短）：全长（路径恰好占满整段）、'/' 边界
  // （路径后紧跟其他路径/字段）、以及逐字符（路径后紧跟非路径文本）
  let mut candidates: Vec<usize> = vec![bytes.len()];
  candidates.extend((0..bytes.len()).filter(|&i| bytes[i] == b'/'));
  candidates.extend((0..bytes.len()).rev());
  candidates.sort_unstable_by(|a, b| b.cmp(a));
  candidates.dedup();
  for i in candidates {
    if i <= min_len || !tail.is_char_boundary(i) {
      continue;
    }
    let candidate = &tail[..i];
    let p = Path::new(candidate);
    if p.is_dir() && p != home && p.starts_with(home) {
      return Some(candidate.to_string());
    }
  }
  None
}

/// 统一的合法性检查后入列：目录必须存在，且（路径 + 工具）未被配置过。
fn push_if_valid(
  out: &mut Vec<DetectedProject>,
  path: String,
  tool_id: &str,
  existing: &[(String, String)],
) {
  let p = Path::new(&path);
  // 目录已删除的旧记录直接丢弃——导入了也打不开
  if !p.is_dir() {
    return;
  }
  if existing
    .iter()
    .any(|(ep, et)| ep == &path && et == tool_id)
  {
    return;
  }
  let name = p
    .file_name()
    .map(|n| n.to_string_lossy().into_owned())
    .unwrap_or_else(|| path.clone());
  out.push(DetectedProject {
    name,
    path,
    tool_id: tool_id.to_string(),
  });
}

/// 把 `file:///Users/foo/My%20Dir` 这类 URI 转成本地路径。
/// 只接受 file:// 且主机省略（三斜杠）的形式；远程 / 带主机名的返回 None。
pub fn file_uri_to_path(uri: &str) -> Option<String> {
  let rest = uri.strip_prefix("file://")?;
  // file:///path 的 rest 以 '/' 开头；file://host/path 会被拒绝
  if !rest.starts_with('/') {
    return None;
  }
  let decoded = percent_decode(rest);
  if decoded.starts_with('/') {
    Some(decoded)
  } else {
    None
  }
}

/// 最小化百分号解码：只处理 %XX 序列，非法序列原样保留。
fn percent_decode(input: &str) -> String {
  let bytes = input.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut i = 0;
  while i < bytes.len() {
    if bytes[i] == b'%' && i + 2 < bytes.len() {
      if let Ok(v) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
        out.push(v);
        i += 3;
        continue;
      }
    }
    out.push(bytes[i]);
    i += 1;
  }
  String::from_utf8_lossy(&out).into_owned()
}

/// 从 recentProjects.xml 提取最近项目路径，并把 JetBrains 的
/// `$USER_HOME$` 宏展开成真实主目录。兼容两种存储布局：
/// - 新版：`additionalInfo` map 的 `<entry key="路径">`
/// - 旧版：`recentPaths` 列表的 `<option value="路径">`
///
/// 故意不引入 XML 解析库：目标结构非常固定，字符串定位即可，还便于测试。
fn extract_recent_paths(xml: &str, home: &str) -> Vec<String> {
  let mut raws: Vec<String> = Vec::new();

  // 新版布局：additionalInfo 的 entry key。只扫 key=，不能盲扫 value=——
  // 同一文件里还有时间戳、$APPLICATION_HOME_DIR$/bin 等无关属性值
  raws.extend(collect_attr_values(xml, "entry", "key"));

  // 旧版布局：recentPaths 列表里的 option value（限定在列表块内）
  if let Some(start) = xml.find("name=\"recentPaths\"") {
    if let Some(list_start_rel) = xml[start..].find("<list>") {
      let list_abs = start + list_start_rel;
      if let Some(list_end_rel) = xml[list_abs..].find("</list>") {
        raws.extend(collect_attr_values(
          &xml[list_abs..list_abs + list_end_rel],
          "option",
          "value",
        ));
      }
    }
  }

  let mut out = Vec::new();
  for raw in raws {
    let path = if let Some(p) = file_uri_to_path(&raw) {
      // 少量版本会存成 file:// URI
      p
    } else {
      // 常规格式：$USER_HOME$/Projects/foo 或 /absolute/path
      raw.replacen("$USER_HOME$", home, 1)
    };
    if path.starts_with('/') && !out.contains(&path) {
      out.push(xml_unescape(&path));
    }
  }
  out
}

/// 收集形如 `<tag attr="...">` 的属性值（字符串级扫描，容忍残缺 XML）。
fn collect_attr_values(xml: &str, tag: &str, attr: &str) -> Vec<String> {
  let needle = format!("<{tag} {attr}=\"");
  let mut out = Vec::new();
  let mut cursor = 0;
  while let Some(rel) = xml[cursor..].find(&needle) {
    let v_abs = cursor + rel + needle.len();
    let Some(end) = xml[v_abs..].find('"') else {
      break;
    };
    out.push(xml[v_abs..v_abs + end].to_string());
    cursor = v_abs + end + 1;
  }
  out
}

/// 展开最常用的 XML 实体。路径里出现这些字符的场景极少，全覆盖不划算。
fn xml_unescape(s: &str) -> String {
  s.replace("&amp;", "&")
    .replace("&lt;", "<")
    .replace("&gt;", ">")
    .replace("&quot;", "\"")
    .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
  use super::*;

  /// JetBrains 产品目录名 → 工具 id：带版本号、不带版本号、未知产品回落
  #[test]
  fn maps_jetbrains_product_dirs_to_tools() {
    assert_eq!(jetbrains_tool_id("intellijidea2026.3"), Some("idea"));
    assert_eq!(jetbrains_tool_id("rider2026.1"), Some("rider"));
    assert_eq!(jetbrains_tool_id("pycharm2023.3"), Some("pycharm"));
    assert_eq!(jetbrains_tool_id("androidstudio2024.1"), Some("android-studio"));
    assert_eq!(jetbrains_tool_id("phpstorm"), Some("phpstorm"));
    assert_eq!(jetbrains_tool_id("datagrip2024.2"), Some("datagrip"));
    assert_eq!(jetbrains_tool_id("rubymine2024.1"), Some("rubymine"));
    assert_eq!(jetbrains_tool_id("unknownproduct2026.1"), None);
  }

  /// file:// URI 解码：普通路径、百分号编码路径
  #[test]
  fn decodes_file_uris() {
    assert_eq!(
      file_uri_to_path("file:///Users/alice/Projects/demo"),
      Some("/Users/alice/Projects/demo".into())
    );
    assert_eq!(
      file_uri_to_path("file:///Users/alice/My%20Folder"),
      Some("/Users/alice/My Folder".into())
    );
  }

  /// 非 file:// 协议（vscode-remote 等）与带主机名的 URI 必须被拒绝
  #[test]
  fn rejects_non_local_uris() {
    assert_eq!(file_uri_to_path("vscode-remote://ssh-remote+host/home/x"), None);
    assert_eq!(file_uri_to_path("file://server/share/x"), None);
    assert_eq!(file_uri_to_path("/plain/path"), None);
  }

  /// 旧版 recentPaths 列表提取 + $USER_HOME$ 宏展开 + URI 形式条目
  #[test]
  fn extracts_jetbrains_legacy_recent_paths() {
    let xml = r#"
      <application>
        <component name="RecentProjectsManager">
          <option name="recentPaths">
            <list>
              <option value="$USER_HOME$/Projects/alpha" />
              <option value="/Users/alice/Works/beta" />
              <option value="file:///Users/alice/Works/gamma" />
              <option value="$USER_HOME$/Projects/alpha" />
              <option value="relative/not/absolute" />
            </list>
          </option>
          <option name="somethingElse"><list><option value="nope" /></list></option>
        </component>
      </application>
    "#;
    let paths = extract_recent_paths(xml, "/Users/alice");
    assert_eq!(
      paths,
      vec![
        "/Users/alice/Projects/alpha",
        "/Users/alice/Works/beta",
        "/Users/alice/Works/gamma"
      ]
    );
  }

  /// 新版 additionalInfo 布局：entry key 承载项目路径；
  /// entry 内部的无关 value（时间戳、$APPLICATION_HOME_DIR$ 等）不能混进来
  #[test]
  fn extracts_jetbrains_additional_info_keys() {
    let xml = r#"
      <application>
        <component name="RecentProjectsManager">
          <option name="additionalInfo">
            <map>
              <entry key="$USER_HOME$/Github/Stirling-PDF">
                <value>
                  <RecentProjectMetaInfo frameTitle="Stirling-PDF">
                    <option name="binFolder" value="$APPLICATION_HOME_DIR$/bin" />
                    <option name="activationTimestamp" value="1719295435097" />
                  </RecentProjectMetaInfo>
                </value>
              </entry>
              <entry key="/Users/alice/Works/jeepay">
                <value>
                  <RecentProjectMetaInfo frameTitle="jeepay" />
                </value>
              </entry>
            </map>
          </option>
        </component>
      </application>
    "#;
    let paths = extract_recent_paths(xml, "/Users/alice");
    assert_eq!(
      paths,
      vec!["/Users/alice/Github/Stirling-PDF", "/Users/alice/Works/jeepay"]
    );
  }

  /// workspaceStorage 兜底：新版 VS Code 不再写 recentlyOpenedPathsList 时的唯一来源。
  /// 覆盖「单目录 folder」「.code-workspace 文件取父目录」「mtime 倒序」三种行为。
  #[test]
  fn reads_workspace_storage_entries() {
    let tmp = std::env::temp_dir().join(format!("zoro-ws-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let ws = tmp.join("User/workspaceStorage");

    // 两个真实目录 + 一个 .code-workspace 文件
    let older = tmp.join("code/older");
    let newer = tmp.join("code/newer");
    let ws_file_dir = tmp.join("code/multi-root");
    for d in [&older, &newer, &ws_file_dir] {
      std::fs::create_dir_all(d).unwrap();
    }
    let ws_file = ws_file_dir.join("multi.code-workspace");
    std::fs::write(&ws_file, "{}").unwrap();

    // 三条记录，用 File::set_modified 显式拉开 mtime，避免依赖写盘时序
    let entries = [
      ("aaa", format!(r#"{{"folder":"file://{}"}}"#, older.display()), 1_000u64),
      ("bbb", format!(r#"{{"folder":"file://{}"}}"#, newer.display()), 2_000u64),
      (
        "ccc",
        format!(r#"{{"workspace":"file://{}"}}"#, ws_file.display()),
        3_000u64,
      ),
    ];
    for (hash, body, secs) in entries {
      let d = ws.join(hash);
      std::fs::create_dir_all(&d).unwrap();
      let p = d.join("workspace.json");
      std::fs::write(&p, body).unwrap();
      let handle = std::fs::OpenOptions::new().write(true).open(&p).unwrap();
      let mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
      handle.set_modified(mtime).unwrap();
    }

    let uris = read_uris_from_workspace_storage(&tmp);
    assert_eq!(uris.len(), 3, "三条都应读出：{uris:?}");
    // 最后写入的排最前（mtime 倒序）
    assert!(uris[0].ends_with("multi.code-workspace"), "{uris:?}");

    // 端到端：.code-workspace 应转成其父目录，且被 push_if_valid 接受
    let found = scan_vscode_like(&tmp, "vscode", &[]);
    let paths: Vec<_> = found.iter().map(|p| p.path.clone()).collect();
    assert!(
      paths.contains(&ws_file_dir.to_string_lossy().into_owned()),
      ".code-workspace 应取父目录：{paths:?}"
    );

    std::fs::remove_dir_all(&tmp).unwrap();
  }

  /// 端到端：伪造一个迷你 HOME 目录，覆盖三种来源
  /// （state.vscdb / storage.json / recentProjects.xml）+ 去重 + 过滤。
  #[test]
  fn scans_fake_home_end_to_end() {
    let tmp = std::env::temp_dir().join(format!("zoro-importer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let app_support = tmp.join("Library/Application Support");

    // 三个真实存在的目录（zoro 已配置过，应被过滤）
    let proj_zoro = tmp.join("code/zoro");
    let proj_new = tmp.join("code/brand-new");
    let proj_idea = tmp.join("code/idea-only");
    std::fs::create_dir_all(&proj_zoro).unwrap();
    std::fs::create_dir_all(&proj_new).unwrap();
    std::fs::create_dir_all(&proj_idea).unwrap();

    // VS Code state.vscdb（新版主存储）：一条已配置、一条新
    let code_dir = app_support.join("Code/User/globalStorage");
    std::fs::create_dir_all(&code_dir).unwrap();
    let conn = rusqlite::Connection::open(code_dir.join("state.vscdb")).unwrap();
    conn
      .execute_batch("CREATE TABLE ItemTable (key TEXT UNIQUE, value BLOB);")
      .unwrap();
    conn
      .execute(
        "INSERT INTO ItemTable (key, value) VALUES ('history.recentlyOpenedPathsList', ?1)",
        [format!(
          r#"{{"entries":[{{"folderUri":"file://{}"}},{{"folderUri":"file://{}"}}]}}"#,
          proj_zoro.display(),
          proj_new.display()
        )],
      )
      .unwrap();
    drop(conn);

    // JetBrains 新版 additionalInfo：与 VS Code 一条重复（应去重）+ IDEA 独有
    let jb_dir = app_support.join("JetBrains/IntelliJIdea2026.3/options");
    std::fs::create_dir_all(&jb_dir).unwrap();
    std::fs::write(
      jb_dir.join("recentProjects.xml"),
      format!(
        r#"<option name="additionalInfo"><map>
          <entry key="{}"><value><RecentProjectMetaInfo /></value></entry>
          <entry key="{}"><value><RecentProjectMetaInfo /></value></entry>
        </map></option>"#,
        proj_new.display(),
        proj_idea.display()
      ),
    )
    .unwrap();

    let existing = vec![(proj_zoro.to_string_lossy().into_owned(), "vscode".into())];
    let found = scan(&tmp, &existing);

    assert_eq!(found.len(), 2, "去重与过滤后应剩两条：{found:?}");
    // vscode 来源先扫描，重复路径保留其来源
    assert_eq!(found[0].path, proj_new.to_string_lossy());
    assert_eq!(found[0].tool_id, "vscode");
    assert_eq!(found[1].path, proj_idea.to_string_lossy());
    assert_eq!(found[1].tool_id, "idea");

    std::fs::remove_dir_all(&tmp).unwrap();
  }

  /// 真实环境冒烟：扫描当前用户主目录并打印结果。
  /// 平时忽略（结果取决于机器状态），需要人工验证时执行：
  /// cargo test scans_real_home -- --ignored --nocapture
  #[test]
  #[ignore]
  fn scans_real_home() {
    let home = std::env::var("HOME").unwrap();
    let found = scan(std::path::Path::new(&home), &[]);
    println!("共扫描到 {} 条候选", found.len());
    for p in &found {
      println!("  [{}] {} -> {}", p.tool_id, p.name, p.path);
    }
  }

  /// Zed：kv_store 的 value 是序列化 blob，路径以明文嵌在二进制里。
  /// 伪造 db.sqlite 与 -wal 验证：字节流提取、串尾裁剪、跨文件去重、
  /// 以及「home 本身不算项目」的守卫。
  #[test]
  fn scans_zed_sqlite_blobs() {
    let tmp = std::env::temp_dir().join(format!("zoro-zed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let db_dir = tmp.join("Library/Application Support/Zed/db/0-global");
    std::fs::create_dir_all(&db_dir).unwrap();

    let proj_db = tmp.join("zed-db-proj");
    let proj_wal = tmp.join("zed-wal-proj");
    std::fs::create_dir_all(&proj_db).unwrap();
    std::fs::create_dir_all(&proj_wal).unwrap();

    // db.sqlite：路径后直接贴非路径文本（模拟 blob 相邻字段），且混入 home 本身
    let blob_db = format!(
      "\x02{}gibberish-tail\x03{}\x04",
      proj_db.display(),
      tmp.display()
    );
    std::fs::write(db_dir.join("db.sqlite"), blob_db).unwrap();
    // -wal：另一条路径（应在 checkpoint 前只能从这里扫到）
    let blob_wal = format!("\x01{}\x02", proj_wal.display());
    std::fs::write(db_dir.join("db.sqlite-wal"), blob_wal).unwrap();

    let found = scan(&tmp, &[]);

    assert_eq!(found.len(), 2, "两个文件各提取一条：{found:?}");
    assert!(found.iter().any(|p| p.path == proj_db.to_string_lossy()));
    assert!(found.iter().any(|p| p.path == proj_wal.to_string_lossy()));
    assert!(found.iter().all(|p| p.tool_id == "zed"));
    // home 本身（blob 里出现过）不能被当成项目
    assert!(!found.iter().any(|p| p.path == tmp.to_string_lossy()));

    std::fs::remove_dir_all(&tmp).unwrap();
  }

  /// 回归 🔴-1：旧实现在 >8192 字节的纯可打印段上用段内不存在的 `\n\r\t`
  /// 递归切分，长度不收敛导致栈溢出、整个进程 abort。这里构造同类输入，
  /// 验证新实现能正常返回且仍能提取出路径，不再崩溃。
  #[test]
  fn extracts_paths_from_oversized_printable_run() {
    let tmp = std::env::temp_dir().join(format!("zoro-long-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let proj = tmp.join("long-proj");
    std::fs::create_dir_all(&proj).unwrap();
    let home_str = tmp.to_string_lossy().into_owned();

    // 用可打印 ASCII 填充出远超旧 8192 阈值的一段，中间嵌入真实存在的目录。
    // needle 用系统路径分隔符，保证跨平台（Windows 的 \ 也是可打印 ASCII）。
    let needle = proj.to_string_lossy().into_owned();
    let mut blob = String::new();
    blob.push_str(&"x".repeat(5000));
    blob.push_str(&needle);
    blob.push_str(&"y".repeat(5000));
    assert!(blob.len() > 8192, "测试输入需超过旧的 8192 阈值");
    assert!(
      blob.bytes().all(|b| (0x20..=0x7e).contains(&b)),
      "输入须全为可打印 ASCII，才能复现旧实现的切分失效"
    );

    // 旧实现在此处无限递归并栈溢出；新实现应正常返回并提取出路径
    let found = extract_home_dir_paths(blob.as_bytes(), &home_str);
    assert!(
      found.iter().any(|p| p == &needle),
      "超长可打印段中应仍能提取出已存在的目录：{found:?}"
    );

    std::fs::remove_dir_all(&tmp).unwrap();
  }
}
