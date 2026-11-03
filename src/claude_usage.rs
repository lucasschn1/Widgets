//! Uso do Claude Code: quanto da sessão de 5 horas e da semana já foi usado, como o próprio
//! Claude Code informa à linha de status (`statusLine`, campo `rate_limits`).
//!
//! O Widgets.exe vira o comando da linha de status do Claude Code: a cada atualização, o Claude
//! Code manda os dados da conversa em JSON pela entrada padrão; o Widgets guarda os limites num
//! arquivo (`claude.json`, junto do config.json) e escreve uma linha curta de volta. O widget
//! "Uso do Claude" lê esse arquivo.
//!
//! Conectar mexe só na chave `statusLine` das configurações do Claude Code (com uma cópia de
//! segurança do arquivo antes da primeira mudança), e só quando não há outra linha de status.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Argumento com que o Claude Code chama o Widgets.exe.
pub const FLAG: &str = "--claude-status";
const FILE: &str = "claude.json";
pub const FIVE_HOURS: f64 = 5.0 * 3600.0;
pub const SEVEN_DAYS: f64 = 7.0 * 86400.0;
/// Quantos dias de histórico guardar (para os dias da semana em que mais se usa).
pub const HISTORY_DAYS: i64 = 28;

/// Uma janela de limite: quanto foi usado (0 a 1) e quando ela recomeça (segundos desde 1970).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub used: f64,
    pub resets_at: f64,
}

impl Window {
    /// Uso agora: depois do reinício, a janela começa vazia.
    pub fn used_at(&self, now: f64) -> f64 {
        if now >= self.resets_at { 0.0 } else { self.used.clamp(0.0, 1.0) }
    }
    /// Quanto da janela (de duração `len`) já passou, de 0 a 1; None depois do reinício.
    pub fn elapsed_at(&self, now: f64, len: f64) -> Option<f64> {
        (now < self.resets_at).then(|| ((now - (self.resets_at - len)) / len).clamp(0.0, 1.0))
    }
    /// No ritmo de agora, quando o limite acaba (None: dura até o reinício).
    pub fn runs_out_at(&self, now: f64, len: f64) -> Option<f64> {
        let elapsed = self.elapsed_at(now, len)?;
        let used = self.used_at(now);
        if used >= 1.0 {
            return Some(now);
        }
        // pouco tempo de janela ainda não diz nada sobre o ritmo
        if elapsed < 0.05 || used <= 0.0 {
            return None;
        }
        let full = elapsed + (1.0 - used) / (used / elapsed);
        (full < 1.0).then_some(self.resets_at - len + full * len)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    /// quando o Claude Code mandou estes números
    pub updated: f64,
    /// quanto da sessão de 5 horas se gastou em cada dia ("2026-11-03" -> 0,8 = 80% de uma
    /// sessão), nos últimos `HISTORY_DAYS` dias
    pub days: BTreeMap<String, f64>,
}

fn window_of(v: &Value) -> Option<Window> {
    let used = v.get("used_percentage")?.as_f64()?;
    let resets_at = v.get("resets_at")?.as_f64()?;
    Some(Window { used: (used / 100.0).clamp(0.0, 1.0), resets_at })
}

impl Usage {
    /// Os limites no JSON da linha de status (None se ele não traz nenhum).
    pub fn from_status(input: &Value, now: f64) -> Option<Usage> {
        let r = input.get("rate_limits")?;
        let u = Usage { five_hour: window_of(&r["five_hour"]), seven_day: window_of(&r["seven_day"]), updated: now, days: BTreeMap::new() };
        (u.five_hour.is_some() || u.seven_day.is_some()).then_some(u)
    }

    pub fn to_json(&self) -> Value {
        let w = |w: &Option<Window>| w.map(|w| json!({"used": w.used, "resets_at": w.resets_at}));
        json!({"five_hour": w(&self.five_hour), "seven_day": w(&self.seven_day), "updated": self.updated, "days": self.days})
    }

    pub fn from_json(v: &Value) -> Option<Usage> {
        let w = |v: &Value| Some(Window { used: v.get("used")?.as_f64()?, resets_at: v.get("resets_at")?.as_f64()? });
        let days = v["days"].as_object().map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect()).unwrap_or_default();
        Some(Usage { five_hour: w(&v["five_hour"]), seven_day: w(&v["seven_day"]), updated: v.get("updated")?.as_f64()?, days })
    }

    /// Soma ao dia `today` o que se gastou da sessão desde os números anteriores (`prev`), e
    /// esquece os dias que já saíram do histórico.
    pub fn record(&mut self, prev: Option<&Usage>, today: &str) {
        self.days = prev.map(|p| p.days.clone()).unwrap_or_default();
        let spent = match (prev.and_then(|p| p.five_hour), self.five_hour) {
            // a mesma sessão: só o que subiu
            (Some(p), Some(n)) if (p.resets_at - n.resets_at).abs() < 60.0 => (n.used - p.used).max(0.0),
            // sessão nova (ou a primeira vez): tudo o que já foi usado nela
            (_, Some(n)) => n.used,
            _ => 0.0,
        };
        if spent > 0.0 {
            *self.days.entry(today.to_string()).or_insert(0.0) += spent;
        }
        if let Some(t) = day_number(today) {
            self.days.retain(|d, _| day_number(d).is_some_and(|n| t - n < HISTORY_DAYS));
        }
    }

    /// O que se gastou em cada dia da semana (segunda a domingo) nos últimos `HISTORY_DAYS`
    /// dias até `today`.
    pub fn by_weekday(&self, today: &str) -> [f64; 7] {
        let mut out = [0.0; 7];
        let Some(t) = day_number(today) else { return out };
        for (d, v) in &self.days {
            if let Some(n) = day_number(d).filter(|n| (0..HISTORY_DAYS).contains(&(t - n))) {
                out[weekday(n)] += v;
            }
        }
        out
    }
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

/// A data "AAAA-MM-DD" de um número de dias desde 1970-01-01.
pub fn iso_of_day(n: i64) -> String {
    let z = n + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// 0 = segunda ... 6 = domingo (1970-01-01 foi uma quinta).
pub fn weekday(day: i64) -> usize {
    (day + 3).rem_euclid(7) as usize
}

/// O arquivo com os últimos números.
pub fn data_file() -> PathBuf {
    crate::config::data_dir().join(FILE)
}

pub fn load() -> Option<Usage> {
    load_from(&data_file())
}

fn load_from(path: &Path) -> Option<Usage> {
    let text = std::fs::read_to_string(path).ok()?;
    Usage::from_json(&serde_json::from_str(&text).ok()?)
}

/// Grava inteiro de uma vez (num arquivo ao lado e depois troca), para o widget nunca ler pela
/// metade.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// O texto da linha de status: o modelo e os dois limites.
pub fn status_text(input: &Value, now: f64) -> String {
    let mut parts = Vec::new();
    if let Some(model) = input["model"]["display_name"].as_str() {
        parts.push(model.to_string());
    }
    if let Some(u) = Usage::from_status(input, now) {
        if let Some(w) = u.five_hour {
            parts.push(format!("Sessão {}%", (w.used * 100.0).round()));
        }
        if let Some(w) = u.seven_day {
            parts.push(format!("Semana {}%", (w.used * 100.0).round()));
        }
    }
    parts.join(" · ")
}

/// `Widgets.exe --claude-status`: lê o JSON do Claude Code, guarda os limites e responde a linha.
pub fn run_status_line() {
    use std::io::{Read, Write};
    let mut text = String::new();
    let _ = std::io::stdin().read_to_string(&mut text);
    let input: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let now = crate::win::now();
    if let Some(mut u) = Usage::from_status(&input, now) {
        u.record(load().as_ref(), &crate::win::local_time().iso_date());
        let _ = write_atomic(&data_file(), &u.to_json().to_string());
    }
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{}", status_text(&input, now));
    let _ = out.flush();
}

// ---------- ligação com o Claude Code ----------

/// Pasta das configurações do Claude Code.
pub fn claude_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".claude"))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Link {
    /// a linha de status é o Widgets
    Connected,
    /// ainda não: dá para conectar
    Off,
    /// o Claude Code já tem outra linha de status
    Other,
    /// o Claude Code não está instalado (não há a pasta dele)
    Missing,
}

/// O comando que o Claude Code roda: este executável com o argumento.
pub fn command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" {FLAG}", exe.to_string_lossy().replace('\\', "/")))
}

fn read_settings(path: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => {
            let v: Value = serde_json::from_str(&text).map_err(|_| "Não consegui ler as configurações do Claude Code.".to_string())?;
            if v.is_object() { Ok(v) } else { Err("As configurações do Claude Code estão num formato inesperado.".into()) }
        }
        _ => Ok(json!({})),
    }
}

/// Como está a ligação nas configurações em `dir`.
pub fn link_in(dir: &Path) -> Link {
    if !dir.is_dir() {
        return Link::Missing;
    }
    let Ok(settings) = read_settings(&dir.join("settings.json")) else { return Link::Other };
    match settings.get("statusLine") {
        None => Link::Off,
        Some(s) if s["command"].as_str().is_some_and(|c| c.contains(FLAG)) => Link::Connected,
        Some(_) => Link::Other,
    }
}

pub fn link() -> Link {
    claude_dir().map_or(Link::Missing, |d| link_in(&d))
}

/// Põe o Widgets como linha de status do Claude Code (em `dir`), com `command` como comando.
pub fn connect_in(dir: &Path, command: &str) -> Result<(), String> {
    if !dir.is_dir() {
        return Err("O Claude Code não foi encontrado neste computador.".into());
    }
    let path = dir.join("settings.json");
    let mut settings = read_settings(&path)?;
    if link_in(dir) == Link::Other {
        return Err("O Claude Code já tem outra linha de status.".into());
    }
    if path.exists() {
        // cópia de segurança, uma vez só (a de antes da primeira mudança)
        let backup = dir.join("settings.json.antes-do-widgets");
        if !backup.exists() {
            std::fs::copy(&path, &backup).map_err(|e| e.to_string())?;
        }
    }
    settings["statusLine"] = json!({"type": "command", "command": command, "padding": 0});
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())? + "\n";
    write_atomic(&path, &text).map_err(|_| "Não consegui gravar as configurações do Claude Code.".to_string())
}

pub fn connect() -> Result<(), String> {
    let dir = claude_dir().ok_or("O Claude Code não foi encontrado neste computador.")?;
    let command = command().ok_or("Não achei o caminho do Widgets.")?;
    connect_in(&dir, &command)
}

/// Tira o Widgets da linha de status (só se for ele que está lá).
pub fn disconnect_in(dir: &Path) -> Result<(), String> {
    if link_in(dir) != Link::Connected {
        return Ok(());
    }
    let path = dir.join("settings.json");
    let mut settings = read_settings(&path)?;
    settings.as_object_mut().unwrap().remove("statusLine");
    let text = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())? + "\n";
    write_atomic(&path, &text).map_err(|_| "Não consegui gravar as configurações do Claude Code.".to_string())
}

pub fn disconnect() -> Result<(), String> {
    claude_dir().map_or(Ok(()), |d| disconnect_in(&d))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("widgets-claude-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_the_status_line_limits() {
        let input = json!({
            "model": {"display_name": "Opus"},
            "rate_limits": {"five_hour": {"used_percentage": 23.5, "resets_at": 1000.0}, "seven_day": {"used_percentage": 41.2, "resets_at": 5000.0}}
        });
        let u = Usage::from_status(&input, 10.0).unwrap();
        let (s, w) = (u.five_hour.unwrap(), u.seven_day.unwrap());
        assert!((s.used - 0.235).abs() < 1e-9 && s.resets_at == 1000.0);
        assert!((w.used - 0.412).abs() < 1e-9 && w.resets_at == 5000.0);
        assert_eq!(Usage::from_json(&u.to_json()), Some(u));
        assert_eq!(status_text(&input, 10.0), "Opus · Sessão 24% · Semana 41%");
        // sem limites (outros planos, ou antes da primeira resposta): nada a guardar
        let plain = json!({"model": {"display_name": "Opus"}});
        assert_eq!(Usage::from_status(&plain, 10.0), None);
        assert_eq!(status_text(&plain, 10.0), "Opus");
        // uma janela pode faltar
        let half = json!({"rate_limits": {"seven_day": {"used_percentage": 10, "resets_at": 5}}});
        assert!(Usage::from_status(&half, 0.0).unwrap().five_hour.is_none());
    }

    #[test]
    fn windows_reset_and_forecast() {
        // sessão de 5 h que reinicia daqui a 2 h 15 (55% passou) com 46% usado: dura até o fim
        let now = 100_000.0;
        let w = Window { used: 0.46, resets_at: now + 2.25 * 3600.0 };
        assert!((w.elapsed_at(now, FIVE_HOURS).unwrap() - 0.55).abs() < 1e-9);
        assert_eq!(w.runs_out_at(now, FIVE_HOURS), None);
        // 88% em 60% da janela: acaba antes de reiniciar, daqui a uns 25 min
        let w = Window { used: 0.88, resets_at: now + 2.0 * 3600.0 };
        let end = w.runs_out_at(now, FIVE_HOURS).unwrap();
        assert!(end > now && end < w.resets_at, "{}", end - now);
        assert!(((end - now) / 60.0 - 24.5).abs() < 1.0, "{}", (end - now) / 60.0);
        // depois do reinício: vazia, sem ritmo
        assert_eq!(w.used_at(w.resets_at + 1.0), 0.0);
        assert_eq!(w.elapsed_at(w.resets_at + 1.0, FIVE_HOURS), None);
        // no limite: acabou agora
        assert_eq!(Window { used: 1.0, resets_at: now + 60.0 }.runs_out_at(now, FIVE_HOURS), Some(now));
    }

    #[test]
    fn connects_without_touching_the_rest() {
        let dir = temp_dir("connect");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{\n  \"theme\": \"dark\",\n  \"model\": \"opus\",\n  \"permissions\": {\"allow\": [\"Bash(ls)\"]}\n}\n").unwrap();
        assert_eq!(link_in(&dir), Link::Off);
        connect_in(&dir, "\"C:/W/Widgets.exe\" --claude-status").unwrap();
        assert_eq!(link_in(&dir), Link::Connected);
        let text = std::fs::read_to_string(&path).unwrap();
        // as outras chaves continuam, na mesma ordem, e a cópia de segurança é a de antes
        let order: Vec<usize> = ["\"theme\"", "\"model\"", "\"permissions\"", "\"statusLine\""].iter().map(|k| text.find(k).unwrap()).collect();
        assert!(order.windows(2).all(|p| p[0] < p[1]), "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["permissions"]["allow"][0], "Bash(ls)");
        assert_eq!(v["statusLine"]["command"], "\"C:/W/Widgets.exe\" --claude-status");
        assert!(!std::fs::read_to_string(dir.join("settings.json.antes-do-widgets")).unwrap().contains("statusLine"));
        // desconectar tira só a linha de status
        disconnect_in(&dir).unwrap();
        assert_eq!(link_in(&dir), Link::Off);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["theme"], "dark");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn never_replaces_another_status_line() {
        let dir = temp_dir("other");
        let path = dir.join("settings.json");
        let mine = "{\"statusLine\": {\"type\": \"command\", \"command\": \"~/.claude/statusline.sh\"}}";
        std::fs::write(&path, mine).unwrap();
        assert_eq!(link_in(&dir), Link::Other);
        assert!(connect_in(&dir, "x --claude-status").is_err());
        disconnect_in(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), mine, "nada mudou");
        // sem o arquivo: conecta criando um
        let empty = temp_dir("empty");
        connect_in(&empty, "x --claude-status").unwrap();
        assert_eq!(link_in(&empty), Link::Connected);
        // sem a pasta do Claude Code
        assert_eq!(link_in(&empty.join("nada")), Link::Missing);
        assert!(connect_in(&empty.join("nada"), "x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn keeps_the_last_numbers_in_a_file() {
        let dir = temp_dir("file");
        let path = dir.join(FILE);
        let mut u = Usage { five_hour: Some(Window { used: 0.5, resets_at: 9.0 }), seven_day: None, updated: 3.0, ..Default::default() };
        u.days.insert("2026-11-03".into(), 0.5);
        write_atomic(&path, &u.to_json().to_string()).unwrap();
        assert_eq!(load_from(&path), Some(u));
        assert!(!path.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dates_and_weekdays() {
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(day_number("2026-11-03"), Some(20760));
        assert_eq!(iso_of_day(20760), "2026-11-03");
        assert_eq!(iso_of_day(day_number("2024-02-29").unwrap() + 1), "2024-03-01");
        // 2026-11-03 é uma terça
        assert_eq!(weekday(day_number("2026-11-03").unwrap()), 1);
        assert_eq!(day_number("ontem"), None);
    }

    #[test]
    fn keeps_what_was_spent_each_day() {
        let at = |used: f64, resets_at: f64| Usage { five_hour: Some(Window { used, resets_at }), ..Default::default() };
        // primeira vez: conta o que a sessão já tinha
        let mut a = at(0.2, 1000.0);
        a.record(None, "2026-11-03");
        assert!((a.days["2026-11-03"] - 0.2).abs() < 1e-9);
        // a mesma sessão subiu para 50%: mais 30% no dia
        let mut b = at(0.5, 1000.0);
        b.record(Some(&a), "2026-11-03");
        assert!((b.days["2026-11-03"] - 0.5).abs() < 1e-9);
        // sessão nova no dia seguinte, com 10%
        let mut c = at(0.1, 20000.0);
        c.record(Some(&b), "2026-11-04");
        assert!((c.days["2026-11-04"] - 0.1).abs() < 1e-9);
        let week = c.by_weekday("2026-11-04");
        assert!((week[1] - 0.5).abs() < 1e-9 && (week[2] - 0.1).abs() < 1e-9, "{week:?}");
        // os dias velhos saem do histórico
        let mut d = at(0.3, 90000.0);
        d.record(Some(&c), "2026-12-15");
        assert_eq!(d.days.keys().collect::<Vec<_>>(), ["2026-12-15"]);
    }
}
