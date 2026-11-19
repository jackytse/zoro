# zoro 代码审查报告

- **审查对象**：zoro 桌面应用 v0.1.9（Tauri 2 + Vue 3 + Nuxt UI 4 + Vite+）
- **审查范围**：`src/` 全部前端源码、`src-tauri/src/` 全部 Rust 后端源码、构建与 CI 配置
- **审查方式**：逐文件静态分析 + 本地运行类型检查/构建/Rust 单测 + i18n 键一致性脚本 + 针对可疑点的独立最小复现
- **结论**：发现 **1 个严重 bug（可致进程崩溃）**、4 个中等隐患、5 个轻微问题，另有 4 项需作者确认的疑点。整体工程质量较高，命令注入防护与 i18n 一致性处理尤其扎实。

> 说明：本报告只审查、不修改任何源码。所有临时验证脚本均写在 `/tmp/`，项目内仅新增本文件 `docs/CODE_REVIEW.md`。

---

## 一、实际运行的检查命令与结果

| 命令 | 结果 | 摘要 |
| --- | --- | --- |
| `node /tmp/zoro_i18n_check.cjs`（基于 typescript transpile 遍历三语包） | ✅ 通过 | 三语各 **125** 个 key，缺 key=0、多 key=0、占位符不一致=0 |
| `pnpm run type-check`（vue-tsc --build） | ✅ 通过 | 无类型错误 |
| `pnpm run build-only`（vp build） | ✅ 通过 | 852 modules transformed，双入口 `index.html` / `tray.html` 构建成功 |
| `cargo test --locked --manifest-path src-tauri/Cargo.toml` | ✅ 通过 | **21 passed / 0 failed / 1 ignored**（`scans_real_home` 为有意忽略） |
| `CI=1 vp check` | ❌ 失败（退出码 1） | **14 个文件格式不合规**（详见 🔵-1）；非 lint 逻辑错误 |
| `rustc -O /tmp/zoro_repro.rs && ./zoro_repro` | ❌ 异常退出（137） | 复现 importer 长文本场景，进程被 SIGKILL（栈/内存耗尽），见 🔴-1 |
| `rustc -O /tmp/zoro_repro2.rs && ./zoro_repro2` | ✅ 输出证据 | 9000 字节纯可打印段 `split(['\n','\r','\t'])` 得到 **1 段**，递归长度不收敛 |

> 注：`ci/test.sh` 的门禁是 `pnpm run build` + `cargo test`，二者均通过；门禁**不含** `vp check`，故格式问题不会让当前 CI 失败。

---

## 二、🔴 严重（bug / 安全）

### 🔴-1 Zed 项目扫描存在无限递归 → 栈溢出 → 进程崩溃【确认】

- **位置**：`src-tauri/src/importer.rs:391-398`（`extract_from_run`），关联 `373-388`（`extract_home_dir_paths`）
- **问题描述**：
  - `extract_home_dir_paths` 只把字节值在 `0x20..=0x7E`（可打印 ASCII）的连续段切成一个 `run` 交给 `extract_from_run`。因此 `run` 内**绝不可能**出现 `\n`(0x0A)、`\r`(0x0D)、`\t`(0x09)——它们都小于 0x20。
  - 而 `extract_from_run` 对超长段的处理是：
    ```rust
    if run.len() > 8192 {
        for piece in std::str::from_utf8(run).unwrap_or("").split(['\n', '\r', '\t']) {
            extract_from_run(piece.as_bytes(), home_str, out);
        }
        return;
    }
    ```
    `split` 用三个**永远不存在**的分隔符去切 `run`，只会返回 `run` 自身一段；递归传入的 `piece.len() == run.len() > 8192`，再次进入同一分支 → **无限递归**。
  - `longest_existing_dir_prefix`（`421-442`）本身是正确的，但永远执行不到。
- **影响**：Zed 的 `db.sqlite` / `db.sqlite-wal` 中只要出现一段连续超过 8192 字节的可打印 ASCII（长 JSON、序列化状态、连续路径串都很常见），用户在设置页点「扫描最近项目」触发 `scan_recent_projects` 时，Rust 侧就会栈溢出。Rust 栈溢出是 `abort`，**整个应用直接崩溃**，且用户无法自行规避（只要装了 Zed 并打开过项目）。
- **证据**：独立最小复现（`/tmp/zoro_repro.rs`）确认无收敛：直接复现进程退出码 137；对照程序（`/tmp/zoro_repro2.rs`）证明 9000 字节纯可打印段 `split(['\n','\r','\t'])` 得到 1 段、递归深度无限增长。
- **修复建议**：不要用 `run` 中不存在的字符切分。可改为定长切片递归，例如
  ```rust
  if run.len() > 8192 {
      for chunk in run.chunks(8192) {           // 定长切分，长度必然递减
          extract_from_run(chunk, home_str, out);
      }
      return;
  }
  ```
  或去掉该分支，直接对整段做 `find(home_str)`（`find` 循环本身是 O(n)，无需按长度限制）。另建议补充一个「>8192 字节可打印段」的单元测试防止回归。

---

## 三、🟡 中等（隐患 / 健壮性）

### 🟡-1 Windows 平台硬编码依赖 `HOME` 环境变量，相关功能失效【确认】

- **位置**：`src-tauri/src/commands.rs:36`、`src-tauri/src/config.rs:469-482`（`expand_tilde`）、`src-tauri/src/importer.rs:268-270`、`338-340`
- **问题描述**：
  - `scan_recent_projects`：`std::env::var("HOME").map_err(|_| "无法获取用户主目录")`。Windows 默认没有 `HOME`（用 `USERPROFILE`），该命令在 Windows 上必然返回 Err，前端弹「扫描失败」。
  - `expand_tilde` 只识别 `HOME`，Windows 上用户输入 `~/Projects/demo` 不会展开，`is_dir()` 校验失败，添加项目报错。
  - `scan_jetbrains` / `scan_zed` 同样依赖 `HOME`。
  - 项目 CI（`.cnb.yml`、`release.yml`）明确产出 Windows 安装包，`tauri.conf.json` 也有 Windows 配置，说明 Windows 是受支持的平台。
- **影响**：Windows 用户无法使用路径 `~` 展开；「从工具导入」功能直接报错。（注：importer 的数据源路径均为 macOS `~/Library/...`，即使在 Windows 拿到 HOME 也扫不到数据，但命令不应整体失败。）
- **修复建议**：用 `tauri` 的 `app.path().home_dir()` 统一获取主目录，并在 `expand_tilde` 中同时兼容 `HOME` 与 `USERPROFILE`；`scan` 在非 macOS 上可尽早返回空列表而非 Err。

### 🟡-2 更新检测在「已是最新」时未清空待安装对象【确认】

- **位置**：`src-tauri/src/updater.rs:79-88`
- **问题描述**：`updater_check` 仅在 `update.map(|u| ...)` 命中时写入 `pending`；当返回 `None`（已最新 / 清单回滚）时，`pending` 里上一次检测写入的旧 `Update` 仍被保留。
  ```rust
  Ok(update.map(|u| {
      ...
      *pending.0.lock().unwrap() = Some(u);   // 仅 Some 分支清理/写入
      info
  }))
  ```
- **影响**：切通道或重新检测后若结果为「已最新」，`PendingUpdate` 仍持有旧通道的更新对象。当前前端状态机在 `phase=latest` 时不显示下载按钮，故暂不会被误装；但这是跨命令共享状态的不一致，一旦前端流程调整（如自动续装）即可导致安装到过期/错误通道的包。
- **修复建议**：在返回 `None` 分支显式 `*pending.0.lock().unwrap() = None;`，保证「检测结果」与「待安装对象」严格一一对应。

### 🟡-3 多个配置命令「先改内存、后落盘」非原子，失败不回滚且存在并发竞态【确认】

- **位置**：`src-tauri/src/commands.rs` 的 `set_autostart:212-227`、`set_global_shortcut:242-253`、`set_theme:350-356`、`set_locale:373-381`、`set_terminal_tool:303-309`、`set_terminal_mode:327-333`
- **问题描述**：
  - 这些命令先把新值写入内存 `config`，再调用 `config::save(&state)?`。若 `save` 失败（磁盘满、权限、rename 失败），`?` 直接返回 Err，**内存已改但磁盘未写**，且没有回滚。前端 catch 分支会重新 `getConfig()`，拿到的是内存新值 → UI 显示「设置成功」的状态，但重启后回退，用户困惑。
  - `set_autostart` / `set_global_shortcut` 采用**多次独立 `lock()`**（读 previous → unlock → 写新值 → unlock → 调系统 API → 再 lock 回滚/读取），读-改-写不是原子的。Tauri 的同步命令在 blocking 线程池执行，多窗口并发调用时存在交错，previous 可能不准、回滚可能覆盖其他命令的修改。
- **影响**：配置状态在磁盘/内存/系统三者间可能不一致；并发设置时极端情况丢失更新。整体概率不高，但属于真实健壮性缺口。
- **修复建议**：
  1. 统一封装「在单次持锁内完成读-改-写，clone 快照后再落盘」，落盘失败时把内存回滚到旧快照。
  2. 参照 `set_global_shortcut` 的回滚写法，为所有 `set_*` 补齐 save 失败回滚。
  3. 或对 config 写操作串行化（单一 `mutex` 覆盖整个事务，含 save）。

### 🟡-4 `tauri.conf.json` 关闭了 CSP【安全加固建议，当前无已知可利用点】

- **位置**：`src-tauri/tauri.conf.json:29-31`（`"csp": null`）
- **问题描述**：`security.csp` 设为 `null`，WebView 不对内嵌脚本/远程请求做内容安全策略约束。本项目前端用 `v-html` 渲染品牌 SVG（`ToolIcon.vue`），虽然内容来自代码内硬编码的 `brands.ts`、无用户输入拼接，**当前不存在可利用的注入点**；但关闭 CSP 放弃了纵深防御。
- **影响**：万一未来引入任意 HTML/远程内容渲染，缺少 CSP 会放大 XSS 后果。
- **修复建议**：配置最小 CSP（如 `default-src 'self'; img-src 'self' asset: http://asset.localhost data:; style-src 'self' 'unsafe-inline'`），并确认不影响 Nuxt UI 的内联样式。

---

## 四、🔵 轻微（风格 / 可维护性）

### 🔵-1 `vp check` 报告 14 个文件格式问题
- **位置**：`.cnb.yml`、`package.json`、`src-tauri/Cargo.toml`、`src/api/desktop.ts`、`src/components/OverlayScrollbar.vue`、`src/i18n/locales/{en,zh-CN,zh-TW}.ts`、`src/tray/TrayPopup.vue`、`src/utils/time.ts`、`src/views/SettingsView.vue`、`README.md`、`README_EN.md`、`.workbuddy/memory/2026-11-18.md`
- **影响**：仅格式（空格/换行等），非逻辑错误；当前门禁未运行 `vp check`，不影响构建与测试。其中 `README.md` / `README_EN.md` 等为工程师正在编辑的文档，本次审查**未做改动**。
- **建议**：发布前统一执行一次 `vp check --fix`；注意与文档作者的改动协调，避免互相覆盖。

### 🔵-2 CI 镜像名注释与实际配置不一致
- **位置**：`Dockerfile.ci:10`、`ci/setup.sh:46` 注释写 `docker.cnb.cool/jackytse/zoro:ci`；实际 `.cnb.yml:44,74` 用的是 `docker.cnb.cool/jackytse/docker:ci`
- **影响**：无功能影响（三处实际配置一致），但注释会误导后续维护者排查镜像来源。
- **建议**：统一注释为 `.../docker:ci`。

### 🔵-3 `<html lang>` 首屏硬编码 `zh-CN`
- **位置**：`index.html:2`、`tray.html:2`
- **影响**：配置加载后 `applyLocale` 会更新 `document.documentElement.lang`，但首屏瞬间 lang 与实际语言不符，对屏幕阅读器/排版有极小影响。
- **建议**：可保留（影响极小），或在首屏内联脚本中一并写入 lang。

### 🔵-4 启动更新检测的 `setTimeout` 未保留句柄
- **位置**：`src/composables/useUpdater.ts:234-247`
- **影响**：回调在 5s 后执行，若期间窗口卸载仍会弹 toast。但 App 是根组件、生命周期等于应用进程，实际无泄漏。模块级守卫 `startupCheckScheduled` 已保证只触发一次（注释说明为有意设计）。
- **建议**：可接受；若追求严格可清理，可返回取消函数并在 `onBeforeUnmount` 调用。

### 🔵-5 大量 `Mutex::lock().unwrap()`
- **位置**：`commands.rs`、`config.rs:539`、`tray.rs`、`lib.rs:125,149` 等
- **影响**：当前持锁区间内操作均很短且不含会 panic 的逻辑，投毒风险低；但任一处 panic 会使后续所有命令连锁 panic。
- **建议**：作为长期健壮性改进，可用 `lock().map_err(...)?` 把锁错误转成命令错误返回。

---

## 五、需作者确认的疑点（尚不能判定为 bug，请勿直接改动）

1. **macOS 多显示器几何推算正确性**（`tray.rs:350-396` `fallback_geometry_macos` / `398-438` `fallback_geometry_other`）：代码用事件 `rect`（物理坐标）除以各屏 `scale_factor` 还原到点空间再匹配显示器，逻辑自洽，但混合缩放多屏的组合行为只能在真机验证。正常路径走 `status_item_geometry`，此分支仅兜底。建议在混合缩放多屏环境实测一次。
2. **权限集是否覆盖 `getVersion()`**（`capabilities/default.json:7`）：前端 `useUpdater.ts:21` 调用 `@tauri-apps/api/app` 的 `getVersion()`，需要 `core:app:allow-version`。当前只声明 `core:default`，需确认该默认集在你的 Tauri 版本中包含 app 默认权限。若运行时报权限错误，补充 `core:app:allow-version`。
3. **Windows `open_in_terminal` 特殊字符路径**（`opener.rs:231-245`）：用 `cmd /C start "" /D <target> cmd`，`target` 作为独立参数传入（经 Rust Command 转义），一般安全；但路径含 `&`、`^` 等 cmd 元字符时，`cmd /C` 的二次解析行为需实机确认。
4. **i18n 三语一致性缺少自动化守护**：当前三语 125 键完全一致（已验证），但完全靠人工维护。未来新增 key 若漏译只能回落 zh-CN，不会有 CI 提示。建议在 CI 加一个键一致性检查（可复用本次审查用的脚本思路）。

---

## 六、✅ 亮点

1. **命令注入防护到位**：`opener.rs` 的 `shell_quote`（POSIX 单引号转义，测试覆盖）、`apple_escape`（AppleScript 字符串转义）、`open -a` 用参数数组、`Command::spawn` 不经 shell；硬编码 `TOOL_CATALOG` 的 `command -v` 探测也无注入面。**未发现命令注入风险**。
2. **i18n 质量高**：三语各 125 键、占位符（`{n}`/`{name}`/`{tool}`/`{version}` 等）完全一致；`normalizeLocale` / `detectLocale` / `resolveLocale` 归一化链路清晰。
3. **配置原子写盘**：`config.rs:538-545` 采用「写 `.tmp` 再 `rename`」，避免半截 JSON；文件损坏时回退默认配置；还实现了旧包名目录的配置迁移。
4. **并发与 IO 细节**：`open_project` / `add_project` 在持锁期间只 clone 数据、`drop(cfg)` 后再做进程启动与广播；`PendingUpdate` 的 `MutexGuard` 未跨越 `.await`，无异步持锁死锁。
5. **打开策略健壮**：CLI → 登录 shell（借用用户 PATH）→ `open -a` 三级降级；终端打开用 AppleScript 精确 `cd`，并对 tab 失败逐层降级到新窗口、`open -a Terminal` 兜底，错误文案带可操作指引。
6. **三处版本号一致**：`Cargo.toml` / `tauri.conf.json` / `package.json` 均为 `0.1.9`，发布脚本有 tag 与版本一致性校验。
7. **测试与平台适配**：22 个 Rust 单测覆盖编辑距离、路径去重、URI 解码、JetBrains/Zed 解析、快捷键解析等；`#[cfg]` 条件编译对 macOS/Windows/Linux 分支裁剪严谨，注释详尽。
8. **托盘生命周期设计**：弹窗只创建一次、失焦隐藏与点击意图用 `RECLICK_GRACE`/`FOCUS_GRACE` 双重防抖，避免 blur-hide 与 click-show 抖动。
9. **更新链路完整**：双通道清单 + minisign 公钥校验 + 进度事件 + 前端状态机，设计清晰、错误翻译友好。

---

## 七、优先级建议

1. **立即修复**：🔴-1（栈溢出崩溃）——这是唯一会导致用户应用崩溃的问题，且触发门槛低（安装 Zed 即可）。
2. **本次迭代修复**：🟡-1（Windows HOME）、🟡-2（pending 未清）、🟡-3（配置写事务）。
3. **择机处理**：🟡-4（CSP）、全部 🔵 与「需确认疑点」。
