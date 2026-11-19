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

# ---------- 2. 打包（.app + .dmg） ----------
# 应用内自动更新：TAURI_SIGNING_PRIVATE_KEY（CNB 密钥仓库 imports 注入）存在时
# 通过 --config 覆盖开启 createUpdaterArtifacts——macOS 会在 macos bundle 目录
# 额外产出 zoro.app.tar.gz（.app 的 tar 打包）与 .sig 签名，即应用内更新包。
# 密钥未配置时不加覆盖，dmg/app.zip 链路照常（应用内更新无 macOS 条目）。
# 覆盖配置写成临时文件再传 --config <路径>：tauri CLI 的 --bundles 是变长参数，
# 命令尾部追加内联 JSON 会被 --bundles 吞掉（v0.1.5 tag 流水线实测报
# invalid value ... for '--bundles'）；文件路径无空格无引号，拼接安全。
# 签名覆盖（v0.1.10 的「damaged」教训）：tauri.conf.json 默认
# signingIdentity "-"（ad-hoc），让 bundler 在封装 dmg / tar.gz **之前**先
# codesign——此前 dmg/tar.gz 里是未签名 .app，下载后被 Gatekeeper 判损坏。
# 节点钥匙串装有真证书时设 CODESIGN_IDENTITY 覆盖（如 Apple Development）。
BUILD_OVERRIDES="{}"
if [[ -n "${CODESIGN_IDENTITY:-}" ]]; then
  BUILD_OVERRIDES=$(printf '{"bundle":{"macOS":{"signingIdentity":"%s"}}}' "$CODESIGN_IDENTITY")
  echo "使用证书签名: $CODESIGN_IDENTITY"
else
  echo "无 CODESIGN_IDENTITY，使用 tauri.conf.json 的默认 ad-hoc 签名"
fi
if [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  # 两个覆盖合并进同一份 config：--config 可多次传但按深度合并不如一次给全
  BUILD_OVERRIDES=$(python3 -c '
import json, sys
cfg = json.loads(sys.argv[1])
cfg["bundle"]["createUpdaterArtifacts"] = True
print(json.dumps(cfg))
' "$BUILD_OVERRIDES")
  echo "检测到签名密钥，开启应用内更新产物"
else
  echo "未配置 TAURI_SIGNING_PRIVATE_KEY，跳过更新包签名（应用内更新将无 macOS 条目）"
fi
printf '%s' "$BUILD_OVERRIDES" > .tauri-updater-config.json
UPDATER_CONFIG="--config .tauri-updater-config.json"

# tauri build 会先跑 beforeBuildCommand（pnpm build）再编译 release；
# --bundles app,dmg 同时产出应用包与磁盘映像，一次编译不浪费。
# 先清掉 bundle 输出目录：编译缓存卷里会残留旧版本的 deb/dmg/app，
# 不清理的话后面的产物收集（ls | head -1）可能挑中上一次的旧包。
TARGET_ROOT="${CARGO_TARGET_DIR:-src-tauri/target}"
rm -rf "$TARGET_ROOT/release/bundle"
start_at=$SECONDS
# shellcheck disable=SC2086
pnpm exec tauri build --bundles app,dmg $UPDATER_CONFIG
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

# ---------- 4. 签名校验 ----------
# 签名由 bundler 在打包期完成（tauri.conf.json 的 signingIdentity，或上面的
# CODESIGN_IDENTITY 覆盖），发生在 dmg / tar.gz 封装之前——v0.1.10 的教训：
# 此前是打包「后」补签，dmg/tar.gz 里装的是未签名 .app，下载后被
# Gatekeeper 判「damaged」。这里只验证，签名缺失/失效直接终止。
if [[ ! -d "$APP/Contents/_CodeSignature" ]]; then
  echo ".app 缺少 _CodeSignature（bundler 未签名），请检查 signingIdentity 配置" >&2
  exit 1
fi
codesign --verify --strict --verbose=2 "$APP" >/dev/null
echo "✓ codesign 校验通过"

# 热更新包同理：tar.gz 里的 .app 必须已签名（应用内更新替换 /Applications
# 下的 .app 后若签名损坏，同样会触发「damaged」）
UPDATER_TAR_CANDIDATE="$APP_DIR/zoro.app.tar.gz"
if [[ -f "$UPDATER_TAR_CANDIDATE" ]]; then
  TAR_VERIFY_DIR=$(mktemp -d)
  tar -xzf "$UPDATER_TAR_CANDIDATE" -C "$TAR_VERIFY_DIR"
  codesign --verify --strict "$TAR_VERIFY_DIR"/*.app >/dev/null
  rm -rf "$TAR_VERIFY_DIR"
  echo "✓ 热更新包内 .app 签名有效"
fi

# ---------- 5. .dmg 校验 ----------
# hdiutil imageinfo 能读出映像元数据即说明 dmg 结构完好（比只查文件存在强）
hdiutil imageinfo "$DMG" >/dev/null
echo "✓ dmg 映像完整"

# dmg 内容签名验证：挂载镜像，验证里面 .app 的签名——这是用户「下载 dmg
# → 双击安装」的真实路径，外层 .app 校验通过不代表 dmg 里那份没被动过
DMG_VERIFY_DIR=$(mktemp -d)
hdiutil attach -nobrowse -readonly -mountpoint "$DMG_VERIFY_DIR" "$DMG" >/dev/null
codesign --verify --strict "$DMG_VERIFY_DIR"/*.app >/dev/null
hdiutil detach "$DMG_VERIFY_DIR" >/dev/null
rm -rf "$DMG_VERIFY_DIR"
echo "✓ dmg 内 .app 签名有效"

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
  # zoro_0.1.8_aarch64.dmg → zoro-0.1.8-macos-aarch64.dmg（统一短横线连接）
  DMG_BASE="$(basename "$DMG")"
  ASSET_DMG="$(echo "$DMG_BASE" | sed -E 's/_(aarch64|amd64|arm64)\.dmg$/-macos-\1.dmg/' | tr '_' '-')"
  # zip 命名与 dmg 对齐（带版本 + OS + 架构），不再是笼统的 zoro.app.zip
  ASSET_ZIP="zoro-${PLIST_VER}-macos-${MAC_ARCH}.app.zip"
  # zip 用 ditto 打包：保留 macOS 元数据与代码签名，比 zip 命令更可靠
  ditto -c -k --keepParent "$APP" "$ASSET_DIR/$ASSET_ZIP"
  # dmg 与指纹文件一并收集到 ASSET_DIR，统一上传
  cp "$DMG" "$ASSET_DIR/$ASSET_DMG"
  ( cd "$ASSET_DIR" && shasum -a 256 "$ASSET_DMG" > "$ASSET_DMG.sha256" )
  # 更新包（.app.tar.gz + .sig）一并上传，upload-release.sh 据此生成
  # latest.json 的 darwin-aarch64 条目；仅开启 createUpdaterArtifacts 后才有
  UPDATER_TAR="$APP_DIR/zoro.app.tar.gz"
  if [[ -f "$UPDATER_TAR" && -f "$UPDATER_TAR.sig" ]]; then
    ASSET_TAR="zoro-${PLIST_VER}-macos-${MAC_ARCH}.app.tar.gz"
    cp "$UPDATER_TAR" "$ASSET_DIR/$ASSET_TAR"
    cp "$UPDATER_TAR.sig" "$ASSET_DIR/$ASSET_TAR.sig"
  else
    echo "WARN: 未找到应用内更新包（$UPDATER_TAR），跳过 macOS 更新条目" >&2
  fi
  bash ci/upload-release.sh "$ASSET_DIR"/*
else
  echo "非 tag 构建（$REF），跳过 release 上传"
fi

echo "✓ macOS 发布测试通过（.app / .dmg）"
