# 本地静态服务（ES module + wasm 需要 http 协议，不能用 file://）。
# 用法：pwsh -File apps/phaser/serve.ps1 [-Port 8099]
param([int]$Port = 8099)
$ErrorActionPreference = 'Stop'

$root = $PSScriptRoot
$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add("http://localhost:$Port/")
$listener.Start()
Write-Host "掼蛋 Phaser 版： http://localhost:$Port/  (Ctrl+C 停止)"

$mime = @{
  '.html' = 'text/html; charset=utf-8'
  '.js'   = 'text/javascript; charset=utf-8'
  '.wasm' = 'application/wasm'
  '.css'  = 'text/css; charset=utf-8'
  '.json' = 'application/json; charset=utf-8'
  '.png'  = 'image/png'
  '.svg'  = 'image/svg+xml'
}

try {
  while ($listener.IsListening) {
    $ctx = $listener.GetContext()
    try {
      $path = [System.Uri]::UnescapeDataString($ctx.Request.Url.AbsolutePath)
      if ($path -eq '/') { $path = '/index.html' }
      $file = Join-Path $root ($path.TrimStart('/') -replace '/', [System.IO.Path]::DirectorySeparatorChar)
      $full = [System.IO.Path]::GetFullPath($file)
      if (-not $full.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)) {
        $ctx.Response.StatusCode = 403
      } elseif (-not (Test-Path -LiteralPath $full -PathType Leaf)) {
        $ctx.Response.StatusCode = 404
      } else {
        $ext = [System.IO.Path]::GetExtension($full).ToLowerInvariant()
        $ctx.Response.ContentType = if ($mime.ContainsKey($ext)) { $mime[$ext] } else { 'application/octet-stream' }
        $bytes = [System.IO.File]::ReadAllBytes($full)
        $ctx.Response.ContentLength64 = $bytes.Length
        $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
      }
    } finally {
      $ctx.Response.Close()
    }
  }
} finally {
  $listener.Stop()
  $listener.Close()
}
