# Gera o dist\Widgets\Widgets.exe e o dist\Widgets-win64.zip (o que o install.ps1 baixa):
#
#   powershell -ExecutionPolicy Bypass -File packaging\build.ps1
#
# Precisa do Rust (https://rustup.rs) e das ferramentas de compilação do Visual Studio.
# O instalador (Widgets-Setup.exe) é gerado depois, pelo Inno Setup, com packaging\Widgets.iss.

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
$Dist = Join-Path $Root 'dist'
$Out = Join-Path $Dist 'Widgets'

Push-Location $Root
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo build falhou' }
} finally {
    Pop-Location
}

if (Test-Path $Out) { Remove-Item $Out -Recurse -Force }
New-Item -ItemType Directory -Force -Path $Out | Out-Null
Copy-Item (Join-Path $Root 'target\release\widgets.exe') (Join-Path $Out 'Widgets.exe')

$zip = Join-Path $Dist 'Widgets-win64.zip'
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $Out -DestinationPath $zip -CompressionLevel Optimal

$exe = Get-Item (Join-Path $Out 'Widgets.exe')
Write-Host ''
Write-Host "Pronto: $($exe.FullName)  (versão $($exe.VersionInfo.ProductVersion), $([math]::Round($exe.Length / 1MB, 1)) MB)"
Write-Host "        $zip  ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB)"
