//! 把项目交给对应开发工具打开。
//!
//! 策略分三级，逐级降级，任何一级成功即视为打开成功：
//! 1. 直接 spawn CLI（PATH 里能找到时最快、无额外开销）；
//! 2. 通过用户的登录 shell 再试一次——GUI 启动的 App 进程 PATH 通常只有
//!    /usr/bin:/bin:/usr/sbin:/sbin，而 `code`、`cursor` 这类 CLI 装在
//!    /usr/local/bin 或用户目录里，必须借用登录 shell 才能解析到；
//! 3. macOS 上最后用 `open -a <应用名> <路径>` 兜底（不依赖 PATH）。

use crate::config::Tool;
use std::path::Path;
use std::process::Command;

/// 判断是否为「把终端应用当工具」的配置（如出厂默认的 iTerm 工具）。
///
/// 这类工具的正确语义是「在该终端里 cd 到项目目录」，不能走通用的
/// spawn 链路：`open -a iTerm <目录>` 里 iTerm 会忽略目录参数，
/// 新窗口只会落在 $HOME。必须交给 open_in_terminal 的 AppleScript
/// 专用链路处理。
fn is_terminal_tool(tool: &Tool) -> bool {
  /// 视为终端应用的 -a 目标名（contains 匹配，兼容 "iTerm2" 等写法）
  const TERMINAL_APPS: [&str; 2] = ["iTerm", "Terminal"];

  if tool.command == "open" {
    // 形如 open -a <App> 的配置：看 <App> 是不是终端应用
    if let Some(pos) = tool.args.iter().position(|a| a == "-a") {
      if let Some(app) = tool.args.get(pos + 1) {
        return TERMINAL_APPS.iter().any(|t| app.contains(t));
      }
    }
  }
  // 按 id 兜底：老配置升级后 command 可能被用户改过
  matches!(tool.id.as_str(), "iterm" | "terminal")
}

/// 用给定工具打开一个已存在的路径。
pub fn open_with_tool(tool: &Tool, path: &str) -> Result<(), String> {
  // 终端类工具语义特殊，直接复用终端打开链路（含 AppleScript + 降级）
  if is_terminal_tool(tool) {
    return open_in_terminal(path, "window");
  }

  let target = Path::new(path);
  if !target.exists() {
    return Err(format!("路径不存在或已被移动：{path}"));
  }

  // 第 1 级：直接执行。spawn 只负责拉起进程，工具自身会 fork 后退出。
  match Command::new(&tool.command).args(&tool.args).arg(target).spawn() {
    Ok(_) => return Ok(()),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
    Err(err) => return Err(format!("启动 {} 失败：{err}", tool.name)),
  }

  // 第 2 级：登录 shell 解析。先用 `command -v` 探测，探测到了再真正执行，
  // 这样不会把 shell 的启动开销浪费在根本不存在的命令上。
  #[cfg(unix)]
  {
    if resolve_via_login_shell(&tool.command) {
      match spawn_via_login_shell(tool, target) {
        Ok(()) => return Ok(()),
        Err(err) => log::warn!("登录 shell 方式启动 {} 失败：{err}", tool.name),
      }
    }
  }

  // 第 3 级：macOS 的 open -a，不依赖 PATH，只要应用装在 /Applications 即可。
  // 注意用 output() 等待并检查退出码：open 自身总是能启动，应用不存在时
  // 是以非零退出 + stderr 报错的，不能只看 spawn 是否成功。
  #[cfg(target_os = "macos")]
  if let Some(app_name) = &tool.app_name {
    match Command::new("open").arg("-a").arg(app_name).arg(target).output() {
      Ok(out) if out.status.success() => return Ok(()),
      Ok(out) => {
        let stderr = String::from_utf8_lossy(&out.stderr);
        log::warn!("open -a {app_name} 失败：{}", stderr.trim());
      }
      Err(err) => log::warn!("无法执行 open -a {app_name}：{err}"),
    }
  }

  Err(format!(
    "无法用 {} 打开该项目：未找到 {}，且应用名兜底也失败。请确认工具已安装，或将其 CLI 加入 PATH",
    tool.name, tool.command
  ))
}

/// 用系统终端打开一个目录（最近项目悬停的终端图标入口）。
///
/// 为什么不用 `open -a iTerm <目录>`：iTerm2 不像 Terminal.app 那样支持
/// 「文件夹作为文档」打开，路径参数会被忽略，新窗口只会落在 $HOME——
/// 这正是「打开了但目录不对」的根因。所以改用 AppleScript 明确 cd：
///
/// - iTerm：`create window / tab with default profile` + `write text "cd ..."`
/// - Terminal：`do script "cd ..."`（不带窗口参数即开新窗口）；
///   新选项卡需要借助 System Events 发 ⌘T（需要「辅助功能」权限）
///
/// mode："window" = 新窗口（默认）；"tab" = 新选项卡（旧版 "existing" 同义）。
/// 选项卡失败（应用没装 / 无现有窗口 / 缺辅助功能权限）时逐级降级成新窗口，
/// 目录正确永远优先于窗口形态偏好。
pub fn open_in_terminal(path: &str, mode: &str) -> Result<(), String> {
  let target = Path::new(path);
  if !target.exists() {
    return Err(format!("路径不存在或已被移动：{path}"));
  }
  // 只有目录才能作为终端工作目录，文件路径直接拒绝并说明原因
  if !target.is_dir() {
    return Err(format!("不是目录，无法在终端打开：{path}"));
  }

  #[cfg(target_os = "macos")]
  {
    let cd = format!("cd {}", shell_quote(path));
    let new_window = mode == "window";

    // 依次尝试（AppleScript 控制应用需要 macOS 的「自动化」授权，
    // 首次调用系统会弹权限确认，拒绝时脚本报错并自动降级）：
    // - tab 模式：iTerm 选项卡 → Terminal 选项卡 → 都失败退回新窗口形态
    // - window 模式：iTerm 新窗口 → Terminal 新窗口
    let mut attempts: Vec<(&str, String)> = Vec::new();
    if !new_window {
      attempts.push(("iTerm 选项卡", iterm_script(&cd, false)));
      attempts.push(("Terminal 选项卡", terminal_script(&cd, false, true)));
    }
    attempts.push(("iTerm 新窗口", iterm_script(&cd, true)));
    attempts.push(("Terminal 新窗口", terminal_script(&cd, true, false)));

    for (label, script) in &attempts {
      match osascript(script) {
        Ok(()) => return Ok(()),
        Err(err) => log::warn!("AppleScript 启动 {label} 失败：{}", enrich_osascript_error(&err)),
      }
    }
    // 自动化权限被拒（用户点了「不允许」/ 从未授权）时，用 open 兜底：
    // Terminal.app 支持「文件夹作为文档」，`open -a Terminal <目录>` 不依赖
    // Apple Events 授权也能在正确目录开新窗口。iTerm 无此能力，只能退到
    // Terminal——目录正确比应用偏好更重要。
    match Command::new("open")
      .args(["-a", "Terminal"])
      .arg(target)
      .output()
    {
      Ok(out) if out.status.success() => return Ok(()),
      Ok(out) => {
        log::warn!("open -a Terminal 兜底失败：{}", String::from_utf8_lossy(&out.stderr).trim())
      }
      Err(err) => log::warn!("无法执行 open -a Terminal：{err}"),
    }
    return Err(
      "未能通过 iTerm / Terminal 打开目录。请在 系统设置 → 隐私与安全性 → 自动化 中允许 Zoro 控制 iTerm / Terminal，或检查应用是否已安装".into(),
    );
  }

  #[cfg(target_os = "windows")]
  {
    // start 后第一个空参数是窗口标题占位；/D 让新 cmd 的工作目录落在项目目录
    match Command::new("cmd")
      .args(["/C", "start", "", "/D"])
      .arg(target)
      .arg("cmd")
      .spawn()
    {
      Ok(_) => Ok(()),
      Err(err) => Err(format!("打开终端失败：{err}")),
    }
  }

  #[cfg(not(any(target_os = "macos", target_os = "windows")))]
  {
    Err("当前平台暂不支持从终端打开目录".into())
  }
}

/// 执行一段 AppleScript，失败时返回 stderr 文本（便于日志定位）。
#[cfg(target_os = "macos")]
fn osascript(script: &str) -> Result<(), String> {
  match Command::new("osascript").arg("-e").arg(script).output() {
    Ok(out) if out.status.success() => Ok(()),
    Ok(out) => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
    Err(err) => Err(err.to_string()),
  }
}

/// 把 cd 命令转成 AppleScript 字符串字面量里的安全文本：
/// 反斜杠与双引号是 AppleScript 字符串仅有的两个转义字符；
/// 单引号留给内层 shell（cd '...' 由 shell_quote 保证安全）。
#[cfg(target_os = "macos")]
fn apple_escape(text: &str) -> String {
  text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// 把底层 AppleScript 错误翻译成可行动的提示：
/// - `-10004 privilege violation`：几乎总是进程被沙箱/受限环境包裹（比如
///   从带沙箱的终端里启动的 dev 进程），Apple Events 被系统拦截；
/// - `-1743`：TCC 自动化权限被拒绝。
/// 日志里带上这两种判别信息，避免排查时再走弯路。
#[cfg(target_os = "macos")]
fn enrich_osascript_error(err: &str) -> String {
  if err.contains("-10004") || err.to_lowercase().contains("privilege violation") {
    format!("{err}（进程似乎运行在沙箱/受限环境中，Apple Events 被系统拦截；请从用户自己的终端启动应用）")
  } else if err.contains("-1743") {
    format!("{err}（自动化权限被拒绝，请到 系统设置 → 隐私与安全性 → 自动化 中允许 Zoro）")
  } else {
    err.to_string()
  }
}

/// iTerm 的 AppleScript：new_window = true 开新窗口，false 在现有窗口开新标签页。
#[cfg(target_os = "macos")]
fn iterm_script(cd: &str, new_window: bool) -> String {
  let cmd = apple_escape(cd);
  if new_window {
    format!(
      "tell application \"iTerm\"\n  create window with default profile\n  tell current session of current window to write text \"{cmd}\"\nend tell"
    )
  } else {
    format!(
      "tell application \"iTerm\"\n  tell current window\n    set s to (create tab with default profile)\n    tell s to write text \"{cmd}\"\n  end tell\nend tell"
    )
  }
}

/// Terminal.app 的 AppleScript。
/// - new_window = true：`do script` 不带窗口参数即开新窗口；
/// - new_window = false（新选项卡）：借助 System Events 发 ⌘T 开标签页
///   （需要「辅助功能」权限），再把 cd 写进最前面的窗口（即新标签页）。
#[cfg(target_os = "macos")]
fn terminal_script(cd: &str, new_window: bool, tab_via_keystroke: bool) -> String {
  let cmd = apple_escape(cd);
  if new_window || !tab_via_keystroke {
    return format!("tell application \"Terminal\" to do script \"{cmd}\"");
  }
  format!(
    "tell application \"Terminal\"\n  activate\n  tell application \"System Events\" to keystroke \"t\" using command down\n  delay 0.3\n  do script \"{cmd}\" in window 1\nend tell"
  )
}

/// 在登录 shell 里探测命令是否存在（返回 bool，不抛错）。
#[cfg(unix)]
fn resolve_via_login_shell(command: &str) -> bool {
  let shell = login_shell();
  Command::new(&shell)
    .arg("-lc")
    .arg(format!("command -v {}", shell_quote(command)))
    .output()
    .map(|out| out.status.success())
    .unwrap_or(false)
}

/// 通过登录 shell 执行 `command args... path`。
#[cfg(unix)]
fn spawn_via_login_shell(tool: &Tool, path: &Path) -> Result<(), String> {
  // 逐段做单引号转义后再拼接，防止路径里的空格/引号被 shell 二次解释
  let mut line = shell_quote(&tool.command);
  for arg in &tool.args {
    line.push(' ');
    line.push_str(&shell_quote(arg));
  }
  line.push(' ');
  line.push_str(&shell_quote(&path.to_string_lossy()));

  Command::new(login_shell())
    .arg("-lc")
    .arg(format!("exec {line}"))
    .spawn()
    .map(|_| ())
    .map_err(|err| err.to_string())
}

/// 登录 shell：优先用用户自己的 shell（macOS 上多半是 zsh），
/// 这样 ~/.zshrc 里的 PATH（nvm、homebrew 等）才会生效；sh -lc 读不到 .zshrc。
#[cfg(unix)]
fn login_shell() -> String {
  std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
}

/// POSIX 单引号转义：把整个参数包进 '...'，内部单引号按 '\'' 展开。
#[cfg(unix)]
fn shell_quote(value: &str) -> String {
  format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(all(test, unix))]
mod tests {
  use super::*;
  use crate::config::Tool;

  fn tool(command: &str, app_name: Option<&str>) -> Tool {
    Tool {
      id: "t".into(),
      name: "测试工具".into(),
      command: command.into(),
      args: vec![],
      app_name: app_name.map(Into::into),
      icon: "i-lucide-box".into(),
    }
  }

  #[test]
  fn missing_path_is_rejected_without_spawning() {
    let t = tool("echo", None);
    let err = open_with_tool(&t, "/definitely/not/exist/nope").unwrap_err();
    assert!(err.contains("路径不存在"), "实际错误：{err}");
  }

  #[test]
  fn existing_command_opens_directory() {
    let t = tool("echo", None);
    let dir = std::env::temp_dir();
    assert!(
      open_with_tool(&t, dir.to_str().unwrap()).is_ok(),
      "PATH 内存在的命令应能直接 spawn"
    );
  }

  #[test]
  fn unknown_command_falls_back_to_open_a() {
    // 假 CLI + 假应用名：三级策略都失败时必须给出可读错误而不是静默成功
    let t = tool("definitely-not-a-real-cli-xyz", Some("Definitely Not A Real App"));
    let dir = std::env::temp_dir();
    let err = open_with_tool(&t, dir.to_str().unwrap()).unwrap_err();
    assert!(err.contains("无法用"), "实际错误：{err}");
  }

  #[test]
  fn shell_quote_escapes_single_quotes() {
    assert_eq!(shell_quote("plain"), "'plain'");
    assert_eq!(shell_quote("a b"), "'a b'");
    assert_eq!(shell_quote("it's"), r"'it'\''s'");
  }

  #[test]
  fn terminal_tools_are_routed_to_terminal_chain() {
    // 出厂默认的 iTerm 工具：open -a iTerm，必须走 open_in_terminal 专用链路，
    // 否则目录参数会被 iTerm 忽略、窗口落在 $HOME
    let iterm = Tool {
      id: "iterm".into(),
      name: "iTerm".into(),
      command: "open".into(),
      args: vec!["-a".into(), "iTerm".into()],
      app_name: None,
      icon: "i-lucide-terminal".into(),
    };
    assert!(is_terminal_tool(&iterm));

    // 普通 IDE 工具即便 command 恰好叫别的也不受影响
    let mut vscode = tool("code", None);
    vscode.id = "vscode".into();
    assert!(!is_terminal_tool(&vscode));

    // 用户改过 command 的 iterm 配置也能按 id 兜底识别
    let mut iterm2 = iterm.clone();
    iterm2.command = "/Applications/iTerm.app/Contents/MacOS/iTerm2".into();
    iterm2.args = vec![];
    assert!(is_terminal_tool(&iterm2));
  }
}
