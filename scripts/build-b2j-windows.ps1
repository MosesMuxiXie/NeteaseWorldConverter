# Reproducible Windows x64 source build. Never replaces the shipped b2j binary.
# Example: .\scripts\build-b2j-windows.ps1 -WorkDir .e2e-b2j-source -CMake 'C:\path\to\cmake.exe'
param(
    [Parameter(Mandatory = $true)][string]$WorkDir,
    [string]$CMake = 'cmake',
    [string]$CandidatePatch
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$work = [IO.Path]::GetFullPath($WorkDir)
if (Test-Path -LiteralPath $work) {
    throw "Build directory already exists: $work"
}
New-Item -ItemType Directory -Path $work | Out-Null
$source = Join-Path $work 'je2be-core'
$build = Join-Path $work 'build'

git clone --depth 1 --no-checkout --branch web-4.3.0 https://github.com/kbinani/je2be-core.git $source
if ($LASTEXITCODE -ne 0) { throw 'je2be-core clone failed' }
# The upstream test/data tree contains Windows-incompatible long paths and is not needed for b2j.
git -C $source sparse-checkout set --no-cone '/*' '!/*/' '/src/' '/include/' '/example/' '/deps/' '/cmake/' '/test/*' '!/test/data/'
if ($LASTEXITCODE -ne 0) { throw 'sparse checkout failed' }
git -C $source checkout web-4.3.0
if ($LASTEXITCODE -ne 0) { throw 'tag checkout failed' }
$revision = (git -C $source rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $revision -ne 'b3facb832c39222d3d6f9bcb5192e4c0b1bd0691') {
    throw "unexpected je2be-core revision: $revision"
}

$buildPatch = Join-Path $root 'scripts\patches\je2be-web-4.3.0-msvc-build.patch'
git -C $source apply --check $buildPatch
if ($LASTEXITCODE -ne 0) { throw 'MSVC build patch does not apply' }
git -C $source apply $buildPatch
if ($LASTEXITCODE -ne 0) { throw 'MSVC build patch failed' }
if ($CandidatePatch) {
    $candidate = [IO.Path]::GetFullPath($CandidatePatch)
    git -C $source apply --check $candidate
    if ($LASTEXITCODE -ne 0) { throw 'candidate patch does not apply' }
    git -C $source apply $candidate
    if ($LASTEXITCODE -ne 0) { throw 'candidate patch failed' }
}

Write-Output "je2be-core revision: $revision"
& $CMake -S $source -B $build -G 'Visual Studio 17 2022' -A x64 '-DCMAKE_POLICY_VERSION_MINIMUM=3.5'
if ($LASTEXITCODE -ne 0) { throw 'CMake configuration failed' }
& $CMake --build $build --config Release --target b2j --parallel 1
if ($LASTEXITCODE -ne 0) { throw 'b2j source build failed' }

$binary = Join-Path $build 'Release\b2j.exe'
if (-not (Test-Path -LiteralPath $binary)) { throw "Built binary not found: $binary" }
Get-FileHash -Algorithm SHA256 -LiteralPath $binary
