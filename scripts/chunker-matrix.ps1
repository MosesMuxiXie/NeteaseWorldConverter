# chunker-matrix.ps1 — 用真实/中间 Java 世界对 Chunker 支持的全部 JAVA_* 目标跑一遍转换矩阵。
# 用法:
#   powershell -ExecutionPolicy Bypass -File scripts/chunker-matrix.ps1
#   powershell -ExecutionPolicy Bypass -File scripts/chunker-matrix.ps1 -InputDir <dir> -Only JAVA_1_21_11,JAVA_26_3
param(
    [Parameter(Mandatory = $true)]
    [string]$InputDir,
    [string]$Java = "",
    [string]$Jar = "",
    [string]$Root = "",
    [string]$Results = "",
    [string[]]$Only = @()
)

$ErrorActionPreference = "Continue"
$repo = Split-Path -Parent $PSScriptRoot
if (-not $Java) { $Java = Join-Path $repo "src-tauri\runtime\bin\java.exe" }
if (-not $Jar) { $Jar = Join-Path $repo "src-tauri\backends\chunker-cli.jar" }
if (-not $Root) { $Root = Join-Path $env:TEMP "nwc-matrix" }
if (-not $Results) { $Results = Join-Path $repo "scripts\chunker-matrix-results.csv" }

if (-not (Test-Path $InputDir)) { throw "输入世界目录不存在: $InputDir" }
if (-not (Test-Path $Java)) { throw "Java 不存在: $Java" }
if (-not (Test-Path $Jar)) { throw "Chunker jar 不存在: $Jar" }

$logRoot = Join-Path $Root "logs"
New-Item -ItemType Directory -Force -Path $Root | Out-Null
New-Item -ItemType Directory -Force -Path $logRoot | Out-Null

# 从 `-f ?` 帮助里枚举 Chunker 认识的全部 Java 目标。
# 注意：不能用 Out-String——它会按控制台宽度折行，可能把一个 token 拆成两行
# （实测漏掉 JAVA_1_8_8…1.11、JAVA_1_12/1.12.1、JAVA_1_20_6、JAVA_1_21_8 等）。
# 逐行原样收进数组后再 join，可保证每个 token 完整落在一行里。
$helpLines = @()
& $Java -jar $Jar -f "?" 2>&1 | ForEach-Object { $script:helpLines = $script:helpLines + [string]$_ }
$help = $script:helpLines -join "`n"
if ($help -notmatch "JAVA_") { throw "未能读取 Chunker 目标列表" }
$tokens = [regex]::Matches($help, "JAVA_(?:26|1)_\d+(?:_\d+){0,2}") |
    ForEach-Object { $_.Value } | Sort-Object -Unique

function Parse-Token([string]$token) {
    $parts = $token.Split("_")
    $major = [int]$parts[1]
    $minor = [int]$parts[2]
    $patch = 0
    if ($parts.Count -ge 4) { $patch = [int]$parts[3] }
    return ,@($major, $minor, $patch)
}

$tokens = @($tokens | Sort-Object @{Expression = {
        $v = Parse-Token $_
        return -($v[0] * 1000000 + $v[1] * 1000 + $v[2])
    }})
if ($Only.Count -gt 0) {
    $wanted = @($Only | ForEach-Object { $_.Trim().ToUpper() })
    $tokens = @($tokens | Where-Object { $wanted -contains $_ })
}
Write-Host "矩阵目标数: $($tokens.Count)"
Write-Host "输入: $InputDir"

$rows = @("token,exitCode,ok,elapsedSec,bytes,error")

foreach ($token in $tokens) {
    $outDir = Join-Path $Root $token
    if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir }
    New-Item -ItemType Directory -Path $outDir | Out-Null
    $stdout = Join-Path $logRoot "$token.out.log"
    $stderr = Join-Path $logRoot "$token.err.log"
    $startedAt = Get-Date
    $argList = @("-Xms512m", "-Xmx4G", "-jar", $Jar, "-i", $InputDir, "-o", $outDir, "-f", $token)
    $exitCode = 0
    try {
        & $Java @argList *> $stdout
        $exitCode = $LASTEXITCODE
    } catch {
        $exitCode = -1
        $_.Exception.Message | Out-File -FilePath $stderr -Encoding utf8
    }
    $elapsedSec = [int]((Get-Date) - $startedAt).TotalSeconds
    $level = Join-Path $outDir "level.dat"
    $ok = (Test-Path $level)
    $bytes = if ($ok) { (Get-Item $level).Length } else { 0 }
    $errText = ""
    if (-not $ok -or $exitCode -ne 0) {
        $tail = if (Test-Path $stdout) { (Get-Content $stdout -Tail 8 -ErrorAction SilentlyContinue) -join " | " } else { "" }
        if (Test-Path $stderr) {
            $errTail = (Get-Content $stderr -Tail 8 -ErrorAction SilentlyContinue) -join " | "
            if ($errTail) { $tail = "$tail :: $errTail" }
        }
        if ($tail.Length -gt 600) { $tail = $tail.Substring(0, 600) }
        $errText = $tail
    }
    $csv = $token + "," + $exitCode + "," + $ok + "," + $elapsedSec + "," + $bytes + ',"' + $errText.Replace('"', '""') + '"'
    $rows += $csv
    Write-Host ("{0,-16} exit={1,-3} ok={2,-5} {3,4}s level.dat={4}" -f $token, $exitCode, $ok, $elapsedSec, $bytes)
    if (-not $ok -and $errText) { Write-Host "   error: $errText" }
}

$rows -join "`r`n" | Out-File -FilePath $Results -Encoding utf8
$passed = 0
for ($i = 1; $i -lt $rows.Count; $i++) {
    $fields = $rows[$i].Split(",", 5)
    if ($fields.Length -ge 3 -and $fields[1] -eq "0" -and $fields[2] -eq "True") { $passed++ }
}
Write-Host "结果: $Results"
Write-Host ("通过 {0}/{1}" -f $passed, $tokens.Count)
