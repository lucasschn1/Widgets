# Instala (ou atualiza) os Widgets, sem precisar de administrador:
#
#   irm https://raw.githubusercontent.com/lucasschn1/Widgets/main/install.ps1 | iex
#
# Baixa o último Release do GitHub, instala em %LOCALAPPDATA%\Programs\Widgets, cria o atalho
# no Menu Iniciar e uma entrada em "Aplicativos instalados" (para desinstalar), e abre o app.
# Suas configurações ficam em %APPDATA%\Widgets e são mantidas nas atualizações.
# (Para testar com um .zip local: $env:WIDGETS_ZIP = "caminho\Widgets-win64.zip" antes de rodar.)
#
# Antes de fechar os widgets abertos, confere se o Windows deixa a versão nova abrir (o
# Controle Inteligente de Aplicativos barra programas novos sem assinatura digital); se não
# deixar, nada é trocado. Códigos de saída (o botão "Atualizar" do app os lê):
#   0 instalado e aberto; 3 o Windows barrou a versão nova (nada mudou);
#   4 a versão nova não abriu depois de instalada (a anterior voltou); 1 outro erro.

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
# o que o app manda para a internet, antes de instalar (exigência da assinatura pela SignPath)
Write-Host '  Privacidade: sem conta, telemetria nem anúncios. O app só busca na internet o que cada'
Write-Host '  widget mostra (clima, GitHub, capa em alta do YouTube) e procura atualizações no GitHub.'
Write-Host "  O que é enviado e como desligar: https://github.com/$Repo#privacidade"
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

# 2. extrai numa pasta temporária
$tmp = Join-Path $env:TEMP ("Widgets-" + [guid]::NewGuid().ToString('N'))
Expand-Archive -Path $zip -DestinationPath $tmp -Force
$src = if (Test-Path (Join-Path $tmp 'Widgets\Widgets.exe')) { Join-Path $tmp 'Widgets' } else { $tmp }

# 3. o Windows deixa a versão nova abrir? Ela roda como a linha de status do Claude Code, com a
# entrada vazia: só escreve uma linha e fecha, sem abrir os widgets nem gravar nada. Toda versão 3.x
# conhece essa opção (a --versao só existe da 3.11 em diante: numa versão mais velha, ela abriria
# o app inteiro, da pasta temporária, no computador de quem ainda não tem os widgets abertos).
# (a entrada e a saída do teste ficam fora da pasta extraída, que é copiada para a instalação)
$empty = "$tmp-entrada.txt"
$out = "$tmp-saida.txt"
Set-Content -Path $empty -Value '' -NoNewline
try {
    $probe = Start-Process -FilePath (Join-Path $src 'Widgets.exe') -ArgumentList '--claude-status' -RedirectStandardInput $empty -RedirectStandardOutput $out -WindowStyle Hidden -PassThru
    if (-not $probe.WaitForExit(10000)) { $probe | Stop-Process -Force -ErrorAction SilentlyContinue }
} catch {
    Remove-Item $empty, $out -Force -ErrorAction SilentlyContinue
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host ''
    Write-Host '  O Windows não deixou a versão nova abrir.' -ForegroundColor Yellow
    Write-Host '  O Controle Inteligente de Aplicativos barra programas novos que ainda não têm'
    Write-Host '  assinatura digital. Os Widgets continuam na versão que você já tinha.'
    Write-Host "  ($($_.Exception.Message))"
    Write-Host ''
    exit 3
}

Remove-Item $empty, $out -Force -ErrorAction SilentlyContinue

# 4. fecha os Widgets, se estiverem abertos (os arquivos estariam em uso)
$running = Get-Process -Name 'Widgets' -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path -like "$Dir\*" }
if ($running) {
    Write-Host '  Fechando os Widgets abertos...'
    $running | Stop-Process -Force
    Start-Sleep -Milliseconds 800
}

# 5. troca os arquivos do programa (as configurações, em %APPDATA%, não são tocadas), guardando o
# Widgets.exe anterior para voltar a ele se o novo não abrir
Write-Host '  Instalando...'
New-Item -ItemType Directory -Force -Path $Dir | Out-Null
$backup = Join-Path $Dir 'Widgets.anterior.exe'
if (Test-Path $backup) { Remove-Item $backup -Force }
if (Test-Path $Exe) { Move-Item $Exe $backup -Force }
# só o que é do programa (o desinstalador do Inno, se houver, fica); _internal era da versão 2.x, em Python
$internal = Join-Path $Dir '_internal'
if (Test-Path $internal) { Remove-Item $internal -Recurse -Force }
Copy-Item -Path (Join-Path $src '*') -Destination $Dir -Recurse -Force
Remove-Item $tmp -Recurse -Force
if (-not $env:WIDGETS_ZIP) { Remove-Item $zip -Force -ErrorAction SilentlyContinue }

# 6. atalho no Menu Iniciar
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($Shortcut)
$link.TargetPath = $Exe
$link.WorkingDirectory = $Dir
$link.Description = 'Widgets da área de trabalho'
$link.Save()

# 7. desinstalador e entrada em Configurações > Aplicativos > Aplicativos instalados
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

# 8. abre; se a versão nova não abrir, volta a anterior (e abre ela)
try {
    Start-Process -FilePath $Exe -WorkingDirectory $Dir
} catch {
    if (Test-Path $backup) {
        Remove-Item $Exe -Force -ErrorAction SilentlyContinue
        Move-Item $backup $Exe -Force
        Start-Process -FilePath $Exe -WorkingDirectory $Dir -ErrorAction SilentlyContinue
    }
    Write-Host ''
    Write-Host '  A versão nova não abriu; os Widgets voltaram para a versão anterior.' -ForegroundColor Yellow
    Write-Host "  ($($_.Exception.Message))"
    Write-Host ''
    exit 4
}
Remove-Item $backup -Force -ErrorAction SilentlyContinue
Write-Host ''
Write-Host "  Pronto! Widgets $version instalado." -ForegroundColor Green
Write-Host '  Ele está no Menu Iniciar; para desinstalar, use Configurações > Aplicativos.'
Write-Host ''
