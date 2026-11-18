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
#     （只需 curl + node，两端都已具备）；
#   - 附件同名时 overwrite=true 覆盖，重跑流水线幂等；
#   - release 已存在时直接复用（GET 404 才 POST 创建），重跑不会报冲突。
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

# ---------- 1. 确保 release 存在（已存在则复用，不存在则创建） ----------
ensure_release() {
  local code
  code="$(curl -s -o "$RESP_FILE" -w '%{http_code}' -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" "$RELEASE_API/tags/$TAG")"

  if [[ "$code" == "200" ]]; then
    RELEASE_ID="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).id")"
    echo "✓ release 已存在（id=${RELEASE_ID}），直接复用"
    return
  fi

  if [[ "$code" == "404" ]]; then
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
  curl -sf -X POST -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" -H "Content-Type: application/json" \
    -d "$body" "$RELEASE_API" > "$RESP_FILE"
    RELEASE_ID="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).id")"
    echo "✓ release 已创建（id=${RELEASE_ID}）"
    return
  fi

  echo "查询 release 失败（HTTP ${code}）：" >&2
  cat "$RESP_FILE" >&2
  exit 1
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
  local payload
  payload=$(node -e '
    process.stdout.write(JSON.stringify({
      asset_name: process.argv[1],
      size: Number(process.argv[2]),
      overwrite: true,
    }));
  ' "${name}" "${size}")
  curl -sf -X POST -H "$AUTH_HEADER" -H "$ACCEPT_HEADER" -H "Content-Type: application/json" \
    -d "$payload" "$RELEASE_API/$RELEASE_ID/asset-upload-url" > "$RESP_FILE"

  local upload_url verify_url
  upload_url="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).upload_url")"
  verify_url="$(node -p "JSON.parse(require('fs').readFileSync('$RESP_FILE')).verify_url")"

  # 2.2 预签名地址直传文件（无需鉴权头）
  curl -sf -X PUT --upload-file "$file" "$upload_url"

  # 2.3 确认上传（verify_url 即确认端点，带鉴权调一次即入库）
  curl -sf -X POST -H "$AUTH_HEADER" "$verify_url"

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
