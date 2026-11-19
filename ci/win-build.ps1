# Windows 发布构建：尽量产出 msi / nsis 安装包，便携 zip 兜底保证必有产物。
#
# 运行环境：CNB 自建 Windows 节点（宿主机 PowerShell 直跑，无 docker）。
#
# 产物策略（按优先级，任何一项失败都不阻塞其余产物）：
#   1. nsis setup.exe（Windows 通用安装器）——本节点因管家类安全软件
#      拦截未签名工具（makensis 被静默破坏：首次 0xC0000135 加载失败、
#      重下后文件被截断成 2560 字节），经常失败；
#   2. msi（企业部署场景，依赖 WiX candle，同样受杀软拦截）；
#   3. 便携 zip（release 目录下的主程序 + 说明文件）——零工具链依赖，
#      只要 Rust 编译成功就必有，作为兜底交付物。
#
# 校验项：
#   - tag_push 场景 tag 名 == tauri.conf.json version；
#   - 最终至少产出一件附件，否则失败；
#   - tag 构建时建 release 并上传附件（复用 ci/upload-release.sh，走 git bash）。
#
# 注意：self-hosted Windows runner 默认 shell 是 Windows PowerShell 5.x，
# 本文件必须带 UTF-8 BOM 才能正确解析中文注释（PS5 无 BOM 按 ANSI 读取）。
$ErrorActionPreference = "Stop"

# rustup 装在 USERPROFILE\.cargo\bin，可能不在 runner 默认 PATH 里，显式补上；
# node 是 setup 阶段便携 zip 装的（USERPROFILE\.cache\nodejs\node-v*-win-x64）——
# stage 之间是新进程，setup 写入的用户 PATH 在本进程不生效，也要在这里补齐，
# 否则调用 node/pnpm 会报 CommandNotFoundException
$NodeHome = Join-Path $env:USERPROFILE ".cache\nodejs"
$NodeDir = Get-ChildItem $NodeHome -Directory -Filter "node-v*-win-x64" -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending | Select-Object -First 1
$ExtraPaths = @("$env:USERPROFILE\.cargo\bin") + @($NodeDir.FullName)
$env:PATH = (($ExtraPaths | Where-Object { $_ }) -join ";") + ";$env:PATH"
Write-Host "node: $(node -v)"

# 与 Mac 节点同理：宿主 USERPROFILE 持久，把 release 编译产物放在固定目录，
# 跨构建复用；cargo 按指纹增量编译，commit 变化只会重编变动的 crate
$env:CARGO_TARGET_DIR = "$env:USERPROFILE\.cache\zoro-target"
$TargetRoot = $env:CARGO_TARGET_DIR

# ---------- 1. 版本一致性 ----------
$Version = node -p "require('./src-tauri/tauri.conf.json').version"
# tag_push 事件下 CNB_BRANCH 即 tag 名；手动触发时是分支名
$Ref = if ($env:CNB_BRANCH) { $env:CNB_BRANCH } else { "unknown" }
Write-Host "构建引用: $Ref / 应用版本: $Version / 架构: $env:PROCESSOR_ARCHITECTURE"

# updater-manifest 是热更新清单承载 tag（见 upload-release.sh finalize），
# 不是版本 tag——CI 维护清单时若触发了 tag_push 流水线，直接跳过构建
if ($Ref -eq "updater-manifest") {
    Write-Host "updater-manifest 为热更新清单 tag，跳过应用构建"
    exit 0
}

if ($Ref -like "v*") {
    if ("v$Version" -ne $Ref) {
        Write-Host "tag 与版本号不一致：tag=$Ref，tauri.conf.json version=$Version。" -ForegroundColor Red
        Write-Host "请同步修改 src-tauri/tauri.conf.json 与 package.json 的 version 后重新打 tag。" -ForegroundColor Red
        exit 1
    }
    Write-Host "OK: tag 与应用版本一致"
}

# ---------- 2. 编译（release） ----------
# 先只做编译，不指定 bundle 类型——产物在 $TargetRoot\release\<crate>.exe。
# 安装包随后按需分次打包：每一类失败都不影响编译成果的交付。
# 先清掉 bundle 输出目录：编译缓存卷里会残留旧版本的安装包，
# 不清理的话后面的产物收集可能挑中上一次的旧包。
$BundleDir = Join-Path $TargetRoot "release\bundle"
if (Test-Path $BundleDir) { Remove-Item -Recurse -Force $BundleDir }
$StartAt = [DateTime]::Now
pnpm exec tauri build --no-bundle
if ($LASTEXITCODE -ne 0) {
    Write-Host "== FAIL: Rust release 编译失败（tauri 退出码 $LASTEXITCODE）" -ForegroundColor Red
    exit 1
}
Write-Host ("编译耗时: {0}s" -f ([int]([DateTime]::Now - $StartAt).TotalSeconds))

# ---------- 3. 安装包打包（尽力而为） ----------
# 应用内自动更新：TAURI_SIGNING_PRIVATE_KEY（CNB 密钥仓库 imports 注入）存在时，
# 给 nsis 打包追加 --config 覆盖开启 createUpdaterArtifacts——tauri 会在
# setup.exe 旁产出 .sig 签名（Windows 更新链路用的就是 setup.exe）。
# 覆盖配置必须写成临时 JSON 文件再传 --config <路径>，原因有二：
#   1. tauri CLI 的 --bundles 是变长参数，命令尾部追加裸 JSON 会被它当作
#      bundle 名吞掉（invalid value ... for '--bundles'）；
#   2. PS 5.1 向原生命令传含双引号的参数有引号转义坑，JSON 内嵌引号会被
#      拼坏；文件路径无空格无引号，天然安全。
# 注意：--config 与路径必须作为两个独立参数传（不能用
# $UpdaterConfig = "--config xxx" 拼接——PS 5.1 会把含空格的字符串变量当作
# 单个参数加引号传给原生命令，tauri 收到 flag 与值粘连的参数直接报错）。
if ($env:TAURI_SIGNING_PRIVATE_KEY) {
    Set-Content -Path .tauri-updater-config.json -Value '{"bundle":{"createUpdaterArtifacts":true}}' -Encoding ascii
    Write-Host "检测到签名密钥，开启应用内更新产物"
} else {
    Write-Host "未配置 TAURI_SIGNING_PRIVATE_KEY，跳过更新包签名（应用内更新将无 Windows 条目）"
}

$HasNsis = $false

<#
.SYNOPSIS
    修复被安全软件"疫苗化"的打包工具目录，返回是否值得重试。
.DESCRIPTION
    节点上的杀软会把未签名的 makensis.exe / candle.exe 截断或拦截（典型表现：
    文件只剩几 KB、启动报 0xC0000135 STATUS_DLL_NOT_FOUND）。两种修复手段：
    1. 主程序被截断 → 删除整个工具目录，tauri 下次构建会重新下载干净副本；
    2. 文件完整但启动被拦 → 大概率是 MOTW（Zone.Identifier 下载标记），
       递归解除后重试。
#>
function Repair-ToolDir {
    param([string]$ToolDir, [string]$MainExe, [int]$MinSize)
    $Exe = Join-Path $ToolDir $MainExe
    if (-not (Test-Path $Exe)) { return $false }
    if ((Get-Item $Exe).Length -lt $MinSize) {
        Write-Host "检测到 $MainExe 被截断（疫苗化），删除 $ToolDir 强制重新下载..." -ForegroundColor Yellow
        Remove-Item -Recurse -Force $ToolDir -ErrorAction SilentlyContinue
        return $true
    }
    Write-Host "解除 $ToolDir 的 MOTW 标记后重试..." -ForegroundColor Yellow
    Get-ChildItem $ToolDir -Recurse -File -ErrorAction SilentlyContinue |
        Unblock-File -ErrorAction SilentlyContinue
    return $true
}

if ($env:TAURI_SIGNING_PRIVATE_KEY) {
    pnpm exec tauri build --bundles nsis --config .tauri-updater-config.json
} else {
    pnpm exec tauri build --bundles nsis
}
if ($LASTEXITCODE -eq 0) {
    $HasNsis = $true
} else {
    # tauri 只报一句 "Failed to bundle app with makensis"，真实报错被吞掉。
    # 打三行诊断进 CI 日志：工具目录内容（看 makensis 是否被杀软截断/疫苗化）、
    # MOTW 标记、直接调用 makensis 的退出码。
    $NsisToolDir = Join-Path $env:LOCALAPPDATA "tauri\NSIS"
    Write-Host "WARN: nsis 打包失败，诊断信息：" -ForegroundColor Yellow
    if (Test-Path $NsisToolDir) {
        Get-ChildItem $NsisToolDir -File -ErrorAction SilentlyContinue |
            ForEach-Object { Write-Host ("  {0}  {1} bytes" -f $_.Name, $_.Length) }
        $Makensis = Join-Path $NsisToolDir "makensis.exe"
        if (Test-Path $Makensis) {
            & $Makensis -VERSION 2>&1 | ForEach-Object { Write-Host "  $_" } | Out-Null
            Write-Host ("  直接调用 makensis -VERSION 退出码: {0}（-1073741515 = 0xC0000135 加载被拦）" -f $LASTEXITCODE)
        }
    } else {
        Write-Host "  NSIS 工具目录不存在: $NsisToolDir"
    }
    # 自修复重试：截断 → 删目录让 tauri 重新下载；MOTW → 解除标记
    if (Repair-ToolDir -ToolDir $NsisToolDir -MainExe "makensis.exe" -MinSize 100KB) {
        if ($env:TAURI_SIGNING_PRIVATE_KEY) {
            pnpm exec tauri build --bundles nsis --config .tauri-updater-config.json
        } else {
            pnpm exec tauri build --bundles nsis
        }
        if ($LASTEXITCODE -eq 0) { $HasNsis = $true }
    }
    if (-not $HasNsis) {
        Write-Host "WARN: 跳过 nsis 安装包。若需恢复安装包产物，请在构建机安全软件中" -ForegroundColor Yellow
        Write-Host "      把 $env:LOCALAPPDATA\tauri 目录加入信任/白名单。" -ForegroundColor Yellow
    }
}

$HasMsi = $false
pnpm exec tauri build --bundles msi
if ($LASTEXITCODE -eq 0) {
    $HasMsi = $true
} else {
    # WiX 目录名随 tauri 版本变化（WixTools / WixTools314），通配查找
    $WixToolDir = Get-ChildItem (Join-Path $env:LOCALAPPDATA "tauri") -Directory -Filter "WixTools*" -ErrorAction SilentlyContinue |
        Select-Object -First 1
    Write-Host "WARN: msi 打包失败（WiX candle 被安全软件拦截？），尝试修复重试" -ForegroundColor Yellow
    if ($WixToolDir -and (Repair-ToolDir -ToolDir $WixToolDir.FullName -MainExe "candle.exe" -MinSize 1MB)) {
        pnpm exec tauri build --bundles msi
        if ($LASTEXITCODE -eq 0) { $HasMsi = $true }
    }
    if (-not $HasMsi) {
        Write-Host "WARN: 跳过 msi（面向企业部署场景，应用内更新走 nsis，不受影响）" -ForegroundColor Yellow
    }
}

# ---------- 4. 收集产物（zip 兜底，保证至少一件） ----------
$Assets = @()          # 相对 AssetDir 的附件文件名（不含 sha256，指纹另行生成）
$AssetDir = "release-assets"
if (Test-Path $AssetDir) { Remove-Item -Recurse -Force $AssetDir }
New-Item -ItemType Directory -Path $AssetDir | Out-Null

if ($HasNsis) {
    $Nsis = Get-ChildItem -Path (Join-Path $BundleDir "nsis") -Filter "*.exe" -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($Nsis) {
        $Name = "zoro_${Version}_windows_amd64-setup.exe"
        Copy-Item $Nsis.FullName (Join-Path $AssetDir $Name)
        $Assets += $Name
        Write-Host ("产物: {0} ({1:N1} MB)" -f $Nsis.FullName, ($Nsis.Length / 1MB))
        # 更新包签名（Windows 应用内更新链路用它校验 setup.exe），随附件一并上传
        $NsisSig = "$($Nsis.FullName).sig"
        if (Test-Path $NsisSig) {
            Copy-Item $NsisSig (Join-Path $AssetDir "$Name.sig")
            Write-Host "产物: $Name.sig（应用内更新签名）"
        } else {
            Write-Host "WARN: 未找到 setup.exe.sig，应用内更新将无 Windows 条目" -ForegroundColor Yellow
        }
    }
}
if ($HasMsi) {
    $Msi = Get-ChildItem -Path (Join-Path $BundleDir "msi") -Filter "*.msi" -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($Msi) {
        $Name = "zoro_${Version}_windows_amd64.msi"
        Copy-Item $Msi.FullName (Join-Path $AssetDir $Name)
        $Assets += $Name
        Write-Host ("产物: {0} ({1:N1} MB)" -f $Msi.FullName, ($Msi.Length / 1MB))
    }
}

# 便携 zip 兜底：主程序 exe + WebView2 说明。Tauri 前端资源内嵌在 exe 中，
# Win10/11 20H2+ 系统自带 WebView2 运行时，解压即可运行，无需安装。
if ($Assets.Count -eq 0) {
    Write-Host "安装包均未产出，走便携 zip 兜底..."
    $MainExe = Get-ChildItem -Path (Join-Path $TargetRoot "release") -Filter "*.exe" -File -ErrorAction SilentlyContinue |
        Sort-Object Length -Descending | Select-Object -First 1
    if (-not $MainExe) {
        Write-Host "== FAIL: release 目录下找不到主程序 exe" -ForegroundColor Red
        exit 1
    }
    Write-Host ("  主程序: {0} ({1:N1} MB)" -f $MainExe.FullName, ($MainExe.Length / 1MB))
    $Stage = Join-Path $env:TEMP "zoro-portable"
    if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
    New-Item -ItemType Directory $Stage | Out-Null
    Copy-Item $MainExe.FullName (Join-Path $Stage "zoro.exe")
    $Readme = @"
Zoro ${Version} (Windows 便携版)

使用方法：
  1. 解压本压缩包到任意目录；
  2. 双击 zoro.exe 运行。

说明：
  - 需要 WebView2 运行时（Windows 10 20H2+ / Windows 11 系统自带；
    如缺失请安装 https://go.microsoft.com/fwlink/p/?LinkId=2124703 ）；
  - 首次运行后，配置文件位于 %APPDATA%\me.jackytse.zoro\config.json。
"@
    $Readme | Out-File (Join-Path $Stage "README.txt") -Encoding utf8
    $ZipName = "zoro_${Version}_windows_amd64.zip"
    Compress-Archive -Path (Join-Path $Stage "*") -DestinationPath (Join-Path $AssetDir $ZipName) -Force
    $Assets += $ZipName
    Write-Host "产物: $ZipName（便携版）"
}

# sha256 指纹留档，格式与 shasum -a 256 输出对齐（小写哈希 + 两个空格 + 文件名）
foreach ($name in $Assets) {
    $Path = Join-Path $AssetDir $name
    $Hash = (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLower()
    "$Hash  $name" | Out-File -FilePath "$Path.sha256" -Encoding ascii
    Write-Host "指纹: $Hash  $name"
}

# ---------- 5. 发布：建 release 并上传附件 ----------
# 仅 tag_push（tag 名形如 vX.Y.Z）时上传；非 tag 构建只验证链路。
# 上传复用 ci/upload-release.sh（bash 脚本，对 Linux/mac/Windows 三端零改动）；
# Windows 侧通过 Git for Windows 自带的 bash 执行，优先 PATH，其次常规安装路径
if ($Ref -like "v*") {
    # 优先从 git.exe 的安装路径反推 Git 自带的 bash（CI 只需 Git for Windows）；
    # 千万不能抓到 C:\WINDOWS\system32\bash.exe——那是 WSL 入口，跑 bash 脚本
    # 会进入 Linux 环境输出乱码、PATH 语义全变，上传必然失败
    $BashExe = $null
    $GitCmd = (Get-Command git -ErrorAction SilentlyContinue).Source
    if ($GitCmd) {
        $GitDir = Split-Path $GitCmd                      # ...\Git\cmd 或 ...\Git\bin
        $GitRoot = Split-Path $GitDir                     # ...\Git
        foreach ($c in @(
            (Join-Path $GitRoot "bin\bash.exe"),
            (Join-Path $GitRoot "usr\bin\bash.exe"),
            (Join-Path $GitDir "bash.exe")
        )) { if (Test-Path $c) { $BashExe = $c; break } }
    }
    if (-not $BashExe) {
        foreach ($c in @("C:\Program Files\Git\bin\bash.exe", "C:\Program Files\Git\usr\bin\bash.exe")) {
            if (Test-Path $c) { $BashExe = $c; break }
        }
    }
    if (-not $BashExe) {
        $Bash = Get-Command bash -ErrorAction SilentlyContinue
        if ($Bash -and $Bash.Source -notlike "*system32*") { $BashExe = $Bash.Source }
    }
    if (-not $BashExe) {
        Write-Host "== FAIL: 找不到 Git for Windows 的 bash，无法调用 upload-release.sh" -ForegroundColor Red
        exit 1
    }
    Write-Host "使用 bash: $BashExe"
    $UploadFiles = @()
    foreach ($name in $Assets) {
        $UploadFiles += "$AssetDir/$name"
        $UploadFiles += "$AssetDir/$name.sha256"
        # 更新包签名（仅 setup.exe 有）：一并上传供更新清单生成使用
        $SigPath = "$AssetDir/$name.sig"
        if (Test-Path $SigPath) { $UploadFiles += $SigPath }
    }
    # 显式合并 stderr 并回显：CI 日志对 bash 子进程的 stderr 捕获不可靠，
    # 上传失败时若不捕获 2>&1，报错会被吞掉只剩一句 FAIL
    $BashOutput = & $BashExe ci/upload-release.sh @UploadFiles 2>&1
    $BashOutput | ForEach-Object { Write-Host "$_" }
    if ($LASTEXITCODE -ne 0) {
        Write-Host ("== FAIL: release 上传失败（bash exit={0}）" -f $LASTEXITCODE) -ForegroundColor Red
        exit 1
    }
} else {
    Write-Host "非 tag 构建（$Ref），跳过 release 上传"
}

$Summary = if ($HasNsis -or $HasMsi) { ($Assets -join " / ") } else { "便携 zip（安装包被安全软件拦截，见上方 WARN）" }
Write-Host "== OK: Windows 发布完成（$Summary） =="
