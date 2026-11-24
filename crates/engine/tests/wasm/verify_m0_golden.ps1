# M0 gate 第 1 项：native 端与 wasm 端各跑同一份 m0-minimal 用例，
# 32 字节 stateHash 逐字节比对；不一致 exit 1。
$ErrorActionPreference = 'Stop'

$engineDir = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$rootDir = Resolve-Path (Join-Path $engineDir '..\..')

Push-Location $rootDir
try {
    Write-Host '[1/4] cargo build --workspace'
    cargo build --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo build 失败 ($LASTEXITCODE)" }

    Write-Host '[2/4] cargo build --target wasm32-unknown-unknown --release'
    cargo build -p gf-engine --target wasm32-unknown-unknown --release
    if ($LASTEXITCODE -ne 0) { throw "wasm 构建失败 ($LASTEXITCODE)" }

    Write-Host '[3/4] native 端跑 golden 用例'
    $nativeLines = cargo run -p gf-engine --example m0_golden 2>$null
    $native = ($nativeLines | Where-Object { $_ -match '^stateHash: ' }) -replace '^stateHash: ', ''
    if (-not $native) { throw '未取到 native stateHash' }

    Write-Host '[4/4] wasm 端（Node 裸加载）跑 golden 用例'
    $wasm = node (Join-Path $PSScriptRoot 'm0_golden_host.mjs')
    if ($LASTEXITCODE -ne 0) { throw 'wasm 宿主运行失败' }

    Write-Host "native: $native"
    Write-Host "wasm  : $wasm"
    if ($native -ne $wasm) {
        Write-Error "stateHash 双端不一致：native=$native wasm=$wasm"
        exit 1
    }
    if ($native.Length -ne 64) {
        Write-Error "stateHash 长度异常：$($native.Length) 字符（应为 64）"
        exit 1
    }
    Write-Host "OK: 双端 stateHash 逐字节一致 ($native)"
    exit 0
}
finally {
    Pop-Location
}
