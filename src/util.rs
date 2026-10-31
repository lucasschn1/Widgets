//! Datas em português, tempo relativo ("há 5 min") e acesso à internet.

use crate::win::{self, LocalTime};

pub const MONTHS: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
pub const WEEKDAYS: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
pub const MONTHS_LONG: [&str; 12] =
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
pub const WEEKDAYS_LONG: [&str; 7] = ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"];

/// "sábado, 31 de outubro"
pub fn long_date(t: &LocalTime) -> String {
    format!("{}, {} de {}", WEEKDAYS_LONG[t.weekday as usize], t.day, MONTHS_LONG[t.month as usize - 1])
}

/// "sáb, 31 de out"
pub fn short_date(t: &LocalTime) -> String {
    format!("{}, {} de {}", WEEKDAYS[t.weekday as usize], t.day, MONTHS[t.month as usize - 1])
}

/// "agora", "há 5 min", "há 3 h", "ontem", "há 4 dias", "12 out".
pub fn time_ago(epoch: f64) -> String {
    let seconds = win::now() - epoch;
    if seconds < 60.0 {
        return "agora".into();
    }
    if seconds < 3600.0 {
        return format!("há {} min", (seconds / 60.0) as i64);
    }
    if seconds < 86400.0 {
        return format!("há {} h", (seconds / 3600.0) as i64);
    }
    let days = (seconds / 86400.0) as i64;
    if days == 1 {
        return "ontem".into();
    }
    if days < 7 {
        return format!("há {days} dias");
    }
    let (_, m, d) = civil_from_days(((epoch - tz_offset()) / 86400.0).floor() as i64);
    format!("{} {}", d, MONTHS[m as usize - 1])
}

/// Diferença (segundos) entre UTC e o horário local.
fn tz_offset() -> f64 {
    let t = win::local_time();
    let local_secs = t.hour as f64 * 3600.0 + t.minute as f64 * 60.0 + t.second as f64;
    let utc_secs = win::now().rem_euclid(86400.0);
    let mut diff = utc_secs - local_secs;
    if diff > 43200.0 {
        diff -= 86400.0;
    } else if diff < -43200.0 {
        diff += 86400.0;
    }
    (diff / 900.0).round() * 900.0 // fusos são múltiplos de 15 min
}

/// (ano, mês, dia) de um número de dias desde 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Dias desde 1970-01-01 de uma data (ano, mês, dia).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// "2026-10-31" -> dias desde 1970.
pub fn parse_date(s: &str) -> Option<i64> {
    let mut parts = s.get(..10)?.split('-');
    let y = parts.next()?.parse().ok()?;
    let m = parts.next()?.parse().ok()?;
    let d = parts.next()?.parse().ok()?;
    Some(days_from_civil(y, m, d))
}

/// Segundos desde 1970 de "2026-10-31T14:05:00Z" (UTC).
pub fn parse_iso_utc(s: &str) -> Option<f64> {
    let days = parse_date(s)?;
    let time = s.get(11..19)?;
    let mut t = time.split(':');
    let (h, m, sec): (f64, f64, f64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?, t.next()?.parse().ok()?);
    Some(days as f64 * 86400.0 + h * 3600.0 + m * 60.0 + sec)
}

/// Baixa um JSON (com tempo limite) — chamar fora da thread da interface.
pub fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    fetch_json_with(url, &[])
}

pub fn fetch_json_with(url: &str, headers: &[(&str, &str)]) -> Result<serde_json::Value, String> {
    // TLS do próprio Windows (Schannel): executável menor e usa os certificados do sistema
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build();
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .tls_config(tls)
        .build()
        .new_agent();
    let mut req = agent.get(url).header("User-Agent", "desktop-widget");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// Codifica um texto para usar numa URL.
pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
