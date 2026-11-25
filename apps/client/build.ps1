# 构建 Web 端所需的 wasm 产物并放到 apps/client 目录（跨平台：路径全部走 Join-Path）。
# 用法：pwsh -File apps/client/build.ps1
$ErrorActionPreference = 'Stop'

$root = Join-Path $PSScriptRoot '..' '..'
Push-Location $root
try {
    Write-Host '[1/2] cargo build -p gf-client-wasm --target wasm32-unknown-unknown --release'
    cargo build -p gf-client-wasm --target wasm32-unknown-unknown --release
    if ($LASTEXITCODE -ne 0) { throw "wasm 构建失败 ($LASTEXITCODE)" }

    $wasm = Join-Path $root 'target' 'wasm32-unknown-unknown' 'release' 'gf_client_wasm.wasm'
    if (-not (Test-Path -LiteralPath $wasm)) { throw "未找到产物：$wasm" }
    $dest = Join-Path $PSScriptRoot 'gf_client_wasm.wasm'
    Copy-Item -LiteralPath $wasm -Destination $dest -Force

    $size = (Get-Item -LiteralPath $dest).Length
    Write-Host ("[2/2] 已复制 gf_client_wasm.wasm（{0:N1} KB）" -f ($size / 1KB))
}
finally {
    Pop-Location
}
