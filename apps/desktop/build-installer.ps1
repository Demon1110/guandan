#!/usr/bin/env pwsh
<#
一键产出 Windows 安装包：cargo build --release → makensis。
为什么不用 `cargo tauri build --bundles nsis`：tauri-bundler 打包前会从 GitHub 拉 nsis-3.13.zip，
国内网络经常 timeout（本机实测失败），且它要求自带 nsis_tauri_utils.dll 的定制包。
本脚本改用官方 NSIS 3.13 + 自维护脚本 installer/installer.nsi，链路完全离线。

前置：把 nsis-3.13.zip 解压到 <repo>\target\nsis-tool（内层目录名必须是 nsis-3.13）。
下载可用代理：https://ghfast.top/https://github.com/tauri-apps/binary-releases/releases/download/nsis-3.13/nsis-3.13.zip
#>
param(
    [switch]$SkipBuild,
    [string]$ToolDir = (Join-Path $PSScriptRoot '..\..\target\nsis-tool'),
    [string]$NsisVersion = '3.13'
)

$ErrorActionPreference = 'Stop'

$makensis = Join-Path (Join-Path $ToolDir "nsis-$NsisVersion") 'makensis.exe'
if (-not (Test-Path $makensis)) {
    throw "缺 NSIS 工具：期望 $makensis。把 nsis-$NsisVersion.zip 解压到 $ToolDir 后重试。"
}

$env:Path = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:Path
$srcTauri = Join-Path $PSScriptRoot 'src-tauri'

if (-not $SkipBuild) {
    Push-Location $srcTauri
    try { cargo build --release } finally { Pop-Location }
}

New-Item -ItemType Directory -Force -Path (Join-Path $PSScriptRoot 'dist') | Out-Null
& $makensis -INPUTCHARSET UTF8 /V2 (Join-Path $PSScriptRoot 'installer\installer.nsi')

Write-Host '--- 产物 ---'
Get-ChildItem (Join-Path $PSScriptRoot 'dist') -File | Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } } | Format-Table -AutoSize
