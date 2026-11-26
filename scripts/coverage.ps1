# 覆盖率基线（M1，2026-11-26）
#
# 用法：pwsh -NoProfile -File scripts/coverage.ps1 [-Exclude <正则>]
# 默认排除三类：
#   1. wasm 导出层（`client-wasm`）与 protobuf 生成代码（`generated`）：host 测试进程跑不到，
#      各有自己的 gate（双产物 stateHash / 协议编译）；
#   2. `engine/src/cases.rs`：测试辅助的局态构造器（`lead_state` / `mid_game_state` 等），
#      只被测试调用，不是业务路径；
#   3. `runtime-spike`：性能原型 crate，不进发布产物；
#   4. `crates/deckart`：牌面主题工具包（素材拼 SVG + 素材包加解密），自带单测但属表现层资产，
#      不参与 M1 的引擎覆盖率门禁（接入条件见基线文档）。
# 断言的反向分支（`if 条件不成立 { panic }`）与 `#[cfg(test)]` 参照实现无法按文件排除，
# 仍留在分母里，属统计固有噪声，口径说明见 `docs/assets/m1-coverage-baseline.md`。

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $env:Path = "C:\Program Files\Git\cmd;" + $env:USERPROFILE + '\.cargo\bin;' + $env:Path
    if (Test-Path 'D:\soft\anaconda3\Library\bin\protoc.exe') {
        $env:PROTOC = 'D:\soft\anaconda3\Library\bin\protoc.exe'
    }
    $exclude = if ($args.Count -gt 0) { $args[0] } else { '(client-wasm|generated|engine[\\/]src[\\/]cases\.rs|runtime-spike|crates[\\/]deckart)' }
    Write-Host "[coverage] 排除口径：$exclude（分支覆盖需要 nightly + llvm-tools-preview）"
    & cargo +nightly llvm-cov --workspace --branch --summary-only --ignore-filename-regex $exclude
    if ($LASTEXITCODE -ne 0) { throw "覆盖率统计失败（$LASTEXITCODE）" }
}
finally {
    Pop-Location
}
