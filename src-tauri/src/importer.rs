//! 从本机开发工具的历史记录里扫描「最近打开的项目目录」。
//!
//! 支持的数据源（macOS）：
//! - VS Code / Cursor（同构存储）：
//!   `<app>/User/globalStorage/storage.json` 的 `history.recentlyOpenedPathsList.entries[].folderUri`
//! - JetBrains 系列产品（IDEA / PyCharm / GoLand 等）：
//!   `~/Library/Application Support/JetBrains/<产品版本>/options/recentProjects.xml` 的 `recentPaths` 列表
//!
//! 设计约定：
//! - 扫描是**只读**的，不会写入任何工具的数据文件；
//! - 结果按路径全局去重（同一路径只保留最先命中的来源）；
//! - 过滤掉不存在的目录与「路径 + 工具」均已配置过的条目，
//!   保证设置页看到的导入候选全是新增项。

use std::collections::HashSet;
use std::path::Path;

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

/// 每个来源最多返回的条数：工具历史可能有几十上百条，截断保持弹窗可用。
const MAX_PER_SOURCE: usize = 30;

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
  for p in scan_jetbrains(&support.join("JetBrains"), existing) {
    if seen.insert(p.path.clone()) {
      found.push(p);
    }
  }
  found
}

/// 扫描 VS Code / Cursor 的 storage.json。
fn scan_vscode_like(
  base: &Path,
  tool_id: &str,
  existing: &[(String, String)],
) -> Vec<DetectedProject> {
  let storage = base.join("User/globalStorage/storage.json");
  // 文件不存在 / 不可读 / JSON 损坏都静默返回空：某个工具没装是常态，不该报错
  let Ok(raw) = std::fs::read_to_string(&storage) else {
    return Vec::new();
  };
  let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
    return Vec::new();
  };

  // 收集 folderUri 列表（workspace 文件不带 folderUri，天然被跳过）
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

  let mut out = Vec::new();
  for uri in uris {
    if out.len() >= MAX_PER_SOURCE {
      break;
    }
    // vscode-remote:// 等远程 URI 在这里返回 None，被自然过滤
    let Some(path) = file_uri_to_path(&uri) else {
      continue;
    };
    push_if_valid(&mut out, path, tool_id, existing);
  }
  out
}

/// 扫描 JetBrains 系列产品的 recentProjects.xml。
fn scan_jetbrains(base: &Path, existing: &[(String, String)]) -> Vec<DetectedProject> {
  let Ok(home) = std::env::var("HOME") else {
    return Vec::new();
  };
  let Ok(entries) = std::fs::read_dir(base) else {
    return Vec::new();
  };

  // 目录名形如 IntelliJIdea2023.2 / PyCharm2023.3，排序让输出顺序稳定
  let mut products: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
  products.sort();

  let mut out = Vec::new();
  for product in products {
    let xml_path = product.join("options/recentProjects.xml");
    let Ok(xml) = std::fs::read_to_string(&xml_path) else {
      continue;
    };
    for path in extract_recent_paths(&xml, &home) {
      if out.len() >= MAX_PER_SOURCE {
        return out;
      }
      push_if_valid(&mut out, path, "idea", existing);
    }
  }
  out
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

/// 从 recentProjects.xml 的 recentPaths 列表提取路径，并把
/// JetBrains 的 `$USER_HOME$` 宏展开成真实主目录。
///
/// 故意不引入 XML 解析库：目标结构非常固定，字符串定位即可，还便于测试。
fn extract_recent_paths(xml: &str, home: &str) -> Vec<String> {
  let Some(start) = xml.find("name=\"recentPaths\"") else {
    return Vec::new();
  };
  let Some(list_start_rel) = xml[start..].find("<list>") else {
    return Vec::new();
  };
  let list_abs = start + list_start_rel;
  let Some(list_end_rel) = xml[list_abs..].find("</list>") else {
    return Vec::new();
  };
  let list = &xml[list_abs..list_abs + list_end_rel];

  let mut out = Vec::new();
  let mut cursor = 0;
  while let Some(rel) = list[cursor..].find("value=\"") {
    let v_abs = cursor + rel + "value=\"".len();
    let Some(end_rel) = list[v_abs..].find('"') else {
      break;
    };
    let raw = &list[v_abs..v_abs + end_rel];
    cursor = v_abs + end_rel + 1;

    let path = if let Some(p) = file_uri_to_path(raw) {
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

  /// recentPaths 提取 + $USER_HOME$ 宏展开 + URI 形式条目
  #[test]
  fn extracts_jetbrains_recent_paths() {
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

  /// 端到端：伪造一个迷你 HOME 目录，验证扫描结果
  /// 含去重、已存在目录校验与已配置项过滤。
  #[test]
  fn scans_fake_home_end_to_end() {
    let tmp = std::env::temp_dir().join(format!("zoro-importer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let app_support = tmp.join("Library/Application Support");

    // 两个真实存在的目录（其中 zoro 已配置过，应被过滤）
    let proj_zoro = tmp.join("code/zoro");
    let proj_new = tmp.join("code/brand-new");
    std::fs::create_dir_all(&proj_zoro).unwrap();
    std::fs::create_dir_all(&proj_new).unwrap();

    // VS Code storage.json：一条已配置、一条新
    let code_dir = app_support.join("Code/User/globalStorage");
    std::fs::create_dir_all(&code_dir).unwrap();
    std::fs::write(
      code_dir.join("storage.json"),
      format!(
        r#"{{"history":{{"recentlyOpenedPathsList":{{"entries":[
          {{"folderUri":"file://{}"}},
          {{"folderUri":"file://{}"}}
        ]}}}}}}"#,
        proj_zoro.display(),
        proj_new.display()
      ),
    )
    .unwrap();

    // JetBrains：与 VS Code 记录同一路径（应被全局去重，只剩 vscode 来源）
    let jb_dir = app_support.join("JetBrains/IntelliJIdea2024.1/options");
    std::fs::create_dir_all(&jb_dir).unwrap();
    std::fs::write(
      jb_dir.join("recentProjects.xml"),
      format!(
        r#"<option name="recentPaths"><list><option value="{}" /></list></option>"#,
        proj_new.display()
      ),
    )
    .unwrap();

    let existing = vec![(proj_zoro.to_string_lossy().into_owned(), "vscode".into())];
    let found = scan(&tmp, &existing);

    assert_eq!(found.len(), 1, "去重与过滤后应只剩一条：{found:?}");
    assert_eq!(found[0].path, proj_new.to_string_lossy());
    assert_eq!(found[0].tool_id, "vscode");
    assert_eq!(found[0].name, "brand-new");

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
}
