# M0 gate 第 1 项：native 端与 wasm 端各跑同一份 m0-minimal 用例，
# 按 64 字节 ABI 布局逐字段比对 abiVersion / stateHash / steps / phase /
# finishOrder / completedTeam / winnerTeam / discardCount / handsTotal，
# 任一不一致 exit 1。只比哈希会漏掉「两边同样走错」，因此终局名次、张数一并比对。
$ErrorActionPreference = 'Stop'

$engineDir = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$rootDir = Resolve-Path (Join-Path $engineDir '..\..')

# B11 后现行锚点（native/wasm 双端一致；m0-minimal 夹具，run_m0_golden）。
# 轨迹若再漂移，以 run_m0_golden 实测回填，不许改旧期望凑绿。
$Expected = [ordered]@{
    stateHash      = '9b2ff28b384e8d125f925005fdef659fb8abddd45cbf270e58e2d984d0907ab0'
    steps          = '148'
    phase          = '3'
    finishOrder    = '1,3,0,2'
    completedTeam  = '1'
    winnerTeam     = '1'
    discardCount   = '104'
    handsTotal     = '4'
}

function Get-NativeField($lines, $prefix) {
    $line = $lines | Where-Object { $_ -match "^$prefix`: " } | Select-Object -First 1
    if (-not $line) { throw "native 未取到字段 $prefix" }
    return ($line -replace "^$prefix`: ", '').Trim()
}

Push-Location $rootDir
try {
    Write-Host '[1/4] cargo build -p gf-engine'
    cargo build -p gf-engine
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
    $nativeCompleted = Get-NativeField $nativeLines 'completedTeam'
    $nativeWinner = Get-NativeField $nativeLines 'winnerTeam'
    $nativeDiscard = Get-NativeField $nativeLines 'discardCount'
    $nativeHands = Get-NativeField $nativeLines 'handsTotal'

    Write-Host '[4/4] wasm 端（Node 裸加载）跑 golden 用例'
    $wasmRaw = node (Join-Path $PSScriptRoot 'm0_golden_host.mjs')
    if ($LASTEXITCODE -ne 0) { throw 'wasm 宿主运行失败' }
    $wasm = $wasmRaw | ConvertFrom-Json
    if ($wasm.abiVersion -ne 2) {
        Write-Error "abiVersion 不兼容：期望 2（64 字节布局），实际 $($wasm.abiVersion)"
        exit 1
    }
    $wasmHash = $wasm.hash
    $wasmSteps = [string]$wasm.steps
    $wasmPhase = [string]$wasm.phase
    $wasmFinish = ($wasm.finishOrder -join ',')
    $wasmCompleted = if ($null -eq $wasm.completedTeam) { '-' } else { [string]$wasm.completedTeam }
    $wasmWinner = if ($null -eq $wasm.winnerTeam) { '-' } else { [string]$wasm.winnerTeam }
    $wasmDiscard = [string]$wasm.discardCount
    $wasmHands = [string]$wasm.handsTotal

    Write-Host ("native: hash={0} steps={1} phase={2} finishOrder=[{3}] completedTeam={4} winnerTeam={5} discardCount={6} handsTotal={7}" -f $nativeHash, $nativeSteps, $nativePhase, $nativeFinish, $nativeCompleted, $nativeWinner, $nativeDiscard, $nativeHands)
    Write-Host ("wasm  : hash={0} steps={1} phase={2} finishOrder=[{3}] completedTeam={4} winnerTeam={5} discardCount={6} handsTotal={7}" -f $wasmHash, $wasmSteps, $wasmPhase, $wasmFinish, $wasmCompleted, $wasmWinner, $wasmDiscard, $wasmHands)

    if ($nativeHash.Length -ne 64) {
        Write-Error "stateHash 长度异常：$($nativeHash.Length) 字符（应为 64）"
        exit 1
    }

    $mismatches = @()
    if ($nativeHash -ne $wasmHash) { $mismatches += "stateHash: native=$nativeHash wasm=$wasmHash" }
    if ($nativeSteps -ne $wasmSteps) { $mismatches += "steps: native=$nativeSteps wasm=$wasmSteps" }
    if ($nativePhase -ne $wasmPhase) { $mismatches += "phase: native=$nativePhase wasm=$wasmPhase" }
    if ($nativeFinish -ne $wasmFinish) { $mismatches += "finishOrder: native=[$nativeFinish] wasm=[$wasmFinish]" }
    if ($nativeCompleted -ne $wasmCompleted) { $mismatches += "completedTeam: native=$nativeCompleted wasm=$wasmCompleted" }
    if ($nativeWinner -ne $wasmWinner) { $mismatches += "winnerTeam: native=$nativeWinner wasm=$wasmWinner" }
    if ($nativeDiscard -ne $wasmDiscard) { $mismatches += "discardCount: native=$nativeDiscard wasm=$wasmDiscard" }
    if ($nativeHands -ne $wasmHands) { $mismatches += "handsTotal: native=$nativeHands wasm=$wasmHands" }

    if ($mismatches.Count -gt 0) {
        Write-Error "双端字段不一致：`n$($mismatches -join "`n")"
        exit 1
    }

    # 锚点漂移断言：双端一致还不够，必须等于现行 golden 锚点。
    $anchors = @(
        @{ name = 'stateHash'; actual = $nativeHash; expected = $Expected.stateHash },
        @{ name = 'steps'; actual = $nativeSteps; expected = $Expected.steps },
        @{ name = 'phase'; actual = $nativePhase; expected = $Expected.phase },
        @{ name = 'finishOrder'; actual = $nativeFinish; expected = $Expected.finishOrder },
        @{ name = 'completedTeam'; actual = $nativeCompleted; expected = $Expected.completedTeam },
        @{ name = 'winnerTeam'; actual = $nativeWinner; expected = $Expected.winnerTeam },
        @{ name = 'discardCount'; actual = $nativeDiscard; expected = $Expected.discardCount },
        @{ name = 'handsTotal'; actual = $nativeHands; expected = $Expected.handsTotal }
    )
    $anchorMismatch = @()
    foreach ($a in $anchors) {
        if ($a.actual -ne $a.expected) {
            $anchorMismatch += "$($a.name): 实际=$($a.actual) 期望=$($a.expected)"
        }
    }
    if ($anchorMismatch.Count -gt 0) {
        Write-Error "golden 锚点漂移：`n$($anchorMismatch -join "`n")`n若轨迹为有意变更，以 run_m0_golden 实测回填本脚本 `$Expected 常量。"
        exit 1
    }

    Write-Host "OK: 双端 8 字段逐字段一致，且匹配现行锚点 ($nativeHash)"
    exit 0
}
finally {
    Pop-Location
}
