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

# 让中文输出在 CI 日志中不乱码：runner 控制台默认 GBK 代码页，
# 脚本本身是 UTF-8（带 BOM），不改代码页的话输出会被按 GBK 转码成乱码
try {
    chcp 65001 > $null
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    $OutputEncoding = [System.Text.Encoding]::UTF8
} catch {
    # 编码设置失败不影响主流程（个别精简系统可能缺 chcp）
}

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
# Tauri 的 beforeBuildCommand 走 pnpm，pnpm 又依赖 node，链条最上游。
# 构建机没装时自动安装 Node.js 22 LTS：用官方便携 zip（免管理员权限），
# 解压到 USERPROFILE\.cache\nodejs 并写入用户 PATH——宿主机持久，
# 后续流水线重跑时直接命中，不会重复下载。
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Write-Host "node 未安装，自动安装 Node.js 22 LTS（便携 zip，无需管理员）..."
    $NodeHome = Join-Path $env:USERPROFILE ".cache\nodejs"
    New-Item -ItemType Directory -Force -Path $NodeHome | Out-Null
    # 从官方 latest-v22.x 目录页解析出 zip 文件名，写死版本号容易 404
    $Listing = Invoke-WebRequest -UseBasicParsing -Uri "https://nodejs.org/dist/latest-v22.x/"
    $ZipName = ([regex]::Matches($Listing.Content, "node-v[\d.]+-win-x64\.zip") | Select-Object -First 1).Value
    if (-not $ZipName) {
        Fail "无法解析 Node.js 安装包文件名" "请检查 https://nodejs.org/dist/latest-v22.x/ 是否可访问，或在该构建机手动安装 Node.js 20+"
    }
    Write-Host "下载: $ZipName"
    $ZipPath = Join-Path $env:TEMP $ZipName
    # keepalive 后台任务：下载包 30MB+，防止 CNB 10 分钟无输出超时误杀
    $Keepalive = Start-Job -ScriptBlock {
        while ($true) {
            Start-Sleep -Seconds 20
            Write-Output "[keepalive] node 安装包下载/解压中... $(Get-Date -Format HH:mm:ss)"
        }
    }
    try {
        Invoke-WebRequest -UseBasicParsing -Uri "https://nodejs.org/dist/latest-v22.x/$ZipName" -OutFile $ZipPath
        Expand-Archive -Path $ZipPath -DestinationPath $NodeHome -Force
    } finally {
        # 无论成败都要收掉心跳任务，不然它会一直活着打日志
        Stop-Job $Keepalive -ErrorAction SilentlyContinue
        Remove-Job $Keepalive -Force -ErrorAction SilentlyContinue
    }
    $NodeDir = Join-Path $NodeHome ($ZipName -replace "\.zip$", "")
    # 当前进程 PATH（本次 stage 生效）
    $env:PATH = "$NodeDir;$env:PATH"
    # 用户级持久 PATH（后续 stage 与重跑流水线直接命中，不再重复下载）
    $UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($UserPath -notlike "*$NodeDir*") {
        [Environment]::SetEnvironmentVariable("PATH", "$NodeDir;$UserPath", "User")
        Write-Host "已写入用户 PATH: $NodeDir"
    }
}
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    Fail "node 安装失败" "自动安装未成功，请在该构建机手动安装 Node.js 20+（官网安装包或 nvm-windows）"
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
    # npm 全局 bin 目录不一定在 PATH（节点 node 升到 26 后 corepack 不再捆绑，
    # 走 npm -g 兜底时实测 pnpm 命令找不到）：显式把 prefix 目录加进
    # 当前进程 PATH（本次 stage 生效）与用户级持久 PATH（后续 stage 命中）
    $NpmPrefix = (npm config get prefix 2>$null)
    if ($LASTEXITCODE -eq 0 -and $NpmPrefix -and (Test-Path $NpmPrefix.Trim())) {
        $NpmPrefix = $NpmPrefix.Trim()
        $env:PATH = "$NpmPrefix;$env:PATH"
        $UserPath2 = [Environment]::GetEnvironmentVariable("PATH", "User")
        if ($UserPath2 -notlike "*$NpmPrefix*") {
            [Environment]::SetEnvironmentVariable("PATH", "$NpmPrefix;$UserPath2", "User")
            Write-Host "已写入用户 PATH: $NpmPrefix"
        }
    }
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
