# zoro

[简体中文](./README.md) · English

**A fast launcher for your recent projects, right in the menu bar.** zoro lives in the system tray: left-click to pop up your recently opened projects and open any of them in VS Code, Cursor, JetBrains IDEs, Zed, and more. You can also bulk-import projects from those editors' own recent-workspace records.

Current version: `0.1.9`. Open sourced under the [MIT](./LICENSE) license.

## Features

- **Menu bar tray**: a macOS template icon that adapts to light/dark themes; left-click toggles the popup, right-click opens the system menu.
- **Recent projects popup**: projects sorted by last-opened time (brand icon / name / path / relative time), one click to open with the bound tool; hover a row to open its directory in the system terminal.
- **Tool auto-sync**: 18 built-in tools (VS Code, Cursor, Windsurf, the full JetBrains family, Android Studio, Xcode, Sublime Text, Zed, iTerm, Terminal, and more), automatically added or removed at startup based on what is installed locally.
- **One-click import**: read-only scan of recent projects from VS Code / Cursor, the JetBrains family, Android Studio, and Zed; check the ones you want, import them in bulk, and pick a different tool per entry.
- **Global shortcut**: `Cmd/Win + Shift + P` by default to toggle the popup; customizable or disableable in Settings.
- **Launch at login**: on by default; starts silently in the tray with `--hidden` and shows no window.
- **i18n and themes**: Simplified Chinese / Traditional Chinese / English UI, following the system by default, switching live with the tray menu synced; theme supports light / dark / system.
- **In-app updates**: stable and prerelease channels, with minisign-verified update artifacts.
- **Robust open strategy**: tries the CLI first, then the login shell, then macOS `open -a`; a clear error is shown if all three fail.

## Tech Stack

| Layer       | Choice                              | Notes                                                                        |
| ----------- | ----------------------------------- | ---------------------------------------------------------------------------- |
| Toolchain   | [Vite+](https://viteplus.dev) 1.7   | dev / build / lint / fmt / test / staged unified in one `vite.config.ts`      |
| Framework   | Vue 3.5 + vue-router 5              | `<script setup>` composition API                                             |
| UI          | [Nuxt UI](https://ui.nuxt.com) 4.12 | Tailwind CSS 4, auto-imported components and composables                     |
| Utilities   | VueUse 15, vue-i18n 11              | theme / time / notification composables and three-language i18n              |
| Desktop shell | Tauri 2.12                        | Rust backend (edition 2024, rust-version 1.90) + system WebView               |

## Requirements

- **Node.js**: `^22.18.0 || >=24.12.0` (see `engines` in `package.json`)
- **pnpm 12** (CI pins 12.21.0; the version is tied to the lockfile format)
- [vp CLI](https://viteplus.dev): `curl -fsSL https://viteplus.dev/install.sh | bash`
- **Tauri**: Rust 1.90+ and Xcode Command Line Tools (macOS); WebView2 and MSVC build tools on Windows

## Install and Develop

```sh
vp install            # recommended: Vite+ manages dependencies (matching pnpm 12 + catalog)
# or pnpm install

pnpm dev              # browser dev server, port 5173
pnpm run desktop:dev  # Tauri desktop window, port 1420 (starts the frontend dev server)
vp check              # format + lint + type check (--fix applies fixes)
vp test               # Vitest unit tests
cargo test --locked --manifest-path src-tauri/Cargo.toml   # Rust unit tests (same as CI gate)
```

> Note: `vp dev` / `vp build` are Vite+ **built-in commands**. To run scripts defined in `package.json`, use `pnpm run xxx` or `vp run xxx`; the two forms are not interchangeable.

### The two dev servers and their ports

| Command                | Port | Description                                        |
| ---------------------- | ---- | --------------------------------------------------- |
| `pnpm dev`             | 5173 | Browser debugging                                   |
| `pnpm run desktop:dev` | 1420 | URL loaded by the Tauri shell, set by `dev:desktop` |

They use **different ports**, so both can run at the same time without conflicts. Do not start another server on port 1420 manually, or `beforeDevCommand` will fail because of `--strictPort`.

## Packaging

```sh
pnpm build             # type check + production build (Rolldown), output in dist/
pnpm run desktop:build # Tauri bundle, output in src-tauri/target/release/bundle/
```

- The frontend is a **multi-entry** build: `index.html` is the main window and `tray.html` is the tray popup (a separate small entry that does not load the main router).
- `bundle.targets = all`; CI produces Linux `deb`, macOS `dmg`, and Windows `nsis` / `msi`, named `zoro-<version>-<os>-<arch>` with a `.sha256` checksum each.
- When an update signing key (`TAURI_SIGNING_PRIVATE_KEY`) is injected, the build also emits `.sig` files used to verify in-app updates.

## Tray Behavior

A tray icon is created at startup in the menu bar (a macOS template icon that adapts to light/dark themes).

- **Left click**: shows the "Recent Projects" popup below the icon; click again, click elsewhere (blur), or press `Esc` to dismiss it.
- **Right click**: system menu, labelled by UI language — "Settings / Quit App" (zh-CN: 设置 / 退出应用; zh-TW: 設定 / 結束應用).
- **Popup contents**: a list sorted by last-opened time; each row shows the brand icon, name, path, and relative time. Click a row to open it with the bound tool, or hover and click the terminal icon to open its directory in the system terminal. "Settings" and "Quit" sit in the footer.
- **Reopening the main window**: on macOS zoro runs as an Accessory app and **does not show a Dock icon**. Bring the main window back via the tray right-click menu ("Settings"), the popup footer ("Settings"), or `⌘/Ctrl + ,` from the popup.
- **Closing ≠ quitting**: clicking the main window's close button only hides it; the process stays in the background. Use the tray menu or the popup's "Quit" to actually exit.
- **Settings page** (main window `/`): three tabs — General (theme, language, launch at login, global shortcut, terminal behavior, about & updates), Projects (add / remove / import), and Tools (read-only list of available local tools).
- **Import recent projects**: "Projects → Scan recent projects" performs a read-only scan; check the entries you want and import them in bulk, optionally changing the tool for each. Sources include VS Code / Cursor (`state.vscdb`, `storage.json`, `workspaceStorage`), the JetBrains family and Android Studio (`recentProjects.xml`), and Zed (`db.sqlite`). Deleted directories and already-configured paths are filtered out; iTerm has no readable record source and is not scanned.
- **Open strategy**: direct CLI → login shell resolution (honors nvm / homebrew PATH) → macOS `open -a` fallback. Terminal-type projects go through an AppleScript that explicitly `cd`s into the directory (iTerm2 / Terminal, new window / new tab).
- **Multi-monitor positioning**: on macOS the authoritative tray icon position comes from `NSStatusItem`, avoiding the popup landing on the wrong display under mixed scaling. Use `zoro --debug-monitors` to print raw monitor data when diagnosing layout issues.

## Configuration Location

Configuration is persisted as JSON and falls back to defaults if the file is corrupted:

- macOS: `~/Library/Application Support/me.jackytse.zoro/config.json`
- Windows: `%APPDATA%\me.jackytse.zoro\config.json` (Tauri's default app config directory)

> Earlier versions used `dev.zoro.desktop` as the identifier. On first launch, if the new directory has no config but the old one does, `config.json` is migrated from `~/Library/Application Support/dev.zoro.desktop/config.json` automatically, so tools, projects, and preferences are preserved.

The tray icon is generated by [`scripts/gen-tray-icon.py`](./scripts/gen-tray-icon.py) (a pure-standard-library PNG writer, no image library needed) and written to `src-tauri/icons/tray.png`.

## Directory Structure

```text
src/                          Vue frontend (two entries: main window + tray popup)
  main.ts / App.vue           main window entry and root component
  tray/                       tray popup (main.ts, TrayPopup.vue, tray.css)
  views/SettingsView.vue      settings page (General / Projects / Tools)
  router/index.ts             router table (main window only has / → Settings)
  api/desktop.ts              Tauri command wrappers and shared types
  i18n/                       i18n (index.ts + locales/{en,zh-CN,zh-TW}.ts)
  composables/                useAppTheme / useNotify / useUpdater
  components/                 ToolIcon / OverlayScrollbar
  icons/brands.ts             IDE brand icon mapping
  utils/time.ts               relative time formatting
  assets/css/main.css         Tailwind and Nuxt UI style entry
src-tauri/                    Rust desktop shell
  src/main.rs / lib.rs        entry, app setup, events and command registration
  src/config.rs               config model, persistence, tool catalog and sync
  src/commands.rs             Tauri commands exposed to the frontend
  src/importer.rs             recent project scanning (VS Code / JetBrains / Zed...)
  src/opener.rs               project and terminal open strategies
  src/shortcut.rs             global shortcut registration and trigger
  src/tray.rs                 tray icon, popup positioning and context menu
  src/updater.rs              in-app updates (two channels)
  capabilities/default.json   permission declarations
  tauri.conf.json             window, bundle and updater configuration
ci/                           CI scripts (setup / test / build / release / upload)
.cnb.yml                      CNB pipelines (quality gate + three-platform release)
.github/workflows/release.yml GitHub Actions release pipeline
vite.config.ts                unified Vite+ config (plugins / lint / fmt / staged)
```

## FAQ

- **Where did the app go after I closed the window?** It was only hidden in the tray, not quit. On macOS it runs as a background Accessory app with no Dock icon.
- **How do I reopen the main window?** Use the tray right-click menu ("Settings"), the popup footer ("Settings"), or press `⌘/Ctrl + ,` inside the popup.
- **Why is a certain IDE missing from the tools list?** The catalog syncs with what is installed; built-in tools that are not installed and not referenced by any project are removed, and reappear once installed. Check the Settings → Tools tab.
- **Opening a project failed.** Make sure the directory still exists and the tool is installed. If the CLI is not on PATH (common with nvm / homebrew), add it or rely on the login-shell / `open -a` fallback. The error message explains the cause.
- **Where is the config file, and what if it is broken?** See "Configuration Location". A parse failure falls back to defaults instead of breaking the tray.
- **Does importing modify my IDE data?** No. Scanning is strictly read-only and never writes to VS Code / JetBrains / Zed files.
- **Terminal open does nothing or lands in the wrong directory?** On macOS, allow Zoro to control iTerm / Terminal (System Settings → Privacy & Security → Automation). If launched from a sandboxed terminal, Apple Events may be blocked; start the app from a normal terminal instead.
- **The popup appears on the wrong monitor.** Run `zoro --debug-monitors` to print raw monitor data for diagnosing mixed-scaling multi-display setups.
- **What are `auto-imports.d.ts` / `components.d.ts`?** Files generated by the Nuxt UI plugin on the first `pnpm dev` or `pnpm build`, gitignored, and required by `vue-tsc` for type checking.

## Contributing

Issues and PRs are welcome. See [CONTRIBUTING.md](./CONTRIBUTING.md) for the workflow, commit conventions, and pre-PR checklist.

## License

[MIT](./LICENSE) © 2026 jackytse
