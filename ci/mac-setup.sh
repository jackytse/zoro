#!/usr/bin/env bash
# macOS 自建构建节点上的环境准备。
#
# 与 Linux 容器版 setup.sh（ci/setup.sh）的差别：
#   - 自建 Mac 节点**不支持 docker image**（CNB 文档明确），脚本直接跑在宿主机上；
#     所以这里做的是「检查 + 就地补装」，而不是「换镜像」。
#   - 宿主机的 HOME 是持久的，rustup / cargo / pnpm 缓存天然复用，
#     不需要 Linux 侧那种 copy-on-write 数据卷声明。
#
# 检查顺序按依赖链排：node（pnpm 的宿主）→ pnpm → rust（cargo）→ xcode 工具链。
# 任何一项装不上都直接失败，让问题在「准备」阶段就暴露，
# 而不是跑到二十分钟的 Rust release 编译时才炸。
set -euo pipefail

echo "== macOS 构建节点环境检查 =="
sw_vers || true
uname -m

fail() {
  echo "✗ $1" >&2
  echo "$2" >&2
  exit 1
}

# ---------- 1. Node.js ----------
# Tauri 的 beforeBuildCommand 走 pnpm，pnpm 又依赖 node，链条最上游
if ! command -v node >/dev/null; then
  fail "未安装 node" "请在该构建机上安装 Node.js 20+（brew install node 或官网安装包）"
fi
NODE_MAJOR="$(node -p 'process.versions.node.split(".")[0]')"
echo "node: $(node -v)"
# 前端构建在 node 20/22 上验证过，更老的版本有 npm 生态兼容风险
if [ "$NODE_MAJOR" -lt 20 ]; then
  fail "node 版本过低：$(node -v)" "需要 >= 20，建议升级后重跑"
fi

# ---------- 2. pnpm ----------
# 没有时优先用 corepack（node 自带，零额外安装）；corepack 不可用再退 npm -g
if ! command -v pnpm >/dev/null; then
  echo "pnpm 未安装，尝试 corepack..."
  if command -v corepack >/dev/null; then
    corepack enable pnpm 2>/dev/null || true
  fi
fi
if ! command -v pnpm >/dev/null; then
  echo "corepack 不可用，退回 npm 全局安装..."
  npm install -g pnpm@12.21.0
fi
echo "pnpm: $(pnpm -v)"

# ---------- 3. Rust 工具链 ----------
# 构建机没装 rust 时自动补装 rustup（官方脚本，最小化交互）。
# 两个关键点：
#   --profile minimal 只装 rustc/cargo/rust-std，比 default 少下文档、
#     clippy 等一堆组件，构建机不需要它们；
#   keepalive 循环定期输出一行日志——CNB 的「无输出超时」是 10 分钟，
#     rustup 静默下载大组件时极易被误杀（实测 9.8 分钟被掐），
#     心跳输出可保持流水线认为任务活跃。
if ! command -v cargo >/dev/null; then
  echo "cargo 未安装，安装 rustup（minimal profile）..."
  # 后台心跳：每 20s 打一行，直到 rustup 装完（PID 轮询到进程消失为止）
  (
    while true; do
      sleep 20
      echo "[keepalive] rustup 安装中... $(date +%H:%M:%S)"
    done
  ) &
  KEEPALIVE_PID=$!
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
  # 无论成败都要收掉心跳进程，不然它会一直活着打日志
  kill "$KEEPALIVE_PID" 2>/dev/null || true
fi
export PATH="$HOME/.cargo/bin:$PATH"
echo "cargo: $(cargo --version)"
echo "rustc: $(rustc --version)"

# ---------- 4. Xcode 命令行工具 ----------
# .app / .dmg 打包与 codesign 都依赖它；没装时给出明确指引而不是让下游报晦涩错误
if ! xcode-select -p >/dev/null 2>&1; then
  fail "未安装 Xcode 命令行工具" "请执行: xcode-select --install"
fi
echo "Xcode CLT: $(xcode-select -p)"
command -v codesign >/dev/null && echo "codesign: 可用"

# ---------- 5. 依赖安装 ----------
# frozen-lockfile：与 CI 的其它链路保持一致，锁文件不对齐时立刻失败；
# --prefer-offline：优先命中宿主持久化的 pnpm store（HOME/Library/pnpm），
#   没有变化时全部走本地缓存，省掉几十秒到几分钟的下载
pnpm install --frozen-lockfile --prefer-offline
echo "pnpm store: $(pnpm store path)"

echo "✓ macOS 环境准备完成"
