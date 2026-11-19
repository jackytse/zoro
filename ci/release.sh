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

# updater-manifest 是热更新清单承载 tag（见 upload-release.sh finalize），
# 不是版本 tag——CI 维护清单时若触发了 tag_push 流水线，直接跳过构建
if [[ "$REF" == "updater-manifest" ]]; then
  echo "updater-manifest 为热更新清单 tag，跳过应用构建"
  exit 0
fi

if [[ "$REF" == v* ]]; then
  if [[ "v$VERSION" != "$REF" ]]; then
    echo "tag 与版本号不一致：tag=$REF，tauri.conf.json version=$VERSION。" >&2
    echo "请同步修改 src-tauri/tauri.conf.json 与 package.json 的 version 后重新打 tag。" >&2
    exit 1
  fi
  echo "✓ tag 与应用版本一致"
fi

# ---------- 2. 打包 ----------
# 应用内自动更新：TAURI_SIGNING_PRIVATE_KEY（CNB 密钥仓库 imports 注入）存在时，
# 通过 --config 覆盖开启 createUpdaterArtifacts，让 tauri 产出带 minisign
# 签名的更新包（Linux 更新包 = AppImage + .sig）。密钥未配置时不加覆盖，
# 纯 deb 链路照常工作——本地构建也不需要签名密钥。
# 注意：覆盖配置必须写成临时文件再传 --config <路径>。不能在命令尾部追加
# 内联 JSON——tauri CLI 的 --bundles 是变长参数，裸 JSON 会被它当作 bundle
# 名吞掉（v0.1.5 tag 流水线实测报 invalid value ... for '--bundles'）；
# 文件路径无空格无引号，三种 shell 拼接都安全。
UPDATER_CONFIG=""
if [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  echo '{"bundle":{"createUpdaterArtifacts":true}}' > .tauri-updater-config.json
  UPDATER_CONFIG="--config .tauri-updater-config.json"
  echo "检测到签名密钥，开启应用内更新产物"
else
  echo "未配置 TAURI_SIGNING_PRIVATE_KEY，跳过更新包签名（应用内更新将无 Linux 条目）"
fi

# tauri build 会先跑 beforeBuildCommand（pnpm build），再编译 release 产物。
# 先清掉 bundle 输出目录：编译缓存卷里会残留旧版本的 deb/appimage，
# 不清理的话后面的产物收集（ls | head -1）可能挑中上一次的旧包。
TARGET_ROOT="${CARGO_TARGET_DIR:-src-tauri/target}"
rm -rf "$TARGET_ROOT/release/bundle"
start_at=$SECONDS
# shellcheck disable=SC2086
pnpm exec tauri build --bundles deb $UPDATER_CONFIG
echo "打包耗时: $((SECONDS - start_at))s"

# AppImage（更新包载体）：联网下载 linuxdeploy 在 CI 上偶发失败，
# 所以单独一次构建、尽力而为——失败只 WARN，deb 主产物不受影响
APPIMAGE=""
if [[ -n "$UPDATER_CONFIG" ]]; then
  if pnpm exec tauri build --bundles appimage $UPDATER_CONFIG; then
    APPIMAGE="$(ls "$TARGET_ROOT/release/bundle/appimage"/*.AppImage 2>/dev/null | head -1)"
    echo "产物: $APPIMAGE"
  else
    echo "WARN: AppImage 打包失败（linuxdeploy 下载不稳定？），应用内更新将无 Linux 条目" >&2
  fi
fi

# ---------- 3. 产物校验 ----------
# deb 落盘位置跟随 CARGO_TARGET_DIR：设置了缓存卷时 bundle 输出在
# $CARGO_TARGET_DIR/release/bundle/ 下，而不是工作区内的 src-tauri/target/
DEB_DIR="$TARGET_ROOT/release/bundle/deb"
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

# ---------- 4. 发布：建 release 并上传附件 ----------
# 仅 tag_push（tag 名形如 vX.Y.Z）时上传；手动 api_trigger_release 跑分支
# 时不发布，只验证链路。脚本内部幂等：release 已存在则复用，附件同名覆盖。
if [[ "$REF" == v* ]]; then
  ASSET_DIR="release-assets"
  rm -rf "$ASSET_DIR" && mkdir -p "$ASSET_DIR"
  # 附件名加上操作系统标识（tauri 默认名只有架构）：同一 release 页里
  # linux/macos 产物并排时一眼可辨。统一短横线连接：zoro-0.1.8-linux-amd64.deb
  DEB_BASE="$(basename "$DEB")"
  ASSET_DEB="$(echo "$DEB_BASE" | sed -E 's/_(amd64|arm64|aarch64)\.deb$/-linux-\1.deb/' | tr '_' '-')"
  cp "$DEB" "$ASSET_DIR/$ASSET_DEB"
  # sha256 指纹文件与 deb 同名，方便下载后校验
  ( cd "$ASSET_DIR" && sha256sum "$ASSET_DEB" > "$ASSET_DEB.sha256" )
  # AppImage 与 .sig 一并上传（update-release.sh 会据此生成更新清单 latest.json）
  if [[ -n "$APPIMAGE" ]]; then
    AIMAGE_BASE="$(basename "$APPIMAGE")"
    ASSET_IMAGE="$(echo "$AIMAGE_BASE" | sed -E 's/_(amd64|arm64|aarch64)\.AppImage$/-linux-\1.AppImage/' | tr '_' '-')"
    cp "$APPIMAGE" "$ASSET_DIR/$ASSET_IMAGE"
    cp "${APPIMAGE}.sig" "$ASSET_DIR/$ASSET_IMAGE.sig"
  fi
  bash ci/upload-release.sh "$ASSET_DIR"/*
else
  echo "非 tag 构建（$REF），跳过 release 上传"
fi

echo "✓ 发布测试通过（Linux deb 链路）"
echo "提示：macOS 安装包需在 mac 上执行 pnpm desktop:build 产出 .app/.dmg"
