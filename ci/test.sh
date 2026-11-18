#!/usr/bin/env bash
# 发布质量门禁：前端构建 + 类型检查 + Rust 单元测试。
#
# 与本地检查链保持一致：
#   - pnpm run build = build-only（Vite 生产构建，顺带生成 auto-imports.d.ts
#     等类型声明）+ type-check（vue-tsc），串行执行的原因见 package.json 注释；
#   - cargo test 跑 src-tauri 全部单元测试（opener / importer / config / shortcut）。
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

pnpm run build
cargo test --manifest-path src-tauri/Cargo.toml
