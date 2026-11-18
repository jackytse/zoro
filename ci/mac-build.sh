#!/usr/bin/env bash
# macOS 发布构建：产出 .app 与 .dmg，并做签名与产物校验。
#
# 运行环境：CNB 自建 Mac 节点（宿主机直跑，无 docker）。
#
# 签名策略：
#   - 默认 ad-hoc 签名（codesign -）：无证书也能签，包能在本机与
#     「信任链一致」的设备上直接运行，用于发布链路验证足够；
#   - 节点钥匙串里装了开发证书时，设置环境变量 CODESIGN_IDENTITY
#     （如 "Apple Development: you@example.com (TEAMID)"）即自动改用真证书。
#
# 校验项（失败任意一项即终止）：
#   1. tag_push 场景 tag 名 == tauri.conf.json version；
#   2. .app 生成且 Info.plist 版本一致；
#   3. .app 完成 codesign 且签名可验证；
#   4. .dmg 生成且可被 hdiutil 挂载读取（完整性）；
#   5. 输出体积 + sha256 指纹留档。
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

# 自建 Mac 节点的 HOME 持久：把 release 编译产物放在 HOME 下的固定目录，
# 跨构建复用（工作区每次重新 clone，workspace 内的 target 目录不会留存）。
# cargo 按指纹做增量编译，commit 变化只会重编变动的 crate。
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/zoro-target}"

# ---------- 1. 版本一致性 ----------
VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
REF="${CNB_BRANCH:-unknown}"
echo "构建引用: $REF / 应用版本: $VERSION / 主机: $(uname -m)"

if [[ "$REF" == v* ]]; then
  if [[ "v$VERSION" != "$REF" ]]; then
    echo "tag 与版本号不一致：tag=$REF，tauri.conf.json version=$VERSION。" >&2
    echo "请同步修改 src-tauri/tauri.conf.json 与 package.json 的 version 后重新打 tag。" >&2
    exit 1
  fi
  echo "✓ tag 与应用版本一致"
fi

# ---------- 2. 打包（.app + .dmg） ----------
# tauri build 会先跑 beforeBuildCommand（pnpm build）再编译 release；
# --bundles app,dmg 同时产出应用包与磁盘映像，一次编译不浪费。
# 先清掉 bundle 输出目录：编译缓存卷里会残留旧版本的 deb/dmg/app，
# 不清理的话后面的产物收集（ls | head -1）可能挑中上一次的旧包。
TARGET_ROOT="${CARGO_TARGET_DIR:-src-tauri/target}"
rm -rf "$TARGET_ROOT/release/bundle"
start_at=$SECONDS
pnpm exec tauri build --bundles app,dmg
echo "打包耗时: $((SECONDS - start_at))s"

# .app/.dmg 落盘位置同样跟随 CARGO_TARGET_DIR（自建 Mac 节点用 HOME 缓存卷增量编译）
APP_DIR="$TARGET_ROOT/release/bundle/macos"
DMG_DIR="$TARGET_ROOT/release/bundle/dmg"
APP="$(ls -d "$APP_DIR"/*.app | head -1)"
DMG="$(ls "$DMG_DIR"/*.dmg | head -1)"
echo "产物: $APP"
echo "产物: $DMG"

# ---------- 3. .app 校验 ----------
# Info.plist 里的 CFBundleShortVersionString 必须与应用版本一致，
# 防止「改了配置但打包用了旧缓存」这类静默不一致
PLIST_VER=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")
# 机器架构（arm64 / x86_64），用于附件命名；统一成 aarch64 与 tauri 的命名习惯一致
MAC_ARCH="$(uname -m | sed 's/^x86_64$/amd64/; s/^arm64$/aarch64/')"
if [[ "$PLIST_VER" != "$VERSION" ]]; then
  echo "Info.plist 版本 ($PLIST_VER) 与 tauri.conf.json ($VERSION) 不一致" >&2
  exit 1
fi
echo "✓ Info.plist 版本一致: $PLIST_VER"

# 可执行文件必须存在且可执行（dmg 装出来能不能双击打开的第一道关）
BIN="$APP/Contents/MacOS"
ls "$BIN" | grep -v '\.app' >/dev/null || {
  echo ".app 内找不到可执行文件" >&2
  exit 1
}

# ---------- 4. 签名 ----------
# tauri 的 mac 打包默认不做 codesign；这里统一补一次。
# --force 覆盖已有签名，--deep 连同内嵌 frameworks 一起签（ad-hoc 场景够用）。
IDENTITY="${CODESIGN_IDENTITY:--}"
echo "codesign identity: ${CODESIGN_IDENTITY:-ad-hoc}"
codesign --force --deep --sign "$IDENTITY" "$APP"

# --verify --strict：确认签名结构完整；返回非零直接退出
codesign --verify --strict --verbose=2 "$APP" >/dev/null
echo "✓ codesign 校验通过"

# ---------- 5. .dmg 校验 ----------
# hdiutil imageinfo 能读出映像元数据即说明 dmg 结构完好（比只查文件存在强）
hdiutil imageinfo "$DMG" >/dev/null
echo "✓ dmg 映像完整"

# ---------- 6. 产物指纹留档 ----------
du -h "$APP"
du -h "$DMG"
shasum -a 256 "$DMG"

# ---------- 7. 发布：建 release 并上传附件 ----------
# 仅 tag_push（tag 名形如 vX.Y.Z）时上传；非 tag 构建只验证链路。
# 附件三件套：dmg（主安装包）、app.zip（免挂载解压即用）、dmg 的 sha256 指纹。
if [[ "$REF" == v* ]]; then
  ASSET_DIR="release-assets"
  rm -rf "$ASSET_DIR" && mkdir -p "$ASSET_DIR"
  # 附件名加上操作系统标识（tauri 默认名只有架构）：dmg 名形如
  # zoro_0.1.3_aarch64.dmg → zoro_0.1.3_macos_aarch64.dmg
  DMG_BASE="$(basename "$DMG")"
  ASSET_DMG="$(echo "$DMG_BASE" | sed -E 's/_(aarch64|amd64|arm64)\.dmg$/_macos_\1.dmg/')"
  # zip 命名与 dmg 对齐（带版本 + OS + 架构），不再是笼统的 zoro.app.zip
  ASSET_ZIP="zoro_${PLIST_VER}_macos_${MAC_ARCH}.app.zip"
  # zip 用 ditto 打包：保留 macOS 元数据与代码签名，比 zip 命令更可靠
  ditto -c -k --keepParent "$APP" "$ASSET_DIR/$ASSET_ZIP"
  # dmg 与指纹文件一并收集到 ASSET_DIR，统一上传
  cp "$DMG" "$ASSET_DIR/$ASSET_DMG"
  ( cd "$ASSET_DIR" && shasum -a 256 "$ASSET_DMG" > "$ASSET_DMG.sha256" )
  bash ci/upload-release.sh "$ASSET_DIR"/*
else
  echo "非 tag 构建（$REF），跳过 release 上传"
fi

echo "✓ macOS 发布测试通过（.app / .dmg）"
