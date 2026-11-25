# ⑦ 组牌推荐同解 gate：native 与 wasm 各跑同一份 `cases::recommend_digest`
# （classic 领出态 seat 0 的 `Layout` 确定性摘要），逐值比对。
# 独立探针：不改既有 64 字节 golden ABI，不加字段到旧报告；摘要 u64 由
# wasm 侧 lo/hi 两个 u32 导出还原，避免 JS Number 超 2^53 失精。
$ErrorActionPreference = 'Stop'

$engineDir = Resolve-Path (Join-Path $PSScriptRoot '..' '..')
$rootDir = Resolve-Path (Join-Path $engineDir '..' '..')

function Get-NativeField($lines, $prefix) {
    $line = $lines | Where-Object { $_ -match "^$prefix`: " } | Select-Object -First 1
    if (-not $line) { throw "native 未取到字段 $prefix" }
    return ($line -replace "^$prefix`: ", '').Trim()
}

Push-Location $rootDir
try {
    Write-Host '[1/3] cargo build -p gf-engine'
    cargo build -p gf-engine
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败 ($LASTEXITCODE)" }

    Write-Host '[2/3] cargo build --target wasm32-unknown-unknown --release'
    cargo build -p gf-engine --target wasm32-unknown-unknown --release
    if ($LASTEXITCODE -ne 0) { throw "wasm 构建失败 ($LASTEXITCODE)" }

    Write-Host '[3/3] native / wasm 双端跑 recommend 摘要'
    $nativeLines = cargo run -p gf-engine --example recommend_golden 2>$null
    $nativeDigest = Get-NativeField $nativeLines 'recommendDigest'
    $nativeScore = Get-NativeField $nativeLines 'score'
    $nativeSingles = Get-NativeField $nativeLines 'singleCount'
    $nativeGroups = Get-NativeField $nativeLines 'groups'

    $wasmRaw = node (Join-Path $PSScriptRoot 'recommend_host.mjs')
    if ($LASTEXITCODE -ne 0) { throw 'wasm 宿主运行失败' }
    $wasmDigestLine = $wasmRaw | Where-Object { $_ -match '^recommendDigest: ' } | Select-Object -First 1
    if (-not $wasmDigestLine) { throw 'wasm 未取到 recommendDigest' }
    $wasmDigest = ($wasmDigestLine -replace '^recommendDigest: ', '').Trim()

    Write-Host ("native: digest={0} score={1} singles={2} groups={3}" -f $nativeDigest, $nativeScore, $nativeSingles, $nativeGroups)
    Write-Host ("wasm  : digest={0}" -f $wasmDigest)

    $nativeU64 = [uint64]::Parse($nativeDigest)
    $wasmU64 = [uint64]::Parse($wasmDigest)
    if ($nativeU64 -ne $wasmU64) {
        Write-Error "recommend 双端摘要不一致：native=$nativeDigest wasm=$wasmDigest"
        exit 1
    }

    Write-Host "OK: recommend native/wasm 同解（digest=$nativeDigest）"
    exit 0
}
finally {
    Pop-Location
}
