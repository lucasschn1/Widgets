//! Atualizações pelo GitHub: compara a versão deste Widgets.exe com o último Release e, com um
//! clique, roda o install.ps1 (que fecha o app, troca o Widgets.exe e abre de novo).

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

pub const REPO: &str = "lucasschn1/Widgets";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// `Widgets.exe --versao`: responde a versão e sai, sem abrir nada (para conferir qual versão é
/// um Widgets.exe). O install.ps1 não usa: ele precisa funcionar também com as versões de antes
/// desta opção, e testa com a da linha de status do Claude Code.
pub const VERSION_FLAG: &str = "--versao";
const RELEASES: &str = "https://github.com/lucasschn1/Widgets/releases";
const INSTALL_SCRIPT: &str = "https://raw.githubusercontent.com/lucasschn1/Widgets/main/install.ps1";

/// "v3.1.0" -> (3, 1, 0); partes que não são números contam como 0.
pub fn parse_version(text: &str) -> (u32, u32, u32) {
    let mut parts = text.trim().trim_start_matches(['v', 'V']).split('.').map(|piece| {
        let digits: String = piece.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().unwrap_or(0)
    });
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

pub fn is_newer(version: &str) -> bool {
    // na compilação de teste dá para fingir outra versão (WIDGETS_PRETEND_VERSION=1.0.0)
    let current = match std::env::var("WIDGETS_PRETEND_VERSION") {
        Ok(v) if cfg!(debug_assertions) => v,
        _ => VERSION.to_string(),
    };
    parse_version(version) > parse_version(&current)
}

/// Resultado da busca: Ok(Some(versão nova)), Ok(None) se já é a mais recente, ou o erro.
pub type Found = Result<Option<String>, String>;

/// Procura numa thread; o laço principal acorda quando a resposta chega.
pub fn check_async() -> Receiver<Found> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
        let found = crate::util::fetch_json(&url).and_then(|data| {
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
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("Widgets"))
}

/// Este Widgets.exe foi instalado (e não é uma cópia solta ou a compilação de teste)?
pub fn installed() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    match (exe, install_dir()) {
        (Some(here), Some(dir)) => {
            let norm = |p: &PathBuf| p.to_string_lossy().to_lowercase().trim_end_matches('\\').to_string();
            norm(&here) == norm(&dir)
        }
        _ => false,
    }
}

/// Instalado: roda o install.ps1 escondido e devolve o processo dele (para saber como terminou).
/// Fora da pasta de instalação: abre a página dos Releases.
pub fn install() -> Option<std::process::Child> {
    if !installed() {
        crate::win::open_target(RELEASES);
        return None;
    }
    use std::os::windows::process::CommandExt;
    let command = format!("irm {INSTALL_SCRIPT} | iex");
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-Command", &command])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .ok()
}

/// O que dizer quando o install.ps1 termina com o código `code` e este app continua aberto (deu
/// certo, ele teria sido fechado e trocado): (título, detalhe).
pub fn failure_message(code: Option<i32>) -> (&'static str, &'static str) {
    match code {
        // o Controle Inteligente de Aplicativos barrou a versão nova antes de trocar qualquer coisa
        Some(3) => (
            "O Windows bloqueou a versão nova",
            "O Controle Inteligente de Aplicativos barra programas novos sem assinatura digital. Os widgets continuam nesta versão.",
        ),
        Some(4) => ("A versão nova não abriu", "Os widgets voltaram para esta versão."),
        _ => ("Não deu para atualizar", "Verifique a conexão e tente de novo."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_are_explained() {
        assert!(failure_message(Some(3)).1.contains("Controle Inteligente"), "bloqueio do Windows");
        assert_eq!(failure_message(Some(4)).0, "A versão nova não abriu");
        assert_eq!(failure_message(Some(1)).0, "Não deu para atualizar");
        assert_eq!(failure_message(None).0, "Não deu para atualizar");
    }

    #[test]
    fn versions() {
        assert_eq!(parse_version("v3.1.0"), (3, 1, 0));
        assert_eq!(parse_version("2.0"), (2, 0, 0));
        assert_eq!(parse_version("3.0.0-beta"), (3, 0, 0));
        assert!(parse_version("3.0.1") > parse_version("3.0.0"));
        assert!(parse_version("10.0.0") > parse_version("9.9.9"));
    }
}
