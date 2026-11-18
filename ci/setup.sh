#!/usr/bin/env bash
# CI 环境准备脚本（CNB 流水线专用，配合 .cnb.yml 使用）。
#
# 基础镜像 node:22-bookworm 自带 Node 与 apt，其余工具链在这里补齐：
#   0. Node 版本前置校验 —— 项目 engines 要求 ^22.18.0 || >=24.12.0，
#      镜像自带的 Node 如果太旧，后面 vite-plus 的原生绑定会装不上，
#      与其在编译阶段报一堆难懂的错，不如一开始就明确失败；
#   1. pnpm —— 版本锁死在本地开发用的同一版，避免 --frozen-lockfile
#      因 pnpm 主版本升级导致的 lockfile 格式不兼容；
#   2. Rust —— tauri 桌面壳的编译工具链（rustup minimal profile 最快）；
#   3. Tauri 在 Linux 上编译/打包所需的系统库（官方 Debian 依赖清单）；
#   4. 前端依赖 —— frozen 模式保证 lockfile 与 package.json 严格一致。
#
# /root/.rustup、/root/.cargo、/root/.local/share/pnpm 已在 .cnb.yml 挂
# copy-on-write 缓存卷，二次运行时工具链与 pnpm store 都直接命中缓存。
set -euo pipefail

# ---------- 0. Node 版本校验 ----------
echo "Node: $(node -v) / npm: $(npm -v)"
NODE_MAJOR="$(node -p 'process.versions.node.split(".")[0]')"
NODE_MINOR="$(node -p 'process.versions.node.split(".")[1]')"
# 与 package.json 的 engines 字段保持一致：^22.18.0 || >=24.12.0
if [ "$NODE_MAJOR" -lt 22 ] || { [ "$NODE_MAJOR" -eq 22 ] && [ "$NODE_MINOR" -lt 18 ]; }; then
  echo "基础镜像的 Node $(node -v) 低于项目要求（^22.18.0 || >=24.12.0），请升级 .cnb.yml 里的镜像" >&2
  exit 1
fi

# ---------- 1. pnpm ----------
# 固定版本而不是 @latest：lockfile 格式与 pnpm 主版本强相关，
# 让 CI 用和本地完全一致的 pnpm 才能真正保证「本地能装 CI 也能装」
PNPM_VERSION="12.21.0"
if ! command -v pnpm >/dev/null 2>&1 || [ "$(pnpm -v)" != "$PNPM_VERSION" ]; then
  npm install -g "pnpm@${PNPM_VERSION}"
fi
echo "pnpm: $(pnpm -v)"

# ---------- 2. Rust 工具链 ----------
if ! command -v cargo >/dev/null 2>&1; then
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
fi
# rustup 装在 $HOME/.cargo；每个 stage 都是全新 shell，必须显式加 PATH
export PATH="$HOME/.cargo/bin:$PATH"
rustc --version && cargo --version

# ---------- 3. Tauri 的 Linux 系统依赖 ----------
# cargo test / tauri build 都要链接 webkit2gtk 与 gtk，缺一个都编译不过；
# libayatana-appindicator3 是托盘图标（Tauri tray-icon）在 Linux 上的依赖。
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
  libwebkit2gtk-4.1-dev \
  build-essential \
  pkg-config \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev

# ---------- 4. 前端依赖 ----------
# --frozen-lockfile：lockfile 与 package.json 不同步时立即失败；
# --prefer-offline：优先用缓存卷里的 pnpm store，减少网络抖动影响
pnpm install --frozen-lockfile --prefer-offline
# store 路径打印出来，便于确认缓存卷确实生效
echo "pnpm store: $(pnpm store path)"
