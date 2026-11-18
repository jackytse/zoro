#!/usr/bin/env bash
# 质量门禁：前端生产构建 + 类型检查 + Rust 单元测试。
#
# 顺序不能反：`tauri::generate_context!` 在编译期就会读 frontendDist（../dist），
# 所以必须先生成前端产物，cargo 才能编译 lib/main。
#
# 对应本地检查链：
#   pnpm run build = build-only（Vite 生产构建，顺带生成 auto-imports.d.ts 等声明）
#                    + type-check（vue-tsc），串行执行的原因见 package.json 注释；
#   cargo test     = src-tauri 全部单元测试（config / opener / importer / shortcut / tray）。
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

# 前端：构建 + vue-tsc 类型检查
pnpm run build

# Rust：--locked 保证 Cargo.lock 与 Cargo.toml 一致（依赖变动必须显式提交 lockfile）
cargo test --locked --manifest-path src-tauri/Cargo.toml
