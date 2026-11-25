//! Atualizações pelo GitHub: compara a versão deste Widgets.exe com o último Release e, com um
//! clique, troca pela nova sem passar por PowerShell nem script:
//!
//! 1. baixa o `Widgets.exe` do Release (numa thread) e confere o SHA-256 que o GitHub publica dele;
//! 2. roda o novo com `--versao`: se o Windows não deixar ele abrir (o Controle Inteligente de
//!    Aplicativos barra programas novos sem assinatura), nada é trocado;
//! 3. troca os arquivos (o Windows deixa renomear o exe que está rodando, só não apagar): o
//!    atual vira `Widgets.anterior.exe` e o novo, `Widgets.exe`;
//! 4. abre o novo com `--depois-de-atualizar <processo>` e fecha este; o novo espera este fechar,
//!    apaga o anterior e acerta a versão em "Aplicativos instalados".
//!
//! Antes, o botão rodava `irm install.ps1 | iex` num PowerShell escondido, e o Windows Defender
//! desconfia disso (é o jeito de quem baixa e roda script da internet). O install.ps1 continua
//! sendo a instalação do zero, rodada pela própria pessoa.

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::Value;

pub const REPO: &str = "lucasschn1/Widgets";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// `Widgets.exe --versao`: responde a versão e sai, sem abrir nada (para conferir qual versão é
/// um Widgets.exe). A atualização confere com ela que a versão nova abre; o install.ps1 não usa:
/// ele precisa funcionar também com as versões de antes desta opção, e testa com a da linha de
/// status do Claude Code.
pub const VERSION_FLAG: &str = "--versao";
/// `Widgets.exe --depois-de-atualizar <processo>`: a versão nova, aberta pela anterior, espera o
/// processo dela fechar antes de abrir os widgets (senão veria o app ainda aberto e sairia).
pub const UPDATED_FLAG: &str = "--depois-de-atualizar";
const RELEASES: &str = "https://github.com/lucasschn1/Widgets/releases";

/// O executável, na pasta de instalação.
const EXE: &str = "Widgets.exe";
/// A versão nova, baixada e conferida, esperando a troca.
const NEW_EXE: &str = "Widgets.novo.exe";
/// A versão de antes, guardada durante a troca (para voltar se a nova não abrir). O install.ps1
/// usa o mesmo nome.
const OLD_EXE: &str = "Widgets.anterior.exe";
/// O maior Widgets.exe que se aceita baixar (hoje ele tem uns 5 MB).
const MAX_EXE_BYTES: u64 = 64 * 1024 * 1024;
/// Quanto a versão nova pode levar para responder o `--versao`.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Quanto a versão nova espera a anterior fechar.
const OLD_APP_TIMEOUT_MS: u32 = 20_000;
/// O anterior só pode ser apagado quando o Windows o solta (logo depois de o processo fechar):
/// tantas tentativas, com esta pausa entre elas.
const DELETE_TRIES: u32 = 20;
const DELETE_PAUSE: Duration = Duration::from_millis(500);
/// As entradas de "Aplicativos instalados" (a do install.ps1 e a do instalador, pelo AppId do
/// Widgets.iss), cuja versão é acertada depois de atualizar.
const UNINSTALL_KEYS: [&str; 2] = [
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Widgets",
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{6F1D2B7E-4C3A-4E59-9C2D-8A7B5E1F3D20}_is1",
];

/// "v3.1.0" -> (3, 1, 0); partes que não são números contam como 0.
pub fn parse_version(text: &str) -> (u32, u32, u32) {
    let mut parts = text.trim().trim_start_matches(['v', 'V']).split('.').map(|piece| {
        let digits: String = piece.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        digits.parse().unwrap_or(0)
    });
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// `version` é mais nova que esta?
pub fn is_newer(version: &str) -> bool {
    // na compilação de teste dá para fingir outra versão (WIDGETS_PRETEND_VERSION=1.0.0)
    let current = match std::env::var("WIDGETS_PRETEND_VERSION") {
        Ok(pretend) if cfg!(debug_assertions) => pretend,
        _ => VERSION.to_string(),
    };
    parse_version(version) > parse_version(&current)
}

/// Resultado da busca: Ok(Some(versão nova)), Ok(None) se já é a mais recente, ou o erro.
pub type Found = Result<Option<String>, String>;

/// O endereço do último Release na API do GitHub.
fn latest_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

/// Procura numa thread; o laço principal acorda quando a resposta chega.
pub fn check_async() -> Receiver<Found> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let found = crate::data::util::fetch_json(&latest_url()).and_then(|data| {
            let tag = data["tag_name"].as_str().ok_or("Release sem tag")?;
            let version = tag.trim_start_matches(['v', 'V']).to_string();
            Ok(is_newer(&version).then_some(version))
        });
        let _ = tx.send(found);
        crate::app::wake();
    });
    rx
}

/// Pasta onde o install.ps1 e o Widgets-Setup.exe instalam.
fn install_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Programs").join("Widgets"))
}

/// Este Widgets.exe foi instalado (e não é uma cópia solta ou a compilação de teste)?
pub fn installed() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    let exe = std::env::current_exe().ok().and_then(|path| path.parent().map(|dir| dir.to_path_buf()));
    match (exe, install_dir()) {
        (Some(here), Some(dir)) => {
            // (os caminhos do Windows não diferenciam maiúsculas)
            let norm = |path: &PathBuf| path.to_string_lossy().to_lowercase().trim_end_matches('\\').to_string();
            norm(&here) == norm(&dir)
        }
        _ => false,
    }
}

/// Abre a página dos Releases (para baixar à mão).
pub fn open_releases() {
    crate::platform::win::open_target(RELEASES);
}

/// Por que a atualização não aconteceu (em todos os casos, os widgets continuam nesta versão).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Failure {
    /// sem conexão, ou o GitHub não respondeu
    Network,
    /// o último Release não tem o Widgets.exe solto (ou o SHA-256 dele): só à mão
    NoExe,
    /// o que chegou não é o que o GitHub diz que publicou
    Corrupt,
    /// o Windows não deixou a versão nova abrir
    Blocked,
    /// a versão nova abriu mas não respondeu a versão certa, ou não abriu depois da troca
    WontRun,
    /// não deu para gravar ou trocar os arquivos na pasta de instalação
    Files,
}

/// O que dizer quando a atualização não acontece: (título, detalhe).
pub fn failure_message(failure: Failure) -> (&'static str, &'static str) {
    match failure {
        Failure::Network => ("Não deu para atualizar", "Verifique a conexão e tente de novo."),
        Failure::NoExe => ("Atualize pela página de download", "Esta versão não se instala sozinha: a página dela foi aberta."),
        Failure::Corrupt => ("O download chegou com defeito", "Nada foi trocado. Tente de novo daqui a pouco."),
        Failure::Blocked => (
            "O Windows bloqueou a versão nova",
            "O Controle Inteligente de Aplicativos barra programas novos sem assinatura digital. Os widgets continuam nesta versão.",
        ),
        Failure::WontRun => ("A versão nova não abriu", "Os widgets continuam nesta versão."),
        Failure::Files => ("Não deu para trocar os arquivos", "Os widgets continuam nesta versão. Tente de novo."),
    }
}

/// O Widgets.exe de um Release: a versão, de onde baixar, o tamanho e o SHA-256.
#[derive(Debug, PartialEq)]
struct ReleaseExe {
    version: String,
    url: String,
    size: u64,
    sha256: [u8; 32],
}

/// O SHA-256 no formato do GitHub ("sha256:" e 64 dígitos hexadecimais).
fn parse_digest(text: &str) -> Option<[u8; 32]> {
    let hex = text.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// O Widgets.exe solto no Release `release` (a resposta da API do GitHub). Só vale um que seja
/// baixado deste repositório e tenha o SHA-256 publicado (sem ele, não há como conferir).
fn release_exe(release: &Value) -> Result<ReleaseExe, Failure> {
    let version = release["tag_name"].as_str().ok_or(Failure::NoExe)?.trim_start_matches(['v', 'V']).to_string();
    let assets = release["assets"].as_array().ok_or(Failure::NoExe)?;
    let asset = assets.iter().find(|asset| asset["name"].as_str() == Some(EXE)).ok_or(Failure::NoExe)?;
    let url = asset["browser_download_url"].as_str().ok_or(Failure::NoExe)?.to_string();
    if !url.starts_with(&format!("https://github.com/{REPO}/releases/download/")) {
        return Err(Failure::NoExe);
    }
    let size = asset["size"].as_u64().ok_or(Failure::NoExe)?;
    let sha256 = asset["digest"].as_str().and_then(parse_digest).ok_or(Failure::NoExe)?;
    Ok(ReleaseExe { version, url, size, sha256 })
}

/// O SHA-256 de `bytes`, pela biblioteca de criptografia do Windows (BCrypt).
fn sha256(bytes: &[u8]) -> Option<[u8; 32]> {
    use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
    let mut out = [0u8; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, bytes, &mut out) }.is_ok().then_some(out)
}

/// O que chegou é o Widgets.exe que o GitHub diz que publicou?
fn matches(bytes: &[u8], exe: &ReleaseExe) -> bool {
    bytes.len() as u64 == exe.size && sha256(bytes) == Some(exe.sha256)
}

/// O Windows deixa `exe` abrir, e ele é a versão `version`? Roda `exe --versao`, escondido.
fn probe(exe: &Path, version: &str) -> Result<(), Failure> {
    let mut child = Command::new(exe)
        .arg(VERSION_FLAG)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .map_err(|_| Failure::Blocked)?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                return Err(Failure::WontRun);
            }
        }
    }
    let mut said = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut said);
    }
    if said.trim() == version { Ok(()) } else { Err(Failure::WontRun) }
}

/// Baixa e confere a versão nova, deixando-a em `dir` como Widgets.novo.exe. Demora: chamar
/// fora da thread da interface.
fn prepare(dir: &Path) -> Result<(), Failure> {
    let release = crate::data::util::fetch_json(&latest_url()).map_err(|_| Failure::Network)?;
    let exe = release_exe(&release)?;
    if exe.size > MAX_EXE_BYTES {
        return Err(Failure::Corrupt);
    }
    let (status, bytes) = crate::data::util::download(&exe.url, exe.size + 1).map_err(|_| Failure::Network)?;
    if status >= 400 {
        return Err(Failure::Network);
    }
    if !matches(&bytes, &exe) {
        return Err(Failure::Corrupt);
    }
    let new = dir.join(NEW_EXE);
    std::fs::write(&new, &bytes).map_err(|_| Failure::Files)?;
    probe(&new, &exe.version).inspect_err(|_| {
        let _ = std::fs::remove_file(&new);
    })
}

/// O preparo da versão nova: Ok quando o Widgets.novo.exe está pronto para a troca.
pub type Prepared = Result<(), Failure>;

/// Instalado: baixa e confere a versão nova numa thread (o laço principal acorda com o
/// resultado; aí `switch_to_new` troca e este app fecha). Fora da pasta de instalação: abre a
/// página dos Releases.
pub fn install() -> Option<Receiver<Prepared>> {
    let dir = install_dir().filter(|_| installed());
    let Some(dir) = dir else {
        open_releases();
        return None;
    };
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(prepare(&dir));
        crate::app::wake();
    });
    Some(rx)
}

/// Troca os arquivos em `dir`: o Widgets.exe vira Widgets.anterior.exe (o Windows deixa
/// renomear o exe que está rodando, só não apagar) e o Widgets.novo.exe vira Widgets.exe. Se a
/// segunda parte falhar, desfaz a primeira.
fn swap_files(dir: &Path) -> std::io::Result<()> {
    let (current, new, old) = (dir.join(EXE), dir.join(NEW_EXE), dir.join(OLD_EXE));
    if old.exists() {
        std::fs::remove_file(&old)?;
    }
    std::fs::rename(&current, &old)?;
    if let Err(error) = std::fs::rename(&new, &current) {
        let _ = std::fs::rename(&old, &current);
        return Err(error);
    }
    Ok(())
}

/// Desfaz `swap_files`: o anterior volta a ser o Widgets.exe (e o novo é apagado).
fn undo_swap(dir: &Path) {
    let (current, old) = (dir.join(EXE), dir.join(OLD_EXE));
    if old.exists() {
        let _ = std::fs::remove_file(&current);
        let _ = std::fs::rename(&old, &current);
    }
}

/// Com a versão nova pronta (`install` deu Ok): troca os arquivos e abre a nova, que espera este
/// processo fechar. Ok: feche o app agora. Se a nova não abrir, tudo volta como estava.
pub fn switch_to_new() -> Result<(), Failure> {
    let dir = install_dir().ok_or(Failure::Files)?;
    swap_files(&dir).map_err(|_| Failure::Files)?;
    let opened = Command::new(dir.join(EXE)).args([UPDATED_FLAG, &std::process::id().to_string()]).current_dir(&dir).spawn();
    if opened.is_err() {
        undo_swap(&dir);
        return Err(Failure::WontRun);
    }
    Ok(())
}

/// Espera o processo `pid` fechar (no máximo `timeout_ms`).
fn wait_for_process(pid: u32, timeout_ms: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        if let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(process, timeout_ms);
            let _ = CloseHandle(process);
        }
    }
}

/// Ao abrir: aberto pela versão anterior (`--depois-de-atualizar`), espera ela fechar. Instalado,
/// apaga o anterior que ficou da última atualização (numa thread: o Windows o solta logo depois
/// de o processo fechar) e acerta a versão em "Aplicativos instalados".
pub fn after_update() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some(UPDATED_FLAG) {
        if let Some(pid) = args.next().and_then(|pid| pid.parse().ok()) {
            wait_for_process(pid, OLD_APP_TIMEOUT_MS);
        }
    }
    if !installed() {
        return;
    }
    for key in UNINSTALL_KEYS {
        crate::platform::win::update_registry_text(key, "DisplayVersion", VERSION);
    }
    let Some(old) = install_dir().map(|dir| dir.join(OLD_EXE)).filter(|old| old.exists()) else { return };
    std::thread::spawn(move || {
        for _ in 0..DELETE_TRIES {
            if std::fs::remove_file(&old).is_ok() || !old.exists() {
                return;
            }
            std::thread::sleep(DELETE_PAUSE);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn failures_are_explained() {
        assert!(failure_message(Failure::Blocked).1.contains("Controle Inteligente"), "bloqueio do Windows");
        assert_eq!(failure_message(Failure::WontRun).0, "A versão nova não abriu");
        assert_eq!(failure_message(Failure::Network).0, "Não deu para atualizar");
        for failure in [Failure::Network, Failure::NoExe, Failure::Corrupt, Failure::Blocked, Failure::WontRun, Failure::Files] {
            let (title, detail) = failure_message(failure);
            assert!(!title.is_empty() && !detail.is_empty(), "{failure:?}");
        }
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("v3.1.0"), (3, 1, 0));
        assert_eq!(parse_version("2.0"), (2, 0, 0));
        assert_eq!(parse_version("3.0.0-beta"), (3, 0, 0));
        assert!(parse_version("3.0.1") > parse_version("3.0.0"));
        assert!(parse_version("10.0.0") > parse_version("9.9.9"));
    }

    /// O SHA-256 do Windows bate com o valor conhecido de "abc" (o exemplo da norma), e o do
    /// GitHub é lido do jeito que a API escreve.
    #[test]
    fn sha256_matches_the_github_digest() {
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let digest = parse_digest(&format!("sha256:{abc}")).expect("lê o digest");
        assert_eq!(sha256(b"abc"), Some(digest));
        assert_eq!(parse_digest(abc), None, "sem o \"sha256:\"");
        assert_eq!(parse_digest("sha256:abc"), None, "curto");
        assert_eq!(parse_digest(&format!("sha256:{}", "zz".repeat(32))), None, "não é hexadecimal");
    }

    /// Um Release como a API do GitHub devolve, com o Widgets.exe solto (`exe`) ou não.
    fn release(exe: Option<Value>) -> Value {
        let mut assets = vec![json!({"name": "Widgets-win64.zip", "size": 10, "digest": format!("sha256:{}", "00".repeat(32)),
            "browser_download_url": format!("https://github.com/{REPO}/releases/download/v9.0.0/Widgets-win64.zip")})];
        assets.extend(exe);
        json!({"tag_name": "v9.0.0", "assets": assets})
    }

    fn exe_asset(url: &str, digest: Option<&str>) -> Value {
        let mut asset = json!({"name": EXE, "size": 3, "browser_download_url": url});
        if let Some(digest) = digest {
            asset["digest"] = json!(digest);
        }
        asset
    }

    /// Só vale o Widgets.exe solto, baixado deste repositório e com o SHA-256 publicado; o que
    /// chega tem de ter o tamanho e o SHA-256 dele.
    #[test]
    fn only_the_published_exe_is_accepted() {
        let url = format!("https://github.com/{REPO}/releases/download/v9.0.0/Widgets.exe");
        let digest = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let exe = release_exe(&release(Some(exe_asset(&url, Some(digest))))).expect("acha o Widgets.exe");
        assert_eq!((exe.version.as_str(), exe.url.as_str(), exe.size), ("9.0.0", url.as_str(), 3));
        assert!(matches(b"abc", &exe), "o arquivo certo");
        assert!(!matches(b"abd", &exe), "outro conteúdo");
        assert!(!matches(b"abcd", &exe), "outro tamanho");
        assert_eq!(release_exe(&release(None)), Err(Failure::NoExe), "Release antigo, só com o .zip");
        assert_eq!(release_exe(&release(Some(exe_asset(&url, None)))), Err(Failure::NoExe), "sem o SHA-256");
        let elsewhere = exe_asset("https://example.com/Widgets.exe", Some(digest));
        assert_eq!(release_exe(&release(Some(elsewhere))), Err(Failure::NoExe), "de outro lugar");
    }

    /// Uma pasta temporária com os arquivos `files` (nome, conteúdo).
    fn temp_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("widgets-teste-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, text) in files {
            std::fs::write(dir.join(file), text).unwrap();
        }
        dir
    }

    fn read(dir: &Path, file: &str) -> Option<String> {
        std::fs::read_to_string(dir.join(file)).ok()
    }

    /// A troca põe o novo no lugar e guarda o atual como anterior (apagando o de uma atualização
    /// passada); desfeita, tudo volta como estava.
    #[test]
    fn swap_and_undo() {
        let dir = temp_dir("troca", &[(EXE, "atual"), (NEW_EXE, "novo"), (OLD_EXE, "velho")]);
        swap_files(&dir).unwrap();
        assert_eq!(read(&dir, EXE).as_deref(), Some("novo"));
        assert_eq!(read(&dir, OLD_EXE).as_deref(), Some("atual"));
        assert_eq!(read(&dir, NEW_EXE), None);
        undo_swap(&dir);
        assert_eq!(read(&dir, EXE).as_deref(), Some("atual"));
        assert_eq!(read(&dir, OLD_EXE), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Sem a versão nova, a troca falha e o Widgets.exe continua o mesmo.
    #[test]
    fn swap_without_new_exe_keeps_the_current_one() {
        let dir = temp_dir("sem-novo", &[(EXE, "atual")]);
        assert!(swap_files(&dir).is_err());
        assert_eq!(read(&dir, EXE).as_deref(), Some("atual"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Um exe que o Windows não abre (aqui, que nem existe) conta como bloqueado.
    #[test]
    fn probe_of_a_file_that_does_not_open() {
        let dir = temp_dir("teste-abre", &[]);
        assert_eq!(probe(&dir.join(NEW_EXE), "9.0.0"), Err(Failure::Blocked));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
