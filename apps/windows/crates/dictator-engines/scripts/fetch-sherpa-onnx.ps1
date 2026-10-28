#Requires -Version 7
<#
.SYNOPSIS
    Downloads the sherpa-onnx prebuilt static library archive for one target, verifies its pinned
    SHA-256 and prints the SHERPA_ONNX_ARCHIVE_DIR to build with.
.DESCRIPTION
    sherpa-onnx-sys 2.0.1 otherwise downloads the same archive from the GitHub release at build time
    without verifying it. With SHERPA_ONNX_ARCHIVE_DIR set it copies the archive from there instead.
    Digests are GitHub's release asset digests for tag v2.0.1. Bump them with the sherpa-onnx pin.
.EXAMPLE
    $env:SHERPA_ONNX_ARCHIVE_DIR = pwsh crates/dictator-engines/scripts/fetch-sherpa-onnx.ps1 win-x64
#>
param([ValidateSet('win-x64', 'win-arm64', 'osx-arm64')] [string] $Target = 'win-x64')

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$version = '2.0.1'
$archives = @{
    'win-x64'   = @("sherpa-onnx-v$version-win-x64-static-MT-Release-lib.tar.bz2", '23e7b986b197eda0bbc47ff3d7d5c87afd6ef2b9077214ba26b0f1ec52ef0e66')
    'win-arm64' = @("sherpa-onnx-v$version-win-arm64-static-MT-Release-lib.tar.bz2", 'ef118817dcf0a585974159855ff8b2a0f4c8069a158766dfd92a5f702b0173be')
    'osx-arm64' = @("sherpa-onnx-v$version-osx-arm64-static-lib.tar.bz2", 'd7a9b14f337958e7373c8c650f046df33d210bbaff2f33cb8322dda663ee5e1c')
}
$name, $expected = $archives[$Target]

$workspace = Resolve-Path (Join-Path $PSScriptRoot '..' '..' '..')
$dir = Join-Path $workspace 'target' 'sherpa-onnx-archives'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$archive = Join-Path $dir $name

function Get-Sha256([string] $File) { (Get-FileHash -Algorithm SHA256 -LiteralPath $File).Hash.ToLowerInvariant() }

if (-not (Test-Path -LiteralPath $archive -PathType Leaf) -or (Get-Sha256 $archive) -ne $expected) {
    $tmp = "$archive.download"
    try {
        Invoke-WebRequest -Uri "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$version/$name" `
            -OutFile $tmp -MaximumRetryCount 3 -RetryIntervalSec 2
        $actual = Get-Sha256 $tmp
        if ($actual -ne $expected) { throw "checksum mismatch for ${name}: expected $expected, got $actual" }
        Move-Item -Force -LiteralPath $tmp -Destination $archive
    }
    finally {
        Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $tmp
    }
}

# sherpa-onnx-sys reuses its own cache, unverified, before it looks at SHERPA_ONNX_ARCHIVE_DIR.
$cached = Join-Path $workspace 'target' 'sherpa-onnx-prebuilt' $name
if ((Test-Path -LiteralPath $cached -PathType Leaf) -and (Get-Sha256 $cached) -ne $expected) {
    throw "$cached does not match the pinned digest; delete target/sherpa-onnx-prebuilt and rebuild"
}

Write-Output $dir
