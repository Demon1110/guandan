# 覆盖率基线（M1，2026-11-25）
#
# 用法：pwsh -NoProfile -File scripts/coverage.ps1 [-Exclude <正则>]
# 默认排除 wasm 导出层与 protobuf 生成代码：这两块在 host 测试进程里跑不到，
# 且各有自己的 gate（双产物 stateHash / 协议编译）。

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $env:Path = "C:\Program Files\Git\cmd;" + $env:USERPROFILE + '\.cargo\bin;' + $env:Path
    if (Test-Path 'D:\soft\anaconda3\Library\bin\protoc.exe') {
        $env:PROTOC = 'D:\soft\anaconda3\Library\bin\protoc.exe'
    }
    $exclude = if ($args.Count -gt 0) { $args[0] } else { '(client-wasm|generated)' }
    Write-Host "[coverage] 排除口径：$exclude（分支覆盖需要 nightly + llvm-tools-preview）"
    & cargo +nightly llvm-cov --workspace --branch --summary-only --ignore-filename-regex $exclude
    if ($LASTEXITCODE -ne 0) { throw "覆盖率统计失败（$LASTEXITCODE）" }
}
finally {
    Pop-Location
}
