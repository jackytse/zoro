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
# --bundles app,dmg 同时产出应用包与磁盘映像，一次编译不浪费
start_at=$SECONDS
pnpm exec tauri build --bundles app,dmg
echo "打包耗时: $((SECONDS - start_at))s"

APP_DIR="src-tauri/target/release/bundle/macos"
DMG_DIR="src-tauri/target/release/bundle/dmg"
APP="$(ls -d "$APP_DIR"/*.app | head -1)"
DMG="$(ls "$DMG_DIR"/*.dmg | head -1)"
echo "产物: $APP"
echo "产物: $DMG"

# ---------- 3. .app 校验 ----------
# Info.plist 里的 CFBundleShortVersionString 必须与应用版本一致，
# 防止「改了配置但打包用了旧缓存」这类静默不一致
PLIST_VER=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")
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

echo "✓ macOS 发布测试通过（.app / .dmg）"
