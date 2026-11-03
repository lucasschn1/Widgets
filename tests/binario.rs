//! Testes de integração com o Widgets.exe de verdade (o que o `cargo test` acabou de compilar).
//!
//! Cada teste roda o executável num perfil de mentira (APPDATA, USERPROFILE e a pasta do Claude
//! Code numa pasta temporária), para nunca mexer nas opções, nos números do Claude nem na
//! inicialização do Windows de quem roda os testes.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const EXE: &str = env!("CARGO_BIN_EXE_widgets");

/// Uma pasta vazia só deste teste.
fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("widgets-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["appdata", "home", "claude"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

fn command(dir: &Path) -> Command {
    let mut cmd = Command::new(EXE);
    cmd.env("APPDATA", dir.join("appdata"))
        .env("USERPROFILE", dir.join("home"))
        .env("CLAUDE_CONFIG_DIR", dir.join("claude"))
        .env_remove("WIDGETS_ZIP");
    cmd
}

/// Abre o processo. No Windows com o Controle Inteligente de Aplicativos ligado, um executável
/// recém-compilado às vezes é barrado por alguns segundos: tenta de novo antes de desistir.
fn spawn(cmd: &mut Command) -> Child {
    let mut last = None;
    for _ in 0..5 {
        match cmd.spawn() {
            Ok(child) => return child,
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
    panic!("não deu para abrir {EXE}: {:?}", last.unwrap());
}

/// O nome da pasta de opções (a compilação de teste usa outra, para não misturar).
fn data_dir(dir: &Path) -> PathBuf {
    dir.join("appdata").join(if cfg!(debug_assertions) { "Widgets-dev" } else { "Widgets" })
}

#[test]
fn status_line_answers_and_saves_the_usage() {
    let dir = sandbox("status");
    let mut child = spawn(command(&dir).arg("--claude-status").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()));
    let input = r#"{"model": {"display_name": "Opus"}, "rate_limits": {"five_hour": {"used_percentage": 23.5, "resets_at": 4102444800}, "seven_day": {"used_percentage": 41.2, "resets_at": 4102444800}}}"#;
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "saiu com {:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(text.trim(), "Opus · Sessão 24% · Semana 41%");
    // os números ficam guardados para o widget ler
    let saved = std::fs::read_dir(data_dir(&dir)).unwrap().filter_map(Result::ok).map(|e| std::fs::read_to_string(e.path()).unwrap_or_default()).any(|t| t.contains("0.235"));
    assert!(saved, "o uso não foi gravado em {}", data_dir(&dir).display());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn status_line_survives_bad_input() {
    let dir = sandbox("status-ruim");
    for input in ["", "isto não é json", "{}", r#"{"rate_limits": {"five_hour": "x"}}"#] {
        let mut child = spawn(command(&dir).arg("--claude-status").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()));
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "entrada {input:?}: saiu com {:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// O install.ps1 confere se o Windows deixa a versão nova abrir rodando-a como linha de status
/// com a entrada vazia (a única opção que toda versão 3.x conhece): ela tem que sair na hora, sem
/// abrir os widgets nem gravar nada.
#[test]
fn status_line_with_empty_input_is_a_safe_probe() {
    let dir = sandbox("sonda");
    let start = Instant::now();
    let mut child = spawn(command(&dir).arg("--claude-status").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()));
    drop(child.stdin.take()); // entrada vazia
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "saiu com {:?}", out.status);
    assert!(start.elapsed() < Duration::from_secs(5), "demorou {:?}", start.elapsed());
    assert!(!data_dir(&dir).exists(), "gravou opções");
    assert!(std::fs::read_dir(dir.join("claude")).unwrap().next().is_none(), "mexeu na pasta do Claude Code");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--versao`: responde a versão e sai na hora, sem abrir os widgets nem criar opções.
#[test]
fn version_flag_answers_and_touches_nothing() {
    let dir = sandbox("versao");
    let start = Instant::now();
    let out = spawn(command(&dir).arg("--versao").stdout(Stdio::piped()).stderr(Stdio::piped())).wait_with_output().unwrap();
    assert!(out.status.success(), "saiu com {:?}", out.status);
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), env!("CARGO_PKG_VERSION"));
    assert!(start.elapsed() < Duration::from_secs(5), "demorou {:?}", start.elapsed());
    assert!(!data_dir(&dir).exists(), "criou a pasta de opções");
    let _ = std::fs::remove_dir_all(&dir);
}

/// O app abre do zero (primeira vez, sem opções), fica de pé por alguns segundos e grava as
/// opções. Só na compilação de teste: a final, aberta de fora da pasta de instalação, trocaria
/// a "iniciar com o Windows" de verdade e daria de cara com o app instalado já aberto.
#[test]
fn app_starts_from_scratch_and_keeps_running() {
    if !cfg!(debug_assertions) {
        eprintln!("pulado na compilação final (rode `cargo test` sem --release)");
        return;
    }
    let dir = sandbox("abre");
    let mut child = spawn(command(&dir).stdout(Stdio::null()).stderr(Stdio::piped()));
    let start = Instant::now();
    let mut alive_for = Duration::ZERO;
    while start.elapsed() < Duration::from_secs(6) {
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            let _ = std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut err);
            panic!("o app fechou sozinho depois de {:?} ({status}):\n{err}", start.elapsed());
        }
        alive_for = start.elapsed();
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = child.kill();
    let _ = child.wait();
    assert!(alive_for >= Duration::from_secs(5));
    assert!(data_dir(&dir).join("config.json").exists(), "o app abriu mas não gravou as opções");
    let _ = std::fs::remove_dir_all(&dir);
}
