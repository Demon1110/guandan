# 双产物同解 gate：native（`gf-ai` example）与 wasm（`gf-client-wasm`）跑同一条
# 「四 bot 两局」流程，终局 `stateHash` 必须逐字节相等。CI 与本机通用（路径跨平台）。
#
# 用法：pwsh -File scripts/verify_dual_hash.ps1 [-Seed 0x51EDA101]
param([string]$Seed = '0x51EDA101')
$ErrorActionPreference = 'Stop'

$root = Join-Path $PSScriptRoot '..'
$buildScript = Join-Path $root 'apps' 'client' 'build.ps1'
$wasmRunner = Join-Path $root 'apps' 'client' 'verify_wasm_hash.mjs'

Push-Location $root
try {
    $seedDec = [uint64]$Seed

    Write-Host '[1/3] 构建 wasm 产物'
    & pwsh -NoProfile -File $buildScript
    if ($LASTEXITCODE -ne 0) { throw "wasm 构建失败 ($LASTEXITCODE)" }

    Write-Host '[2/3] native 端跑两局'
    $nativeOut = cargo run -q -p gf-ai --example local_match -- $seedDec 2>$null
    if ($LASTEXITCODE -ne 0) { throw "native 运行失败 ($LASTEXITCODE)" }
    $nativeHash = ($nativeOut | Where-Object { $_ -like 'fullHash=*' } | Select-Object -First 1) -replace 'fullHash=', ''
    $nativeRecords = ($nativeOut | Where-Object { $_ -like 'records=*' } | Select-Object -First 1) -replace 'records=', ''
    if (-not $nativeHash) { throw 'native 端未打印 fullHash' }

    Write-Host '[3/3] wasm 端跑两局'
    $wasmJson = node $wasmRunner $seedDec
    if ($LASTEXITCODE -ne 0) { throw "wasm 运行失败 ($LASTEXITCODE)" }
    $wasm = $wasmJson | ConvertFrom-Json

    Write-Host ("native: hash={0} records={1}" -f $nativeHash, $nativeRecords)
    Write-Host ("wasm  : hash={0} records={1}" -f $wasm.hash, $wasm.records)

    if ($nativeHash.Length -ne 64) {
        throw "stateHash 长度异常：$($nativeHash.Length)"
    }
    if ($nativeHash -ne $wasm.hash) {
        throw "双产物 stateHash 不一致：native=$nativeHash wasm=$($wasm.hash)"
    }
    if ($nativeRecords -ne $wasm.records) {
        throw "动作条数不一致：native=$nativeRecords wasm=$($wasm.records)"
    }
    Write-Host "OK: 双产物同解（$nativeHash）"
}
finally {
    Pop-Location
}
