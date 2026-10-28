#Requires -Version 7
<#
.SYNOPSIS
    Downloads one model into apps\windows\Model\<model-id> and verifies every file against
    model-manifest.txt (pinned Hugging Face commits + SHA-256). Idempotent.
.DESCRIPTION
    For development and the end-to-end tests only: the app downloads and verifies models itself.
    macOS/Linux equivalent: scripts/fetch-model.sh.
.EXAMPLE
    pwsh apps\windows\scripts\fetch-model.ps1 [model-id]   # default: the first model in the manifest
#>
param([string] $Model)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $root 'model-manifest.txt'

$entries = Get-Content $manifest |
    Where-Object { $_ -and -not $_.StartsWith('#') } |
    ForEach-Object { , ($_ -split '\s+' | Where-Object { $_ }) } |
    Where-Object { $_.Count -eq 5 } |
    ForEach-Object { [pscustomobject]@{ Id = $_[0]; Sha256 = $_[1]; Bytes = $_[2]; Remote = $_[3]; Path = $_[4] } }

if (-not $Model) { $Model = $entries[0].Id }
$files = @($entries | Where-Object Id -CEQ $Model)
if ($files.Count -eq 0) { throw "unknown model: $Model" }
if ($Model -match '[\\/]|\.\.') { throw "refusing unsafe model id: $Model" }
$dest = Join-Path $root 'Model' $Model
New-Item -ItemType Directory -Force -Path $dest | Out-Null

function Get-Sha256([string] $File) { (Get-FileHash -Algorithm SHA256 -LiteralPath $File).Hash.ToLowerInvariant() }

foreach ($file in $files) {
    if ($file.Path -match '^[\\/]|^[A-Za-z]:|\.\.') { throw "refusing unsafe path: $($file.Path)" }
    $target = Join-Path $dest $file.Path
    if ((Test-Path -LiteralPath $target -PathType Leaf) -and (Get-Sha256 $target) -eq $file.Sha256) { continue }

    Write-Host "↓ $Model/$($file.Path)"
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target) | Out-Null
    $tmp = Join-Path $dest ".download.$([guid]::NewGuid().ToString('N'))"
    try {
        Invoke-WebRequest -Uri "https://huggingface.co/$($file.Remote)" -OutFile $tmp `
            -MaximumRetryCount 3 -RetryIntervalSec 2 -SslProtocol Tls12, Tls13
        $actual = Get-Sha256 $tmp
        if ($actual -ne $file.Sha256) { throw "checksum mismatch for $($file.Path): expected $($file.Sha256), got $actual" }
        Move-Item -Force -LiteralPath $tmp -Destination $target
    }
    finally {
        Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $tmp
    }
}

# Remove files that are not in the manifest for this model, so a reused folder stays exact.
$expected = $files.Path | ForEach-Object { $_ -replace '\\', '/' }
Get-ChildItem -LiteralPath $dest -Recurse -File | Where-Object { -not $_.Name.StartsWith('.') } | ForEach-Object {
    $relative = [IO.Path]::GetRelativePath($dest, $_.FullName) -replace '\\', '/'
    if ($relative -cnotin $expected) {
        Write-Host "✗ removing $Model/$relative (not in manifest)"
        Remove-Item -Force -LiteralPath $_.FullName
    }
}
Get-ChildItem -LiteralPath $dest -Recurse -Directory | Sort-Object { $_.FullName.Length } -Descending |
    Where-Object { -not (Get-ChildItem -LiteralPath $_.FullName -Force) } |
    Remove-Item -Force

Write-Host "✓ Model verified in $dest"
