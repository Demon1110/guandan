# 零依赖静态服务（wasm 必须走 http(s) 才能 fetch，不能用 file:// 直接打开）。
# 用法：pwsh -File apps/client/serve.ps1 [-Port 8123]
param([int]$Port = 8123)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path $PSScriptRoot).Path

$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add("http://127.0.0.1:$Port/")
$listener.Start()
Write-Host "serving $root at http://127.0.0.1:$Port/"

$types = @{
    '.html' = 'text/html; charset=utf-8'
    '.js'   = 'text/javascript; charset=utf-8'
    '.css'  = 'text/css; charset=utf-8'
    '.wasm' = 'application/wasm'
    '.mjs'  = 'text/javascript; charset=utf-8'
    '.json' = 'application/json; charset=utf-8'
}

try {
    while ($listener.IsListening) {
        $ctx = $listener.GetContext()
        $rel = $ctx.Request.Url.AbsolutePath.TrimStart('/')
        if ([string]::IsNullOrEmpty($rel)) { $rel = 'index.html' }
        $path = Join-Path $root $rel
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $ctx.Response.StatusCode = 404
            $ctx.Response.Close()
            continue
        }
        $ext = [System.IO.Path]::GetExtension($path).ToLowerInvariant()
        $ctx.Response.ContentType = if ($types.ContainsKey($ext)) { $types[$ext] } else { 'application/octet-stream' }
        $bytes = [System.IO.File]::ReadAllBytes($path)
        $ctx.Response.ContentLength64 = $bytes.Length
        $ctx.Response.OutputStream.Write($bytes, 0, $bytes.Length)
        $ctx.Response.Close()
    }
}
finally {
    $listener.Stop()
    $listener.Close()
}
