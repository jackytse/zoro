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
