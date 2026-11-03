<#
Aplica o código desta pasta (a branch atual, com as alterações ainda não commitadas) nos
widgets instalados, para ver na hora se algo quebrou antes de commitar.

    powershell -File dev\aplicar.ps1            compila, testa e troca o Widgets.exe instalado
    powershell -File dev\aplicar.ps1 -Voltar    volta para a versão lançada (a do Release)
    powershell -File dev\aplicar.ps1 -Auto      modo do hook do Claude Code (ver CLAUDE.md)

O que ele faz:
1. compila a versão final (cargo build --release) e roda os testes;
2. guarda o Widgets.exe lançado como Widgets.release.exe (e as opções como
   config.release.json), só na primeira troca;
3. fecha os widgets, põe o novo no lugar e abre;
4. confere se o app ficou aberto. Se fechou sozinho (ou o Windows barrou o arquivo), volta o
   anterior e avisa.

No modo -Auto (hook "Stop" do Claude Code, ao fim de cada resposta):
- na main, com código alterado e sem branch: pede para criar uma branch;
- numa branch, só recompila se o código mudou desde a última vez;
- erro de compilação, teste falhando ou app que fecha sozinho voltam para o Claude corrigir
  (código de saída 2); o resto vira um aviso curto na tela.
#>
param(
    [switch]$Auto,
    [switch]$Voltar
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$OutputEncoding = [Text.Encoding]::UTF8

$Repo = Split-Path -Parent $PSScriptRoot
$Install = Join-Path $env:LOCALAPPDATA 'Programs\Widgets'
$Exe = Join-Path $Install 'Widgets.exe'
$Release = Join-Path $Install 'Widgets.release.exe'
$Previous = Join-Path $Install 'Widgets.anterior.exe'
$Marker = Join-Path $Install 'build-dev.json'
$Data = Join-Path $env:APPDATA 'Widgets'
$Built = Join-Path $Repo 'target\release\widgets.exe'
$FailedAt = Join-Path $Repo 'target\aplicar-falhou.txt'
# o que entra na versão final (mudança só no README não recompila nada)
$CodePaths = @('src', 'res', 'Cargo.toml', 'Cargo.lock', 'build.rs')

# ---------- saída ----------

function Done([string]$message) {
    # aviso na tela do Claude Code (modo -Auto) ou no terminal
    if ($Auto) {
        if ($message) { @{ systemMessage = $message } | ConvertTo-Json -Compress | Write-Output }
    } elseif ($message) {
        Write-Host $message
    }
    exit 0
}

function Fail([string]$message) {
    # no modo -Auto, o código 2 devolve a mensagem para o Claude, que continua e corrige
    [Console]::Error.WriteLine($message)
    exit $(if ($Auto) { 2 } else { 1 })
}

# ---------- ajudantes ----------

# (no PowerShell 5.1, um programa que escreve no stderr vira erro fatal com 'Stop': por isso o
# 'Continue' local nas chamadas ao git e ao cargo)
function GitOut([string[]]$arguments) {
    $ErrorActionPreference = 'Continue'
    $out = & git -C $Repo @arguments 2>$null
    if ($LASTEXITCODE -ne 0) { return '' }
    return ($out -join "`n")
}

function Sha256([string]$text) {
    $bytes = [Text.Encoding]::UTF8.GetBytes($text)
    $hash = [Security.Cryptography.SHA256]::Create().ComputeHash($bytes)
    return ([BitConverter]::ToString($hash) -replace '-', '').ToLower()
}

function FileHash([string]$path) {
    if (-not (Test-Path $path)) { return '' }
    return (Get-FileHash $path -Algorithm SHA256).Hash.ToLower()
}

# Impressão digital do código: o commit, as alterações e os arquivos novos de código.
function CodeFingerprint {
    $head = GitOut @('rev-parse', 'HEAD')
    $diff = GitOut (@('diff', 'HEAD', '--binary', '--') + $CodePaths)
    $new = GitOut (@('ls-files', '--others', '--exclude-standard', '--') + $CodePaths)
    $contents = ''
    foreach ($f in ($new -split "`n" | Where-Object { $_ })) {
        $contents += $f + (FileHash (Join-Path $Repo $f))
    }
    return Sha256 "$head`n$diff`n$contents"
}

function CodeChanged {
    $diff = GitOut (@('status', '--porcelain', '--') + $CodePaths)
    return [bool]$diff
}

function ReadMarker {
    if (-not (Test-Path $Marker)) { return $null }
    try { $m = Get-Content $Marker -Raw -Encoding UTF8 | ConvertFrom-Json } catch { return $null }
    # o app pode ter sido atualizado por um Release desde então: aí a marca não vale mais
    if ($m.exe -ne (FileHash $Exe)) { return $null }
    return $m
}

function Running {
    return @(Get-Process -Name 'Widgets' -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe })
}

function StopWidgets {
    foreach ($p in Running) {
        Stop-Process -Id $p.Id -Force
        $p.WaitForExit(5000) | Out-Null
    }
}

# Copia por cima, tentando de novo enquanto o arquivo estiver em uso (a linha de status do
# Claude Code abre o Widgets.exe a cada resposta).
function CopyOver([string]$from, [string]$to) {
    for ($i = 0; $i -lt 20; $i++) {
        try { Copy-Item $from $to -Force; return } catch { Start-Sleep -Milliseconds 300 }
    }
    Copy-Item $from $to -Force
}

# Abre os widgets pelo Explorer: assim eles não ficam presos ao processo do hook (que o
# Claude Code encerra ao terminar) e abrem como se fosse pelo Menu Iniciar.
function Launch {
    Start-Process explorer.exe -ArgumentList "`"$Exe`"" | Out-Null
}

# Abre os widgets e diz se ficaram abertos depois de alguns segundos.
function StartAndCheck {
    $since = Get-Date
    Launch
    Start-Sleep -Seconds 5
    if ((Running).Count -gt 0) { return 'ok' }
    $blocked = Get-WinEvent -LogName 'Microsoft-Windows-CodeIntegrity/Operational' -MaxEvents 30 -ErrorAction SilentlyContinue |
        Where-Object { $_.TimeCreated -ge $since.AddSeconds(-2) -and $_.Id -in 3033, 3077 }
    if ($blocked) { return 'bloqueado' }
    return 'fechou'
}

# Roda um comando do cargo e devolve (código de saída, saída). Um executável recém-compilado
# pode ser barrado por alguns segundos pelo Controle Inteligente de Aplicativos ("never
# executed"): aí tenta de novo.
function CargoStep([string[]]$arguments) {
    $ErrorActionPreference = 'Continue'
    for ($i = 0; $i -lt 3; $i++) {
        Push-Location $Repo
        try {
            # (as linhas do stderr chegam como erros; uma linha vazia vira "RemoteException")
            $out = & cargo @arguments 2>&1 | ForEach-Object { "$_" -replace '^System\.Management\.Automation\.RemoteException$', '' }
            $code = $LASTEXITCODE
        } finally { Pop-Location }
        $text = $out -join "`n"
        if ($code -eq 0 -or $text -notmatch 'never executed') { return @($code, $text) }
        Start-Sleep -Seconds 3
    }
    return @(-1, $text)
}

# Só as linhas que importam de uma saída longa do cargo.
function Errors([string]$text, [string]$pattern) {
    $lines = $text -split "`n"
    $hits = for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match $pattern) { $lines[$i..([Math]::Min($i + 12, $lines.Count - 1))] -join "`n" }
    }
    $joined = ($hits | Select-Object -First 6) -join "`n---`n"
    if (-not $joined) { $joined = ($lines | Select-Object -Last 30) -join "`n" }
    return $joined
}

# ---------- voltar para a versão lançada ----------

if ($Voltar) {
    if (-not (Test-Path $Release)) { Done 'Os widgets já estão na versão lançada.' }
    StopWidgets
    CopyOver $Release $Exe
    Remove-Item $Release, $Marker, $Previous -Force -ErrorAction SilentlyContinue
    Launch
    Done "Widgets de volta à versão lançada. As opções de antes da primeira troca estão em $Data\config.release.json."
}

# ---------- modo do hook ----------

if ($Auto) {
    $hook = $null
    try { $hook = [Console]::In.ReadToEnd() | ConvertFrom-Json } catch {}
    $again = $hook -and $hook.stop_hook_active
    $branch = GitOut @('branch', '--show-current')
    if (-not $branch) { Done '' }
    if ($branch -in 'main', 'master') {
        if ((CodeChanged) -and -not $again) {
            Fail "Há alterações de código direto na $branch. Crie uma branch para elas (git switch -c tipo/descricao, ex.: feat/... ou fix/...): o hook só aplica branches nos widgets, e a main só recebe código por PR."
        }
        Done ''
    }
    $applied = ReadMarker
    if ($applied -and $applied.codigo -eq (CodeFingerprint)) { Done '' }
    # o Claude já foi devolvido por um erro deste hook e parou de novo sem mudar o código: não
    # insiste em loop, só avisa
    if ($again -and (Test-Path $FailedAt) -and (Get-Content $FailedAt -Raw).Trim() -eq (CodeFingerprint)) {
        Done "A branch $branch ainda não passa (compilação, testes ou o app abrindo); os widgets continuam na versão anterior."
    }
}

# ---------- compilar, testar e aplicar ----------

if (-not (Test-Path $Exe)) { Fail "Os widgets não estão instalados em $Install (instale pelo install.ps1 primeiro)." }
$branch = GitOut @('branch', '--show-current')
$fingerprint = CodeFingerprint

function RememberFailure {
    # anota a falha para o modo -Auto não recompilar o mesmo código em loop
    New-Item -ItemType Directory -Force (Split-Path $FailedAt) | Out-Null
    Set-Content $FailedAt $fingerprint -Encoding ASCII
}

$build = CargoStep @('build', '--release', '--locked')
if ($build[0] -ne 0) {
    RememberFailure
    Fail ("A branch $branch não compila; os widgets continuam como estavam.`n`n" + (Errors $build[1] '^error'))
}

$tests = CargoStep @('test', '--release', '--locked', '--bin', 'widgets')
if ($tests[0] -eq -1) {
    $testNote = ' (os testes não rodaram: o Windows barrou o executável de teste)'
} elseif ($tests[0] -ne 0) {
    RememberFailure
    Fail ("Testes falhando na branch $branch; os widgets continuam como estavam.`n`n" + (Errors $tests[1] "^---- |panicked|FAILED"))
} else {
    $testNote = ''
}

# primeira troca: guarda a versão lançada e as opções de agora
if (-not (ReadMarker)) {
    Copy-Item $Exe $Release -Force
    if (Test-Path "$Data\config.json") { Copy-Item "$Data\config.json" "$Data\config.release.json" -Force }
}

StopWidgets
Copy-Item $Exe $Previous -Force
CopyOver $Built $Exe
$result = StartAndCheck

if ($result -ne 'ok') {
    StopWidgets
    CopyOver $Previous $Exe
    Launch
    if ($result -eq 'bloqueado') {
        Done "O Controle Inteligente de Aplicativos barrou a build da branch $branch (arquivo novo, sem assinatura). Os widgets voltaram para a versão anterior; tente de novo em alguns minutos com: powershell -File dev\aplicar.ps1"
    }
    RememberFailure
    Fail "A build da branch $branch abriu e fechou sozinha em menos de 5 s. Os widgets voltaram para a versão anterior. Rode 'cargo run' para ver o erro."
}

$commit = GitOut @('rev-parse', '--short', 'HEAD')
$dirty = if (CodeChanged) { ' + alterações não commitadas' } else { '' }
[pscustomobject]@{
    branch   = $branch
    commit   = $commit
    codigo   = $fingerprint
    exe      = (FileHash $Exe)
    aplicado = (Get-Date).ToString('s')
} | ConvertTo-Json | Set-Content $Marker -Encoding UTF8
Remove-Item $FailedAt -Force -ErrorAction SilentlyContinue

Done "Widgets atualizados com a branch $branch ($commit$dirty)$testNote. Para voltar à versão lançada: powershell -File dev\aplicar.ps1 -Voltar"
