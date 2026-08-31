# prepare-resources.ps1 — 填充 backends / runtime 目录（Windows）。
# 来源默认是原版便携包 NeteaseWorldConverter（与本仓库同级目录）。
# 用法：npm run prepare:win  —— 或传来源目录：.\scripts\prepare-resources.ps1 <sourceDir>

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$source = if ($args.Count -ge 1) { $args[0] } else { Join-Path (Split-Path -Parent $root) "NeteaseWorldConverter" }
if (-not (Test-Path $source)) { throw "找不到原版包目录：$source（可手动传入路径）" }

$backends = Join-Path $root "src-tauri\backends"
$runtime  = Join-Path $root "src-tauri\runtime"
New-Item -ItemType Directory -Force -Path $backends | Out-Null
New-Item -ItemType Directory -Force -Path $runtime  | Out-Null

# Chunker CLI
Copy-Item (Join-Path $source "app\chunker-cli.jar") $backends -Force
# b2j + LLVM 运行时
foreach ($name in @("b2j.exe", "libc++.dll", "libunwind.dll")) {
    Copy-Item (Join-Path $source "app\native\$name") $backends -Force
}
# jlink Java 运行时（Chunker 子进程使用）
if (Test-Path (Join-Path $source "runtime")) {
    Copy-Item (Join-Path $source "runtime\*") $runtime -Recurse -Force
} else {
    Write-Warning "原版包没有 runtime 目录；Chunker 将回退使用系统 java"
}

Write-Host "backends:" -ForegroundColor Cyan
Get-ChildItem $backends | ForEach-Object { Write-Host ("  {0}  ({1:N1} MB)" -f $_.Name, ($_.Length / 1MB)) }
Write-Host "runtime:" -ForegroundColor Cyan
$size = (Get-ChildItem $runtime -Recurse -File | Measure-Object Length -Sum).Sum
Write-Host ("  {0:N1} MB" -f ($size / 1MB))
Write-Host "OK" -ForegroundColor Green
