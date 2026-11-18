# Windows 自建构建节点上的环境准备。
#
# 与 Mac 自建节点（ci/mac-setup.sh）同样的处境：
#   - CNB 的 Windows 自建节点不支持 docker image（平台级关闭），
#     stage 脚本直接跑在宿主机 PowerShell 上；
#   - 所以这里做的是「检查 + 就地补装」，而不是「换镜像」；
#   - 宿主机的 USERPROFILE 是持久的，rustup / cargo / pnpm 缓存天然复用。
#
# 注意：self-hosted Windows runner 默认 shell 是 Windows PowerShell 5.x，
# 本文件必须带 UTF-8 BOM 才能正确解析中文注释（PS5 无 BOM 按 ANSI 读取）。
#
# 检查顺序按依赖链排：git（runner 基本依赖，也提供后面上传用的 bash）
# → node（pnpm 的宿主）→ pnpm → rust（cargo）→ MSVC 工具链（Rust 链接器依赖）。
# 任何一项装不上都直接失败，让问题在「准备」阶段就暴露，
# 而不是跑到二十分钟的 Rust release 编译时才炸。
$ErrorActionPreference = "Stop"

Write-Host "== Windows 构建节点环境检查 =="
cmd /c ver
Write-Host "架构: $env:PROCESSOR_ARCHITECTURE"

# 统一失败出口：CI 日志里直接给出可执行的修复指引，而不是让下游报晦涩错误
function Fail($msg, $hint) {
    Write-Host "== FAIL: $msg" -ForegroundColor Red
    Write-Host $hint -ForegroundColor Red
    exit 1
}

# ---------- 1. git ----------
# 自托管 Windows runner 的基本依赖（CNB 文档明确要求必装）；
# 同时 Git for Windows 自带 bash.exe，后面上传 release 附件要复用 ci/upload-release.sh
if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Fail "未安装 git" "请安装 Git for Windows 并加入 PATH: https://git-scm.com/download/win"
}
Write-Host "git: $(git --version)"

# ---------- 2. Node.js ----------
# Tauri 的 beforeBuildCommand 走 pnpm，pnpm 又依赖 node，链条最上游
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Fail "未安装 node" "请在该构建机上安装 Node.js 20+（官网安装包或 nvm-windows）"
}
$NodeMajor = [int](node -p "process.versions.node.split('.')[0]")
Write-Host "node: $(node -v)"
# 前端构建在 node 20/22 上验证过，更老的版本有 npm 生态兼容风险
if ($NodeMajor -lt 20) {
    Fail "node 版本过低: $(node -v)" "需要 >= 20，建议升级后重跑"
}

# ---------- 3. pnpm ----------
# 没有时优先用 corepack（node 自带，零额外安装）；corepack 不可用再退 npm -g
if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    Write-Host "pnpm 未安装，尝试 corepack..."
    if (Get-Command corepack -ErrorAction SilentlyContinue) {
        # corepack 失败不致命，下面还有 npm 兜底
        corepack enable pnpm 2>$null
    }
}
if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    Write-Host "corepack 不可用，退回 npm 全局安装..."
    npm install -g pnpm@12.21.0
}
Write-Host "pnpm: $(pnpm -v)"

# ---------- 4. Rust 工具链 ----------
# 构建机没装 rust 时自动补装 rustup（官方 rustup-init.exe，最小化交互）。
# 两个关键点：
#   --profile minimal 只装 rustc/cargo/rust-std，比 default 少下文档、
#     clippy 等一堆组件，构建机不需要它们；
#   keepalive 后台任务定期输出一行日志——CNB 的「无输出超时」是 10 分钟，
#     rustup 静默下载大组件时极易被误杀（Mac 节点实测 9.8 分钟被掐），
#     心跳输出可保持流水线认为任务活跃。
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "cargo 未安装，安装 rustup（minimal profile）..."
    $RustupInit = Join-Path $env:TEMP "rustup-init.exe"
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $RustupInit -UseBasicParsing
    # 后台心跳：每 20s 打一行，直到 rustup 装完再收掉任务
    $Keepalive = Start-Job -ScriptBlock {
        while ($true) {
            Start-Sleep -Seconds 20
            Write-Output "[keepalive] rustup 安装中... $(Get-Date -Format HH:mm:ss)"
        }
    }
    & $RustupInit -y --profile minimal --default-toolchain stable
    Stop-Job $Keepalive -ErrorAction SilentlyContinue
    Remove-Job $Keepalive -Force -ErrorAction SilentlyContinue
}
# rustup 装在 USERPROFILE\.cargo\bin，当前进程未必在 PATH 里，显式补上
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
Write-Host "cargo: $(cargo --version)"
Write-Host "rustc: $(rustc --version)"

# ---------- 5. MSVC 工具链 ----------
# Windows 上 Rust 默认走 msvc target，链接需要 Visual Studio C++ Build Tools；
# 用 vswhere 探测「C++ 生成工具」工作负载（x64），缺失时给明确指引。
# tauri 编译产物是 x64（amd64），只要求 VCTools x64 版本。
$VsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $VsWhere)) {
    Fail "未安装 Visual Studio C++ Build Tools" "请安装 'Visual Studio Build Tools' 并勾选 '使用 C++ 的桌面开发' 工作负载: https://visualstudio.microsoft.com/visual-cpp-build-tools/"
}
$VsInstall = & $VsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $VsInstall) {
    Fail "VS Build Tools 缺少 C++ x64 工具集" "打开 Visual Studio Installer，为 Build Tools 勾选 '使用 C++ 的桌面开发' 后重跑"
}
Write-Host "MSVC: $VsInstall"

# ---------- 6. 依赖安装 ----------
# frozen-lockfile：与 CI 的其它链路保持一致，锁文件不对齐时立刻失败；
# --prefer-offline：优先命中宿主持久化的 pnpm store，没有变化时全部走本地缓存
pnpm install --frozen-lockfile --prefer-offline
Write-Host "pnpm store: $(pnpm store path)"

Write-Host "== OK: Windows 环境准备完成 =="
