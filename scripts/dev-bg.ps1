# Starts `tauri dev` with the MSVC/Windows SDK env, streaming output to
# $env:TEMP\ktl-dev.log. Usage (background):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\dev-bg.ps1
$msvc = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64'
$sdkVer = (Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Directory |
    Where-Object Name -match '^\d+\.\d+\.\d+\.\d+$' |
    Sort-Object Name -Descending | Select-Object -First 1).Name
$sdk = "C:\Program Files (x86)\Windows Kits\10\bin\$sdkVer\x64"
$env:PATH = "$msvc;$sdk;$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:INCLUDE = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\include;C:\Program Files (x86)\Windows Kits\10\Include\$sdkVer\ucrt;C:\Program Files (x86)\Windows Kits\10\Include\$sdkVer\um;C:\Program Files (x86)\Windows Kits\10\Include\$sdkVer\shared"
$env:LIB = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207\lib\x64;C:\Program Files (x86)\Windows Kits\10\Lib\$sdkVer\ucrt;C:\Program Files (x86)\Windows Kits\10\Lib\$sdkVer\um;C:\Program Files (x86)\Windows Kits\10\Lib\$sdkVer\shared"
Set-Location (Split-Path $PSScriptRoot -Parent)
& npx tauri dev 2>&1 | Out-File -FilePath $env:TEMP\ktl-dev.log -Encoding utf8
