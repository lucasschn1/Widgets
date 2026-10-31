# Instala (ou atualiza) os Widgets, sem precisar de Python nem de administrador:
#
#   irm https://raw.githubusercontent.com/lucasschn1/Widgets/main/install.ps1 | iex
#
# Baixa o último Release do GitHub, instala em %LOCALAPPDATA%\Programs\Widgets, cria o atalho
# no Menu Iniciar e uma entrada em "Aplicativos instalados" (para desinstalar), e abre o app.
# Suas configurações ficam em %APPDATA%\Widgets e são mantidas nas atualizações.
# (Para testar com um .zip local: $env:WIDGETS_ZIP = "caminho\Widgets-win64.zip" antes de rodar.)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'  # o Invoke-WebRequest fica muito mais rápido sem a barra
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$Repo = 'lucasschn1/Widgets'
$Dir = Join-Path $env:LOCALAPPDATA 'Programs\Widgets'
$Exe = Join-Path $Dir 'Widgets.exe'
$Shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Widgets.lnk'
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Widgets'

Write-Host ''
Write-Host '  Widgets' -ForegroundColor Cyan
Write-Host ''

# 1. o pacote: o último Release do GitHub (ou um .zip local, para testes)
if ($env:WIDGETS_ZIP) {
    $zip = $env:WIDGETS_ZIP
    Write-Host "  Usando o pacote local $zip"
} else {
    Write-Host '  Procurando a versão mais recente...'
    $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest" -Headers @{ 'User-Agent' = 'widgets-installer' }
    $asset = $release.assets | Where-Object { $_.name -eq 'Widgets-win64.zip' } | Select-Object -First 1
    if (-not $asset) { throw "O Release $($release.tag_name) não tem o arquivo Widgets-win64.zip." }
    $zip = Join-Path $env:TEMP "Widgets-$($release.tag_name).zip"
    Write-Host "  Baixando a versão $($release.tag_name.TrimStart('v')) ($([math]::Round($asset.size / 1MB)) MB)..."
    Invoke-WebRequest $asset.browser_download_url -OutFile $zip -UseBasicParsing
}

# 2. fecha os Widgets, se estiverem abertos (os arquivos estariam em uso)
$running = Get-Process -Name 'Widgets' -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -like "$Dir\*" }
if ($running) {
    Write-Host '  Fechando os Widgets abertos...'
    $running | Stop-Process -Force
    Start-Sleep -Milliseconds 800
}

# 3. extrai e troca os arquivos do programa (as configurações, em %APPDATA%, não são tocadas)
Write-Host '  Instalando...'
$tmp = Join-Path $env:TEMP ("Widgets-" + [guid]::NewGuid().ToString('N'))
Expand-Archive -Path $zip -DestinationPath $tmp -Force
$src = if (Test-Path (Join-Path $tmp 'Widgets\Widgets.exe')) { Join-Path $tmp 'Widgets' } else { $tmp }
New-Item -ItemType Directory -Force -Path $Dir | Out-Null
foreach ($old in @('_internal', 'Widgets.exe')) {  # só o que é do programa (o desinstalador do Inno, se houver, fica)
    $path = Join-Path $Dir $old
    if (Test-Path $path) { Remove-Item $path -Recurse -Force }
}
Copy-Item -Path (Join-Path $src '*') -Destination $Dir -Recurse -Force
Remove-Item $tmp -Recurse -Force
if (-not $env:WIDGETS_ZIP) { Remove-Item $zip -Force -ErrorAction SilentlyContinue }

# 4. atalho no Menu Iniciar
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($Shortcut)
$link.TargetPath = $Exe
$link.WorkingDirectory = $Dir
$link.Description = 'Widgets da área de trabalho'
$link.Save()

# 5. desinstalador e entrada em Configurações > Aplicativos > Aplicativos instalados
$uninstall = @'
$ErrorActionPreference = 'SilentlyContinue'
$Dir = Join-Path $env:LOCALAPPDATA 'Programs\Widgets'
Get-Process -Name 'Widgets' | Where-Object { $_.Path -like "$Dir\*" } | Stop-Process -Force
Start-Sleep -Milliseconds 800
Remove-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'DesktopWidgets'
Remove-Item (Join-Path ([Environment]::GetFolderPath('Programs')) 'Widgets.lnk') -Force
Remove-Item 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Widgets' -Recurse -Force
Set-Location $env:TEMP
Start-Process cmd -ArgumentList "/c timeout /t 2 >nul & rmdir /s /q `"$Dir`"" -WindowStyle Hidden
'@
Set-Content -Path (Join-Path $Dir 'uninstall.ps1') -Value $uninstall -Encoding UTF8
$version = (Get-Item $Exe).VersionInfo.ProductVersion
New-Item -Path $UninstallKey -Force | Out-Null
$props = @{
    DisplayName     = 'Widgets'
    DisplayVersion  = $version
    Publisher       = 'lucasschn1'
    DisplayIcon     = $Exe
    InstallLocation = $Dir
    URLInfoAbout    = "https://github.com/$Repo"
    UninstallString = "powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$Dir\uninstall.ps1`""
}
foreach ($name in $props.Keys) { Set-ItemProperty -Path $UninstallKey -Name $name -Value $props[$name] }
Set-ItemProperty -Path $UninstallKey -Name 'NoModify' -Value 1 -Type DWord
Set-ItemProperty -Path $UninstallKey -Name 'NoRepair' -Value 1 -Type DWord

# 6. abre
Start-Process -FilePath $Exe -WorkingDirectory $Dir
Write-Host ''
Write-Host "  Pronto! Widgets $version instalado." -ForegroundColor Green
Write-Host '  Ele está no Menu Iniciar; para desinstalar, use Configurações > Aplicativos.'
Write-Host ''
