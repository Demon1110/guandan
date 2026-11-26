# 一键把 crates/deckart/themes/ 下的所有主题打成 .gfdeck 包
#
# 用法（在仓库任意位置执行）：
#   pwsh -File crates/deckart/scripts/pack-all.ps1                  # 全部主题，release 编一次 gfdeck，产物落 pkgs/
#   pwsh -File crates/deckart/scripts/pack-all.ps1 -Theme defaults  # 只打 defaults
#   pwsh -File crates/deckart/scripts/pack-all.ps1 -OutDir D:\tmp\pkgs -SkipBuild
#
# 每个包打完会立刻跑 info + verify，任何一个失败就返回非 0。
param(
    # 只打指定的主题目录名（缺省 = themes/ 下所有含 theme.json 的目录）
    [string[]]$Theme = @(),

    # 输出目录，缺省 crates/deckart/pkgs
    [string]$OutDir = '',

    # 包内密钥标识；换密钥必须换 id，客户端按它选密钥
    [string]$KeyId = 'builtin-v1',

    # Ed25519 私钥种子（64 位 hex）。缺省用内置开发密钥（种子全 0x11），与 apps/web 的公钥成对
    [string]$SignKey = ('11' * 32),

    # 跳过 cargo build，直接用手上已有的 gfdeck 二进制
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

# 出错只打一行原因再退出，不给用户看 PowerShell 调用栈
trap {
    Write-Host ("打包失败：{0}" -f $_.Exception.Message) -ForegroundColor Red
    exit 1
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$crateDir = (Resolve-Path (Join-Path $scriptDir '..')).Path
$repoDir = (Resolve-Path (Join-Path $crateDir '..\..')).Path
$themesDir = Join-Path $crateDir 'themes'
if (-not $OutDir) { $OutDir = Join-Path $crateDir 'pkgs' }

$exe = Join-Path $repoDir 'target\release\gfdeck.exe'
if (-not $IsWindows) { $exe = Join-Path $repoDir 'target/release/gfdeck' }

function Invoke-Gfdeck {
    param([string[]]$ArgList)
    $out = & $exe @ArgList 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw ("gfdeck {0} 失败：{1}" -f ($ArgList -join ' '), ($out -join "`n"))
    }
    return $out
}

# ---------- 1. 编一次 gfdeck（pack 不需要 PNG feature） ----------
if (-not $SkipBuild) {
    Write-Host "==> cargo build -p gf-deckart --bin gfdeck --release"
    Push-Location $repoDir
    try {
        & cargo build -q -p gf-deckart --bin gfdeck --release
        if ($LASTEXITCODE -ne 0) { throw 'cargo build 失败' }
    }
    finally { Pop-Location }
}
if (-not (Test-Path -LiteralPath $exe)) { throw "找不到 $exe，先去掉 -SkipBuild 编一次" }

# ---------- 2. 收集主题 ----------
if (-not (Test-Path -LiteralPath $themesDir)) { throw "主题目录不存在：$themesDir" }
$dirs = Get-ChildItem -LiteralPath $themesDir -Directory | Where-Object {
    Test-Path -LiteralPath (Join-Path $_.FullName 'theme.json')
}
if ($Theme.Count -gt 0) {
    $dirs = $dirs | Where-Object { $Theme -contains $_.Name }
    $missing = $Theme | Where-Object { $dirs.Name -notcontains $_ }
    if ($missing) { throw ("主题不存在（或没有 theme.json）：{0}" -f ($missing -join ', ')) }
}
if (-not $dirs) { throw "在 $themesDir 下没找到任何含 theme.json 的主题目录" }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$pubDir = Join-Path $repoDir 'target\theme-build'
New-Item -ItemType Directory -Force -Path $pubDir | Out-Null

# ---------- 3. 逐个打包 + 校验 ----------
$rows = @()
foreach ($d in $dirs) {
    $id = $d.Name
    $packPath = Join-Path $OutDir "$id.gfdeck"
    $pubPath = Join-Path $pubDir "$id.pub.hex"

    Write-Host "==> pack $id"
    Invoke-Gfdeck @('pack', $d.FullName, '-o', $packPath, '--key-id', $KeyId, '--sign-key', $SignKey, '--pub-key-out', $pubPath) | Out-Null

    $pub = (Get-Content -LiteralPath $pubPath -Raw).Trim()
    $infoOut = (Invoke-Gfdeck @('info', $packPath)) -join ' '
    $verifyOut = (Invoke-Gfdeck @('verify', $packPath, '--verify-key', $pub)) -join ' '

    $rows += [pscustomobject]@{
        Theme  = $id
        Bytes  = (Get-Item -LiteralPath $packPath).Length
        Pack   = 'pkgs/{0}.gfdeck' -f $id
        Info   = ($infoOut -replace '^info:\s*', '').Trim()
        Verify = $verifyOut.Trim()
    }
}

# ---------- 4. 汇总 ----------
$rows | ForEach-Object {
    Write-Host ''
    Write-Host ("  {0,-16} {1,8} 字节  {2}" -f $_.Theme, $_.Bytes, $_.Pack)
    Write-Host ("  {0,-16} info   {1}" -f '', $_.Info)
    Write-Host ("  {0,-16} verify {1}" -f '', $_.Verify)
}
Write-Host ''
Write-Host ("共 {0} 个主题 → {1}" -f $rows.Count, $OutDir)
