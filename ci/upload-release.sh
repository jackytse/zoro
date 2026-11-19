#!/usr/bin/env bash
# 发布产物上传：确保当前 tag 存在 release，并把文件上传为 release 附件。
#
# 用法: ci/upload-release.sh <文件...>
#
# 运行环境：
#   - CNB 流水线（内置 CNB_TOKEN / CNB_API_ENDPOINT / CNB_REPO_SLUG / CNB_BRANCH）；
#   - 本地调试时可通过环境变量自行提供上述四项（token 用个人访问令牌）。
#
# 设计说明：
#   - CNB 自建 Mac 节点不支持 docker image 插件（cnbcool/attachments 跑不了），
#     所以 Linux 与 mac 统一走 OpenAPI + curl，脚本对两端环境零依赖
#     （只需 curl + node + git，三端都已具备）；
#   - 附件同名时 overwrite=true 覆盖，重跑流水线幂等；
#   - release 已存在时直接复用（GET 404 才 POST 创建），重跑不会报冲突；
#   - 所有请求显式检查状态码并在失败时打印响应体：CI 日志里能直接看到
#     4xx/5xx 的原因，而不是 curl -f 静默退出码；
#   - 附件上传完会后尝试「更新清单 finalize」：从 release 附件里收集各平台
#     更新包的 .sig 签名，按通道生成更新清单，覆盖上传到**固定 tag
#     「updater-manifest」的 Release**（不存在则自动创建）——应用内 updater
#     分「正式版 / 预发布」双通道（见 src-tauri/src/updater.rs），端点分别为
#     https://cnb.cool/<repo>/-/releases/download/updater-manifest/latest.json（正式版）
#     https://cnb.cool/<repo>/-/releases/download/updater-manifest/latest-beta.json（预发布）。
#     每次发版覆盖同名附件即完成清单更新，客户端端点永不变化
#     （与 wocnb 的热更新链路同构；releases/download 附件地址匿名可下载）。
#     三平台流水线并行，谁最后完成谁写入最全的清单；单平台失败时清单里
#     缺该平台条目，对应平台用户表现为「无更新」，不影响其他平台升级。
set -euo pipefail

API_BASE="${CNB_API_ENDPOINT:-https://api.cnb.cool}"
API_BASE="${API_BASE%/}"
REPO_SLUG="${CNB_REPO_SLUG:?缺少 CNB_REPO_SLUG（流水线内置，本地调试请自行导出）}"
TAG="${CNB_BRANCH:?缺少 CNB_BRANCH（tag_push 事件下即 tag 名）}"
AUTH_HEADER="Authorization: Bearer ${CNB_TOKEN:?缺少 CNB_TOKEN（流水线内置，本地调试请自行导出）}"
# OpenAPI 要求显式 JSON Accept，缺失时会返回 406
ACCEPT_HEADER="Accept: application/json"

RELEASE_API="$API_BASE/$REPO_SLUG/-/releases"

# 临时响应文件：node 解析 JSON 时读这个文件，避免把长 JSON 塞进命令行参数
RESP_FILE="$(mktemp)"
trap 'rm -f "$RESP_FILE"' EXIT
# Windows git-bash 下 mktemp 给出 POSIX 风格路径（/tmp/xxx），而脚本里的
# node.exe 是 Windows 原生程序，会把它解析成 C:\tmp\xxx 导致 ENOENT；
# 用 cygpath -m 转成 C:/xxx 形式（正斜杠可安全嵌进 JS 字符串）。
# cygpath 只在 git-bash 里存在，Linux/mac 上该行自动跳过
command -v cygpath >/dev/null 2>&1 && RESP_FILE="$(cygpath -m "$RESP_FILE")"

# 非 2xx 时打印状态码与响应体并退出——CI 日志里能直接定位原因
die_on_http_error() {
  local code="$1" what="$2"
  if [[ ! "$code" =~ ^2 ]]; then
    echo "✗ ${what} 失败（HTTP ${code}），响应：" >&2
    cat "$RESP_FILE" >&2
    echo "" >&2
    exit 1
  fi
}

# ---------- 1. 确保 release 存在（已存在则复用，不存在则创建） ----------
ensure_release() {
  local code
  code="$(curl -s --retry 5 --retry-all-errors --retry-delay 2 -o "$RESP_FILE" -w '%{http_code}' -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" "$RELEASE_API/tags/$TAG")"

  if [[ "$code" == "200" ]]; then
    RELEASE_ID="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).id")"
    echo "✓ release 已存在（id=${RELEASE_ID}），直接复用"
    return
  fi

  # 404 = 该 tag 还没有 release，走创建；其余状态码才是真正的错误
  if [[ "$code" != "404" ]]; then
    die_on_http_error "$code" "查询 release"
  fi

  # 创建 release：标题即 tag 名，置为最新版本；描述给一个可编辑的兜底文案
  local body
  body=$(node -e '
    const tag = process.argv[1];
    process.stdout.write(JSON.stringify({
      tag_name: tag,
      name: tag,
      body: "## " + tag + "\n\n正式发布版本。变更内容详见仓库提交历史；安装包见下方 Assets。",
      make_latest: "true",
      draft: false,
      prerelease: false,
    }));
  ' "$TAG")
  code="$(curl -s --retry 5 --retry-all-errors --retry-delay 2 -o "$RESP_FILE" -w '%{http_code}' -X POST \
    -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" -H "Content-Type: application/json" \
    -d "$body" "$RELEASE_API")"
  die_on_http_error "$code" "创建 release"
  RELEASE_ID="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).id")"
  echo "✓ release 已创建（id=${RELEASE_ID}）"
}

# ---------- 2. 上传单个附件（同名覆盖，幂等） ----------
upload_asset() {
  local file="$1"
  local name size
  name="$(basename "$file")"

  # 兼容 GNU stat（Linux 容器）与 BSD stat（macOS 宿主机）
  if stat -c%s "$file" >/dev/null 2>&1; then
    size="$(stat -c%s "$file")"
  else
    size="$(stat -f%z "$file")"
  fi

  # 2.1 申请预签名上传地址
  local payload code
  payload=$(node -e '
    process.stdout.write(JSON.stringify({
      asset_name: process.argv[1],
      size: Number(process.argv[2]),
      overwrite: true,
    }));
  ' "${name}" "${size}")
  code="$(curl -s --retry 5 --retry-all-errors --retry-delay 2 -o "$RESP_FILE" -w '%{http_code}' -X POST \
    -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" -H "Content-Type: application/json" \
    -d "$payload" "$RELEASE_API/$RELEASE_ID/asset-upload-url")"
  die_on_http_error "$code" "申请上传地址（${name}）"

  local upload_url verify_url
  upload_url="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).upload_url")"
  verify_url="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).verify_url")"

  # 2.2 预签名地址直传文件（无需鉴权头）；失败打印状态码便于排查代理/网络问题
  code="$(curl -s --retry 5 --retry-all-errors --retry-delay 2 -o "$RESP_FILE" -w '%{http_code}' -X PUT --upload-file "$file" "$upload_url")"
  die_on_http_error "$code" "上传附件（${name}）"

  # 2.3 确认上传（verify_url 即确认端点，带鉴权调一次即入库）
  code="$(curl -s --retry 5 --retry-all-errors --retry-delay 2 -o "$RESP_FILE" -w '%{http_code}' -X POST -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" "$verify_url")"
  die_on_http_error "$code" "确认上传（${name}）"

  echo "✓ 附件已上传: ${name} ($(( size / 1024 / 1024 ))M)"
}

# ---------- 主流程 ----------
if [[ $# -eq 0 ]]; then
  echo "用法: $0 <要上传的文件...>" >&2
  exit 1
fi

ensure_release
for f in "$@"; do
  [[ -f "$f" ]] || { echo "附件不存在: $f" >&2; exit 1; }
  upload_asset "$f"
done

echo "✓ 全部附件上传完成: https://cnb.cool/$REPO_SLUG/-/releases/tag/$TAG"

# ---------- 4. 更新清单 finalize：生成 latest.json 并覆盖上传到 updater-manifest ----------
# 说明见文件头部；任何失败只 WARN 不阻塞——发布主链路（release 附件）已完成，
# 更新清单是增强能力，不能因为它把整条发布流水线判红。
finalize_updater_manifest() {
  # ---- 4.1 下载本 release 全部附件里的 .sig 签名文件 ----
  local SIG_DIR
  SIG_DIR="$(mktemp -d)"
  # Windows git-bash：node.exe 是原生程序，会把 POSIX 路径 /tmp/... 解析成
  # C:\tmp\...（与 git-bash 的 /tmp 不是同一目录）；统一转成 C:/ 形式，
  # git-bash 工具链（curl/cp/git）同样接受该形式，三端通用
  command -v cygpath >/dev/null 2>&1 && SIG_DIR="$(cygpath -m "$SIG_DIR")"
  local sig_assets
  code="$(curl -s --retry 5 --retry-all-errors --retry-delay 2 -o "$RESP_FILE" -w '%{http_code}' -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" "$RELEASE_API/$RELEASE_ID")"
  if [[ ! "$code" =~ ^2 ]]; then
    echo "WARN: finalize 跳过（读取 release 附件列表失败 HTTP $code）" >&2
    rm -rf "$SIG_DIR"; return 0
  fi
  node -e '
    const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    for (const a of (r.assets || [])) {
      if (a.path && a.path.endsWith(".sig")) console.log(a.path);
    }
  ' "$RESP_FILE" > "$SIG_DIR/list.txt"

  local count=0
  while IFS= read -r p; do
    [[ -z "$p" ]] && continue
    # 附件下载走 api.cnb.cool 域名（响应里给的就是该域名），需带鉴权头；
    # -L 跟随重定向：该接口会 302 跳到 asset.cnb.cool 的预签名地址
    if curl -sL --retry 5 --retry-all-errors --retry-delay 2 -o "$SIG_DIR/$(basename "$p")" -w '%{http_code}' -H "$AUTH_HEADER" \
        "$API_BASE$p" | grep -q '^2'; then
      count=$((count + 1))
    fi
  done < "$SIG_DIR/list.txt"
  echo "finalize: 下载到 $count 个签名文件"

  # ---- 4.2 组装本 release 的清单体（双通道共用同一份平台数据） ----
  # URL 统一写 cnb.cool 域名：匿名可下载（api.cnb.cool 需 token，客户端没有）
  # 通道规则（与 src-tauri/src/updater.rs 的通道端点对应）：
  #   - 正式版通道 → latest.json
  #   - 预发布通道 → latest-beta.json
  #   - 正式 tag（vX.Y.Z）：两个清单都写成本 release——预发布用户在
  #     semver 下只会「升」到更高的正式版，不会被动回退；
  #   - 预发布 tag（含连字符，如 v0.1.6-beta.1）：只写预发布清单，
  #     避免预发布版本污染正式版通道。
  local OUT="$SIG_DIR/release-meta.json"
  node -e '
    const fs = require("fs");
    const dir = process.argv[1], tag = process.argv[2], slug = process.argv[3];
    const base = "https://cnb.cool/" + slug + "/-/releases/download/" + tag;
    // 只下载过 .sig 签名，包本体不在临时目录——所以从签名文件名反推包名
    // （tauri 的签名文件就是包文件名 + ".sig"），再按平台后缀归类
    const sigs = fs.readdirSync(dir).filter((f) => f.endsWith(".sig"));
    // Linux 唯一支持应用内更新的 bundle 是 AppImage；mac 是 .app.tar.gz；
    // Windows 是 NSIS setup.exe（msi 面向企业部署，不进更新链路）
    const candidates = [
      ["linux-x86_64", ".AppImage"],
      ["darwin-aarch64", ".app.tar.gz"],
      ["windows-x86_64", "-setup.exe"],
    ];
    const platforms = {};
    for (const [platform, suffix] of candidates) {
      // 包名 = 签名文件名去掉 .sig 后缀；防前缀混淆取最长匹配
      const pkg = sigs
        .map((s) => s.slice(0, -".sig".length))
        .filter((n) => n.endsWith(suffix))
        .sort((a, b) => b.length - a.length)[0];
      if (pkg) platforms[platform] = {
        signature: fs.readFileSync(dir + "/" + pkg + ".sig", "utf8").trim(),
        url: base + "/" + pkg,
      };
    }
    const version = tag.replace(/^v/, "");
    const meta = {
      version,
      notes: "zoro " + tag,
      pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"),
      platforms,
    };
    fs.writeFileSync(dir + "/release-meta.json", JSON.stringify(meta, null, 2));
    console.log(JSON.stringify(meta.platforms, null, 2));
  ' "$SIG_DIR" "$TAG" "$REPO_SLUG" || {
    echo "WARN: finalize 跳过（更新清单组装失败）" >&2
    rm -rf "$SIG_DIR"; return 0
  }

  if [[ "$(node -p "Object.keys(JSON.parse(require('fs').readFileSync('$OUT')).platforms).length")" -eq 0 ]]; then
    echo "WARN: finalize 跳过（无任何平台更新包，通常意味着签名密钥未配置或打包失败）" >&2
    rm -rf "$SIG_DIR"; return 0
  fi

  # ---- 4.3 覆盖上传到固定 tag「updater-manifest」的 Release ----
  # 清单附件化（参考 wocnb 热更新链路）：固定 tag 的 Release 充当「latest」
  # 角色，每次发版 overwrite 上传同名附件；客户端端点
  # /-/releases/download/updater-manifest/<清单名> 永不变化且匿名可下载。
  # 通道规则：
  #   - 预发布 tag（含连字符，如 v0.1.6-beta.1）：只覆盖 latest-beta.json，
  #     正式通道清单不被预发布数据污染；
  #   - 正式 tag（vX.Y.Z）：两个清单都指向本 release——预发布用户在 semver
  #     下只会「升」到更高的正式版，不会被动回退。
  local UPDATER_TAG="updater-manifest"
  local code
  code="$(curl -s --retry 5 --retry-all-errors --retry-delay 2 -o "$RESP_FILE" -w '%{http_code}' -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" "$RELEASE_API/tags/$UPDATER_TAG")"
  if [[ "$code" == "200" ]]; then
    RELEASE_ID="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).id")"
    echo "✓ updater-manifest release 已存在（id=${RELEASE_ID}），覆盖上传"
  elif [[ "$code" == "404" ]]; then
    # 首次发布：创建清单承载 Release（tag 不存在时 CNB 会随 release 自动建 tag）。
    # make_latest=false 避免清单 Release 抢走仓库「最新发布」入口；
    # prerelease=false 保持正式身份（它不是预发布，只是清单载体）
    local body
    body='{"tag_name":"updater-manifest","name":"updater-manifest","body":"应用内热更新清单（CI 自动维护，请勿手动编辑）。latest.json = 正式版通道；latest-beta.json = 预发布通道。","make_latest":"false","draft":false,"prerelease":false}'
    code="$(curl -s --retry 5 --retry-all-errors --retry-delay 2 -o "$RESP_FILE" -w '%{http_code}' -X POST \
      -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" -H "Content-Type: application/json" \
      -d "$body" "$RELEASE_API")"
    if [[ ! "$code" =~ ^2 ]]; then
      echo "WARN: finalize 跳过（创建 updater-manifest release 失败 HTTP $code）" >&2
      cat "$RESP_FILE" >&2 || true
      rm -rf "$SIG_DIR"; return 0
    fi
    RELEASE_ID="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).id")"
    echo "✓ updater-manifest release 已创建（id=${RELEASE_ID}）"
  else
    echo "WARN: finalize 跳过（查询 updater-manifest release 失败 HTTP $code）" >&2
    rm -rf "$SIG_DIR"; return 0
  fi

  # overwrite=true：同名附件覆盖（upload_asset 内部已处理），重跑流水线幂等。
  # 注意附件名必须叫 latest.json / latest-beta.json（客户端端点固定），
  # 先 cp 成目标名字再上传
  if [[ "$TAG" == *-* ]]; then
    cp "$OUT" "$SIG_DIR/latest-beta.json"
    upload_asset "$SIG_DIR/latest-beta.json"
    echo "（预发布 tag：正式通道清单保持不变）"
  else
    cp "$OUT" "$SIG_DIR/latest.json"
    upload_asset "$SIG_DIR/latest.json"
    cp "$OUT" "$SIG_DIR/latest-beta.json"
    upload_asset "$SIG_DIR/latest-beta.json"
  fi

  echo "✓ 更新清单已发布:"
  echo "    正式版: https://cnb.cool/$REPO_SLUG/-/releases/download/updater-manifest/latest.json"
  echo "    预发布: https://cnb.cool/$REPO_SLUG/-/releases/download/updater-manifest/latest-beta.json"
  rm -rf "$SIG_DIR"
}

finalize_updater_manifest
