//! Os dias da semana em que mais se usa o Claude Code, das conversas que ele guarda no computador
//! (`~/.claude/projects/**/*.jsonl`, uma linha por mensagem).
//!
//! Cada resposta do Claude traz a hora (`timestamp`, em UTC) e quantos tokens usou
//! (`message.usage`). Soma por dia, nas últimas `HISTORY_DAYS` dias: os tokens de entrada, os
//! gravados no cache e os de saída (os lidos do cache ficam de fora: são muitos e quase não pesam
//! no limite). Uma mesma resposta aparece em várias linhas (uma por bloco, e de novo quando a
//! conversa é retomada), então cada uma conta só uma vez, pelo `id` da mensagem.
//!
//! As linhas são lidas sem montar o JSON inteiro: são centenas de megabytes, e só alguns campos
//! interessam.

use std::collections::HashSet;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Quantos dias de histórico olhar (4 de cada dia da semana).
pub const HISTORY_DAYS: i64 = 28;

/// O uso de cada dia da semana (segunda a domingo, em tokens) nos últimos `HISTORY_DAYS` dias
/// até `today` (dias desde 1970, no fuso local), com o fuso `offset` (segundos a somar ao UTC).
pub fn by_weekday(projects: &Path, today: i64, offset: i64) -> [f64; 7] {
    let mut out = [0.0; 7];
    let mut seen = HashSet::new();
    // arquivos mexidos antes do começo do período não têm nada dele
    let since = SystemTime::now() - Duration::from_secs((HISTORY_DAYS as u64 + 1) * 86400);
    for path in files(projects) {
        let recent = std::fs::metadata(&path).and_then(|m| m.modified()).is_ok_and(|t| t >= since);
        if !recent {
            continue;
        }
        let Ok(file) = std::fs::File::open(&path) else { continue };
        // linha por linha: uma conversa longa passa de 100 MB
        let mut reader = std::io::BufReader::with_capacity(1 << 16, file);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&buf);
            let Some((epoch, id, tokens)) = reply(&line) else { continue };
            let day = (epoch + offset).div_euclid(86400);
            if (0..HISTORY_DAYS).contains(&(today - day)) && seen.insert(id.to_string()) {
                out[weekday(day)] += tokens;
            }
        }
    }
    out
}

/// Os .jsonl da pasta e das subpastas (as conversas dos subagentes ficam numa subpasta).
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() && p.extension().is_some_and(|x| x == "jsonl") => out.push(p),
                _ => {}
            }
        }
    }
    out
}

/// Uma resposta do Claude: (hora em segundos desde 1970, id da mensagem, tokens).
fn reply(line: &str) -> Option<(i64, &str, f64)> {
    if !line.contains("\"type\":\"assistant\"") {
        return None;
    }
    let usage = &line[line.find("\"usage\":{")?..];
    let id = string_after(line, "\"id\":\"msg_")?;
    let epoch = iso_epoch(string_after(line, "\"timestamp\":\"")?)?;
    // a primeira ocorrência de cada campo depois de "usage" é a do total da mensagem
    let n = |key: &str| number_after(usage, key).unwrap_or(0.0);
    let tokens = n("\"input_tokens\":") + n("\"cache_creation_input_tokens\":") + n("\"output_tokens\":");
    Some((epoch, id, tokens))
}

/// O texto entre aspas logo depois de `key` (que termina na aspa de abertura).
fn string_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = &line[line.find(key)? + key.len()..];
    Some(&rest[..rest.find('"')?])
}

fn number_after(text: &str, key: &str) -> Option<f64> {
    let rest = &text[text.find(key)? + key.len()..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// "2026-11-03T11:39:16.543Z" em segundos desde 1970 (UTC).
fn iso_epoch(iso: &str) -> Option<i64> {
    let day = day_number(iso.get(..10)?)?;
    let num = |a: usize, b: usize| iso.get(a..b)?.parse::<i64>().ok();
    (iso.as_bytes().get(10) == Some(&b'T')).then_some(())?;
    Some(day * 86400 + num(11, 13)? * 3600 + num(14, 16)? * 60 + num(17, 19)?)
}

/// Dias desde 1970-01-01 de uma data "AAAA-MM-DD".
pub fn day_number(iso: &str) -> Option<i64> {
    let mut it = iso.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (it.next()??, it.next()??, it.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// 0 = segunda ... 6 = domingo (1970-01-01 foi uma quinta).
pub fn weekday(day: i64) -> usize {
    (day + 3).rem_euclid(7) as usize
}

/// Quantos segundos o fuso local está à frente do UTC (pelo relógio de agora).
pub fn utc_offset() -> i64 {
    let t = crate::win::local_time();
    let local = day_number(&t.iso_date()).unwrap_or(0) * 86400 + t.hour as i64 * 3600 + t.minute as i64 * 60 + t.second as i64;
    // em múltiplos de 15 min, para o segundo de diferença entre as duas leituras não contar
    ((local - crate::win::now() as i64) as f64 / 900.0).round() as i64 * 900
}

/// O dia de hoje no fuso local (dias desde 1970).
pub fn today() -> i64 {
    day_number(&crate::win::local_time().iso_date()).unwrap_or(0)
}

/// A pasta das conversas do Claude Code.
pub fn projects_dir() -> Option<PathBuf> {
    crate::claude_usage::claude_dir().map(|d| d.join("projects"))
}

// ---------- o que uma conversa está fazendo agora ----------

/// Como está uma conversa, pelo fim do arquivo dela.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turn {
    /// o Claude está trabalhando (None: nada reconhecível no fim do arquivo)
    pub busy: Option<bool>,
    /// passos (respostas e resultados de ferramentas) no último minuto
    pub steps: u32,
    /// a ferramenta que a última resposta chamou (None: a última resposta foi texto)
    pub tool: Option<Tool>,
}

/// Uma ferramenta chamada pelo Claude: o nome ("Edit", "Bash"...) e o principal do que recebeu
/// (o caminho do arquivo, o comando, o que procurar).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tool {
    pub name: String,
    pub input: String,
}

impl Tool {
    /// A última ferramenta chamada numa resposta (`content` da mensagem), se houver.
    fn last_in(content: &serde_json::Value) -> Option<Tool> {
        let call = content.as_array()?.iter().rev().find(|c| c["type"] == "tool_use")?;
        let name = call["name"].as_str()?.to_string();
        let i = &call["input"];
        let input = ["file_path", "notebook_path", "command", "pattern", "query", "url", "description"]
            .iter()
            .find_map(|k| i[*k].as_str())
            .unwrap_or("")
            .to_string();
        Some(Tool { name, input })
    }
}

/// Quanto do fim do arquivo ler: bastam as últimas linhas (os resultados de ferramentas podem
/// ser grandes, então sobra folga).
const TAIL: u64 = 256 * 1024;

/// As últimas linhas inteiras do arquivo.
pub fn read_tail(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    // começou no meio de uma linha: ela fica de fora
    Some(if start > 0 { text.split_once('\n').map_or(String::new(), |(_, rest)| rest.to_string()) } else { text })
}

/// O que o fim de uma conversa diz, às `now` (segundos desde 1970). A última linha que importa
/// decide: uma resposta final do Claude (`stop_reason` "end_turn"), o fim da vez
/// (`turn_duration`) ou um pedido interrompido: terminou; um pedido novo, um passo no meio
/// (usando ferramentas) ou o resultado de uma ferramenta: trabalhando. Comandos locais (as
/// linhas do usuário que começam com "<", como os do "!" e do "/") não contam.
pub fn turn(tail: &str, now: f64) -> Turn {
    let mut t = Turn::default();
    // a ferramenta vem da última resposta (o Claude Code grava cada parte numa linha)
    let mut answered = false;
    for line in tail.lines().rev() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let at = v["timestamp"].as_str().and_then(iso_epoch).map(|s| s as f64);
        let recent = at.is_some_and(|at| now - at <= 60.0);
        let state = match v["type"].as_str() {
            Some("assistant") => {
                t.steps += recent as u32;
                if !answered {
                    answered = true;
                    t.tool = Tool::last_in(&v["message"]["content"]);
                }
                Some(v["message"]["stop_reason"].as_str() != Some("end_turn"))
            }
            Some("user") if v["isMeta"].as_bool() != Some(true) => {
                let content = &v["message"]["content"];
                let result = content.as_array().is_some_and(|a| a.iter().any(|c| c["type"] == "tool_result"));
                let text = content.as_str().or_else(|| content.as_array()?.iter().find_map(|c| c["text"].as_str())).unwrap_or("");
                if result {
                    t.steps += recent as u32;
                    Some(true)
                } else if text.starts_with("[Request interrupted") {
                    Some(false)
                } else if text.trim_start().starts_with('<') {
                    None
                } else {
                    Some(true)
                }
            }
            Some("system") if v["subtype"] == "turn_duration" => Some(false),
            _ => None,
        };
        if t.busy.is_none() {
            t.busy = state;
        }
        // já sabe o estado e passou do último minuto: o resto não muda nada
        if t.busy.is_some() && answered && at.is_some_and(|at| now - at > 60.0) {
            break;
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_weekdays() {
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(day_number("2026-11-03"), Some(20760));
        // 2026-11-03 é uma terça
        assert_eq!(weekday(20760), 1);
        assert_eq!(day_number("ontem"), None);
        assert_eq!(iso_epoch("1970-01-02T01:02:03.500Z"), Some(86400 + 3723));
        assert_eq!(iso_epoch("2026-11-03"), None);
    }

    #[test]
    fn reads_replies_once_each() {
        let dir = std::env::temp_dir().join(format!("widgets-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("proj/sub")).unwrap();
        let line = |id: &str, ts: &str, out: u32| {
            format!(r#"{{"message":{{"model":"x","id":"msg_{id}","type":"message","role":"assistant","usage":{{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":99999,"output_tokens":{out},"iterations":[{{"input_tokens":5,"output_tokens":1}}]}}}},"type":"assistant","timestamp":"{ts}"}}"#)
        };
        // terça 2026-11-03 às 11h UTC (8h em Brasília) e segunda às 01h UTC (domingo 22h em Brasília)
        let a = [
            line("a", "2026-11-03T11:00:00.000Z", 40),
            line("a", "2026-11-03T11:00:00.100Z", 40), // a mesma resposta, outro bloco
            r#"{"type":"user","message":{"role":"user","content":"oi"},"timestamp":"2026-11-03T10:59:00.000Z"}"#.to_string(),
            line("b", "2026-11-02T01:00:00.000Z", 90),
            line("velha", "2026-09-01T12:00:00.000Z", 500), // fora das 4 semanas
        ];
        std::fs::write(dir.join("proj/s1.jsonl"), a.join("\n")).unwrap();
        // a conversa retomada repete a resposta "a"; o subagente tem outra
        std::fs::write(dir.join("proj/s2.jsonl"), line("a", "2026-11-03T11:00:00.000Z", 40)).unwrap();
        std::fs::write(dir.join("proj/sub/agent.jsonl"), line("c", "2026-11-03T12:00:00.000Z", 0)).unwrap();
        let week = by_weekday(&dir, 20760, -3 * 3600);
        assert_eq!(week[1], 150.0 + 110.0, "terça: a e c, cada uma uma vez");
        assert_eq!(week[6], 200.0, "a resposta de segunda 01h UTC foi domingo à noite aqui");
        assert_eq!(week.iter().sum::<f64>(), 460.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_offset_is_whole() {
        assert_eq!(utc_offset() % 900, 0);
        assert!(utc_offset().abs() <= 14 * 3600);
    }

    /// Uma linha da conversa, `secs` segundos depois de 2026-11-03 12:00 UTC.
    fn entry(kind: &str, extra: &str, secs: u32) -> String {
        let (m, s) = (secs / 60, secs % 60);
        format!(r#"{{"type":"{kind}",{extra},"timestamp":"2026-11-03T12:{m:02}:{s:02}.000Z"}}"#)
    }
    const NOON: f64 = 20760.0 * 86400.0 + 12.0 * 3600.0;
    const PROMPT: &str = r#""message":{"role":"user","content":"faça o caderno"}"#;
    const TOOL: &str = r#""message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use"}]}"#;
    const RESULT: &str = r#""message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}"#;
    const FINAL: &str = r#""message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"pronto"}]}"#;

    #[test]
    fn knows_when_claude_is_working_and_when_it_finished() {
        let at = |lines: &[String], now: u32| turn(&lines.join("\n"), NOON + now as f64);
        // pediu: trabalhando; usando ferramentas: trabalhando, contando os passos do último minuto
        let mut c = vec![entry("user", PROMPT, 0)];
        assert_eq!(at(&c, 1).busy, Some(true));
        for k in 0..5 {
            c.push(entry("assistant", TOOL, 10 + k * 10));
            c.push(entry("user", RESULT, 12 + k * 10));
        }
        assert_eq!((at(&c, 60).busy, at(&c, 60).steps), (Some(true), 10));
        assert_eq!(at(&c, 85).steps, 6, "só os do último minuto");
        // respondeu de vez: terminou (e o fim da vez, depois, também diz isso)
        c.push(entry("assistant", FINAL, 70));
        assert_eq!(at(&c, 71).busy, Some(false));
        c.push(entry("system", r#""subtype":"turn_duration""#, 72));
        assert_eq!(at(&c, 72).busy, Some(false));
        // anexos e linhas de controle depois do fim não mudam nada
        c.push(entry("attachment", r#""x":1"#, 73));
        assert_eq!(at(&c, 73).busy, Some(false));
        // um comando "!" ou "/" do usuário não é um pedido ao Claude
        c.push(entry("user", r#""message":{"role":"user","content":"<bash-input>ls</bash-input>"}"#, 80));
        assert_eq!(at(&c, 80).busy, Some(false));
        // interrompido com Esc: parou
        let stopped = [entry("user", PROMPT, 0), entry("assistant", TOOL, 5), entry("user", r#""message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}"#, 6)];
        assert_eq!(at(&stopped, 7).busy, Some(false));
        // começo cortado no meio de uma linha, ou nada que importe: não sabe
        assert_eq!(turn(r#"ype":"assistant"}"#, NOON).busy, None);
        assert_eq!(turn("", NOON), Turn::default());
    }

    #[test]
    fn knows_the_last_tool_claude_called() {
        let at = |lines: &[String]| turn(&lines.join("\n"), NOON + 30.0).tool;
        let call = |name: &str, input: &str| format!(r#""message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","name":"{name}","input":{input}}}]}}"#);
        let edit = entry("assistant", &call("Edit", r#"{"file_path":"C:\\dev\\src\\claude.rs","old_string":"a"}"#), 10);
        let bash = entry("assistant", &call("Bash", r#"{"command":"cargo test --release","description":"testes"}"#), 20);
        // a última chamada vale, mesmo com o resultado dela depois
        assert_eq!(at(&[entry("user", PROMPT, 0), edit.clone()]), Some(Tool { name: "Edit".into(), input: r"C:\dev\src\claude.rs".into() }));
        let tool = at(&[edit.clone(), entry("user", RESULT, 12), bash.clone(), entry("user", RESULT, 22)]).unwrap();
        assert_eq!((tool.name.as_str(), tool.input.as_str()), ("Bash", "cargo test --release"));
        // depois de responder em texto, nenhuma
        assert_eq!(at(&[bash, entry("assistant", FINAL, 25)]), None);
        // uma ferramenta sem nada reconhecível: só o nome
        let other = entry("assistant", &call("TodoWrite", r#"{"todos":[]}"#), 5);
        assert_eq!(at(&[other]), Some(Tool { name: "TodoWrite".into(), input: String::new() }));
    }

    #[test]
    fn reads_only_the_end_of_long_conversations() {
        let dir = std::env::temp_dir().join(format!("widgets-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("conversa.jsonl");
        // uma conversa enorme: o fim é lido inteiro, a primeira linha cortada fica de fora
        let filler = entry("user", &format!(r#""message":{{"role":"user","content":"{}"}}"#, "x".repeat(400 * 1024)), 0);
        std::fs::write(&path, [filler, entry("user", PROMPT, 1), entry("assistant", FINAL, 2)].join("\n")).unwrap();
        let tail = read_tail(&path).unwrap();
        assert!(tail.len() < 300 * 1024 && tail.lines().count() == 2);
        assert_eq!(turn(&tail, NOON + 3.0).busy, Some(false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Quanto demora ler o histórico de verdade deste computador.
    ///   cargo test --release real_history -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_history() {
        let start = std::time::Instant::now();
        let week = by_weekday(&projects_dir().unwrap(), today(), utc_offset());
        println!("{week:?} em {:?}", start.elapsed());
    }
}
