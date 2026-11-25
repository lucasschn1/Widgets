//! Datas em português, tempo relativo ("há 5 min") e acesso à internet.

use crate::platform::win::{self, LocalTime};

pub const MONTHS: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
pub const WEEKDAYS: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
pub const MONTHS_LONG: [&str; 12] =
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
pub const WEEKDAYS_LONG: [&str; 7] = ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"];

/// "sábado, 31 de outubro"
pub fn long_date(date: &LocalTime) -> String {
    format!("{}, {} de {}", WEEKDAYS_LONG[date.weekday as usize], date.day, MONTHS_LONG[date.month as usize - 1])
}

/// "sáb, 31 de out"
pub fn short_date(date: &LocalTime) -> String {
    format!("{}, {} de {}", WEEKDAYS[date.weekday as usize], date.day, MONTHS[date.month as usize - 1])
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
    let date = civil_from_days(((epoch - tz_offset()) / 86400.0).floor() as i64);
    format!("{} {}", date.day, MONTHS[date.month as usize - 1])
}

/// Diferença (segundos) entre UTC e o horário local.
pub fn tz_offset() -> f64 {
    let clock = win::local_time();
    let local_secs = clock.hour as f64 * 3600.0 + clock.minute as f64 * 60.0 + clock.second as f64;
    let utc_secs = win::now().rem_euclid(86400.0);
    let mut diff = utc_secs - local_secs;
    if diff > 43200.0 {
        diff -= 86400.0;
    } else if diff < -43200.0 {
        diff += 86400.0;
    }
    (diff / 900.0).round() * 900.0 // fusos são múltiplos de 15 min
}

/// Uma data do calendário.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilDate {
    pub year: i64,
    pub month: u32,
    pub day: u32,
}

/// A data de um número de dias desde 1970-01-01 (a conta de "civil_from_days", de Howard
/// Hinnant: o ano começa em março, e as eras têm 400 anos).
pub fn civil_from_days(z: i64) -> CivilDate {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    CivilDate { year: if month <= 2 { y + 1 } else { y }, month, day }
}

/// Dias desde 1970-01-01 de uma data (ano, mês, dia).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// "2026-10-31" -> dias desde 1970.
pub fn parse_date(text: &str) -> Option<i64> {
    let mut parts = text.get(..10)?.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    Some(days_from_civil(year, month, day))
}

/// Baixa um JSON (com tempo limite) — chamar fora da thread da interface.
pub fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    fetch_json_with(url, &[])
}

/// Como `fetch_json`, com cabeçalhos a mais.
pub fn fetch_json_with(url: &str, headers: &[(&str, &str)]) -> Result<serde_json::Value, String> {
    let (status, text) = fetch_text(url, headers)?;
    if status >= 400 {
        return Err(format!("HTTP {status}"));
    }
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

/// Quanto um pedido pode levar, do começo ao fim (as buscas de dados, pequenas).
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
/// Quanto um download de arquivo grande pode levar (o Widgets.exe da atualização, numa conexão lenta).
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Pedido GET já com a configuração de rede do app, com o tempo limite `timeout`.
fn get(url: &str, headers: &[(&str, &str)], timeout: std::time::Duration) -> Result<ureq::http::Response<ureq::Body>, String> {
    // TLS do próprio Windows (Schannel): executável menor e usa os certificados do sistema (sem
    // PlatformVerifier, o ureq usaria só a lista da Mozilla, e sites como o YouTube, cuja cadeia o
    // Windows monta por outra raiz, falhariam)
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .tls_config(tls)
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut req = agent.get(url).header("User-Agent", "desktop-widget");
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    req.call().map_err(|error| error.to_string())
}

/// Baixa uma página: (código HTTP, texto). Erros de rede viram Err.
pub fn fetch_text(url: &str, headers: &[(&str, &str)]) -> Result<(u16, String), String> {
    let mut resp = get(url, headers, FETCH_TIMEOUT)?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().with_config().limit(20 * 1024 * 1024).read_to_string().map_err(|error| error.to_string())?;
    Ok((status, text))
}

/// Baixa um arquivo (ex.: uma imagem): (código HTTP, bytes).
pub fn fetch_bytes(url: &str) -> Result<(u16, Vec<u8>), String> {
    let mut resp = get(url, &[], FETCH_TIMEOUT)?;
    let status = resp.status().as_u16();
    let bytes = resp.body_mut().with_config().limit(10 * 1024 * 1024).read_to_vec().map_err(|error| error.to_string())?;
    Ok((status, bytes))
}

/// Baixa um arquivo grande, de até `max_bytes`, com mais tempo que as buscas (o Widgets.exe da
/// atualização): (código HTTP, bytes). Chamar fora da thread da interface.
pub fn download(url: &str, max_bytes: u64) -> Result<(u16, Vec<u8>), String> {
    let mut resp = get(url, &[], DOWNLOAD_TIMEOUT)?;
    let status = resp.status().as_u16();
    let bytes = resp.body_mut().with_config().limit(max_bytes).read_to_vec().map_err(|error| error.to_string())?;
    Ok((status, bytes))
}

/// Codifica um texto para usar numa URL.
pub fn urlencode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
