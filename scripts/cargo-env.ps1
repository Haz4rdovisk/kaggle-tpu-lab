# Sets the MSVC/Windows SDK environment for cargo, then runs the command.
# Usage: powershell -ExecutionPolicy Bypass -File scripts\cargo-env.ps1 -Command "check"
param([Parameter(Mandatory)][string]$Command)

$msvc = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64'
$sdkVer = (Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Directory |
    Where-Object Name -match '^\d+\.\d+\.\d+\.\d+$' |
    Sort-Object Name -Descending | Select-Object -First 1).Name
$sdk = "C:\Program Files (x86)\Windows Kits\10\bin\$sdkVer\x64"

$env:PATH = "$msvc;$sdk;$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:INCLUDE = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\include;C:\Program Files (x86)\Windows Kits\10\Include\$sdkVer\ucrt;C:\Program Files (x86)\Windows Kits\10\Include\$sdkVer\um;C:\Program Files (x86)\Windows Kits\10\Include\$sdkVer\shared"
$env:LIB = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\lib\x64;C:\Program Files (x86)\Windows Kits\10\Lib\$sdkVer\ucrt\x64;C:\Program Files (x86)\Windows Kits\10\Lib\$sdkVer\um\x64"

Set-Location (Split-Path $PSScriptRoot -Parent)
Write-Host "MSVC: $msvc"
Write-Host "SDK:  $sdkVer"
$code = 0
$argv = @("--manifest-path", "src-tauri/Cargo.toml") + ($Command -split ' ')
& cargo $argv 2>&1 | ForEach-Object { Write-Host $_ }
$code = $LASTEXITCODE
exit $code
