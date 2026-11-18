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

/// 用给定工具打开一个已存在的路径。
pub fn open_with_tool(tool: &Tool, path: &str) -> Result<(), String> {
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
/// - Terminal：`do script "cd ..."`（不带 window 参数即开新窗口）
///
/// mode："window" = 新窗口（默认）；"existing" = 现有窗口（iTerm 开新标签页，
/// Terminal 写入最前面的窗口）。应用没装 / 无窗口时逐级降级兜底。
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
    let new_window = mode != "existing";

    // 依次尝试 iTerm → Terminal.app。AppleScript 控制应用需要 macOS 的
    // 「自动化」授权：首次调用系统会弹权限确认，拒绝时脚本报错并走降级。
    let attempts = [
      ("iTerm", iterm_script(&cd, new_window)),
      ("Terminal", terminal_script(&cd, new_window)),
    ];
    for (app, script) in &attempts {
      match osascript(script) {
        Ok(()) => return Ok(()),
        Err(err) => log::warn!("AppleScript 启动 {app} 失败：{err}"),
      }
    }
    // Terminal 的「现有窗口」在没有任何窗口时会报错，降级成新窗口再试一次
    if !new_window {
      if osascript(&terminal_script(&cd, true)).is_ok() {
        return Ok(());
      }
    }
    return Err("未能通过 iTerm / Terminal 打开目录（请检查是否已安装并授权自动化权限）".into());
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

/// iTerm 的 AppleScript：新窗口 / 在现有窗口里开新标签页。
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

/// Terminal.app 的 AppleScript：do script 不带 window 参数即开新窗口；
/// 「现有窗口」写入最前面的窗口（无窗口时由调用方降级重试）。
#[cfg(target_os = "macos")]
fn terminal_script(cd: &str, new_window: bool) -> String {
  let cmd = apple_escape(cd);
  if new_window {
    format!("tell application \"Terminal\" to do script \"{cmd}\"")
  } else {
    format!("tell application \"Terminal\" to do script \"{cmd}\" in front window")
  }
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
}
