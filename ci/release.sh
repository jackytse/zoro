#!/usr/bin/env bash
# 发布测试：打桌面安装包，并校验「版本号 / 产物完整性」。
#
# 为什么只打 deb：
#   - CNB Runner 是 Linux 容器，macOS 的 .app/.dmg 出不来（需要 mac 机器）；
#   - AppImage 打包要联网下载 linuxdeploy 与 fuse 运行时，CI 上不稳定；
#   - deb 用容器里自带的 dpkg-deb 就能生成，速度快且完全可离线。
# 所以这一步的目标不是「产出可下载的正式安装包」，而是证明发布链路
# （前端构建 → Rust release 编译 → 打包 → 版本一致性）在干净环境里能跑通。
#
# 校验项：
#   1. tag 名与 tauri.conf.json 的 version 一致（只在 tag_push 场景校验）；
#   2. deb 确实生成，且能读出 Package/Version/Architecture 元信息；
#   3. 包内包含可执行文件与 .desktop 入口（能被桌面环境识别的必要条件）；
#   4. 输出 sha256，作为产物指纹留档在构建日志里。
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

# release 编译产物也落数据卷：与 test.sh 的 debug 产物共存于同一 target 目录
# （cargo 按 profile 分目录存放），tag 发布二次构建只做增量编译
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/root/zoro-target}"

# ---------- 1. 版本一致性 ----------
VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
# tag_push 事件里 CNB_BRANCH 就是 tag 名；分支触发时是分支名
REF="${CNB_BRANCH:-unknown}"
echo "构建引用: $REF / 应用版本: $VERSION"

if [[ "$REF" == v* ]]; then
  if [[ "v$VERSION" != "$REF" ]]; then
    echo "tag 与版本号不一致：tag=$REF，tauri.conf.json version=$VERSION。" >&2
    echo "请同步修改 src-tauri/tauri.conf.json 与 package.json 的 version 后重新打 tag。" >&2
    exit 1
  fi
  echo "✓ tag 与应用版本一致"
fi

# ---------- 2. 打包 ----------
# tauri build 会先跑 beforeBuildCommand（pnpm build），再编译 release 产物
start_at=$SECONDS
pnpm exec tauri build --bundles deb
echo "打包耗时: $((SECONDS - start_at))s"

# ---------- 3. 产物校验 ----------
DEB_DIR="src-tauri/target/release/bundle/deb"
DEB="$(ls "$DEB_DIR"/*.deb | head -1)"
echo "产物: $DEB"

# 元信息必须可读，否则说明 deb 结构已损坏
dpkg-deb -f "$DEB" Package Version Architecture

# 包内必须同时有可执行文件与桌面入口，否则装上也没法启动
dpkg-deb -c "$DEB" | grep -E 'usr/bin/|\.desktop' || {
  echo "deb 内缺少可执行文件或 .desktop 入口" >&2
  exit 1
}

# 体积与指纹：日志里留档，便于对比不同版本的产物是否异常
du -h "$DEB"
sha256sum "$DEB"

echo "✓ 发布测试通过（Linux deb 链路）"
echo "提示：macOS 安装包需在 mac 上执行 pnpm desktop:build 产出 .app/.dmg"
