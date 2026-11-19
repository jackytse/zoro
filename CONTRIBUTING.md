# 贡献指南 / Contributing to zoro

> 语言 / Language: 简体中文 | [English](#english)

感谢你愿意改进 zoro！本文说明如何报告问题、搭建环境、提交代码与发起 PR。文档与界面一样保持中英双语风格。

---

## 简体中文

### 贡献方式

- **报告问题（Issue）**：描述现象、复现步骤、期望行为，并附上系统版本、应用版本（设置页「当前版本」）与相关日志。提交前请先搜索是否已有相同 issue。
- **功能建议**：说明使用场景与动机，而不只是实现方式；涉及新工具 / 新导入源时，请附上其 CLI 或数据文件的实际路径。
- **代码贡献（PR）**：先从 issue 认领或讨论方案，避免重复劳动；一个 PR 聚焦一件事，便于 review。

### 开发环境搭建

1. 安装前置依赖：Node.js `^22.18.0 || >=24.12.0`、pnpm 12、[vp CLI](https://viteplus.dev)、Rust 1.90+；macOS 另需 Xcode Command Line Tools，Windows 需 WebView2 与 MSVC 构建工具。
2. 拉取代码后安装依赖（Vite+ 会匹配仓库的 pnpm 与 catalog 版本）：

   ```sh
   vp install        # 或 pnpm install
   ```

3. 启动开发环境：

   ```sh
   pnpm dev              # 浏览器，端口 5173
   pnpm run desktop:dev  # Tauri 桌面窗口，端口 1420
   ```

4. 若遇到环境 / 包管理异常，运行 `vp env doctor` 并在求助时附上输出。

### 分支与提交规范

分支建议使用短前缀命名，如 `feat/import-zed`、`fix/tray-position`、`docs/readme-i18n`。

提交信息采用 [Conventional Commits](https://www.conventionalcommits.org/) 风格，与仓库现有记录一致（`feat:` / `fix:` / `chore:` / `docs:` / `ci:` / `ui:`），可带作用域括号：

```text
feat(tray): 托盘菜单文案调整
fix(ci): win release 上传加外层重试
ui(settings): 当前版本与更新通道拆分为两行
docs: README 补充英文版
chore: bump version to 0.1.9
```

- 类型：`feat` 新功能、`fix` 修复、`docs` 文档、`chore` 杂务 / 依赖 / 版本、`ci` 流水线、`ui` 界面调整、`refactor` 重构、`test` 测试。
- 描述用中文或英文均可，保持简洁、说明「做了什么」而非「改了哪个文件」；一行说不清时用空行分隔正文。
- 避免在提交里夹带无关改动；发版版本变更由维护者统一 `chore: bump version to x.y.z`。

### 代码风格与检查

- 项目使用 Vite+ 统一工具链：格式化（oxfmt）、lint（oxlint）、类型检查（vue-tsc）、测试（Vitest）都收敛在 `vite.config.ts`。
- 提交前运行：

  ```sh
  vp check    # 格式化 + lint + 类型检查；加 --fix 自动修复
  vp test     # Vitest 单测
  ```

- `vite.config.ts` 的 `staged` 配置为 `"*": "vp check --fix"`，提交时会对改动文件自动检查与修复。
- lint 规则 `vite-plus/prefer-vite-plus-imports` 为 error：优先使用 Vite+ 推荐的导入方式。
- Rust 侧使用标准 `cargo fmt` 与 `cargo test`（edition 2024）：

  ```sh
  cargo fmt --manifest-path src-tauri/Cargo.toml
  cargo test --locked --manifest-path src-tauri/Cargo.toml
  ```

- 前端与后端字段通过 `serde(rename_all = "camelCase")` 对齐；新增或修改配置字段时，请同时更新 Rust 结构体、`src/api/desktop.ts` 的类型与设置页。

### Tauri 前端 + 后端改动的注意事项

- **双入口**：主窗口走 `index.html`，托盘弹窗走独立的 `tray.html`（`src/tray/`）。修改界面或主题时，两个窗口都要验证。
- **命令与类型同步**：在 `src-tauri/src/commands.rs` 新增 / 改命令后，要在 `src-tauri/src/lib.rs` 的 `generate_handler!` 注册，并在 `src/api/desktop.ts` 补上封装与类型；字段名遵循 camelCase。
- **权限声明**：新增 Tauri 插件或调用系统能力时，检查 `src-tauri/capabilities/default.json` 是否已授权对应权限，否则运行时会被拒绝。
- **配置兼容**：配置字段优先用 `Option<T>` + `#[serde(default)]`，旧配置缺字段时能正常反序列化；语义变更时做归一化（可参考 `terminal_mode()` / `locale()`），不要让用户手工迁移。
- **多语言同步**：界面文案在 `src/i18n/locales/{en,zh-CN,zh-TW}.ts` 三份语言包中保持一致（以 zh-CN 为最全、`fallbackLocale`）；修改托盘右键菜单文案时，同步 `src-tauri/src/tray.rs` 中按 locale 的分支。
- **版本一致性**：`package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 的版本需一致；发布 tag 必须与 `tauri.conf.json` 版本相同，CI 会校验。
- **构建顺序**：CI 门禁（`ci/test.sh`）先 `pnpm run build` 再 `cargo test --locked`——`tauri::generate_context!` 在编译期读取 `frontendDist`（`../dist`），必须先产出前端构建。
- **依赖锁文件**：`Cargo.lock` 变动需随 PR 提交（CI 使用 `--locked`）；pnpm 12 构建脚本白名单在 `pnpm-workspace.yaml` 的 `allowBuilds`。
- **Windows 脚本**：`ci/win-*.ps1` 必须带 UTF-8 BOM，否则 PowerShell 5.x 无法正确解析中文。

### PR 提交前检查清单

- [ ] 已拉取远程变更并执行 `vp install`
- [ ] `vp check` 与 `vp test` 通过
- [ ] `cargo fmt` 与 `cargo test --locked --manifest-path src-tauri/Cargo.toml` 通过（若改动 Rust）
- [ ] `pnpm build` 通过；涉及桌面行为时用 `pnpm run desktop:dev` 在真实窗口验证
- [ ] 前端 / 后端字段、权限声明、三语文案、类型封装已同步
- [ ] 提交信息符合 Conventional Commits，PR 描述说明动机与验证方式
- [ ] 未提交 `node_modules`、`dist`、`src-tauri/target` 等生成物（已在 `.gitignore`）

### 行为准则简述

尊重他人、友善沟通、假设善意；不接受人身攻击、骚扰或歧视性言论。详情见 [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md)。

---

## English

Thanks for helping improve zoro! This guide covers how to report issues, set up the environment, commit code, and open a pull request. Like the UI, our docs keep a bilingual style.

### Ways to contribute

- **Report an issue**: describe what happened, steps to reproduce, and expected behavior, plus your OS version, app version (Settings → Current version), and relevant logs. Search existing issues first.
- **Propose a feature**: explain the use case and motivation, not just the implementation. For new tools or import sources, include the actual CLI or data-file paths.
- **Send a PR**: claim an issue or discuss the approach first to avoid duplicate work. Keep one PR focused on one thing for easier review.

### Development setup

1. Install prerequisites: Node.js `^22.18.0 || >=24.12.0`, pnpm 12, [vp CLI](https://viteplus.dev), Rust 1.90+. On macOS also install Xcode Command Line Tools; on Windows install WebView2 and MSVC build tools.
2. Install dependencies after cloning (Vite+ matches the repo's pnpm and catalog versions):

   ```sh
   vp install        # or pnpm install
   ```

3. Start developing:

   ```sh
   pnpm dev              # browser, port 5173
   pnpm run desktop:dev  # Tauri desktop window, port 1420
   ```

4. If setup or package-manager behavior looks wrong, run `vp env doctor` and include its output when asking for help.

### Branching and commit conventions

Use short branch prefixes, e.g. `feat/import-zed`, `fix/tray-position`, `docs/readme-i18n`.

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/), matching existing history (`feat:` / `fix:` / `chore:` / `docs:` / `ci:` / `ui:`), with an optional scope:

```text
feat(tray): localize tray menu labels
fix(ci): add retry to Windows release upload
ui(settings): split version and channel into separate rows
docs: add English README
chore: bump version to 0.1.9
```

- Types: `feat` new feature, `fix` bug fix, `docs` documentation, `chore` maintenance / deps / version, `ci` pipelines, `ui` UI tweaks, `refactor`, `test`.
- Chinese or English descriptions are both fine; be concise and say "what changed", not "which file changed". Use a blank line for a longer body.
- Keep unrelated changes out of one commit. Release version bumps are handled by maintainers via `chore: bump version to x.y.z`.

### Code style and checks

- The project uses Vite+ as the unified toolchain: formatting (oxfmt), lint (oxlint), type checking (vue-tsc), and tests (Vitest) are all configured in `vite.config.ts`.
- Before submitting:

  ```sh
  vp check    # format + lint + type check; add --fix to apply fixes
  vp test     # Vitest unit tests
  ```

- `vite.config.ts` sets `staged` to `"*": "vp check --fix"`, so changed files are checked and fixed automatically on commit.
- The lint rule `vite-plus/prefer-vite-plus-imports` is an error: prefer Vite+'s recommended imports.
- Rust uses standard `cargo fmt` and `cargo test` (edition 2024):

  ```sh
  cargo fmt --manifest-path src-tauri/Cargo.toml
  cargo test --locked --manifest-path src-tauri/Cargo.toml
  ```

- Frontend and backend fields align via `serde(rename_all = "camelCase")`. When adding or changing a config field, update the Rust struct, the types in `src/api/desktop.ts`, and the settings UI together.

### Notes on Tauri frontend + backend changes

- **Two entries**: the main window uses `index.html`, the tray popup uses a separate `tray.html` (`src/tray/`). Verify both windows when changing UI or theming.
- **Commands and types**: after adding/changing a command in `src-tauri/src/commands.rs`, register it in `generate_handler!` in `src-tauri/src/lib.rs` and add a wrapper plus types in `src/api/desktop.ts`; fields are camelCase.
- **Permissions**: when adding a Tauri plugin or a system capability, check `src-tauri/capabilities/default.json` grants the needed permission, or it will be denied at runtime.
- **Config compatibility**: prefer `Option<T>` + `#[serde(default)]` for config fields so old files without them still deserialize; normalize semantic changes (see `terminal_mode()` / `locale()`) instead of asking users to migrate manually.
- **i18n sync**: keep messages consistent across `src/i18n/locales/{en,zh-CN,zh-TW}.ts` (zh-CN is the most complete and the `fallbackLocale`). When changing tray context-menu labels, update the locale branches in `src-tauri/src/tray.rs` too.
- **Version consistency**: keep versions aligned in `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`. Release tags must match the `tauri.conf.json` version; CI enforces this.
- **Build order**: the CI gate (`ci/test.sh`) runs `pnpm run build` before `cargo test --locked`, because `tauri::generate_context!` reads `frontendDist` (`../dist`) at compile time.
- **Lockfiles**: commit `Cargo.lock` changes with your PR (CI uses `--locked`). The pnpm 12 build-script allowlist lives in `allowBuilds` in `pnpm-workspace.yaml`.
- **Windows scripts**: `ci/win-*.ps1` must carry a UTF-8 BOM, or PowerShell 5.x cannot parse Chinese correctly.

### Pre-PR checklist

- [ ] Pulled remote changes and run `vp install`
- [ ] `vp check` and `vp test` pass
- [ ] `cargo fmt` and `cargo test --locked --manifest-path src-tauri/Cargo.toml` pass (if Rust changed)
- [ ] `pnpm build` passes; desktop behavior verified in a real window via `pnpm run desktop:dev`
- [ ] Frontend/backend fields, permissions, three language packs, and type wrappers are in sync
- [ ] Commit messages follow Conventional Commits; PR describes motivation and verification
- [ ] No generated files committed (`node_modules`, `dist`, `src-tauri/target` are gitignored)

### Code of Conduct (summary)

Be respectful, kind, and assume good intent. Harassment, personal attacks, and discriminatory language are not tolerated. See [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md) for details.
