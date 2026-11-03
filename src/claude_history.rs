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
