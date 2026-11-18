#!/usr/bin/env bash
# CI 环境准备脚本（CNB 流水线专用，配合 .cnb.yml 使用）。
#
# 基础镜像 node:22-bookworm 已带 Node/npm，本脚本补齐其余工具链：
#   1. pnpm —— 项目使用的包管理器；
#   2. Rust —— src-tauri 桌面壳的编译工具链（rustup minimal profile 最快）；
#   3. Tauri 在 Linux 上编译所需的系统库（官方 Debian 依赖清单）；
#   4. 前端依赖 —— 用 frozen 模式保证 lockfile 与 package.json 严格一致。
#
# /root/.rustup 与 /root/.cargo 已在 .cnb.yml 挂 copy-on-write 缓存卷，
# 第二次运行时 rustup 安装步骤几乎瞬时完成。
set -euo pipefail

# ---------- 1. pnpm ----------
# 直接 npm 全局安装，不走 corepack（避免其额外的签名校验网络请求）
if ! command -v pnpm >/dev/null 2>&1; then
  npm install -g pnpm@latest
fi
pnpm --version

# ---------- 2. Rust 工具链 ----------
if ! command -v cargo >/dev/null 2>&1; then
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
fi
# rustup 装在 $HOME/.cargo，每个 script 都是全新 shell，须显式加 PATH
export PATH="$HOME/.cargo/bin:$PATH"
rustc --version && cargo --version

# ---------- 3. Tauri 的 Linux 系统依赖 ----------
# cargo test / tauri build 都要链接 webkit2gtk 与 gtk，缺一个都编译不过
apt-get update -qq
apt-get install -y -qq \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev

# ---------- 4. 前端依赖 ----------
# frozen-lockfile：lockfile 与 package.json 不同步时直接失败，防止"本地能装 CI 装不上"
pnpm install --frozen-lockfile
