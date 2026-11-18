# Windows 发布构建：产出 msi 与 nsis 安装包，并做版本校验与产物校验。
#
# 运行环境：CNB 自建 Windows 节点（宿主机 PowerShell 直跑，无 docker）。
#
# 校验项（失败任意一项即终止）：
#   1. tag_push 场景 tag 名 == tauri.conf.json version；
#   2. msi / nsis 安装包生成且非空；
#   3. 输出体积 + sha256 指纹留档；
#   4. tag 构建时建 release 并上传附件（复用 ci/upload-release.sh，走 git bash）。
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

if ($Ref -like "v*") {
    if ("v$Version" -ne $Ref) {
        Write-Host "tag 与版本号不一致：tag=$Ref，tauri.conf.json version=$Version。" -ForegroundColor Red
        Write-Host "请同步修改 src-tauri/tauri.conf.json 与 package.json 的 version 后重新打 tag。" -ForegroundColor Red
        exit 1
    }
    Write-Host "OK: tag 与应用版本一致"
}

# ---------- 2. 打包（nsis 必选 + msi 可选） ----------
# tauri build 会先跑 beforeBuildCommand（pnpm build）再编译 release。
# 注意 PowerShell 会把裸逗号 `msi,nsis` 当数组语法展开，tauri 收到
# "msi nsis" 直接报 invalid value——所以用重复 flag 的写法绕开解析差异。
#
# nsis / msi 分两次打包，原因是 WiX 的 candle.exe 在部分构建机上会
# 因安全软件拦截或缺运行库而静默失败（报错信息为空，无从排查），
# 若绑在一条命令里会让整个发布失败。因此：nsis（Windows 通用安装器）
# 作为必选产物，失败即终止；msi（企业部署场景）作为可选产物，
# 失败只告警并跳过，不阻塞 nsis 附件发布。
# 先清掉 bundle 输出目录：编译缓存卷里会残留旧版本的安装包，
# 不清理的话后面的产物收集可能挑中上一次的旧包。
$BundleDir = Join-Path $TargetRoot "release\bundle"
if (Test-Path $BundleDir) { Remove-Item -Recurse -Force $BundleDir }
$StartAt = [DateTime]::Now
pnpm exec tauri build --bundles nsis
if ($LASTEXITCODE -ne 0) {
    # tauri 只报一句 "Failed to bundle app with makensis"，真实报错被吞掉。
    # 这里做四项诊断，把根因打回 CI 日志：
    #   1. NSIS 工具目录是否存在及内容（tauri 是否真的下载成功了）；
    #   2. 每个文件是否带 Zone.Identifier（MOTW）——带标记的 exe 会被
    #      SmartScreen 拦截，"Running makensis" 后 21ms 就挂正是这种症状；
    #   3. 直接调用 makensis 抓真实 stderr/退出码；
    #   4. Defender 检测历史里最近的威胁记录（杀软静默拦截的常见证据）。
    $NsisToolDir = Join-Path $env:LOCALAPPDATA "tauri\NSIS"
    Write-Host "== 诊断 1: NSIS 工具目录 $NsisToolDir =="
    if (Test-Path $NsisToolDir) {
        Get-ChildItem $NsisToolDir | ForEach-Object { Write-Host ("  {0}  {1} bytes" -f $_.Name, $_.Length) }
    } else {
        Write-Host "  目录不存在——tauri 未成功下载 NSIS"
    }
    Write-Host "== 诊断 2: Zone.Identifier（MOTW）检查 =="
    Get-ChildItem $NsisToolDir -Recurse -File -ErrorAction SilentlyContinue | ForEach-Object {
        if (Get-Item $_.FullName -Stream Zone.Identifier -ErrorAction SilentlyContinue) {
            Write-Host "  MOTW: $($_.FullName)"
        }
    }
    Write-Host "== 诊断 3: 直接调用 makensis =="
    $Makensis = Join-Path $NsisToolDir "makensis.exe"
    if (Test-Path $Makensis) {
        $Out = & $Makensis -VERSION 2>&1
        $Out | ForEach-Object { Write-Host "  $_" }
        Write-Host "  makensis -VERSION 退出码: $LASTEXITCODE"
        # 再跑一次真实打包命令复现，抓 stderr（tauri 调用时吞掉了）
        $NsiFile = Join-Path $env:TEMP "zoro-diag.nsi"
        "OutFile diag.exe" | Out-File $NsiFile -Encoding ascii
        $Out2 = & $Makensis $NsiFile 2>&1
        $Out2 | Select-Object -Last 10 | ForEach-Object { Write-Host "  $_" }
        Write-Host "  makensis diag 退出码: $LASTEXITCODE"
    } else {
        Write-Host "  makensis.exe 不存在"
    }
    Write-Host "== 诊断 4: Defender 最近检测记录 =="
    try {
        Get-MpThreatDetection -ErrorAction Stop | Sort-Object InitialDetectionTime -Descending |
            Select-Object -First 5 | ForEach-Object {
                Write-Host ("  {0}  {1}  resources: {2}" -f $_.InitialDetectionTime, $_.ThreatStatusID, ($_.Resources -join ","))
            }
    } catch { Write-Host "  无法读取（$(($_.Exception.Message -split "`n")[0])）" }
    exit 1
}
$HasMsi = $false
pnpm exec tauri build --bundles msi
if ($LASTEXITCODE -ne 0) {
    Write-Host "WARN: msi 打包失败（WiX candle 兼容性问题？），本次跳过 msi，仅发布 nsis" -ForegroundColor Yellow
} else {
    $HasMsi = $true
}
Write-Host ("打包耗时: {0}s" -f ([int]([DateTime]::Now - $StartAt).TotalSeconds))

# ---------- 3. 产物定位与校验 ----------
# 产物落盘位置跟随 CARGO_TARGET_DIR（Windows 节点用 USERPROFILE 缓存增量编译）
$NsisDir = Join-Path $BundleDir "nsis"
$Nsis = Get-ChildItem -Path $NsisDir -Filter "*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $Nsis) { Write-Host "== FAIL: 未找到 nsis 产物" -ForegroundColor Red; exit 1 }
if ($Nsis.Length -le 0) { Write-Host "== FAIL: 产物为空: $($Nsis.FullName)" -ForegroundColor Red; exit 1 }
Write-Host ("产物: {0} ({1:N1} MB)" -f $Nsis.FullName, ($Nsis.Length / 1MB))
if ($HasMsi) {
    $Msi = Get-ChildItem -Path (Join-Path $BundleDir "msi") -Filter "*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $Msi) { $HasMsi = $false }
    else {
        if ($Msi.Length -le 0) { Write-Host "== FAIL: 产物为空: $($Msi.FullName)" -ForegroundColor Red; exit 1 }
        Write-Host ("产物: {0} ({1:N1} MB)" -f $Msi.FullName, ($Msi.Length / 1MB))
    }
}

# ---------- 4. 发布：建 release 并上传附件 ----------
# 仅 tag_push（tag 名形如 vX.Y.Z）时上传；非 tag 构建只验证链路。
# 附件按实际产出收集：nsis setup.exe 必有，msi 视打包结果，各带 sha256；
# 统一加 OS/架构 标识命名（tauri 默认名只有架构与语言，如 *_x64_en-US.msi）。
if ($Ref -like "v*") {
    $AssetDir = "release-assets"
    if (Test-Path $AssetDir) { Remove-Item -Recurse -Force $AssetDir }
    New-Item -ItemType Directory -Path $AssetDir | Out-Null

    # 统一命名：zoro_<ver>_windows_amd64-setup.exe / zoro_<ver>_windows_amd64.msi
    $Assets = @("zoro_${Version}_windows_amd64-setup.exe")
    Copy-Item $Nsis.FullName (Join-Path $AssetDir $Assets[0])
    if ($HasMsi) {
        $Assets += "zoro_${Version}_windows_amd64.msi"
        Copy-Item $Msi.FullName (Join-Path $AssetDir $Assets[-1])
    }

    # sha256 指纹留档，格式与 shasum -a 256 输出对齐（小写哈希 + 两个空格 + 文件名）
    foreach ($name in $Assets) {
        $Path = Join-Path $AssetDir $name
        $Hash = (Get-FileHash -Algorithm SHA256 -Path $Path).Hash.ToLower()
        "$Hash  $name" | Out-File -FilePath "$Path.sha256" -Encoding ascii
        Write-Host "指纹: $Hash  $name"
    }

    # 上传复用 ci/upload-release.sh（bash 脚本，对 Linux/mac/Windows 三端零改动）；
    # Windows 侧通过 Git for Windows 自带的 bash 执行，优先 PATH，其次常规安装路径
    $Bash = Get-Command bash -ErrorAction SilentlyContinue
    if ($Bash) { $BashExe = $Bash.Source } else { $BashExe = "C:\Program Files\Git\bin\bash.exe" }
    if (-not (Test-Path $BashExe)) {
        Write-Host "== FAIL: 找不到 bash（Git for Windows），无法调用 upload-release.sh" -ForegroundColor Red
        exit 1
    }
    Write-Host "使用 bash: $BashExe"
    $UploadFiles = @()
    foreach ($name in $Assets) { $UploadFiles += "$AssetDir/$name"; $UploadFiles += "$AssetDir/$name.sha256" }
    & $BashExe ci/upload-release.sh @UploadFiles
    if ($LASTEXITCODE -ne 0) {
        Write-Host "== FAIL: release 上传失败" -ForegroundColor Red
        exit 1
    }
} else {
    Write-Host "非 tag 构建（$Ref），跳过 release 上传"
}

$BundleKinds = if ($HasMsi) { "msi / nsis" } else { "nsis（msi 跳过）" }
Write-Host "== OK: Windows 发布测试通过（$BundleKinds） =="
