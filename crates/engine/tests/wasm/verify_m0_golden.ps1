# M0 gate 第 1 项：native 端与 wasm 端各跑同一份 m0-minimal 用例，
# 逐字段比对 stateHash / steps / phase / finishOrder，任一不一致 exit 1。
# 只比哈希会漏掉「两边同样走错」，因此终局名次与步数一并比对。
$ErrorActionPreference = 'Stop'

$engineDir = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$rootDir = Resolve-Path (Join-Path $engineDir '..\..')

function Get-NativeField($lines, $prefix) {
    $line = $lines | Where-Object { $_ -match "^$prefix`: " } | Select-Object -First 1
    if (-not $line) { throw "native 未取到字段 $prefix" }
    return ($line -replace "^$prefix`: ", '').Trim()
}

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
    $nativeHash = Get-NativeField $nativeLines 'stateHash'
    $nativeSteps = Get-NativeField $nativeLines 'steps'
    $nativePhase = Get-NativeField $nativeLines 'phase'
    $nativeFinish = (Get-NativeField $nativeLines 'finishOrder').Trim('[', ']')

    Write-Host '[4/4] wasm 端（Node 裸加载）跑 golden 用例'
    $wasmRaw = node (Join-Path $PSScriptRoot 'm0_golden_host.mjs')
    if ($LASTEXITCODE -ne 0) { throw 'wasm 宿主运行失败' }
    $wasm = $wasmRaw | ConvertFrom-Json
    $wasmHash = $wasm.hash
    $wasmSteps = [string]$wasm.steps
    $wasmPhase = [string]$wasm.phase
    $wasmFinish = ($wasm.finishOrder -join ',')

    Write-Host "native: hash=$nativeHash steps=$nativeSteps phase=$nativePhase finishOrder=[$nativeFinish]"
    Write-Host "wasm  : hash=$wasmHash steps=$wasmSteps phase=$wasmPhase finishOrder=[$wasmFinish]"

    if ($nativeHash.Length -ne 64) {
        Write-Error "stateHash 长度异常：$($nativeHash.Length) 字符（应为 64）"
        exit 1
    }

    $mismatches = @()
    if ($nativeHash -ne $wasmHash) { $mismatches += "stateHash: native=$nativeHash wasm=$wasmHash" }
    if ($nativeSteps -ne $wasmSteps) { $mismatches += "steps: native=$nativeSteps wasm=$wasmSteps" }
    if ($nativePhase -ne $wasmPhase) { $mismatches += "phase: native=$nativePhase wasm=$wasmPhase" }
    if ($nativeFinish -ne $wasmFinish) { $mismatches += "finishOrder: native=[$nativeFinish] wasm=[$wasmFinish]" }

    if ($mismatches.Count -gt 0) {
        Write-Error "双端字段不一致：`n$($mismatches -join "`n")"
        exit 1
    }
    Write-Host "OK: 双端 stateHash/steps/phase/finishOrder 逐字段一致 ($nativeHash)"
    exit 0
}
finally {
    Pop-Location
}
