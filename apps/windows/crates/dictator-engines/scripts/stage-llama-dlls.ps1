# Copies the llama.cpp DLLs next to the built binaries and tests, and prints their names.
# llama-cpp-sys-2 is supposed to copy them, but on MSVC it only copies the import .lib files.
#   pwsh stage-llama-dlls.ps1 debug|release
param([Parameter(Mandatory)][string]$BuildProfile)
$ErrorActionPreference = 'Stop'

$target = Resolve-Path (Join-Path $PSScriptRoot '../../../target')
$bin = Get-ChildItem "$target/$BuildProfile/build/llama-cpp-sys-2-*/out/bin" -Directory -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $bin) { throw "No llama.cpp build in $target/$BuildProfile; build first." }
$dlls = @(Get-ChildItem "$bin/*.dll")
if (-not $dlls) { throw "No DLLs in $bin." }
foreach ($dir in "$target/$BuildProfile", "$target/$BuildProfile/deps") {
    Copy-Item $dlls $dir -Force
}
$dlls.Name
