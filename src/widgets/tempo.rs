//! Widget de Tempo ("céu vivo"): o fundo é o próprio céu, como no app Tempo do iPhone e do Mac
//! (as cores andam com a hora da cidade, do nascer ao pôr do sol, e cada clima tem as suas, com
//! chuva, neve, granizo, vento e raios de verdade; veja `sky`), as próximas horas e os próximos
//! dias.
//!
//! Navegável como o Tempo do Mac:
//! - a faixa de horas rola (roda do mouse ou setinhas nas pontas) pelas próximas 24 horas;
//! - clicar no tempo de agora abre os painéis de detalhes (sensação, umidade, vento, UV, sol e
//!   chuva), em páginas com pontinhos no pequeno e no médio;
//! - clicar num dia (no grande) ou numa hora abre o dia: as horas dele, chuva, vento, UV e o sol,
//!   com setas para o dia anterior e o seguinte.

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::odometer::Odometer;
use super::sky::{self, Weather};
use super::island_ui as iu;
use super::{alert_hash, Ctx, Env, Hot, IslandForm, IslandItem, Kind, Layout, MenuItem, Mode, NotifyAction, Status, CARD_PAD, LARGE_H, MEDIUM_H, SMALL, WIDE};
use crate::data::config::Get;
use crate::gfx::drawing::weather_art;
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::ui::theme::*;
use crate::data::util;
use crate::platform::win;

const W: f32 = WIDE;
const PAD: f32 = CARD_PAD;
const M_H: f32 = MEDIUM_H;
/// colunas de horas visíveis na faixa
const HOURS: usize = 6;
/// horas na faixa da tela principal (rolando)
const NEXT_HOURS: usize = 24;
const STRIP_H: f32 = 66.0;
/// A temperatura de agora, o maior número do widget: a fonte dos números, mais leve que nos
/// outros widgets (de tão grande, mesmo no peso leve pesava demais).
const TEMP_FACE: Face = Face::NumbersExtraLight;
/// A temperatura de cada hora e a máxima de cada dia: a fonte dos números, no peso médio (em
/// seminegrito, as linhas de números pesavam mais que o resto).
const HOUR_TEMP_FACE: Face = super::VALUE;
/// Uma coluna de hora com a chance de chuva (a faixa que rola e a ilha): onde fica o ícone (e o
/// tamanho dele), a chance de chuva e a temperatura, a partir do alto da coluna. A chance de
/// chuva tem a linha dela, com respiro do ícone e da temperatura.
const HOUR_ICON_Y: f32 = 14.0;
const HOUR_ICON: f32 = 20.0;
const HOUR_POP_Y: f32 = 37.0;
const HOUR_TEMP_Y: f32 = 50.0;
/// No grande, as horas: o respiro entre o ícone e a chance de chuva, e onde fica a temperatura
/// (abaixo da linha da chance de chuva, em todas as horas, para ficarem alinhadas).
const BIG_POP_GAP: f32 = 2.5;
const BIG_HOUR_TEMP_Y: f32 = 57.0;
// tela principal, com respiro (como o Tempo da Apple): margem maior, a temperatura solta das
// horas e, no grande, 5 dias em linhas altas
const M_PAD: f32 = CARD_PAD;
const G_PAD: f32 = CARD_PAD;
const M_HOURS_Y: f32 = 84.0;
const G_HOURS_Y: f32 = 110.0;
/// grande: a linha que separa as horas dos dias, o aviso de chuva e os dias
const G_RULE_Y: f32 = 194.0;
const G_RAIN: (f32, f32) = (200.0, 226.0);
const G_DAYS: usize = 5;
/// painéis de detalhe no grande: 3 linhas de 2, ocupando a altura toda
const G_TILE_GAP: f32 = 8.0;
const G_TILES_Y: f32 = 46.0;
const G_TILE_H: f32 = (LARGE_H - G_TILES_Y - 14.0 - 2.0 * G_TILE_GAP) / 3.0;
/// detalhe do dia: onde ficam a faixa de horas (médio / grande) e os painéis (grande)
const DAY_STRIP_M: f32 = 44.0;
const DAY_STRIP_G: f32 = 104.0;
const DAY_TILES_G: f32 = 182.0;
const DAY_TILE_H: f32 = (LARGE_H - DAY_TILES_G - 14.0 - G_TILE_GAP) / 2.0;
const SCROLL_MS: f32 = 260.0;
/// Condição do tempo no pequeno: tamanho e distância entre as duas linhas, quando quebra.
const COMPACT_DESC_SIZE: f32 = 12.0;
const COMPACT_DESC_LINE: f32 = 15.0;

/// Tamanho do nome de um evento ("Nascer", "Pôr do sol") na faixa de horas: 10, ou menos (até
/// 8,5) para caber em `max_w`.
fn event_label_size(c: &Canvas, text: &str, max_w: f32) -> f32 {
    let mut size = 10.0;
    while size > 8.5 && c.text_width(text, Face::Semibold, size) > max_w {
        size -= 0.25;
    }
    size
}

/// Tamanho da condição no cabeçalho do médio (`big` = grande).
fn top_desc_size(big: bool) -> f32 {
    if big { 13.5 } else { 12.5 }
}
const RAIN_BLUE: Rgba = CYAN;
/// a chance de chuva escrita sobre o céu: azul bem claro (o ciano some no céu azul)
const POP_TEXT: Rgba = Rgba::rgb(206, 238, 255);
/// faixa do índice UV: baixo, moderado, alto, muito alto e extremo
const UV_STOPS: [(f32, Rgba); 5] =
    [(0.0, Rgba::rgb(48, 209, 88)), (0.3, Rgba::rgb(255, 214, 10)), (0.55, Rgba::rgb(255, 159, 10)), (0.8, Rgba::rgb(255, 69, 58)), (1.0, Rgba::rgb(191, 90, 242))];
/// faixa de temperatura: do frio ao quente
/// umidade: do seco (quase branco) ao úmido (o azul claro da chance de chuva, que lê sobre o céu)
const HUMID_STOPS: [(f32, Rgba); 2] = [(0.0, Rgba(255, 255, 255, 110)), (1.0, POP_TEXT)];
const TEMP_STOPS: [(f32, Rgba); 4] = [(0.0, Rgba::rgb(90, 200, 250)), (0.45, Rgba::rgb(120, 220, 160)), (0.75, Rgba::rgb(255, 214, 10)), (1.0, Rgba::rgb(255, 120, 40))];

const WEATHER_EVERY: Duration = Duration::from_secs(15 * 60);
const WEATHER_RETRY: Duration = Duration::from_secs(2 * 60);
const RAIN_FPS: u32 = 30;
/// de quanto em quanto tempo o céu é redesenhado (as cores andam com a hora)
const SKY_EVERY: Duration = Duration::from_secs(60);
/// ventania: vento médio ou rajadas a partir destes km/h (a previsão não tem código para isso)
const WINDY_KMH: f64 = 40.0;
/// chuva a partir disto (mm em 15 minutos) avisa que vai chover
const RAIN_MM: f64 = 0.2;
/// a ilha avisa da trovoada que chega nestas próximas horas
const STORM_SOON: usize = 3;
/// a máxima de amanhã longe assim da de hoje é uma mudança forte (uma frente fria, um calorão)
const TEMP_JUMP: i64 = 6;
/// as cores dos avisos na ilha
const STORM_COLOR: Rgba = Rgba::rgb(255, 214, 10);
const COLD: Rgba = Rgba::rgb(90, 200, 250);
const WARM: Rgba = Rgba::rgb(255, 140, 60);
const WINDY_GUSTS: f64 = 65.0;

// ações
const OPEN_FORECAST: u32 = 3;
pub const UPDATE_WEATHER: u32 = 4;
/// na ilha: "OK" (não mostrar mais este aviso)
const ISLAND_OK: u32 = 7;
pub const ASK_CITY: u32 = 5;
const BACK: u32 = 20;
const DETAILS: u32 = 21;
const DAY_PREV: u32 = 22;
const DAY_NEXT: u32 = 23;
const SCROLL_BACK: u32 = 24;
const SCROLL_FWD: u32 = 25;
const NEXT_PAGE: u32 = 26;
const PAGE: u32 = 30; // + página dos detalhes
const DAY: u32 = 40; // + dia (0 = hoje)
const HOUR: u32 = 60; // + posição na faixa de horas

const WEEKDAYS: [&str; 7] = ["Domingo", "Segunda", "Terça", "Quarta", "Quinta", "Sexta", "Sábado"];
const MONTHS: [&str; 12] = ["jan.", "fev.", "mar.", "abr.", "maio", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."];

type Rect4 = (f32, f32, f32, f32);

fn wmo(code: i64) -> &'static str {
    match code {
        0 => "Céu limpo",
        1 => "Predominantemente limpo",
        2 => "Parcialmente nublado",
        3 => "Nublado",
        45 => "Neblina",
        48 => "Neblina com geada",
        51 => "Garoa fraca",
        53 => "Garoa",
        55 => "Garoa forte",
        56 | 57 => "Garoa congelante",
        61 => "Chuva fraca",
        63 => "Chuva",
        65 => "Chuva forte",
        66 | 67 => "Chuva congelante",
        71 => "Neve fraca",
        73 => "Neve",
        75 => "Neve forte",
        77 => "Grãos de neve",
        80 | 81 => "Pancadas de chuva",
        82 => "Pancadas fortes",
        85 | 86 => "Pancadas de neve",
        95 => "Trovoada",
        96 | 99 => "Trovoada com granizo",
        _ => "—",
    }
}

/// A condição do tempo em poucas letras, para a ilha (onde "Predominantemente limpo" não cabe ao
/// lado de mais nada).
fn wmo_short(code: i64) -> &'static str {
    match code {
        1 => "Quase limpo",
        2 => "Algumas nuvens",
        48 => "Neblina",
        56 | 57 => "Garoa gelada",
        66 | 67 => "Chuva gelada",
        77 => "Neve fina",
        80 | 81 => "Pancadas",
        82 => "Temporal",
        85 | 86 => "Neve",
        96 | 99 => "Granizo",
        _ => wmo(code),
    }
}

/// Todas as condições (inteira, curta) e todos os avisos das próximas horas (inteiro, curto) que
/// o Tempo pode guardar, com as horas mais largas, para os testes da ilha conferirem que nada
/// fica cortado com "…".
#[cfg(test)]
pub fn all_island_texts() -> (Vec<(String, String)>, Vec<(String, String)>) {
    let codes: Vec<i64> = (0..=99).filter(|code| wmo(*code) != "—").collect();
    let mut descs: Vec<(String, String)> = codes.iter().map(|code| (wmo(*code).to_string(), wmo_short(*code).to_string())).collect();
    descs.push(("Ventania".into(), "Ventania".into()));
    let hours = |codes: &[i64]| json!(codes.iter().enumerate().map(|(i, code)| json!([if i == 0 { "Agora" } else { "23h" }, "x", 20, code, 0])).collect::<Vec<_>>());
    let mut lines: Vec<(String, String)> = Vec::new();
    for now in &codes {
        for next in &codes {
            lines.extend(sky_line(&hours(&[*now, *next]), None).map(|l| (l.text, l.short)));
            lines.extend(sky_line(&hours(&[0, 0, *next]), None).map(|l| (l.text, l.short)));
        }
    }
    lines.extend(sky_line(&hours(&[0, 0]), Some(120)).map(|l| (l.text, l.short)));
    lines.sort();
    lines.dedup();
    (descs, lines)
}

/// Nome do ícone (ver weather_art) para um código de tempo da Open-Meteo.
pub fn weather_icon(code: i64, is_day: bool) -> &'static str {
    match code {
        0 | 1 => if is_day { "sun" } else { "moon" },
        2 => if is_day { "partly_day" } else { "partly_night" },
        3 => "clouds",
        45 | 48 => "fog",
        51..=57 => "drizzle",
        65 | 67 | 82 => "heavy_rain",
        61..=67 | 80..=82 => "rain",
        71..=77 | 85 | 86 => "snow",
        96 | 99 => "hail",
        c if c >= 95 => "thunder",
        _ => "clouds",
    }
}

fn is_rain(code: i64) -> bool {
    (51..68).contains(&code) || (80..83).contains(&code) || matches!(code, 95 | 96 | 99)
}

fn is_snow(code: i64) -> bool {
    (71..78).contains(&code) || matches!(code, 85 | 86)
}

fn is_fog(code: i64) -> bool {
    matches!(code, 45 | 48)
}

fn is_storm(code: i64) -> bool {
    code >= 95
}

/// Tremer com o trovão (opção do usuário; ligada no começo).
pub fn shakes(cfg: &Value) -> bool {
    cfg["tempo"].bool_of("tremer").unwrap_or(true)
}

/// Cidades que combinam com o texto: [{"name", "detail", "label", "lat", "lon"}].
pub fn search_cities(query: &str, count: u32) -> Result<Vec<Value>, String> {
    let name = query.split(',').next().unwrap_or("").trim();
    if name.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count={count}&language=pt&format=json",
        util::urlencode(name)
    );
    let data = util::fetch_json(&url)?;
    let mut out = Vec::new();
    for place in data["results"].as_array().cloned().unwrap_or_default() {
        let pname = place.str_of("name").unwrap_or_default().to_string();
        let admin = place.str_of("admin1").unwrap_or_default();
        let country = place.str_of("country").unwrap_or_default();
        let label = if admin.is_empty() { pname.clone() } else { format!("{pname}, {admin}") };
        let detail = [admin, country].iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join(", ");
        out.push(json!({"name": pname, "detail": detail, "label": label, "lat": place["latitude"], "lon": place["longitude"]}));
    }
    Ok(out)
}

enum Fetch {
    /// (cidade, lat, lon, lugar), dados da previsão
    Ok((String, f64, f64, String), Value),
    NotFound(String),
    Offline,
}

fn fetch_weather(city: String, lat: Option<f64>, lon: Option<f64>, place: Option<String>) -> Fetch {
    let (lat, lon, place) = match (lat, lon) {
        (Some(lat), Some(lon)) => (lat, lon, place.unwrap_or_else(|| city.clone())),
        _ => match search_cities(&city, 1) {
            Ok(found) if !found.is_empty() => {
                let best = &found[0];
                (best["lat"].as_f64().unwrap_or(0.0), best["lon"].as_f64().unwrap_or(0.0), best.str_of("label").unwrap_or(&city).to_string())
            }
            Ok(_) => return Fetch::NotFound(format!("Cidade não encontrada: {}", city.split(',').next().unwrap_or("").trim())),
            Err(_) => return Fetch::Offline,
        },
    };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,dew_point_2m,weather_code,wind_speed_10m,wind_gusts_10m,wind_direction_10m,uv_index,is_day,precipitation\
         &hourly=temperature_2m,weather_code,is_day,precipitation_probability&minutely_15=precipitation&forecast_minutely_15=6\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,precipitation_probability_max,precipitation_sum,wind_speed_10m_max,wind_direction_10m_dominant,uv_index_max\
         &timezone=auto&forecast_days=7"
    );
    match util::fetch_json(&url) {
        Ok(data) => Fetch::Ok((city, lat, lon, place), data),
        Err(_) => Fetch::Offline,
    }
}

/// Minutos desde a meia-noite de "2026-10-31T14:15" (o horário local que a Open-Meteo devolve).
fn local_minutes(stamp: &str) -> Option<f64> {
    let days = util::parse_date(stamp)? as f64;
    let h: f64 = stamp.get(11..13)?.parse().ok()?;
    let m: f64 = stamp.get(14..16)?.parse().ok()?;
    Some(days * 1440.0 + h * 60.0 + m)
}

/// Minutos desde a meia-noite do dia (só a hora de "2026-10-31T14:15").
fn minute_of_day(stamp: &str) -> Option<i64> {
    let h: i64 = stamp.get(11..13)?.parse().ok()?;
    let m: i64 = stamp.get(14..16)?.parse().ok()?;
    Some(h * 60 + m)
}

/// "2026-10-31T05:21" -> "5:21".
fn hhmm(stamp: &str) -> String {
    match minute_of_day(stamp) {
        Some(m) => format!("{}:{:02}", m / 60, m % 60),
        None => "--:--".into(),
    }
}

fn round(v: &Value) -> i64 {
    v.as_f64().unwrap_or(0.0).round() as i64
}

/// Dia da semana de "2026-11-01" (0 = domingo; 1970-01-01 foi uma quinta).
fn weekday(date: &str) -> usize {
    util::parse_date(date).map(|d| ((d % 7 + 11) % 7) as usize).unwrap_or(0)
}

fn uv_level(uv: f64) -> &'static str {
    match uv.round() as i64 {
        ..=2 => "Baixo",
        3..=5 => "Moderado",
        6..=7 => "Alto",
        8..=10 => "Muito alto",
        _ => "Extremo",
    }
}

/// De onde vem o vento (graus, 0 = norte).
fn wind_from(deg: f64) -> &'static str {
    const NAMES: [&str; 8] = ["norte", "nordeste", "leste", "sudeste", "sul", "sudoeste", "oeste", "noroeste"];
    NAMES[(((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize) % 8]
}

/// Próximos dias: [rótulo, ícone, mínima, máxima].
fn days(data: &Value) -> Value {
    let daily = &data["daily"];
    let empty = Vec::new();
    let times = daily["time"].as_array().unwrap_or(&empty);
    let names = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
    let mut out = Vec::new();
    for (i, t) in times.iter().enumerate() {
        let label = if i == 0 { "Hoje".to_string() } else { t.as_str().map(|d| names[weekday(d)].to_string()).unwrap_or_default() };
        let code = daily["weather_code"][i].as_i64().unwrap_or(0);
        out.push(json!([label, weather_icon(code, true), round(&daily["temperature_2m_min"][i]), round(&daily["temperature_2m_max"][i])]));
    }
    Value::Array(out)
}

/// O aviso das próximas horas: o texto inteiro, o curto (para a ilha) e o ícone.
#[derive(Clone, Debug, PartialEq)]
struct SkyLine {
    text: String,
    short: String,
    icon: &'static str,
}

/// O aviso das próximas horas, ou nada: "Trovoadas até 17h, depois chuva" ("Trovoada até
/// 17h"), "Chuva até 0h, depois nublado" ("Chuva até 0h"), "Neve a partir das 15h" ("Neve às
/// 15h"), "Ventos de até 64 km/h" ("Vento de 64 km/h"; `gusts`: só quando está ventando forte).
fn sky_line(hours: &Value, gusts: Option<i64>) -> Option<SkyLine> {
    let list = hours.as_array()?;
    let code = |h: &Value| h[3].as_i64().unwrap_or(0);
    let label = |h: &Value| h[0].as_str().unwrap_or("").to_string();
    // (o que é, o nome no aviso inteiro, o nome no curto, o ícone)
    type Kind = (fn(i64) -> bool, &'static str, &'static str, &'static str);
    let kinds: [Kind; 4] = [(is_storm, "Trovoadas", "Trovoada", "thunder"), (is_rain, "Chuva", "Chuva", "rain"), (is_snow, "Neve", "Neve", "snow"), (is_fog, "Neblina", "Neblina", "fog")];
    let first = code(list.first()?);
    // o que está acontecendo agora, e até quando
    for (is, name, short, icon) in kinds {
        if is(first) {
            let (text, short) = match list.iter().position(|h| !is(code(h))) {
                Some(i) => (format!("{name} até {}, depois {}", label(&list[i]), wmo(code(&list[i])).to_lowercase()), format!("{short} até {}", label(&list[i]))),
                None => (format!("{name} nas próximas horas"), format!("{short} por horas")),
            };
            return Some(SkyLine { text, short, icon });
        }
    }
    if let Some(kmh) = gusts {
        return Some(SkyLine { text: format!("Ventos de até {kmh} km/h"), short: format!("Vento de {kmh} km/h"), icon: "wind" });
    }
    // chuva ou neve chegando
    list.iter().skip(1).find_map(|h| {
        kinds[1..3].iter().find(|k| (k.0)(code(h))).map(|k| SkyLine { text: format!("{} a partir das {}", k.1, label(h)), short: format!("{} às {}", k.2, label(h)), icon: k.3 })
    })
}

/// As horas com o nascer e o pôr do sol no meio, como no Tempo da Apple: uma coluna "18:52" com
/// o ícone e "Pôr do sol" no lugar da temperatura. `first`: o horário da primeira hora ("Agora");
/// `now`: o horário de agora (o que já passou não entra).
fn with_sun(hours: Vec<Value>, first: &str, now: &str, daily: &Value) -> Vec<Value> {
    let Some(start) = local_minutes(first) else { return hours };
    let now = local_minutes(now).unwrap_or(start);
    let end = start + hours.len() as f64 * 60.0;
    let mut events: Vec<(f64, Value)> = Vec::new();
    for (key, name, icon) in [("sunrise", "Nascer", "sunrise"), ("sunset", "Pôr do sol", "sunset")] {
        for d in 0..3 {
            let Some(stamp) = daily[key][d].as_str() else { continue };
            let Some(t) = local_minutes(stamp) else { continue };
            if t > now && t < end {
                events.push((t, json!([hhmm(stamp), icon, name, 0, 0, d, minute_of_day(stamp).unwrap_or(0) / 60])));
            }
        }
    }
    let mut out = Vec::with_capacity(hours.len() + events.len());
    for (i, h) in hours.into_iter().enumerate() {
        out.push(h);
        let (h0, h1) = (start + i as f64 * 60.0, start + (i + 1) as f64 * 60.0);
        out.extend(events.iter().filter(|(t, _)| *t >= h0 && *t < h1).map(|(_, e)| e.clone()));
    }
    out.truncate(NEXT_HOURS);
    out
}

/// Em quantos minutos começa a chover (na próxima hora), pela previsão de 15 em 15 minutos. Nada
/// se já está chovendo ou se não vem chuva.
fn rain_in(data: &Value) -> Option<f64> {
    let cur = &data["current"];
    if is_rain(cur.i64_of("weather_code").unwrap_or(0)) || cur.f64_of("precipitation").unwrap_or(0.0) > 0.0 {
        return None;
    }
    let now = cur.str_of("time").and_then(local_minutes)?;
    let steps = &data["minutely_15"];
    let (times, amounts) = (steps["time"].as_array()?, steps["precipitation"].as_array()?);
    times.iter().zip(amounts).find_map(|(stamp, amount)| {
        let minutes = stamp.as_str().and_then(local_minutes)? - now;
        (minutes > 0.0 && minutes <= 60.0 && amount.as_f64().unwrap_or(0.0) >= RAIN_MM).then_some(minutes)
    })
}

/// Tudo que o widget mostra, a partir da resposta da Open-Meteo. As horas são
/// [rótulo, ícone, temperatura, código, chance de chuva %, dia (0 = hoje), hora do dia].
fn process(data: &Value, place: &str) -> Value {
    let cur = &data["current"];
    let daily = &data["daily"];
    let hourly = &data["hourly"];
    let times: Vec<String> = hourly["time"].as_array().map(|a| a.iter().map(|t| t.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    let dates: Vec<String> = daily["time"].as_array().map(|a| a.iter().map(|t| t.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
    let code = cur.i64_of("weather_code").unwrap_or(0);
    let is_day = cur.i64_of("is_day").unwrap_or(1) != 0;
    // ventania: o céu sem chuva com vento forte (a previsão não tem um código para isso)
    let wind = cur.f64_of("wind_speed_10m").unwrap_or(0.0);
    let gusts = cur.f64_of("wind_gusts_10m").unwrap_or(wind);
    let windy = code <= 3 && (wind >= WINDY_KMH || gusts >= WINDY_GUSTS);
    let hour_of = |i: usize| times[i].get(11..13).and_then(|h| h.parse::<i64>().ok()).unwrap_or(0);
    // uma hora da previsão: [rótulo, ícone, temperatura, código, chance de chuva, dia, hora]
    let entry = |i: usize, label: String| -> Value {
        let hour_code = hourly["weather_code"][i].as_i64().unwrap_or(0);
        let daylight = hourly["is_day"][i].as_i64().unwrap_or(1) != 0;
        let pop = hourly["precipitation_probability"][i].as_i64().unwrap_or(0);
        let day_index = dates.iter().position(|date| !date.is_empty() && times[i].starts_with(date.as_str())).unwrap_or(0);
        json!([label, weather_icon(hour_code, daylight), round(&hourly["temperature_2m"][i]), hour_code, pop, day_index, hour_of(i)])
    };
    // a faixa começa na hora de agora
    let now_key = cur.str_of("time").and_then(|t| t.get(..13)).unwrap_or("");
    let start = times.iter().position(|t| t.get(..13) == Some(now_key)).unwrap_or(0);
    let hours: Vec<Value> = (start..times.len().min(start + NEXT_HOURS))
        .map(|i| entry(i, if i == start { "Agora".into() } else { format!("{}h", hour_of(i)) }))
        .collect();
    let day_info: Vec<Value> = dates
        .iter()
        .enumerate()
        .map(|(day, date)| {
            let day_hours: Vec<Value> = (0..times.len()).filter(|&i| times[i].starts_with(date.as_str())).map(|i| entry(i, format!("{}h", hour_of(i)))).collect();
            day_summary(day, date, daily, day_hours)
        })
        .collect();
    let next12 = Value::Array(hours.iter().take(12).cloned().collect());
    let line = sky_line(&next12, windy.then(|| gusts.max(wind).round() as i64));
    let hours = with_sun(hours, times.get(start).map(String::as_str).unwrap_or(""), cur.str_of("time").unwrap_or(""), daily);
    let now = now_details(cur, daily, &hours);
    json!({
        "place": place,
        "temp": round(&cur["temperature_2m"]),
        "code": code,
        "is_day": is_day,
        "icon": if windy { "wind" } else { weather_icon(code, is_day) },
        "desc": if windy { "Ventania" } else { wmo(code) },
        "desc_short": if windy { "Ventania" } else { wmo_short(code) },
        "windy": windy,
        "utc_offset": data["utc_offset_seconds"].as_f64(),
        "max": round(&daily["temperature_2m_max"][0]),
        "min": round(&daily["temperature_2m_min"][0]),
        "details": format!("Sensação {}°  ·  Umidade {}%  ·  Vento {} km/h",
            round(&cur["apparent_temperature"]), round(&cur["relative_humidity_2m"]), round(&cur["wind_speed_10m"])),
        "rain_line": line.as_ref().map(|l| l.text.clone()),
        "rain_line_short": line.as_ref().map(|l| l.short.clone()),
        "rain_in": rain_in(data),
        "line_icon": line.map(|l| l.icon),
        "hours": hours,
        "days": days(data),
        "day_info": day_info,
        "now": now,
    })
}

/// Um dia da previsão, para os detalhes e a lista dos próximos dias: o nome ("Hoje", "Amanhã",
/// "Sexta"), a data, o céu, a mínima e a máxima, a chuva, o vento, o UV, o sol e as horas.
fn day_summary(day: usize, date: &str, daily: &Value, day_hours: Vec<Value>) -> Value {
    let name = match day {
        0 => "Hoje".to_string(),
        1 => "Amanhã".to_string(),
        _ => WEEKDAYS[weekday(date)].to_string(),
    };
    let day_of_month = date.get(8..10).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
    let month = date.get(5..7).and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).clamp(1, 12);
    let day_code = daily["weather_code"][day].as_i64().unwrap_or(0);
    json!({
        "name": name,
        "date": format!("{day_of_month} de {}", MONTHS[month - 1]),
        "icon": weather_icon(day_code, true),
        "desc": wmo(day_code),
        "max": round(&daily["temperature_2m_max"][day]),
        "min": round(&daily["temperature_2m_min"][day]),
        "pop": daily["precipitation_probability_max"][day].as_i64().unwrap_or(0),
        "rain": daily["precipitation_sum"][day].as_f64().unwrap_or(0.0),
        "wind": round(&daily["wind_speed_10m_max"][day]),
        "wind_dir": daily["wind_direction_10m_dominant"][day].as_f64(),
        "uv": daily["uv_index_max"][day].as_f64().unwrap_or(0.0),
        "sunrise": daily["sunrise"][day].as_str().unwrap_or(""),
        "sunset": daily["sunset"][day].as_str().unwrap_or(""),
        "hours": day_hours,
    })
}

/// Os detalhes de agora (os painéis da tela de detalhes): sensação, umidade, vento, UV, chuva e
/// o sol de hoje e de amanhã.
fn now_details(cur: &Value, daily: &Value, hours: &[Value]) -> Value {
    json!({
        "time": cur.str_of("time").unwrap_or(""),
        "temp": round(&cur["temperature_2m"]),
        "feels": round(&cur["apparent_temperature"]),
        "humidity": round(&cur["relative_humidity_2m"]),
        "dew": round(&cur["dew_point_2m"]),
        "wind": round(&cur["wind_speed_10m"]),
        "wind_dir": cur.f64_of("wind_direction_10m").unwrap_or(0.0),
        "uv": cur.f64_of("uv_index").unwrap_or(0.0),
        "uv_max": daily["uv_index_max"][0].as_f64().unwrap_or(0.0),
        "pop": hours.iter().take(12).filter_map(|h| h[4].as_i64()).max().unwrap_or(0),
        "rain_today": daily["precipitation_sum"][0].as_f64().unwrap_or(0.0),
        "sunrise": daily["sunrise"][0].as_str().unwrap_or(""),
        "sunset": daily["sunset"][0].as_str().unwrap_or(""),
        "sunrise_next": daily["sunrise"][1].as_str().unwrap_or(""),
    })
}

/// Desacelera no fim (de 0 a 1): a faixa de horas rolando para no lugar.
/// Onde fica um painel de detalhe: o retângulo, a margem de dentro (`inner_x`, `inner_w`) e se é
/// o painel alto (do grande) ou o baixo.
struct TileArea {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    inner_x: f32,
    inner_w: f32,
    tall: bool,
}

impl TileArea {
    fn new(x: f32, y: f32, w: f32, h: f32) -> TileArea {
        TileArea { x, y, w, h, inner_x: x + 12.0, inner_w: w - 24.0, tall: h >= 100.0 }
    }
}

/// Onde o valor grande do painel ficou: a linha de cima, o tamanho da letra, a largura do número
/// e o pé do valor.
struct TileValue {
    y: f32,
    size: f32,
    number_w: f32,
    bottom: f32,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Posição de uma faixa que rola, com animação (em colunas).
struct Scroll {
    from: f32,
    to: f32,
    since: Instant,
}

impl Scroll {
    fn at(pos: f32) -> Scroll {
        Scroll { from: pos, to: pos, since: Instant::now() - Duration::from_secs(1) }
    }

    /// Onde a faixa está agora (andando de `from` para `to`).
    fn shown(&self) -> f32 {
        let progress = ease(self.since.elapsed().as_secs_f32() * 1000.0 / SCROLL_MS);
        self.from + (self.to - self.from) * progress
    }

    fn go(&mut self, to: f32) {
        if (to - self.to).abs() < 0.001 {
            return;
        }
        self.from = self.shown();
        self.to = to;
        self.since = Instant::now();
    }

    fn animating(&self) -> bool {
        self.since.elapsed().as_secs_f32() * 1000.0 < SCROLL_MS + 40.0
    }
}

/// O que o widget está mostrando.
#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Main,
    /// painéis de detalhes de agora
    Details,
    /// um dia (0 = hoje)
    Day(usize),
}

/// Desenho extra de um painel.
enum Visual {
    None,
    /// índice UV (barra de baixo a extremo, com o ponto)
    Uv(f64),
    /// o caminho do sol no dia: nascer, pôr e agora (frações do dia, de 0 a 1)
    Sun(f32, f32, Option<f32>),
    /// vento: de onde vem (graus) e a velocidade (km/h)
    Wind(f64, i64),
    /// sensação: a temperatura real e a sensação
    Feels(i64, i64),
    /// umidade (%): faixa do seco ao úmido, com o ponto
    Humidity(i64),
    /// chance de chuva hora a hora (%): barrinhas
    Rain(Vec<i64>),
}

/// Um painel de detalhe: título, valor grande, frase embaixo e o desenho.
struct Tile {
    title: String,
    value: String,
    sub: String,
    /// a frase no painel baixo (pequeno e médio), quando a inteira não cabe numa linha ao lado
    /// do desenho
    sub_short: Option<String>,
    visual: Visual,
}

pub struct Tempo {
    weather: Option<Value>,
    weather_msg: String,
    next_weather: Option<Instant>,
    pending: Option<Receiver<Fetch>>,
    poll_at: Instant,
    status: Status,
    started: Instant,
    view: Cell<View>,
    /// quando a vista (ou a página) mudou: os painéis entram um depois do outro
    view_at: Cell<Instant>,
    /// próximo quadro das animações (um prazo fixo: "agora + 16 ms" calculado a cada pergunta
    /// ficaria sempre no futuro, e a animação nunca andaria)
    frame_at: Cell<Instant>,
    /// estava animando no último tick (para desenhar o quadro final)
    was_animating: Cell<bool>,
    /// página dos detalhes (pequeno e médio)
    page: Cell<usize>,
    /// roda do mouse acumulada para virar a página (o touchpad manda passos pequenos)
    page_wheel: f32,
    /// roda do mouse acumulada na faixa de horas (anda de hora em hora)
    hour_wheel: f32,
    /// faixas de horas: a da tela principal e a do dia aberto
    main_scroll: Scroll,
    day_scroll: Scroll,
    /// o último tamanho desenhado (ao mudar de tamanho, volta para a tela principal)
    mode_seen: Cell<Mode>,
    /// os raios da trovoada
    storm: sky::Storm,
    /// próximo redesenho do céu (as cores andam com a hora)
    sky_at: Cell<Instant>,
    /// tremer com o trovão (a opção, lida a cada desenho)
    shake_on: Cell<bool>,
    /// hora fixa do céu (exemplos e imagens do README); None: a hora da cidade
    fixed_hour: Option<f32>,
    /// a temperatura, rolando como num hodômetro quando muda
    temp_roll: Odometer,
    /// a chuva chegando (o aviso da ilha): o aviso e quando ela começa (segundos desde 1970)
    rain_alert: Option<(String, f64)>,
    /// os avisos da ilha dispensados ("OK")
    island_ok: Vec<String>,
    /// os raios da ilha aberta (os do cartão são do tamanho dele)
    island_storm: sky::Storm,
    /// o céu da ilha aberta, guardado: (largura e altura em pixels, clima, minuto) e a imagem
    island_sky: RefCell<Option<((u32, u32, Weather, i64), Image)>>,
}

/// Um aviso do Tempo na ilha: trovoada, chuva chegando ou uma mudança forte de temperatura.
struct Alert {
    key: String,
    /// a pílula fechada: o nome e o valor forte ("Chuva", "20 min")
    short: String,
    value: String,
    /// a pílula esticada com o aviso (com o mesmo valor à direita), e a versão curta quando o
    /// inteiro não cabe (sem a cidade)
    banner: String,
    banner_short: String,
    /// a ilha aberta: o título e a linha de baixo (e a curta, quando a inteira não cabe)
    title: String,
    sub: String,
    sub_short: String,
    /// o céu do aviso (a chuva que vem, a trovoada) e o ícone
    weather: Weather,
    icon: String,
    color: Rgba,
}

impl Tempo {
    pub fn new(cfg: &Value, cache: &Value) -> Tempo {
        let wcfg = &cfg["tempo"];
        let place = wcfg.str_of("place").or(wcfg.str_of("city"));
        // abre com o último clima guardado, enquanto busca o atual
        let cached = cache.get("weather").filter(|w| place.is_some() && w.str_of("place") == place && w.get("code").is_some()).cloned();
        let updated = if cached.is_some() { cache.f64_of("weather_time") } else { None };
        let mut tempo = Tempo {
            weather: cached,
            weather_msg: "Carregando…".into(),
            next_weather: None,
            pending: None,
            poll_at: Instant::now(),
            status: Status { updated, ..Default::default() },
            started: Instant::now(),
            view: Cell::new(View::Main),
            view_at: Cell::new(Instant::now() - Duration::from_secs(5)),
            frame_at: Cell::new(Instant::now()),
            was_animating: Cell::new(false),
            page: Cell::new(0),
            page_wheel: 0.0,
            hour_wheel: 0.0,
            main_scroll: Scroll::at(0.0),
            day_scroll: Scroll::at(0.0),
            mode_seen: Cell::new(Mode::M),
            storm: sky::Storm::default(),
            sky_at: Cell::new(Instant::now() + SKY_EVERY),
            shake_on: Cell::new(true),
            fixed_hour: None,
            temp_roll: Odometer::default(),
            rain_alert: None,
            island_ok: Vec::new(),
            island_storm: sky::Storm::default(),
            island_sky: RefCell::new(None),
        };
        if let Some(fetched) = updated {
            tempo.refresh_rain(fetched);
        }
        tempo
    }

    pub fn update_weather(&mut self, ctx: &mut Ctx) {
        let settings = ctx.cfg[ctx.key].clone();
        let Some(city) = settings.str_of("city").filter(|c| !c.is_empty()).map(String::from) else {
            self.weather_msg = "Clique para escolher a cidade".into();
            self.next_weather = None;
            return;
        };
        if self.pending.is_some() {
            return;
        }
        let (lat, lon, place) = (settings.f64_of("lat"), settings.f64_of("lon"), settings.str_of("place").map(String::from));
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(fetch_weather(city, lat, lon, place));
            crate::platform::events::wake();
        });
        self.pending = Some(rx);
        self.poll_at = Instant::now() + Duration::from_millis(200);
        self.status.loading = true;
        self.next_weather = None;
    }

    /// Troca a cidade; com `place` (escolhida na lista de sugestões) não precisa procurar de novo.
    pub fn set_city(&mut self, city: &str, place: Option<&Value>, ctx: &mut Ctx) {
        let w = ctx.wcfg();
        match place {
            Some(p) => {
                let label = p.str_of("label").unwrap_or(city).to_string();
                w.insert("city".into(), json!(label));
                w.insert("lat".into(), p["lat"].clone());
                w.insert("lon".into(), p["lon"].clone());
                w.insert("place".into(), json!(label));
            }
            None => {
                w.insert("city".into(), json!(city));
                for k in ["lat", "lon", "place"] {
                    w.insert(k.into(), Value::Null);
                }
            }
        }
        ctx.save();
        self.weather = None;
        self.weather_msg = "Atualizando…".into();
        self.pending = None;
        self.view.set(View::Main);
        self.main_scroll = Scroll::at(0.0);
        self.update_weather(ctx);
    }

    fn receive(&mut self, result: Fetch, ctx: &mut Ctx) {
        self.status.loading = false;
        match result {
            Fetch::NotFound(msg) => {
                self.weather = None;
                self.weather_msg = msg;
                self.status.error = None;
                self.next_weather = Some(Instant::now() + WEATHER_EVERY);
            }
            Fetch::Offline => {
                // sem conexão: mantém o último clima na tela, com o aviso no canto
                self.weather_msg = "Sem conexão. Clique para tentar de novo".into();
                self.status.error = Some("Sem conexão com o serviço do clima".into());
                self.next_weather = Some(Instant::now() + WEATHER_RETRY);
            }
            Fetch::Ok((city, lat, lon, place), data) => {
                let w = ctx.wcfg();
                w.insert("city".into(), json!(city));
                w.insert("lat".into(), json!(lat));
                w.insert("lon".into(), json!(lon));
                w.insert("place".into(), json!(place));
                ctx.save();
                let weather = process(&data, &place);
                self.status.error = None;
                self.status.updated = Some(win::now());
                ctx.cache["weather"] = weather.clone();
                ctx.cache["weather_time"] = json!(self.status.updated);
                ctx.out.push(super::Request::SaveCache);
                self.weather = Some(weather);
                self.refresh_rain(win::now());
                // a faixa começa de novo na hora de agora
                if self.view.get() == View::Main {
                    self.main_scroll = Scroll::at(0.0);
                }
                if let View::Day(d) = self.view.get() {
                    self.view.set(View::Day(d.min(self.day_count().saturating_sub(1))));
                }
                self.check_rain(&data, ctx);
                self.next_weather = Some(Instant::now() + WEATHER_EVERY);
            }
        }
    }

    /// Avisa quando vai começar a chover na próxima hora (e ainda não está chovendo).
    fn check_rain(&self, data: &Value, ctx: &mut Ctx) {
        let Some(minutes) = rain_in(data) else { return };
        let place = self.place_short();
        let query = format!("https://www.google.com/search?q={}", util::urlencode(&format!("previsão do tempo {place}")));
        ctx.notify("chuva", "Chuva chegando", format!("Deve começar a chover em cerca de {} min em {place}.", minutes.round()), Some(NotifyAction::Open(query)));
    }

    /// A chuva chegando, da previsão buscada em `fetched` (segundos desde 1970). A mesma chuva,
    /// com a previsão nova mexendo uns minutos, continua o mesmo aviso (a ilha não estica de novo).
    fn refresh_rain(&mut self, fetched: f64) {
        let at = self.weather.as_ref().and_then(|w| w.f64_of("rain_in")).map(|m| fetched + m * 60.0);
        self.rain_alert = match (at, self.rain_alert.take()) {
            (None, _) => None,
            (Some(at), Some((key, old))) if (at - old).abs() < 45.0 * 60.0 => Some((key, at)),
            (Some(at), _) => Some((format!("chuva|{}", at.round() as i64), at)),
        };
    }

    /// Os avisos para a ilha agora, do mais importante ao menos: trovoada (agora ou nas próximas
    /// horas), chuva começando na próxima hora, amanhã bem mais frio ou mais quente.
    fn island_alerts(&self) -> Vec<Alert> {
        let Some(weather) = self.weather.as_ref() else { return Vec::new() };
        [self.storm_alert(weather), self.rain_soon_alert(weather), self.temperature_alert(weather)].into_iter().flatten().collect()
    }

    /// " em Campinas" (vazio sem a cidade): para o texto dos avisos.
    fn in_place(&self) -> String {
        let place = self.place_short();
        if place.is_empty() { String::new() } else { format!(" em {place}") }
    }

    /// "Agora 24° · Nublado": a linha de baixo dos avisos.
    fn now_line(&self, weather: &Value) -> String {
        format!("Agora {} · {}", self.temp_text(), weather.str_of("desc").unwrap_or(""))
    }

    /// A linha de baixo curta: "Agora 24° · Quase limpo".
    fn now_line_short(&self, weather: &Value) -> String {
        let desc = weather.str_of("desc_short").or(weather.str_of("desc")).unwrap_or("");
        format!("Agora {} · {}", self.temp_text(), desc)
    }

    /// Trovoada agora, ou chegando nas próximas horas (com a hora).
    fn storm_alert(&self, weather: &Value) -> Option<Alert> {
        let code = self.code();
        let date = weather["now"].str_of("time").and_then(|t| t.get(..10)).unwrap_or("");
        // None: agora; Some(hora): a hora em que chega
        let arrives: Option<Option<String>> = if is_storm(code) {
            Some(None)
        } else {
            let hours = weather["hours"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            hours.iter().skip(1).take(STORM_SOON).find(|h| is_storm(h[3].as_i64().unwrap_or(0))).map(|h| Some(h[0].as_str().unwrap_or("").to_string()))
        };
        let in_place = self.in_place();
        let (now_line, now_short) = (self.now_line(weather), self.now_line_short(weather));
        let (value, banner, banner_short, title, sub, sub_short) = match arrives? {
            None => {
                let line = weather.str_of("rain_line").map(String::from);
                let short = weather.str_of("rain_line_short").map(String::from).or(line.clone());
                ("agora".to_string(), format!("Trovoada{in_place}"), "Trovoada".to_string(), "Trovoada agora".to_string(), line.unwrap_or(now_line), short.unwrap_or(now_short))
            }
            Some(hour) => (hour.clone(), format!("Trovoada chegando{in_place}"), "Trovoada chegando".to_string(), format!("Trovoada às {hour}"), now_line, now_short),
        };
        let sky = if is_storm(code) { self.sky_weather() } else { Weather::Storm };
        Some(Alert { key: format!("trovoada|{date}"), short: "Trovoada".into(), value, banner, banner_short, title, sub, sub_short, weather: sky, icon: "thunder".into(), color: STORM_COLOR })
    }

    /// Chuva começando na próxima hora (até um minuto depois da hora prevista: "agora").
    fn rain_soon_alert(&self, weather: &Value) -> Option<Alert> {
        let (key, at) = self.rain_alert.as_ref()?;
        let minutes_left = (at - win::now()) / 60.0;
        if minutes_left <= -1.0 || minutes_left > 60.0 || is_rain(self.code()) {
            return None;
        }
        let minutes = minutes_left.max(0.0).round() as i64;
        let (value, title) = if minutes < 1 { ("agora".to_string(), "Chuva começando".to_string()) } else { (format!("{minutes} min"), format!("Chuva em {minutes} min")) };
        let banner = format!("Chuva chegando{}", self.in_place());
        Some(Alert {
            key: key.clone(),
            short: "Chuva".into(),
            value,
            banner,
            banner_short: "Chuva chegando".into(),
            title,
            sub: self.now_line(weather),
            sub_short: self.now_line_short(weather),
            weather: Weather::Rain,
            icon: "rain".into(),
            color: RAIN_BLUE,
        })
    }

    /// Amanhã bem mais frio ou mais quente que hoje (a máxima muda `TEMP_JUMP` graus ou mais).
    fn temperature_alert(&self, weather: &Value) -> Option<Alert> {
        let days = weather["day_info"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let (today, tomorrow) = (days.first()?, days.get(1)?);
        let (today_max, tomorrow_max) = (today.i64_of("max")?, tomorrow.i64_of("max")?);
        let change = tomorrow_max - today_max;
        if change.abs() < TEMP_JUMP {
            return None;
        }
        let verb = if change < 0 { "esfria" } else { "esquenta" };
        Some(Alert {
            key: format!("temperatura|{}|{change}", tomorrow.str_of("date").unwrap_or("")),
            short: "Amanhã".into(),
            // a máxima de amanhã com uma seta para onde ela vai ("Amanhã ↓ 19°"): a diferença
            // sozinha ("Amanhã −8°") parecia a temperatura de amanhã, abaixo de zero
            value: format!("{} {tomorrow_max}°", if change < 0 { "↓" } else { "↑" }),
            banner: format!("Amanhã {verb}: máxima de {tomorrow_max}°"),
            banner_short: format!("Amanhã {verb}"),
            title: format!("Amanhã {verb} {}°", change.abs()),
            sub: format!("Máxima de {tomorrow_max}° (hoje, {today_max}°)"),
            sub_short: format!("Máxima de {tomorrow_max}°"),
            weather: self.sky_weather(),
            icon: tomorrow.str_of("icon").unwrap_or("clouds").to_string(),
            color: if change < 0 { COLD } else { WARM },
        })
    }

    /// O aviso que a ilha mostra (o mais importante que ninguém dispensou).
    fn island_alert(&self) -> Option<Alert> {
        self.island_alerts().into_iter().find(|a| !self.island_ok.contains(&a.key))
    }

    /// O céu da ilha aberta no clima `weather`, guardado (refeito quando muda o minuto ou o clima).
    fn island_sky(&self, s: f32, weather: Weather) -> std::cell::Ref<'_, Option<((u32, u32, Weather, i64), Image)>> {
        let iu::FormSize { w, h, radius: r } = iu::size(IslandForm::Large);
        let clock = self.clock();
        let key = ((w * s).round() as u32, (h * s).round() as u32, weather, (clock.now * 60.0) as i64);
        if self.island_sky.borrow().as_ref().is_none_or(|(k, _)| *k != key) {
            let mut c = Canvas::new(key.0, key.1, s);
            sky::draw_back(&mut c, w, h, r, weather, &clock);
            *self.island_sky.borrow_mut() = Some((key, c.finish()));
        }
        self.island_sky.borrow()
    }

    /// O aviso na ilha, em cada forma. Aberta: o céu do aviso (com a chuva caindo), o aviso e as
    /// próximas horas.
    fn draw_island(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>) {
        let Some(alert) = self.island_alert() else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let art = |c: &mut Canvas, cx: f32, cy: f32, size: f32| {
            let px = (size * c.s).round() as u32;
            c.image(&weather_art(&alert.icon, px), cx - size / 2.0, cy - size / 2.0);
        };
        match form {
            IslandForm::Bubble => art(c, x + w / 2.0, cy, 28.0),
            IslandForm::Compact | IslandForm::Banner => {
                art(c, x + 26.0, cy, 26.0);
                let options = if form == IslandForm::Banner { [alert.banner.as_str(), alert.banner_short.as_str(), alert.short.as_str()] } else { [alert.short.as_str(); 3] };
                let label = iu::fitting(c, &options, Face::Regular, 12.0, iu::compact_room(c, x + 46.0, r, &alert.value));
                iu::compact_text(c, x + 46.0, r, label, &alert.value, alert.color);
            }
            IslandForm::Medium => {
                art(c, x + 39.0, cy, 46.0);
                let room = w - 74.0 - 86.0;
                let sub = iu::fitting(c, &[&alert.sub, &alert.sub_short], Face::Regular, 11.5, room);
                iu::two_lines(c, x + 74.0, cy, room, &alert.title, sub, alert.color);
                for (act, b) in island_buttons(form, w, h) {
                    iu::pill_button(c, (x + b.0, y + b.1, x + b.2, y + b.3), "OK", false, hot == Some(act));
                }
            }
            IslandForm::Large => self.draw_island_large(c, &alert, r, hot),
        }
    }

    /// A ilha aberta com um aviso: o céu dele (com a chuva caindo), o aviso, as próximas horas e
    /// os botões OK e Previsão.
    fn draw_island_large(&self, c: &mut Canvas, alert: &Alert, r: Rect4, hot: Option<u32>) {
        let (x, y, w, h) = r;
        let iu::FormSize { w: lw, h: lh, radius: lr } = iu::size(IslandForm::Large);
        {
            let sky = self.island_sky(c.s, alert.weather);
            if let Some((_, img)) = sky.as_ref() {
                c.image_px(img, (x * c.s).round(), (y * c.s).round(), img.w as f32, img.h as f32, 1.0);
            }
        }
        // a chuva, a neve, os raios (calculados pelo tempo, sem estado; a volta a cada hora
        // mantém a precisão)
        let secs = self.started.elapsed().as_secs_f32() % 3600.0;
        sky::draw_front(c, x, y, lw, lh, lr, alert.weather, secs, &self.island_storm);
        iu::header(c, r, "TEMPO", WHITE, &self.place_short(), MUTED);
        c.text(x + 22.0, y + 58.0, &alert.title, Face::Semibold, 20.0, FG, Anchor::LM, Some(w - 44.0));
        let sub = iu::fitting(c, &[&alert.sub, &alert.sub_short], Face::Regular, 12.0, w - 44.0);
        c.text(x + 22.0, y + 82.0, sub, Face::Regular, 12.0, MUTED, Anchor::LM, Some(w - 44.0));
        // as próximas horas (sem o nascer e o pôr do sol, que não têm temperatura)
        let hours: Vec<&Value> = self.weather.as_ref().and_then(|w| w["hours"].as_array()).map(|list| list.iter().filter(|h| h[2].is_i64()).take(6).collect()).unwrap_or_default();
        let (x0, x1) = (x + 14.0, x + w - 14.0);
        let col_w = (x1 - x0) / 6.0;
        let top = y + 102.0;
        for (i, hour) in hours.iter().enumerate() {
            let cx = x0 + col_w * (i as f32 + 0.5);
            c.text(cx, top, hour[0].as_str().unwrap_or(""), Face::Regular, 10.5, MUTED, Anchor::MA, None);
            draw_hour_column(c, cx, top, hour);
        }
        for (act, b) in island_buttons(IslandForm::Large, w, h) {
            let label = if act == ISLAND_OK { "OK" } else { "Previsão" };
            iu::pill_button(c, (x + b.0, y + b.1, x + b.2, y + b.3), label, act == ISLAND_OK, hot == Some(act));
        }
    }

    fn place_short(&self) -> String {
        self.weather.as_ref().and_then(|w| w.str_of("place")).unwrap_or("").split(',').next().unwrap_or("").to_string()
    }

    fn code(&self) -> i64 {
        self.weather.as_ref().and_then(|w| w.i64_of("code")).unwrap_or(3)
    }

    /// O clima no céu (com a ventania, que não tem código próprio).
    fn sky_weather(&self) -> Weather {
        let windy = self.weather.as_ref().and_then(|w| w.bool_of("windy")).unwrap_or(false);
        Weather::of(self.code(), windy)
    }

    /// A hora na cidade (pelo fuso dela; sem ele, a do computador) e o nascer e o pôr do sol.
    fn clock(&self) -> sky::Clock {
        let weather = self.weather.as_ref();
        let now = match (self.fixed_hour, weather.and_then(|w| w.f64_of("utc_offset"))) {
            (Some(hour), _) => hour,
            (None, Some(offset)) => ((win::now() + offset) / 3600.0).rem_euclid(24.0) as f32,
            _ => {
                let local = win::local_time();
                local.hour as f32 + local.minute as f32 / 60.0
            }
        };
        let hour_of = |key: &str| weather.and_then(|w| w["now"].str_of(key)).and_then(minute_of_day).map(|m| m as f32 / 60.0);
        sky::Clock::new(now, hour_of("sunrise").unwrap_or(6.0), hour_of("sunset").unwrap_or(18.0))
    }

    fn width(mode: Mode) -> f32 {
        Layout::of(mode).w
    }

    fn height(&self, mode: Mode) -> f32 {
        Layout::of(mode).h
    }

    fn radius(mode: Mode) -> f32 {
        Layout::of(mode).radius
    }

    fn temp_text(&self) -> String {
        self.weather.as_ref().and_then(|w| w.i64_of("temp")).map(|t| format!("{t}°")).unwrap_or_else(|| "--°".into())
    }

    fn minmax(&self) -> String {
        match self.weather.as_ref().map(|w| (w.i64_of("max"), w.i64_of("min"))) {
            Some((Some(max), Some(min))) => format!("Máx. {max}°   Mín. {min}°"),
            _ => String::new(),
        }
    }

    fn city(&self, env: &Env) -> String {
        let wcfg = env.wcfg();
        self.weather.as_ref().and_then(|w| w.str_of("place")).or(wcfg.str_of("place")).or(wcfg.str_of("city")).unwrap_or("Tempo").to_string()
    }

    // ---------- dados para a navegação ----------

    fn list(&self, key: &str) -> Vec<Value> {
        self.weather.as_ref().and_then(|w| w[key].as_array().cloned()).unwrap_or_default()
    }

    fn next_hours(&self) -> Vec<Value> {
        self.list("hours")
    }

    fn day_count(&self) -> usize {
        self.list("day_info").len()
    }

    fn day(&self, d: usize) -> Option<Value> {
        self.list("day_info").get(d).cloned()
    }

    fn day_hours(&self, d: usize) -> Vec<Value> {
        self.day(d).and_then(|v| v["hours"].as_array().cloned()).unwrap_or_default()
    }

    /// Dá para navegar? (o clima guardado por uma versão antiga não tem os detalhes)
    fn navigable(&self) -> bool {
        self.weather.as_ref().is_some_and(|w| w.get("now").is_some())
    }

    /// A vista certa para o tamanho (pequeno não tem horas nem dias).
    fn view_for(&self, mode: Mode) -> View {
        match self.view.get() {
            View::Day(_) if mode == Mode::P => View::Main,
            v => v,
        }
    }

    fn tiles_per_page(mode: Mode) -> usize {
        match mode {
            Mode::P => 1,
            Mode::M => 2,
            Mode::G => 6,
        }
    }

    fn pages(mode: Mode) -> usize {
        6usize.div_ceil(Self::tiles_per_page(mode))
    }

    fn max_scroll(len: usize) -> f32 {
        len.saturating_sub(HOURS) as f32
    }

    fn open_day(&mut self, d: usize, hour: Option<i64>) {
        if d >= self.day_count() {
            return;
        }
        self.view.set(View::Day(d));
        self.mark_view();
        let len = self.day_hours(d).len();
        // hoje começa na hora de agora; os outros dias, de manhã (ou na hora clicada)
        let now_hour = self.weather.as_ref().and_then(|w| w["now"].str_of("time")).and_then(minute_of_day).map(|m| m / 60);
        let first = match (hour, d) {
            (Some(h), _) => h - 1,
            (None, 0) => now_hour.unwrap_or(6),
            _ => 6,
        };
        self.day_scroll = Scroll::at((first.max(0) as f32).min(Self::max_scroll(len)));
    }

    fn scroll_by(&mut self, cols: f32, round: bool) {
        let (scroll, len) = match self.view.get() {
            View::Day(d) => {
                let len = self.day_hours(d).len();
                (&mut self.day_scroll, len)
            }
            _ => {
                let len = self.next_hours().len();
                (&mut self.main_scroll, len)
            }
        };
        let mut to = (scroll.to + cols).clamp(0.0, Self::max_scroll(len));
        if round {
            to = to.round();
        }
        scroll.go(to);
    }

    /// Roda do mouse na faixa: 2 horas por "clique" da roda, sempre parando numa hora inteira.
    fn wheel_hours(&mut self, steps: f32) {
        self.hour_wheel -= steps * 2.0;
        let whole = self.hour_wheel.trunc();
        if whole != 0.0 {
            self.hour_wheel -= whole;
            self.scroll_by(whole, true);
        }
    }

    fn animating(&self) -> bool {
        self.main_scroll.animating() || self.day_scroll.animating() || self.view_at.get().elapsed() < Duration::from_millis(700) || self.temp_roll.animating()
    }

    /// Começa a entrada dos painéis (abriu os detalhes, um dia ou outra página).
    fn mark_view(&self) {
        self.view_at.set(Instant::now());
        self.frame_at.set(Instant::now());
    }

    /// Opacidade do painel `i` entrando (0 a 1).
    fn tile_in(&self, i: usize) -> f32 {
        let ms = self.view_at.get().elapsed().as_secs_f32() * 1000.0 - i as f32 * 45.0;
        ease((ms / 320.0).clamp(0.0, 1.0))
    }

    // ---------- geometria dos botões ----------

    fn back_rect() -> Rect4 {
        (10.0, 10.0, 34.0, 34.0)
    }

    fn prev_rect() -> Rect4 {
        (W - PAD - 54.0, 10.0, W - PAD - 30.0, 34.0)
    }

    fn next_rect() -> Rect4 {
        (W - PAD - 24.0, 10.0, W - PAD, 34.0)
    }

    /// Setinhas nas pontas da faixa de horas.
    fn chevron_rects(y: f32) -> (Rect4, Rect4) {
        ((0.0, y, PAD + 2.0, y + STRIP_H), (W - PAD - 2.0, y, W, y + STRIP_H))
    }

    fn strip_bounds() -> (f32, f32) {
        (PAD + 2.0, W - PAD - 2.0)
    }

    /// As bolinhas das páginas dos detalhes (uma por página, centralizadas), para clicar.
    fn dot_rects(mode: Mode) -> Vec<Rect4> {
        let pages = Self::pages(mode);
        if pages < 2 {
            return Vec::new();
        }
        let w = Self::width(mode);
        let y = dots_y(mode);
        let step = 14.0;
        let x0 = w / 2.0 - step * (pages as f32 - 1.0) / 2.0;
        (0..pages).map(|i| (x0 + step * i as f32 - 7.0, y - 7.0, x0 + step * i as f32 + 7.0, y + 7.0)).collect()
    }

    /// Onde fica cada painel de detalhe na página (x, y, largura, altura).
    fn tile_slots(mode: Mode) -> Vec<Rect4> {
        match mode {
            Mode::P => vec![(10.0, 42.0, SMALL - 20.0, 88.0)],
            Mode::M => {
                let w = (W - 2.0 * PAD - 8.0) / 2.0;
                vec![(PAD, 42.0, w, 86.0), (PAD + w + 8.0, 42.0, w, 86.0)]
            }
            Mode::G => {
                let w = (W - 2.0 * PAD - 8.0) / 2.0;
                (0..6).map(|i| (PAD + (w + G_TILE_GAP) * (i % 2) as f32, G_TILES_Y + (G_TILE_H + G_TILE_GAP) * (i / 2) as f32, w, G_TILE_H)).collect()
            }
        }
    }

    // ---------- painéis ----------

    fn now_tiles(&self) -> Vec<Tile> {
        let empty = json!({});
        let now = self.weather.as_ref().map(|w| &w["now"]).unwrap_or(&empty);
        // leitores do que o Tempo guardou: número inteiro, fracionário e texto
        let int = |key: &str| now.i64_of(key);
        let float = |key: &str| now.f64_of(key).unwrap_or(0.0);
        let text = |key: &str| now.str_of(key).unwrap_or("").to_string();
        let deg = |v: Option<i64>| v.map(|v| format!("{v}°")).unwrap_or_else(|| "--".into());
        let (temp, feels) = (int("temp").unwrap_or(0), int("feels").unwrap_or(0));
        let feels_sub = if feels > temp + 1 {
            format!("Parece mais quente que os {temp}° reais")
        } else if feels < temp - 1 {
            format!("Parece mais frio que os {temp}° reais")
        } else {
            "Parecida com a temperatura real".into()
        };
        // sol: o próximo acontecimento (nascer ou pôr) e quanto do dia já passou
        let (sunrise, sunset, now_min) = (text("sunrise"), text("sunset"), minute_of_day(&text("time")));
        let (rise_m, set_m) = (minute_of_day(&sunrise), minute_of_day(&sunset));
        let frac = |m: Option<i64>| m.map(|m| m as f32 / 1440.0);
        let path = Visual::Sun(frac(rise_m).unwrap_or(0.25), frac(set_m).unwrap_or(0.75), frac(now_min));
        let tile = |title: &str, value: String, sub: String, visual: Visual| Tile { title: title.into(), value, sub, sub_short: None, visual };
        let sun = match (now_min, rise_m, set_m) {
            (Some(t), Some(r), Some(_)) if t < r => tile("NASCER DO SOL", hhmm(&sunrise), format!("Pôr do sol: {}", hhmm(&sunset)), path),
            (Some(t), Some(_), Some(st)) if t < st => tile("PÔR DO SOL", hhmm(&sunset), format!("Nascer do sol: {}", hhmm(&sunrise)), path),
            _ => tile("NASCER DO SOL", hhmm(&text("sunrise_next")), format!("Pôr do sol: {}", hhmm(&sunset)), path),
        };
        let rain_today = float("rain_today");
        let humidity = int("humidity");
        // as próximas 12 horas da faixa (sem as colunas do nascer e do pôr do sol)
        let pops: Vec<i64> = self.next_hours().iter().filter(|h| h[2].is_i64()).take(12).map(|h| h[4].as_i64().unwrap_or(0)).collect();
        vec![
            Tile { sub_short: Some(format!("Real: {temp}°")), ..tile("SENSAÇÃO", deg(int("feels")), feels_sub, Visual::Feels(temp, feels)) },
            Tile {
                sub_short: Some(format!("Orvalho: {}", deg(int("dew")))),
                ..tile(
                    "UMIDADE",
                    humidity.map(|v| format!("{v}%")).unwrap_or_else(|| "--".into()),
                    format!("O ponto de orvalho está em {} agora.", deg(int("dew"))),
                    humidity.map_or(Visual::None, Visual::Humidity),
                )
            },
            tile("VENTO", format!("{} km/h", int("wind").unwrap_or(0)), format!("Do {}", wind_from(float("wind_dir"))), Visual::Wind(float("wind_dir"), int("wind").unwrap_or(0))),
            tile("ÍNDICE UV", format!("{}", float("uv").round()), format!("Máximo de {} hoje", float("uv_max").round()), Visual::Uv(float("uv"))),
            sun,
            Tile {
                // no painel baixo, as barrinhas já mostram as próximas horas
                sub_short: (rain_today >= 0.1).then(|| format!("{}\u{a0}mm hoje", br1(rain_today))),
                ..tile(
                    "CHUVA",
                    format!("{}%", int("pop").unwrap_or(0)),
                    if rain_today >= 0.1 { format!("Nas próximas 12\u{a0}h. {}\u{a0}mm hoje.", br1(rain_today)) } else { "Nas próximas 12\u{a0}h.".into() },
                    if pops.is_empty() { Visual::None } else { Visual::Rain(pops) },
                )
            },
        ]
    }

    fn day_tiles(day: &Value) -> Vec<Tile> {
        // leitores do que o Tempo guardou: número inteiro, fracionário e texto
        let int = |key: &str| day.i64_of(key).unwrap_or(0);
        let float = |key: &str| day.f64_of(key).unwrap_or(0.0);
        let text = |key: &str| day.str_of(key).unwrap_or("").to_string();
        let tile = |title: &str, value: String, sub: String, visual: Visual| Tile { title: title.into(), value, sub, sub_short: None, visual };
        let rain = float("rain");
        let pops: Vec<i64> = day["hours"].as_array().map(|hs| hs.iter().map(|h| h[4].as_i64().unwrap_or(0)).collect()).unwrap_or_default();
        // a direção que mais soprou no dia (o clima guardado por uma versão antiga não tem)
        let wind = match day.f64_of("wind_dir") {
            Some(dir) => Visual::Wind(dir, int("wind")),
            None => Visual::None,
        };
        vec![
            tile(
                "CHUVA",
                format!("{}%", int("pop")),
                if rain >= 0.1 { format!("{} mm no dia", br1(rain)) } else { "Sem chuva prevista".into() },
                if pops.is_empty() { Visual::None } else { Visual::Rain(pops) },
            ),
            tile("VENTO", format!("{} km/h", int("wind")), "Máximo do dia".into(), wind),
            tile("ÍNDICE UV", format!("{}", float("uv").round()), "Máximo do dia".into(), Visual::Uv(float("uv"))),
            tile(
                "NASCER DO SOL",
                hhmm(&text("sunrise")),
                format!("Pôr do sol: {}", hhmm(&text("sunset"))),
                Visual::Sun(minute_of_day(&text("sunrise")).unwrap_or(360) as f32 / 1440.0, minute_of_day(&text("sunset")).unwrap_or(1080) as f32 / 1440.0, None),
            ),
        ]
    }

    // ---------- desenho ----------

    /// Fundo: o céu, com estrelas nas noites limpas.
    fn draw_sky(&self, c: &mut Canvas, mode: Mode) {
        let (w, h, r) = (Self::width(mode), self.height(mode), Self::radius(mode));
        if self.weather.is_some() {
            sky::draw_back(c, w, h, r, self.sky_weather(), &self.clock());
        } else {
            // ainda sem a previsão (abrindo pela primeira vez): o céu cinza, sem detalhes
            sky::draw_plain(c, w, h, r, Weather::Cloudy, &self.clock());
        }
    }

    /// Faixa de horas que rola (`off` em colunas), com as setinhas nas pontas quando há mais.
    fn draw_strip(&self, c: &mut Canvas, list: &[Value], y: f32, off: f32) {
        if list.is_empty() {
            return;
        }
        let (x0, x1) = Self::strip_bounds();
        let col = (x1 - x0) / HOURS as f32;
        c.push_round_clip(x0, y - 2.0, x1, y + STRIP_H, 0.0);
        let first = (off.floor() as usize).saturating_sub(1);
        for (i, h) in list.iter().enumerate().skip(first).take(HOURS + 2) {
            let cx = x0 + col * (i as f32 - off + 0.5);
            c.text(cx, y, h[0].as_str().unwrap_or(""), Face::Regular, 10.5, MUTED, Anchor::MA, None);
            draw_hour_column(c, cx, y, h);
        }
        c.pop_clip();
        let max = Self::max_scroll(list.len());
        let (left, right) = Self::chevron_rects(y);
        // nas pontas, uma aba alta e fina com a setinha (rolar as horas): bem diferente dos
        // botões redondos de trocar de dia, no título
        let cy = y + 26.0;
        let (tab_top, tab_bottom) = (y + 4.0, y + STRIP_H - 10.0);
        if off > 0.05 {
            let (x0, x1) = (left.0 + 4.0, left.2 - 4.0);
            c.fill_round_rect(x0, tab_top, x1, tab_bottom, (x1 - x0) / 2.0, WHITE.alpha(0.10));
            let x = (x0 + x1) / 2.0 + 1.0;
            c.line(&[(x + 1.8, cy - 3.5), (x - 1.8, cy), (x + 1.8, cy + 3.5)], FG.alpha(0.8), 1.4);
        }
        if off < max - 0.05 {
            let (x0, x1) = (right.0 + 4.0, right.2 - 4.0);
            c.fill_round_rect(x0, tab_top, x1, tab_bottom, (x1 - x0) / 2.0, WHITE.alpha(0.10));
            let x = (x0 + x1) / 2.0 - 1.0;
            c.line(&[(x - 1.8, cy - 3.5), (x + 1.8, cy), (x - 1.8, cy + 3.5)], FG.alpha(0.8), 1.4);
        }
    }

    // ---------- tela principal ----------

    /// Cabeçalho do médio e do grande: cidade e temperatura à esquerda; ícone, condição e
    /// máx./mín. à direita. `big`: grande (tudo um pouco maior e mais solto).
    fn draw_top(&self, c: &mut Canvas, env: &Env, big: bool) {
        let pad = if big { G_PAD } else { M_PAD };
        let (city_y, temp_y, temp_size) = if big { (18.0, 30.0, 54.0) } else { (16.0, 29.0, 40.0) };
        c.text(pad, city_y, &short_city(&self.city(env)), Face::Semibold, if big { 14.0 } else { 13.0 }, FG, Anchor::LA, Some(170.0));
        self.temp_roll.draw(c, pad - if big { 4.0 } else { 3.0 }, temp_y, &self.temp_text(), TEMP_FACE, temp_size, FG, Anchor::LA, None);
        match &self.weather {
            Some(wx) => {
                let size = if big { 26.0 } else { 22.0 };
                c.image(&weather_art(wx.str_of("icon").unwrap_or("clouds"), (size * c.s).round() as u32), W - pad - size, city_y - 2.0);
                let (dy, my) = if big { (48.0, 67.0) } else { (40.0, 57.0) };
                let max_w = self.top_desc_w(c, big);
                c.text(W - pad, dy, wx.str_of("desc").unwrap_or(""), Face::Semibold, top_desc_size(big), FG, Anchor::RA, Some(max_w));
                c.text(W - pad, my, &self.minmax(), Face::Regular, if big { 12.0 } else { 11.0 }, MUTED, Anchor::RA, None);
            }
            // buscando pela primeira vez: o esqueleto do cartão fica no lugar do texto
            None if self.first_load() => {}
            None => {
                c.text(W - pad, 40.0, &self.weather_msg, Face::Semibold, 12.0, FG, Anchor::RA, Some(170.0));
            }
        }
    }

    /// Largura livre para a condição no cabeçalho do médio e do grande: tudo o que sobra à
    /// direita da temperatura ("Predominantemente limpo" não cabia nos 150 de antes).
    fn top_desc_w(&self, c: &Canvas, big: bool) -> f32 {
        let pad = if big { G_PAD } else { M_PAD };
        let temp_size = if big { 54.0 } else { 40.0 };
        let temp_end = pad - if big { 4.0 } else { 3.0 } + c.text_width(&self.temp_text(), TEMP_FACE, temp_size);
        // (12 pt entre a temperatura e a condição)
        W - pad - temp_end - 12.0
    }

    /// Faixa de horas com colunas largas: hora, ícone e temperatura (no grande, a chance de chuva
    /// pequena e azul embaixo do ícone, só quando relevante).
    fn draw_hours(&self, c: &mut Canvas, mode: Mode) {
        let list = self.next_hours();
        if list.is_empty() {
            return;
        }
        let (big, y) = (mode == Mode::G, main_hours_y(mode));
        let off = self.main_scroll.shown();
        let (x0, x1) = main_strip_bounds(mode);
        let col = (x1 - x0) / HOURS as f32;
        let icon = if big { 24.0 } else { 22.0 };
        // no grande, a chance de chuva tem a linha dela entre o ícone e a temperatura
        let (icon_y, temp_y) = if big { (17.0, BIG_HOUR_TEMP_Y) } else { (16.0, 43.0) };
        c.push_round_clip(x0, y - 2.0, x1, y + temp_y + 18.0, 0.0);
        let first = (off.floor() as usize).saturating_sub(1);
        for (i, h) in list.iter().enumerate().skip(first).take(HOURS + 2) {
            let cx = x0 + col * (i as f32 - off + 0.5);
            let label = h[0].as_str().unwrap_or("");
            c.text(cx, y, label, if label == "Agora" { Face::Semibold } else { Face::Regular }, 10.5, if label == "Agora" { FG } else { MUTED }, Anchor::MA, None);
            c.image(&weather_art(h[1].as_str().unwrap_or("clouds"), (icon * c.s).round() as u32), cx - icon / 2.0, y + icon_y);
            let pop = h[4].as_i64().unwrap_or(0);
            if big && pop >= 20 {
                c.text(cx, y + icon_y + icon + BIG_POP_GAP, &format!("{pop}%"), Face::Semibold, 9.0, POP_TEXT, Anchor::MA, None);
            }
            match h[2].as_str() {
                Some(name) => {
                    // "Pôr do sol" diminui um pouco nas colunas mais estreitas, em vez de virar "Pôr do s…"
                    let size = event_label_size(c, name, col - 2.0);
                    c.text(cx, y + temp_y + 1.5 + (10.0 - size) / 2.0, name, Face::Semibold, size, FG, Anchor::MA, Some(col))
                }
                None => c.text(cx, y + temp_y, &format!("{}°", h[2].as_i64().unwrap_or(0)), HOUR_TEMP_FACE, if big { 13.0 } else { 12.5 }, FG, Anchor::MA, None),
            };
        }
        c.pop_clip();
    }

    /// Dias do grande: linhas altas, com o ícone (e a chance de chuva) numa coluna própria.
    fn draw_days(&self, c: &mut Canvas) {
        let days = self.weather.as_ref().and_then(|w| w["days"].as_array().cloned()).unwrap_or_default();
        let infos = self.list("day_info");
        let days: Vec<Value> = days.into_iter().take(G_DAYS).collect();
        if days.is_empty() {
            return;
        }
        let pad = G_PAD;
        let (y0, row) = self.days_geom();
        let lo = days.iter().filter_map(|d| d[2].as_i64()).min().unwrap_or(0) as f32;
        let hi = days.iter().filter_map(|d| d[3].as_i64()).max().unwrap_or(1) as f32;
        let span = (hi - lo).max(1.0);
        let (bx0, bx1) = (146.0, W - pad - 40.0);
        for (i, d) in days.iter().enumerate() {
            let cy = y0 + row * i as f32 + row / 2.0;
            if i > 0 {
                c.hline_px(pad, W - pad, y0 + row * i as f32, WHITE.alpha(0.09));
            }
            let name = capitalize(d[0].as_str().unwrap_or(""));
            c.text(pad, cy, &name, if i == 0 { Face::Semibold } else { Face::Regular }, 13.5, FG, Anchor::LM, None);
            let icon = 24.0;
            let pop = infos.get(i).and_then(|x| x.i64_of("pop")).unwrap_or(0);
            let icon_cy = if pop >= 30 { cy - 5.0 } else { cy };
            c.image(&weather_art(d[1].as_str().unwrap_or("clouds"), (icon * c.s).round() as u32), 88.0 - icon / 2.0, icon_cy - icon / 2.0);
            if pop >= 30 {
                c.text(88.0, icon_cy + icon / 2.0 + 0.5, &format!("{pop}%"), Face::Semibold, 9.0, POP_TEXT, Anchor::MA, None);
            }
            let (mn, mx) = (d[2].as_i64().unwrap_or(0) as f32, d[3].as_i64().unwrap_or(0) as f32);
            c.text(bx0 - 10.0, cy, &format!("{mn}°"), Face::Numbers, 13.0, MUTED, Anchor::RM, None);
            c.text(W - pad, cy, &format!("{mx}°"), HOUR_TEMP_FACE, 13.0, FG, Anchor::RM, None);
            c.fill_round_rect(bx0, cy - 2.5, bx1, cy + 2.5, 2.5, Rgba(0, 0, 0, 55));
            // a faixa da mínima à máxima do dia, na escala da semana (de `lo` a `lo + span`)
            let (from_x, to_x) = (bx0 + (bx1 - bx0) * (mn - lo) / span, bx0 + (bx1 - bx0) * (mx - lo) / span);
            c.gradient_h(from_x, cy - 2.5, to_x.max(from_x + 5.0), cy + 2.5, 2.5, &[(0.0, Rgba::rgb(90, 200, 250)), (0.6, Rgba::rgb(255, 214, 10)), (1.0, Rgba::rgb(255, 159, 10))], bx0, bx1);
            if i == 0 {
                // hoje: o ponto na temperatura de agora
                if let Some(temp) = self.weather.as_ref().and_then(|w| w.i64_of("temp")) {
                    let x = bx0 + (bx1 - bx0) * ((temp as f32 - lo) / span).clamp(0.0, 1.0);
                    c.fill_circle(x, cy, 5.0, Rgba(0, 0, 0, 90));
                    c.fill_circle(x, cy, 3.4, WHITE);
                }
            }
        }
    }

    /// Pequeno: a mesma ordem, com mais ar entre a temperatura e a condição.
    fn draw_compact(&self, c: &mut Canvas, env: &Env) {
        let (x, w) = (16.0, SMALL - 32.0);
        c.text(x, 16.0, &short_city(&self.city(env)), Face::Semibold, 12.5, FG, Anchor::LA, Some(w - 28.0));
        self.temp_roll.draw(c, x - 3.0, 29.0, &self.temp_text(), TEMP_FACE, 44.0, FG, Anchor::LA, None);
        match &self.weather {
            Some(wx) => {
                let size = 22.0;
                c.image(&weather_art(wx.str_of("icon").unwrap_or("clouds"), (size * c.s).round() as u32), SMALL - 16.0 - size, 14.0);
                // a condição comprida ("Predominantemente limpo") quebra em duas linhas, subindo
                let lines = c.wrap(wx.str_of("desc").unwrap_or(""), Face::Semibold, COMPACT_DESC_SIZE, w, 2);
                let top = 104.0 - COMPACT_DESC_LINE * (lines.len().max(1) - 1) as f32;
                for (k, line) in lines.iter().enumerate() {
                    c.text(x, top + COMPACT_DESC_LINE * k as f32, line, Face::Semibold, COMPACT_DESC_SIZE, FG, Anchor::LA, Some(w));
                }
                c.text(x, 121.0, &self.minmax(), Face::Regular, 11.0, MUTED, Anchor::LA, Some(w));
            }
            None if self.first_load() => {}
            None => {
                for (k, line) in c.wrap(&self.weather_msg, Face::Semibold, 12.0, w, 3).iter().enumerate() {
                    c.text(x, 92.0 + 16.0 * k as f32, line, Face::Semibold, 12.0, FG, Anchor::LA, Some(w));
                }
            }
        }
    }

    fn draw_main(&self, c: &mut Canvas, mode: Mode, env: &Env) {
        match mode {
            Mode::P => self.draw_compact(c, env),
            Mode::M => {
                self.draw_top(c, env, false);
                self.draw_hours(c, mode);
            }
            Mode::G => {
                self.draw_top(c, env, true);
                self.draw_hours(c, mode);
                c.hline_px(G_PAD, W - G_PAD, G_RULE_Y, WHITE.alpha(0.14));
                // o aviso de chuva, quando há, numa linha discreta (sem caixa)
                if let Some(text) = self.rain_line() {
                    let cy = (G_RAIN.0 + G_RAIN.1) / 2.0;
                    // o guarda-chuva na chuva; nos outros, o ícone do clima (neve, neblina, vento...)
                    match self.weather.as_ref().and_then(|w| w.str_of("line_icon")).filter(|i| *i != "rain") {
                        Some(icon) => c.image(&weather_art(icon, (15.0 * c.s).round() as u32), G_PAD - 2.0, cy - 7.5),
                        None => {
                            c.text(G_PAD, cy, "\u{e753}", Face::Icons, 11.0, RAIN_BLUE, Anchor::LM, None);
                        }
                    }
                    c.text(G_PAD + 18.0, cy, &text, Face::Regular, 11.5, FG.alpha(0.85), Anchor::LM, Some(W - 2.0 * G_PAD - 18.0));
                    c.hline_px(G_PAD, W - G_PAD, G_RAIN.1, WHITE.alpha(0.09));
                }
                self.draw_days(c);
            }
        }
    }

    /// Buscando o tempo pela primeira vez (ainda sem nada para mostrar).
    fn first_load(&self) -> bool {
        self.weather.is_none() && self.status.loading
    }

    /// O esqueleto da primeira busca: a condição e a mínima/máxima no alto e, no médio e no
    /// grande, as colunas das horas (e as linhas dos dias, no grande).
    fn skeleton_blocks(&self, mode: Mode) -> Vec<Hot> {
        if !self.first_load() {
            return Vec::new();
        }
        if mode == Mode::P {
            return vec![(16.0, 96.0, 104.0, 108.0, 4.0), (16.0, 118.0, 74.0, 128.0, 4.0)];
        }
        let big = mode == Mode::G;
        let pad = if big { G_PAD } else { M_PAD };
        let (dy, my) = if big { (48.0, 67.0) } else { (40.0, 57.0) };
        let mut v = vec![(W - pad - 104.0, dy + 1.0, W - pad, dy + 13.0, 4.0), (W - pad - 64.0, my + 1.0, W - pad, my + 11.0, 4.0)];
        let (x0, x1) = main_strip_bounds(mode);
        let y = main_hours_y(mode);
        let col = (x1 - x0) / HOURS as f32;
        let (icon_y, temp_y) = if big { (17.0, 52.0) } else { (16.0, 43.0) };
        for i in 0..HOURS {
            let cx = x0 + col * (i as f32 + 0.5);
            v.push((cx - 11.0, y + 1.0, cx + 11.0, y + 10.0, 3.0));
            v.push((cx - 10.0, y + icon_y + 1.0, cx + 10.0, y + icon_y + 21.0, 6.0));
            v.push((cx - 9.0, y + temp_y + 1.0, cx + 9.0, y + temp_y + 12.0, 3.0));
        }
        if big {
            let (top, row) = self.days_geom();
            for i in 0..G_DAYS {
                let cy = top + row * (i as f32 + 0.5);
                v.push((G_PAD, cy - 5.0, G_PAD + 60.0, cy + 5.0, 4.0));
                v.push((W - G_PAD - 150.0, cy - 2.5, W - G_PAD, cy + 2.5, 2.5));
            }
        }
        v
    }

    /// O aviso de chuva das próximas horas, quando há.
    fn rain_line(&self) -> Option<String> {
        self.weather.as_ref().and_then(|w| w.str_of("rain_line").map(String::from))
    }

    /// Grande: onde começam os dias e a altura de cada linha (abaixo do aviso de chuva, se houver).
    fn days_geom(&self) -> (f32, f32) {
        let top = if self.rain_line().is_some() { G_RAIN.1 + 2.0 } else { G_RULE_Y + 8.0 };
        (top, (LARGE_H - 14.0 - top) / G_DAYS as f32)
    }

    /// O ✕ de voltar à tela principal (diferente das setas de trocar de dia, que ficam na mesma
    /// linha; o mesmo ✕ do Sistema virado).
    fn draw_close(c: &mut Canvas, r: Rect4) {
        let (cx, cy) = ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0);
        c.fill_circle(cx, cy, 12.0, WHITE.alpha(0.16));
        let arm = 3.8;
        c.line(&[(cx - arm, cy - arm), (cx + arm, cy + arm)], FG, 1.6);
        c.line(&[(cx + arm, cy - arm), (cx - arm, cy + arm)], FG, 1.6);
    }

    /// O botão redondo de trocar de dia, com a setinha para a esquerda ou a direita (apagado
    /// quando não há dia antes ou depois).
    fn draw_button(c: &mut Canvas, r: Rect4, left: bool, enabled: bool) {
        let (cx, cy) = ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0);
        c.fill_circle(cx, cy, 12.0, WHITE.alpha(if enabled { 0.16 } else { 0.06 }));
        let col = FG.alpha(if enabled { 1.0 } else { 0.35 });
        let point = if left { 1.0 } else { -1.0 };
        c.line(&[(cx + 2.5 * point, cy - 5.0), (cx - 2.5 * point, cy), (cx + 2.5 * point, cy + 5.0)], col, 1.8);
    }

    /// Ícone pequeno de linha antes do título do painel (desenhado, sem depender de fonte).
    fn draw_tile_icon(c: &mut Canvas, title: &str, x: f32, y: f32, col: Rgba) {
        let (cx, cy) = (x + 4.5, y + 5.5);
        match title {
            "SENSAÇÃO" => {
                c.line(&[(cx, cy - 4.5), (cx, cy + 1.5)], col, 1.6);
                c.fill_circle(cx, cy + 3.0, 2.4, col);
            }
            "UMIDADE" | "CHUVA" => {
                c.polygon(&[(cx, cy - 5.0), (cx - 3.1, cy + 0.6), (cx + 3.1, cy + 0.6)], col);
                c.fill_circle(cx, cy + 1.6, 3.2, col);
            }
            "VENTO" => {
                c.line(&[(cx - 4.0, cy - 2.5), (cx + 3.0, cy - 2.5)], col, 1.4);
                c.line(&[(cx - 4.0, cy + 0.5), (cx + 4.5, cy + 0.5)], col, 1.4);
                c.line(&[(cx - 4.0, cy + 3.5), (cx + 1.5, cy + 3.5)], col, 1.4);
            }
            "ÍNDICE UV" => {
                c.fill_circle(cx, cy, 2.3, col);
                // oito raios em volta
                for ray in 0..8 {
                    let angle = (ray as f32 * 45.0).to_radians();
                    c.line(&[(cx + angle.cos() * 3.6, cy + angle.sin() * 3.6), (cx + angle.cos() * 5.0, cy + angle.sin() * 5.0)], col, 1.1);
                }
            }
            _ => {
                // nascer / pôr do sol: meio sol sobre o horizonte
                c.arc(cx, cy + 3.0, 3.2, 180.0, 360.0, col, 1.4);
                c.line(&[(cx - 5.0, cy + 3.5), (cx + 5.0, cy + 3.5)], col, 1.4);
                for deg in [-150.0f32, -90.0, -30.0] {
                    let angle = deg.to_radians();
                    c.line(&[(cx + angle.cos() * 4.8, cy + 3.0 + angle.sin() * 4.8), (cx + angle.cos() * 6.4, cy + 3.0 + angle.sin() * 6.4)], col, 1.2);
                }
            }
        }
    }

    /// Um painel de detalhe, no estilo dos módulos do Tempo da Apple: o título pequeno com o ícone,
    /// o valor grande, um desenho próprio (bússola, caminho do sol, faixa do UV ou da sensação) e
    /// uma frase curta no pé. `alpha`: opacidade da entrada (0 a 1), que também o traz um pouco
    /// de baixo.
    fn draw_tile(c: &mut Canvas, tile: &Tile, (x, y, w, h): Rect4, alpha: f32) {
        if alpha <= 0.01 {
            return;
        }
        let fade = |col: Rgba| Rgba(col.0, col.1, col.2, (col.3 as f32 * alpha) as u8);
        let area = TileArea::new(x, y + (1.0 - alpha) * 6.0, w, h);
        Self::draw_tile_frame(c, tile, &area, &fade);
        // a bússola do vento ocupa o painel alto, com a velocidade no meio e a frase no pé
        if let (Visual::Wind(deg, speed), true) = (&tile.visual, area.tall) {
            Self::draw_tile_wind_tall(c, tile, &area, *deg, *speed, &fade);
            return;
        }
        let value = Self::draw_tile_value(c, tile, &area, &fade);
        let text_top = Self::draw_tile_footer(c, tile, &area, &fade);
        Self::draw_tile_visual(c, tile, &area, &value, text_top, &fade);
    }

    /// O fundo do painel e o título com o ícone.
    fn draw_tile_frame(c: &mut Canvas, tile: &Tile, area: &TileArea, fade: &dyn Fn(Rgba) -> Rgba) {
        let (x, y, w, h) = (area.x, area.y, area.w, area.h);
        let radius = if area.tall { 16.0 } else { 14.0 };
        c.fill_round_rect(x, y, x + w, y + h, radius, fade(Rgba(0, 0, 0, 46)));
        c.stroke_round_rect(x + 0.5, y + 0.5, x + w - 0.5, y + h - 0.5, radius, fade(WHITE.alpha(0.07)), 1.0 / c.s);
        let head = fade(FG.alpha(0.62));
        Self::draw_tile_icon(c, &tile.title, area.inner_x - 1.0, y + 10.5, head);
        c.text(area.inner_x + 13.0, y + 11.0, &tile.title, Face::Semibold, 9.5, head, Anchor::LA, Some(area.inner_w - 13.0));
    }

    /// O vento no painel alto: a bússola grande, com a velocidade no meio e a frase no pé.
    fn draw_tile_wind_tall(c: &mut Canvas, tile: &Tile, area: &TileArea, deg: f64, speed: i64, fade: &dyn Fn(Rgba) -> Rgba) {
        // (do pé do título até um pouco acima da frase; no alto o círculo é estreito e passa
        // ao lado do título)
        let radius = ((area.h - 46.0) / 2.0).min(area.w / 2.0 - 14.0);
        let (cx, cy) = (area.x + area.w / 2.0, area.y + 24.0 + radius);
        Self::draw_compass(c, cx, cy, radius, deg, true, fade);
        c.text(cx, cy - 4.0, &speed.to_string(), super::VALUE, 16.0, fade(FG), Anchor::MM, None);
        c.text(cx, cy + 9.0, "km/h", Face::Regular, 9.0, fade(FG.alpha(0.7)), Anchor::MM, None);
        c.text(area.inner_x, area.y + area.h - 10.0, &tile.sub, Face::Regular, 10.5, fade(FG.alpha(0.8)), Anchor::LS, Some(area.inner_w));
    }

    /// O valor grande (a unidade, "km/h", vai menor ao lado do número) e, no UV, o nível
    /// ("Alto") logo abaixo ou ao lado. Devolve onde ele ficou, para os desenhos se encaixarem.
    fn draw_tile_value(c: &mut Canvas, tile: &Tile, area: &TileArea, fade: &dyn Fn(Rgba) -> Rgba) -> TileValue {
        let (ix, iw) = (area.inner_x, area.inner_w);
        let (size, y) = if area.tall { (30.0, area.y + 24.0) } else { (21.0, area.y + 23.0) };
        let (number, unit) = match tile.value.split_once(' ') {
            Some((number, unit)) => (number, Some(unit)),
            None => (tile.value.as_str(), None),
        };
        // no painel baixo do vento, a bússola pequena fica à direita do valor
        let room = if matches!(tile.visual, Visual::Wind(..)) { 56.0 } else { 0.0 };
        let number_w = c.text(ix - 1.0, y, number, super::HERO, size, fade(FG), Anchor::LA, Some(iw - room));
        if let Some(unit) = unit {
            let baseline = y + size * 0.98;
            let unit_size = if area.tall { 15.0 } else { 12.0 };
            c.text(ix - 1.0 + number_w + 4.0, baseline, unit, Face::Regular, unit_size, fade(FG.alpha(0.85)), Anchor::LS, Some((iw - room - number_w - 4.0).max(10.0)));
        }
        let bottom = y + size * 1.18;
        // uma palavra logo abaixo do número (o nível do UV), como no Tempo da Apple
        if let Visual::Uv(uv) = tile.visual {
            if area.tall {
                c.text(ix, bottom + 1.0, uv_level(uv), Face::Semibold, 14.0, fade(FG), Anchor::LA, None);
            } else {
                c.text(ix - 1.0 + number_w + 7.0, y + size * 0.98, uv_level(uv), Face::Semibold, 12.0, fade(FG), Anchor::LS, None);
            }
        }
        TileValue { y, size, number_w, bottom }
    }

    /// A frase no pé (até 2 linhas no alto; 1 no baixo quando há desenho, com a frase curta).
    /// Devolve onde o texto começa (o desenho fica acima dele).
    fn draw_tile_footer(c: &mut Canvas, tile: &Tile, area: &TileArea, fade: &dyn Fn(Rgba) -> Rgba) -> f32 {
        let has_art = !matches!(tile.visual, Visual::None | Visual::Wind(..));
        let max_lines = if area.tall || !has_art { 2 } else { 1 };
        // no painel baixo, a frase curta (a inteira não cabe ao lado do desenho)
        let sub = match &tile.sub_short {
            Some(short) if !area.tall => short.clone(),
            _ => tile.sub.clone(),
        };
        let lines = c.wrap(&sub, Face::Regular, 10.5, area.inner_w, max_lines);
        let line_count = lines.len() as f32;
        let last_baseline = area.y + area.h - 10.0;
        for (index, line) in lines.iter().enumerate() {
            let line_y = last_baseline - 13.0 * (line_count - 1.0 - index as f32);
            c.text(area.inner_x, line_y, line, Face::Regular, 10.5, fade(FG.alpha(0.8)), Anchor::LS, Some(area.inner_w));
        }
        last_baseline - 13.0 * line_count - 4.0
    }

    /// O desenho do painel: a faixa do UV, da umidade ou da sensação; as barrinhas da chuva; o
    /// caminho do sol; a bússola pequena do vento.
    fn draw_tile_visual(c: &mut Canvas, tile: &Tile, area: &TileArea, value: &TileValue, text_top: f32, fade: &dyn Fn(Rgba) -> Rgba) {
        let (ix, iw) = (area.inner_x, area.inner_w);
        // no painel baixo, os desenhos de lado ficam à direita do valor
        let beside_x = ix + value.number_w + 14.0;
        let beside_w = (ix + iw - beside_x).max(30.0);
        let scale_y = text_top - 8.0;
        match &tile.visual {
            &Visual::Uv(uv) => Self::draw_scale(c, ix, scale_y, iw, (uv as f32 / 11.0).clamp(0.0, 1.0), None, &UV_STOPS, fade),
            &Visual::Humidity(pct) => Self::draw_scale(c, ix, scale_y, iw, (pct as f32 / 100.0).clamp(0.0, 1.0), None, &HUMID_STOPS, fade),
            Visual::Rain(pops) => {
                if area.tall {
                    // entre o valor e a frase, na largura do painel
                    let (top, bottom) = (value.bottom + 4.0, text_top - 6.0);
                    Self::draw_rain_bars(c, ix, top, iw, (bottom - top).max(10.0), pops, fade);
                } else {
                    Self::draw_rain_bars(c, beside_x, value.y + 4.0, beside_w, value.size - 2.0, pops, fade);
                }
            }
            &Visual::Feels(real, feels) => {
                // faixa da temperatura: a real como um traço, a sensação como o ponto
                let lo = real.min(feels) as f32 - 8.0;
                let hi = real.max(feels) as f32 + 8.0;
                let pos = |v: i64| ((v as f32 - lo) / (hi - lo)).clamp(0.0, 1.0);
                Self::draw_scale(c, ix, scale_y, iw, pos(feels), Some(pos(real)), &TEMP_STOPS, fade);
            }
            &Visual::Sun(rise, set, now) => {
                if area.tall {
                    let top = value.bottom + 6.0;
                    Self::draw_sun_path(c, ix, top, iw, (text_top - 4.0 - top).max(12.0), rise, set, now, fade);
                } else {
                    Self::draw_sun_path(c, beside_x, value.y + 3.0, beside_w, value.size - 2.0, rise, set, now, fade);
                }
            }
            &Visual::Wind(deg, _) => {
                // baixo: a bússola pequena à direita do valor
                let radius = (area.h - 30.0).min(52.0) / 2.0;
                let (cx, cy) = (area.x + area.w - 12.0 - radius, area.y + 18.0 + (area.h - 18.0) / 2.0 - 4.0);
                Self::draw_compass(c, cx, cy, radius, deg, false, fade);
            }
            Visual::None => {}
        }
    }

    /// Barrinhas da chance de chuva, uma por hora, de baixo para cima; sem chance, só um tracinho
    /// apagado no pé (a linha do tempo continua lá).
    fn draw_rain_bars(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, pops: &[i64], fa: &dyn Fn(Rgba) -> Rgba) {
        if pops.is_empty() {
            return;
        }
        let step = w / pops.len() as f32;
        let bw = (step * 0.62).clamp(1.5, 7.0);
        let bottom = y + h;
        for (i, &p) in pops.iter().enumerate() {
            let cx = x + step * (i as f32 + 0.5);
            let frac = (p as f32 / 100.0).clamp(0.0, 1.0);
            let bh = (h * frac).max(2.0);
            let col = if p <= 0 { FG.alpha(0.25) } else { POP_TEXT.alpha(0.55 + 0.45 * frac) };
            c.fill_round_rect(cx - bw / 2.0, bottom - bh, cx + bw / 2.0, bottom, (bw / 2.0).min(1.5), fa(col));
        }
    }

    /// Faixa em degradê com o ponto (e, se houver, um traço de referência).
    #[allow(clippy::too_many_arguments)]
    fn draw_scale(c: &mut Canvas, x: f32, y: f32, w: f32, at: f32, mark: Option<f32>, stops: &[(f32, Rgba)], fa: &dyn Fn(Rgba) -> Rgba) {
        let stops: Vec<(f32, Rgba)> = stops.iter().map(|&(p, col)| (p, fa(col))).collect();
        c.gradient_h(x, y - 2.0, x + w, y + 2.0, 2.0, &stops, x, x + w);
        if let Some(m) = mark {
            let mx = x + w * m;
            c.fill_round_rect(mx - 1.0, y - 5.0, mx + 1.0, y + 5.0, 1.0, fa(WHITE.alpha(0.9)));
        }
        let px = x + w * at;
        c.fill_circle(px, y, 5.0, fa(Rgba(0, 0, 0, 90)));
        c.fill_circle(px, y, 3.6, fa(WHITE));
    }

    /// Bússola: marcações a cada 10°, os pontos cardeais (no alto) e a seta para onde o vento vai.
    fn draw_compass(c: &mut Canvas, cx: f32, cy: f32, r: f32, from_deg: f64, letters: bool, fa: &dyn Fn(Rgba) -> Rgba) {
        for k in 0..36 {
            let ang = (k as f32 * 10.0).to_radians();
            let (sx, sy) = (ang.sin(), -ang.cos());
            let cardinal = k % 9 == 0;
            let inner = r - if cardinal { 6.0 } else { 3.5 };
            let col = if cardinal { FG.alpha(0.85) } else { FG.alpha(0.3) };
            c.line(&[(cx + sx * inner, cy + sy * inner), (cx + sx * r, cy + sy * r)], fa(col), if cardinal { 1.4 } else { 1.0 });
        }
        if letters {
            for (k, l) in ["N", "L", "S", "O"].iter().enumerate() {
                let ang = (k as f32 * 90.0).to_radians();
                let d = r - 13.0;
                c.text(cx + ang.sin() * d, cy - ang.cos() * d, l, Face::Semibold, 8.5, fa(FG.alpha(if k == 0 { 0.95 } else { 0.6 })), Anchor::MM, None);
            }
        }
        // a seta atravessa a bússola; no meio fica a velocidade (no alto) ou nada (no baixo)
        let to = ((from_deg + 180.0) as f32).to_radians();
        let (dx, dy) = (to.sin(), -to.cos());
        let len = r - if letters { 18.0 } else { 5.0 };
        let gap = if letters { 15.0 } else { 0.0 };
        let tail = (cx - dx * len, cy - dy * len);
        let head = (cx + dx * len, cy + dy * len);
        let white = fa(FG);
        if gap > 0.0 {
            c.line(&[tail, (cx - dx * gap, cy - dy * gap)], white, 1.6);
            c.line(&[(cx + dx * gap, cy + dy * gap), head], white, 1.6);
        } else {
            c.line(&[tail, head], white, 1.6);
        }
        c.fill_circle(tail.0, tail.1, 2.4, white);
        let (lx, ly) = (-dy, dx);
        let back = (head.0 - dx * 6.0, head.1 - dy * 6.0);
        c.polygon(&[(head.0 + dx * 1.5, head.1 + dy * 1.5), (back.0 + lx * 4.0, back.1 + ly * 4.0), (back.0 - lx * 4.0, back.1 - ly * 4.0)], white);
    }

    /// O caminho do sol no dia (de meia-noite a meia-noite): uma onda que passa pelo horizonte no
    /// nascer e no pôr, mais clara acima dele, com o ponto na hora de agora.
    #[allow(clippy::too_many_arguments)]
    fn draw_sun_path(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, rise: f32, set: f32, now: Option<f32>, fa: &dyn Fn(Rgba) -> Rgba) {
        let noon = (rise + set) / 2.0;
        let tau = std::f32::consts::TAU;
        let at_horizon = (tau * (rise - noon)).cos();
        // altura (de -1 a 1) do sol na fração `u` do dia
        let elev = |u: f32| ((tau * (u - noon)).cos() - at_horizon) / (1.0 - at_horizon).max(0.2);
        let lowest = elev(noon + 0.5).min(-0.2);
        let horizon_y = y + h * (1.0 / (1.0 - lowest));
        let to_y = |e: f32| horizon_y - e * (horizon_y - y);
        let steps = 60;
        let pts: Vec<(f32, f32, bool)> = (0..=steps)
            .map(|k| {
                let u = k as f32 / steps as f32;
                let e = elev(u);
                (x + w * u, to_y(e.max(lowest)), e >= 0.0)
            })
            .collect();
        for pair in pts.windows(2) {
            let up = pair[0].2 && pair[1].2;
            c.line(&[(pair[0].0, pair[0].1), (pair[1].0, pair[1].1)], fa(if up { FG.alpha(0.85) } else { FG.alpha(0.25) }), if up { 1.8 } else { 1.4 });
        }
        c.line(&[(x, horizon_y), (x + w, horizon_y)], fa(FG.alpha(0.35)), 1.0);
        if let Some(u) = now {
            let e = elev(u);
            let (px, py) = (x + w * u, to_y(e.max(lowest)));
            if e >= 0.0 {
                c.fill_circle(px, py, 7.0, fa(Rgba(255, 255, 255, 50)));
                c.fill_circle(px, py, 4.0, fa(WHITE));
            } else {
                c.fill_circle(px, py, 3.5, fa(FG.alpha(0.55)));
            }
        }
    }

    fn draw_details(&self, c: &mut Canvas, mode: Mode, env: &Env) {
        Self::draw_close(c, Self::back_rect());
        let w = Self::width(mode);
        let city = self.city(env);
        let city = city.split(',').next().unwrap_or(&city).to_string();
        let tw = c.text(42.0, 22.0, &city, Face::Semibold, 13.0, FG, Anchor::LM, Some(w - 42.0 - 14.0));
        if mode != Mode::P {
            c.text(42.0 + tw + 8.0, 22.5, "agora", Face::Regular, 11.5, MUTED, Anchor::LM, None);
        }
        let tiles = self.now_tiles();
        let per = Self::tiles_per_page(mode);
        let page = self.page.get().min(Self::pages(mode) - 1);
        for (i, (slot, tile)) in Self::tile_slots(mode).into_iter().zip(tiles.iter().skip(page * per)).enumerate() {
            Self::draw_tile(c, tile, slot, self.tile_in(i));
        }
        for (i, (x0, y0, x1, y1)) in Self::dot_rects(mode).into_iter().enumerate() {
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            c.fill_circle(cx, cy, 3.0, if i == page { FG } else { FG.alpha(0.35) });
        }
    }

    /// Um dia: título com a data, as setas, a faixa de horas e (no grande) os painéis.
    fn draw_day(&self, c: &mut Canvas, mode: Mode, d: usize) {
        let Some(day) = self.day(d) else { return };
        Self::draw_close(c, Self::back_rect());
        Self::draw_button(c, Self::prev_rect(), true, d > 0);
        Self::draw_button(c, Self::next_rect(), false, d + 1 < self.day_count());
        let nw = c.text(42.0, 22.0, day.str_of("name").unwrap_or(""), Face::Semibold, 13.5, FG, Anchor::LM, Some(120.0));
        c.text(42.0 + nw + 7.0, 22.5, day.str_of("date").unwrap_or(""), Face::Regular, 11.5, MUTED, Anchor::LM, Some(Self::prev_rect().0 - 50.0 - nw - 8.0));
        let hours = self.day_hours(d);
        let minmax = format!("Máx. {}°  Mín. {}°", day.i64_of("max").unwrap_or(0), day.i64_of("min").unwrap_or(0));
        match mode {
            Mode::G => {
                let icon = 42.0;
                c.image(&weather_art(day.str_of("icon").unwrap_or("clouds"), (icon * c.s).round() as u32), PAD - 2.0, 48.0);
                c.text(PAD + 52.0, 52.0, day.str_of("desc").unwrap_or(""), Face::Semibold, 13.5, FG, Anchor::LA, Some(W - 2.0 * PAD - 52.0));
                c.text(PAD + 52.0, 72.0, &minmax, Face::Regular, 11.5, MUTED, Anchor::LA, None);
                c.hline_px(PAD, W - PAD, DAY_STRIP_G - 8.0, WHITE.alpha(0.12));
                self.draw_strip(c, &hours, DAY_STRIP_G, self.day_scroll.shown());
                let w = (W - 2.0 * PAD - 8.0) / 2.0;
                for (i, tile) in Self::day_tiles(&day).iter().enumerate() {
                    let slot = (PAD + (w + G_TILE_GAP) * (i % 2) as f32, DAY_TILES_G + (DAY_TILE_H + G_TILE_GAP) * (i / 2) as f32, w, DAY_TILE_H);
                    Self::draw_tile(c, tile, slot, self.tile_in(i));
                }
            }
            _ => {
                self.draw_strip(c, &hours, DAY_STRIP_M, self.day_scroll.shown());
                let y = DAY_STRIP_M + STRIP_H + 18.0;
                let icon = 18.0;
                c.image(&weather_art(day.str_of("icon").unwrap_or("clouds"), (icon * c.s).round() as u32), PAD - 1.0, y - icon / 2.0);
                let line = format!("{minmax}  ·  Chuva {}%  ·  UV {}", day.i64_of("pop").unwrap_or(0), day.f64_of("uv").unwrap_or(0.0).round());
                c.text(PAD + icon + 6.0, y, &line, Face::Regular, 11.5, FG.alpha(0.85), Anchor::LM, Some(W - 2.0 * PAD - icon - 6.0));
            }
        }
    }

    fn forecast_url(env: &Env) -> String {
        let settings = env.wcfg();
        let place = settings.str_of("place").or(settings.str_of("city")).unwrap_or("");
        format!("https://www.google.com/search?q={}", util::urlencode(&format!("previsão do tempo {place}")))
    }

    /// Chuva, garoa ou neve caindo (0: céu sem nada caindo).
    fn falling(&self) -> u32 {
        if self.weather.is_some() && self.sky_weather().moves() { RAIN_FPS } else { 0 }
    }

    /// Clique na tela principal: o tempo de agora (abre os detalhes), uma hora da faixa (abre o
    /// dia dela), o aviso de chuva ou um dos próximos dias (no grande).
    fn hit_main(&self, x: f32, y: f32, mode: Mode) -> Option<u32> {
        if mode == Mode::P {
            return Some(DETAILS);
        }
        let hours_y = main_hours_y(mode);
        if y < hours_y - 4.0 {
            return Some(DETAILS);
        }
        if y < hours_y + main_strip_h(mode) {
            let (x0, x1) = main_strip_bounds(mode);
            let column = ((x - x0) / ((x1 - x0) / HOURS as f32) + self.main_scroll.shown()).floor();
            let len = self.next_hours().len();
            return (x >= x0 && x <= x1 && column >= 0.0 && (column as usize) < len).then_some(HOUR + column as u32);
        }
        if mode == Mode::G {
            if self.rain_line().is_some() && (G_RAIN.0..G_RAIN.1).contains(&y) {
                return Some(DETAILS);
            }
            let (top, row) = self.days_geom();
            if y >= top {
                let day = ((y - top) / row) as usize;
                return (day < G_DAYS.min(self.day_count())).then_some(DAY + day as u32);
            }
        }
        None
    }

    /// Clique nos detalhes: voltar, uma bolinha (aquela página) ou o resto (a próxima página).
    fn hit_details(x: f32, y: f32, mode: Mode) -> Option<u32> {
        if inside(Self::back_rect(), x, y) {
            return Some(BACK);
        }
        if let Some(page) = Self::dot_rects(mode).into_iter().position(|r| inside(r, x, y)) {
            return Some(PAGE + page as u32);
        }
        (Self::pages(mode) > 1).then_some(NEXT_PAGE)
    }

    /// Clique num dia: voltar, o dia anterior ou o próximo, ou as setinhas da faixa de horas.
    fn hit_day(&self, x: f32, y: f32, mode: Mode, day: usize) -> Option<u32> {
        if inside(Self::back_rect(), x, y) {
            return Some(BACK);
        }
        if inside(Self::prev_rect(), x, y) {
            return (day > 0).then_some(DAY_PREV);
        }
        if inside(Self::next_rect(), x, y) {
            return (day + 1 < self.day_count()).then_some(DAY_NEXT);
        }
        let strip_y = if mode == Mode::G { DAY_STRIP_G } else { DAY_STRIP_M };
        self.strip_hit(x, y, strip_y, self.day_hours(day).len(), self.day_scroll.shown(), false)
    }

    /// Clique na faixa de horas em `y`: setinhas (rolar) ou uma hora (`hours`: abre o dia dela).
    fn strip_hit(&self, x: f32, y: f32, strip_y: f32, len: usize, off: f32, hours: bool) -> Option<u32> {
        if !(strip_y - 4.0..strip_y + STRIP_H).contains(&y) {
            return None;
        }
        let (left, right) = Self::chevron_rects(strip_y);
        if x <= left.2 {
            return (off > 0.05).then_some(SCROLL_BACK);
        }
        if x >= right.0 {
            return (off < Self::max_scroll(len) - 0.05).then_some(SCROLL_FWD);
        }
        if !hours {
            return None;
        }
        let (x0, x1) = Self::strip_bounds();
        let column = ((x - x0) / ((x1 - x0) / HOURS as f32) + off).floor() as usize;
        (column < len).then_some(HOUR + column as u32)
    }
}

/// Tela principal: onde fica a faixa de horas, a altura dela e as pontas das colunas.
/// O ícone, a chance de chuva (quando passa de 20%) e a temperatura de uma hora `hour`, na coluna
/// centrada em `cx` que começa em `top` (o nome da hora fica no alto, à parte).
fn draw_hour_column(c: &mut Canvas, cx: f32, top: f32, hour: &Value) {
    let art = weather_art(hour[1].as_str().unwrap_or("clouds"), (HOUR_ICON * c.s).round() as u32);
    c.image(&art, cx - HOUR_ICON / 2.0, top + HOUR_ICON_Y);
    let pop = hour[4].as_i64().unwrap_or(0);
    if pop >= 20 {
        c.text(cx, top + HOUR_POP_Y, &format!("{pop}%"), Face::Semibold, 9.0, POP_TEXT, Anchor::MA, None);
    }
    c.text(cx, top + HOUR_TEMP_Y, &format!("{}°", hour[2].as_i64().unwrap_or(0)), HOUR_TEMP_FACE, 12.5, FG, Anchor::MA, None);
}

fn main_hours_y(mode: Mode) -> f32 {
    if mode == Mode::G { G_HOURS_Y } else { M_HOURS_Y }
}

fn main_strip_h(mode: Mode) -> f32 {
    if mode == Mode::G { BIG_HOUR_TEMP_Y + 18.0 } else { 60.0 }
}

fn main_strip_bounds(mode: Mode) -> (f32, f32) {
    let pad = if mode == Mode::G { G_PAD } else { M_PAD };
    (pad - 2.0, W - pad + 2.0)
}

/// "São José dos Pinhais" de "São José dos Pinhais, Paraná".
fn short_city(city: &str) -> String {
    city.split(',').next().unwrap_or(city).trim().to_string()
}

/// "Ter" de "ter".
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Altura dos pontinhos das páginas.
fn dots_y(mode: Mode) -> f32 {
    if mode == Mode::P { SMALL - 13.0 } else { M_H - 11.0 }
}

fn br1(v: f64) -> String {
    format!("{v:.1}").replace('.', ",")
}

fn inside(r: Rect4, x: f32, y: f32) -> bool {
    x >= r.0 && x <= r.2 && y >= r.1 && y <= r.3
}

/// Os botões da ilha do Tempo: "OK" e, aberta, "Previsão".
fn island_buttons(form: IslandForm, w: f32, h: f32) -> Vec<(u32, Rect4)> {
    match form {
        IslandForm::Medium => vec![(ISLAND_OK, (w - 74.0, h / 2.0 - 16.0, w - 18.0, h / 2.0 + 16.0))],
        IslandForm::Large => vec![(OPEN_FORECAST, (w - 186.0, h - 46.0, w - 92.0, h - 14.0)), (ISLAND_OK, (w - 82.0, h - 46.0, w - 22.0, h - 14.0))],
        _ => Vec::new(),
    }
}

impl Kind for Tempo {
    /// Na ilha com trovoada, chuva chegando ou amanhã bem mais frio ou mais quente (um aviso).
    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        let alert = self.island_alert()?;
        Some(IslandItem::alert(alert.color, alert_hash(&alert.key)))
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, _env: &Env) {
        self.draw_island(c, form, r, hot);
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        island_buttons(form, w, h).into_iter().find(|(_, b)| iu::inside(*b, x, y)).map(|(a, _)| a)
    }

    /// Aberta, com chuva, neve ou raios no céu do aviso: os quadros da camada de cima.
    fn island_fps(&self, form: IslandForm) -> u32 {
        match (form, self.island_alert()) {
            (IslandForm::Large, Some(a)) if a.weather.moves() => RAIN_FPS,
            _ => 0,
        }
    }

    fn key(&self) -> &'static str {
        "tempo"
    }
    fn title(&self) -> &'static str {
        "Tempo"
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, env: &Env) {
        if self.mode_seen.get() != mode {
            // mudou de tamanho: volta para a tela principal
            self.mode_seen.set(mode);
            self.view.set(View::Main);
            self.page.set(0);
        }
        self.shake_on.set(shakes(env.cfg));
        self.draw_sky(c, mode);
        match self.view_for(mode) {
            View::Details if self.navigable() => return self.draw_details(c, mode, env),
            View::Day(d) if self.navigable() => return self.draw_day(c, mode, d),
            _ => {}
        }
        self.draw_main(c, mode, env);
    }

    /// O que cai ou sopra por cima do céu (garoa, chuva, neve, granizo, vento) e os raios.
    fn overlay(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        if self.falling() == 0 {
            return;
        }
        // as gotas são calculadas pelo tempo (sem estado); a volta a cada hora mantém a precisão
        let secs = self.started.elapsed().as_secs_f32() % 3600.0;
        sky::draw_front(c, ox, oy, Self::width(mode), self.height(mode), Self::radius(mode), self.sky_weather(), secs, &self.storm);
    }

    fn overlay_fps(&self) -> u32 {
        self.falling()
    }

    fn shake(&self) -> (f32, f32) {
        if self.shake_on.get() && self.falling() > 0 && self.sky_weather().lightning() {
            self.storm.shake(Instant::now())
        } else {
            (0.0, 0.0)
        }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        if env.wcfg().str_of("city").is_none_or(str::is_empty) {
            return Some(ASK_CITY);
        }
        if self.weather.is_none() {
            return self.status.error.as_ref().map(|_| UPDATE_WEATHER);
        }
        if !self.navigable() {
            return env.shortcuts().then_some(OPEN_FORECAST);
        }
        match self.view_for(mode) {
            View::Main => self.hit_main(x, y, mode),
            View::Details => Self::hit_details(x, y, mode),
            View::Day(day) => self.hit_day(x, y, mode, day),
        }
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        let mode = self.mode_seen.get();
        match action {
            OPEN_FORECAST => {
                let url = Self::forecast_url(&ctx.env());
                ctx.open(url);
            }
            UPDATE_WEATHER => self.update_weather(ctx),
            ISLAND_OK => {
                if let Some(a) = self.island_alert() {
                    self.island_ok.push(a.key);
                }
            }
            ASK_CITY => ctx.out.push(super::Request::Ask("city")),
            BACK => self.view.set(View::Main),
            DETAILS => {
                self.view.set(View::Details);
                self.page.set(0);
                self.mark_view();
            }
            NEXT_PAGE => {
                self.page.set((self.page.get() + 1) % Self::pages(mode));
                self.mark_view();
            }
            a if (PAGE..PAGE + 10).contains(&a) => {
                self.page.set(((a - PAGE) as usize).min(Self::pages(mode) - 1));
                self.mark_view();
            }
            a if (DAY..DAY + 10).contains(&a) => self.open_day((a - DAY) as usize, None),
            a if (HOUR..HOUR + NEXT_HOURS as u32).contains(&a) => {
                if let Some(h) = self.next_hours().get((a - HOUR) as usize) {
                    let (d, hour) = (h[5].as_u64().unwrap_or(0) as usize, h[6].as_i64());
                    self.open_day(d, hour);
                }
            }
            DAY_PREV | DAY_NEXT => {
                if let View::Day(d) = self.view.get() {
                    let to = if action == DAY_PREV { d.saturating_sub(1) } else { d + 1 };
                    self.open_day(to, None);
                }
            }
            SCROLL_BACK => self.scroll_by(-(HOURS as f32 - 1.0), true),
            SCROLL_FWD => self.scroll_by(HOURS as f32 - 1.0, true),
            _ => {}
        }
    }

    fn uses_wheel(&self, _x: f32, y: f32, mode: Mode) -> bool {
        if !self.navigable() || mode == Mode::P {
            return false;
        }
        match self.view_for(mode) {
            // na tela principal, só em cima da faixa das horas
            View::Main => {
                let top = main_hours_y(mode);
                (top - 4.0..=top + main_strip_h(mode) + 4.0).contains(&y)
            }
            View::Day(_) => true,
            View::Details => Self::pages(mode) > 1,
        }
    }

    fn wheel(&mut self, _x: f32, y: f32, steps: f32, mode: Mode, _ctx: &mut Ctx) {
        if !self.navigable() {
            return;
        }
        match self.view_for(mode) {
            // na tela principal, a roda rola as horas (no grande, só em cima da faixa)
            View::Main if mode == Mode::M || (mode == Mode::G && y < G_RULE_Y) => self.wheel_hours(steps),
            View::Day(_) => self.wheel_hours(steps),
            View::Details if Self::pages(mode) > 1 => {
                self.page_wheel -= steps;
                if self.page_wheel.abs() >= 1.0 {
                    let n = Self::pages(mode);
                    let p = self.page.get();
                    let next = if self.page_wheel > 0.0 { (p + 1).min(n - 1) } else { p.saturating_sub(1) };
                    if next != p {
                        self.mark_view();
                    }
                    self.page.set(next);
                    self.page_wheel = 0.0;
                }
            }
            _ => {}
        }
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<Hot> {
        let hot = |(x0, y0, x1, y1): Rect4, r: f32| Some((x0, y0, x1, y1, r));
        let round = |r: Rect4| {
            let (cx, cy) = ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0);
            Some((cx - 14.0, cy - 14.0, cx + 14.0, cy + 14.0, 14.0))
        };
        if !self.navigable() {
            return None;
        }
        let action = self.hit(x, y, mode, env)?;
        match self.view_for(mode) {
            View::Main if mode != Mode::P => match action {
                DETAILS if y < main_hours_y(mode) => hot((8.0, 6.0, W - 8.0, main_hours_y(mode) - 4.0), 14.0),
                DETAILS => hot((G_PAD - 6.0, G_RAIN.0 + 1.0, W - G_PAD + 6.0, G_RAIN.1 - 1.0), 8.0),
                a if (DAY..DAY + 10).contains(&a) => {
                    let (top, row) = self.days_geom();
                    let y0 = top + row * (a - DAY) as f32;
                    hot((G_PAD - 8.0, y0 + 2.0, W - G_PAD + 8.0, y0 + row - 2.0), 10.0)
                }
                a if (HOUR..HOUR + NEXT_HOURS as u32).contains(&a) => {
                    let (x0, x1) = main_strip_bounds(mode);
                    let col = (x1 - x0) / HOURS as f32;
                    let i = (a - HOUR) as f32;
                    let cx = x0 + col * (i - self.main_scroll.shown() + 0.5);
                    let hy = main_hours_y(mode);
                    hot((cx - col / 2.0 + 2.0, hy - 4.0, cx + col / 2.0 - 2.0, hy + main_strip_h(mode) - 2.0), 10.0)
                }
                _ => None,
            },
            View::Details => match action {
                BACK => round(Self::back_rect()),
                _ => None,
            },
            View::Day(_) => match action {
                BACK => round(Self::back_rect()),
                DAY_PREV => round(Self::prev_rect()),
                DAY_NEXT => round(Self::next_rect()),
                SCROLL_BACK | SCROLL_FWD => {
                    let (l, r) = Self::chevron_rects(if mode == Mode::G { DAY_STRIP_G } else { DAY_STRIP_M });
                    hot(if action == SCROLL_BACK { l } else { r }, 6.0)
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn hover_style(&self, x: f32, y: f32, mode: Mode, env: &Env) -> super::HoverStyle {
        if self.hit(x, y, mode, env) == Some(DETAILS) && y < main_hours_y(mode) { super::HoverStyle::Quiet } else { super::HoverStyle::Surface }
    }

    fn tooltip(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<String> {
        None
    }

    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        vec![
            MenuItem::Cmd("Atualizar".into(), UPDATE_WEATHER),
            MenuItem::Cmd("Mudar cidade…".into(), ASK_CITY),
            MenuItem::Cmd("Ver a previsão no navegador".into(), OPEN_FORECAST),
        ]
    }

    fn next_tick(&self) -> Option<Instant> {
        let sky_at = self.sky_at.get();
        let mut next = Some(self.next_weather.map_or(sky_at, |n| n.min(sky_at)));
        if self.pending.is_some() {
            next = Some(next.map_or(self.poll_at, |n| n.min(self.poll_at)));
        }
        if self.animating() || self.was_animating.get() {
            let frame = self.frame_at.get();
            next = Some(next.map_or(frame, |n| n.min(frame)));
        }
        next
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        let now = Instant::now();
        // anima a cada 16 ms e desenha mais uma vez quando acaba (o quadro final)
        let animating = self.animating();
        let mut changed = animating || self.was_animating.get();
        self.was_animating.set(animating);
        if animating {
            self.frame_at.set(now + Duration::from_millis(16));
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    self.receive(result, ctx);
                    changed = true;
                }
                // a thread morreu sem responder: trata como "sem conexão" (não fica carregando para sempre)
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.receive(Fetch::Offline, ctx);
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.poll_at = now + Duration::from_millis(200),
            }
        }
        if self.next_weather.is_some_and(|t| now >= t) {
            self.update_weather(ctx);
            changed = true;
        }
        // as cores do céu andam com a hora
        if now >= self.sky_at.get() {
            self.sky_at.set(now + SKY_EVERY);
            changed = true;
        }
        changed
    }

    fn status(&self) -> Status {
        self.status.clone()
    }

    fn skeleton(&self, mode: Mode) -> Vec<Hot> {
        self.skeleton_blocks(mode)
    }

    fn retry(&mut self, ctx: &mut Ctx) {
        self.update_weather(ctx);
    }

    fn input(&mut self, field: &str, value: &str, extra: Option<&Value>, ctx: &mut Ctx) {
        if field == "city" {
            self.set_city(value, extra, ctx);
        }
    }

    fn start(&mut self, ctx: &mut Ctx) {
        self.update_weather(ctx);
    }
}

/// Previsão de exemplo (Florianópolis, céu limpo, chuva no meio da semana) no formato da
/// Open-Meteo, para gerar as imagens do README sem depender do tempo de hoje.
#[cfg(test)]
pub fn sample_data() -> Value {
    let dates: Vec<String> = (1..=7).map(|d| format!("2026-11-{d:02}")).collect();
    let mut time = Vec::new();
    let (mut temp, mut code, mut is_day, mut pop) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (d, date) in dates.iter().enumerate() {
        for h in 0..24 {
            time.push(json!(format!("{date}T{h:02}:00")));
            let warm = [0.0, 1.0, 2.0, 0.0, -2.0, -1.0, 1.0][d];
            let t = 20.0 + warm + 5.0 * ((h as f64 - 8.0) / 24.0 * std::f64::consts::TAU).sin();
            temp.push(json!(t));
            let (c, p) = match (d, h) {
                (3, 15..=21) => (61, 70),
                (3, _) | (4, 0..=9) => (3, 30),
                (0, 17..=19) => (2, 10),
                (1, 13..=16) => (80, 40),
                (_, 11..=16) => (1, 0),
                _ => (0, 0),
            };
            code.push(json!(c));
            pop.push(json!(p));
            is_day.push(json!(if (6..18).contains(&h) { 1 } else { 0 }));
        }
    }
    json!({
        "current": {"time": "2026-11-01T14:15", "temperature_2m": 24.2, "apparent_temperature": 25.4, "relative_humidity_2m": 61,
            "dew_point_2m": 16.1, "weather_code": 0, "wind_speed_10m": 12.0, "wind_direction_10m": 120.0, "uv_index": 6.2, "is_day": 1, "precipitation": 0.0},
        "hourly": {"time": time, "temperature_2m": temp, "weather_code": code, "is_day": is_day, "precipitation_probability": pop},
        "daily": {
            "time": dates,
            "weather_code": [0, 80, 1, 61, 3, 1, 0],
            "temperature_2m_max": [27.0, 28.0, 29.0, 24.0, 22.0, 25.0, 27.0],
            "temperature_2m_min": [18.0, 19.0, 20.0, 18.0, 16.0, 17.0, 18.0],
            "sunrise": ["2026-11-01T05:21", "2026-11-02T05:20", "2026-11-03T05:19", "2026-11-04T05:19", "2026-11-05T05:18", "2026-11-06T05:17", "2026-11-07T05:16"],
            "sunset": ["2026-11-01T18:52", "2026-11-02T18:53", "2026-11-03T18:54", "2026-11-04T18:54", "2026-11-05T18:55", "2026-11-06T18:56", "2026-11-07T18:57"],
            "precipitation_probability_max": [10, 40, 5, 80, 35, 10, 0],
            "precipitation_sum": [0.0, 2.4, 0.0, 12.6, 0.8, 0.0, 0.0],
            "wind_speed_10m_max": [18.0, 22.0, 15.0, 31.0, 26.0, 14.0, 12.0],
            "wind_direction_10m_dominant": [120.0, 90.0, 60.0, 200.0, 180.0, 135.0, 100.0],
            "uv_index_max": [8.4, 6.1, 9.0, 2.3, 3.4, 7.8, 8.9]
        }
    })
}

#[cfg(test)]
impl Tempo {
    /// Exemplo numa vista: "inicio", "detalhes" ou "dia1" (dia 1 = amanhã), com a página dos
    /// detalhes e a faixa de horas rolada `scroll` horas.
    pub fn sample(view: &str, page: usize, scroll: f32, mode: Mode) -> Tempo {
        let place = "Florianópolis, Santa Catarina";
        let cfg = json!({"tempo": {"city": place, "place": place}});
        let cache = json!({"weather": process(&sample_data(), place)});
        let mut t = Tempo::new(&cfg, &cache);
        t.mode_seen.set(mode);
        match view {
            "detalhes" => t.view.set(View::Details),
            v if v.starts_with("dia") => t.open_day(v[3..].parse().unwrap_or(0), None),
            _ => {}
        }
        t.page.set(page);
        t.main_scroll = Scroll::at(scroll);
        // o céu na hora da previsão de exemplo (14:15), sempre igual
        t.fixed_hour = Some(14.25);
        // a entrada dos painéis já terminou
        t.view_at.set(Instant::now() - Duration::from_secs(5));
        t
    }

    /// A temperatura acabou de mudar (de um grau a menos): os dígitos parados a `at` (0 a 1) do
    /// caminho.
    #[cfg(test)]
    pub fn sample_temp_rolling(at: f32, mode: Mode) -> Tempo {
        let mut t = Tempo::sample("inicio", 0, 0.0, mode);
        let (before, now) = t.temp_change();
        t.temp_roll = Odometer::frozen(&before, &now, at);
        t
    }

    /// A temperatura de agora e a de um grau a menos (para os exemplos rolando).
    #[cfg(test)]
    fn temp_change(&self) -> (String, String) {
        let temp = self.weather.as_ref().and_then(|w| w.i64_of("temp")).unwrap_or(0);
        (format!("{}°", temp - 1), self.temp_text())
    }

    /// Buscando o tempo pela primeira vez (o esqueleto no lugar da condição e das horas).
    #[cfg(test)]
    pub fn sample_loading() -> Tempo {
        let cfg = json!({"tempo": {"city": "Florianópolis"}, "app": {}});
        let mut t = Tempo::new(&cfg, &json!({}));
        t.weather = None;
        t.status.loading = true;
        t
    }

    /// Chovendo agora, com a chuva caindo há `secs` segundos (a capa do README).
    pub fn sample_raining(secs: f32, mode: Mode) -> Tempo {
        let mut data = sample_data();
        data["current"]["weather_code"] = json!(61);
        data["current"]["temperature_2m"] = json!(19.4);
        data["current"]["precipitation"] = json!(0.6);
        let mut t = Tempo::sample("inicio", 0, 0.0, mode);
        t.weather = Some(process(&data, "Florianópolis, Santa Catarina"));
        t.started = Instant::now() - Duration::from_secs_f32(secs);
        t
    }

    /// Com um aviso na ilha: "chuva" (começa em 30 min), "trovoada" (às 16h) ou "frio" (amanhã
    /// 8° mais frio).
    pub fn sample_island(alert: &str) -> Tempo {
        let mut data = sample_data();
        match alert {
            "chuva" => data["minutely_15"] = json!({"time": ["2026-11-01T14:15", "2026-11-01T14:30", "2026-11-01T14:45"], "precipitation": [0.0, 0.0, 0.8]}),
            "trovoada" => data["hourly"]["weather_code"][16] = json!(95),
            "frio" => data["daily"]["temperature_2m_max"][1] = json!(19.0),
            _ => {}
        }
        let mut t = Tempo::sample("inicio", 0, 0.0, Mode::M);
        t.weather = Some(process(&data, "Florianópolis, Santa Catarina"));
        t.refresh_rain(win::now());
        t
    }

    /// Num clima (código da Open-Meteo) e numa hora quaisquer; `windy`: ventando forte. Na
    /// trovoada, com um raio caindo agora.
    pub fn sample_sky(code: i64, hour: f32, windy: bool, mode: Mode) -> Tempo {
        let mut data = sample_data();
        data["current"]["weather_code"] = json!(code);
        if windy {
            data["current"]["wind_speed_10m"] = json!(52.0);
            data["current"]["wind_gusts_10m"] = json!(71.0);
        }
        let mut t = Tempo::sample("inicio", 0, 0.0, mode);
        t.weather = Some(process(&data, "Florianópolis, Santa Catarina"));
        t.fixed_hour = Some(hour);
        t.started = Instant::now() - Duration::from_secs(3);
        if t.sky_weather().lightning() {
            t.storm.strike(Self::width(mode), t.height(mode), Instant::now());
        }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada aviso do Tempo em cada forma da ilha, num PNG por aviso (para conferir a olho):
    /// cargo test --release tempo_island_png -- --ignored --nocapture
    #[test]
    #[ignore]
    fn tempo_island_png() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let s = 1.5;
        let forms = [IslandForm::Bubble, IslandForm::Compact, IslandForm::Banner, IslandForm::Medium, IslandForm::Large];
        let samples = [("chuva", Tempo::sample_island("chuva")), ("trovoada", Tempo::sample_island("trovoada")), ("frio", Tempo::sample_island("frio")), ("trovoada_agora", Tempo::sample_sky(95, 15.0, false, Mode::M)), ("chuva_provavel", {
            let mut t = Tempo::sample_island("chuva");
            for hour in t.weather.as_mut().unwrap()["hours"].as_array_mut().unwrap() {
                hour[4] = json!(80);
            }
            t
        })];
        let cfg = json!({"app": {}});
        for (name, tempo) in samples {
            let env = Env { cfg: &cfg, key: "tempo" };
            let total_h: f32 = forms.iter().map(|f| iu::size(*f).h + 14.0).sum::<f32>() + 14.0;
            let mut c = Canvas::new((400.0 * s) as u32, (total_h * s) as u32, s);
            c.fill_round_rect(0.0, 0.0, 400.0, total_h, 0.0, Rgba::rgb(70, 80, 100));
            let mut y = 14.0;
            for form in forms {
                let iu::FormSize { w, h, radius: r } = iu::size(form);
                c.fill_round_rect(14.0, y, 14.0 + w, y + h, r, Rgba::rgb(0, 0, 0));
                c.push_round_clip(14.0, y, 14.0 + w, y + h, r);
                tempo.island_draw(&mut c, form, (14.0, y, w, h), None, &env);
                c.pop_clip();
                y += h + 14.0;
            }
            let path = std::env::temp_dir().join(format!("ilha_tempo_{name}.png"));
            crate::gfx::save_png(&c.finish(), &path).unwrap();
            println!("{}", path.display());
        }
    }

    /// Os avisos da ilha: a chuva chegando conta os minutos, a trovoada diz a hora, a mudança
    /// de temperatura diz quanto; o dia comum não avisa nada; o "OK" some com o aviso.
    #[test]
    fn island_alerts_say_what_is_coming() {
        assert!(Tempo::sample("inicio", 0, 0.0, Mode::M).island_alert().is_none(), "dia comum: nada na ilha");
        let rain = Tempo::sample_island("chuva").island_alert().expect("chuva");
        assert_eq!((rain.title.as_str(), rain.value.as_str(), rain.weather), ("Chuva em 30 min", "30 min", Weather::Rain));
        let storm = Tempo::sample_island("trovoada").island_alert().expect("trovoada");
        assert_eq!((storm.title.as_str(), storm.value.as_str()), ("Trovoada às 16h", "16h"));
        let cold = Tempo::sample_island("frio").island_alert().expect("frio");
        // o valor curto é a máxima de amanhã com a seta (a diferença sozinha, "−8°", parecia
        // a temperatura de amanhã abaixo de zero)
        assert_eq!((cold.title.as_str(), cold.value.as_str()), ("Amanhã esfria 8°", "↓ 19°"));
        assert_eq!(cold.color, COLD);
        // trovoada agora: na frente de tudo, com o céu dela
        let now = Tempo::sample_sky(95, 15.0, false, Mode::M).island_alert().expect("trovoada agora");
        assert_eq!((now.title.as_str(), now.weather), ("Trovoada agora", Weather::Storm));
        // chovendo agora: não avisa que vai chover
        assert!(Tempo::sample_raining(0.0, Mode::M).island_alert().is_none());
        // dispensado: some (e o próximo aviso, se houver, aparece)
        let mut t = Tempo::sample_island("chuva");
        t.island_ok.push(rain.key.clone());
        assert!(t.island_alert().is_none());
    }

    /// A previsão nova mexendo uns minutos na mesma chuva não é um aviso novo (a ilha não estica
    /// de novo); outra chuva, bem depois, é.
    #[test]
    fn the_same_rain_keeps_its_alert() {
        let mut t = Tempo::sample_island("chuva");
        let (key, at) = t.rain_alert.clone().expect("chuva");
        t.refresh_rain(win::now() + 10.0 * 60.0);
        let (key2, at2) = t.rain_alert.clone().expect("ainda chuva");
        assert_eq!(key, key2);
        assert!((at2 - at - 600.0).abs() < 5.0);
        t.refresh_rain(win::now() + 3.0 * 3600.0);
        assert_ne!(t.rain_alert.expect("outra chuva").0, key);
    }

    /// A ilha aberta só pede quadros com algo caindo no céu do aviso.
    #[test]
    fn island_animates_only_open_with_rain() {
        let rain = Tempo::sample_island("chuva");
        assert_eq!(rain.island_fps(IslandForm::Large), RAIN_FPS);
        assert_eq!(rain.island_fps(IslandForm::Medium), 0);
        assert_eq!(rain.island_fps(IslandForm::Compact), 0);
        assert_eq!(Tempo::sample_island("frio").island_fps(IslandForm::Large), 0, "céu limpo: parado");
    }

    /// Os minutos até a chuva, pela previsão de 15 em 15 minutos.
    #[test]
    fn rain_in_reads_the_next_hour() {
        let mut data = sample_data();
        assert_eq!(rain_in(&data), None);
        data["minutely_15"] = json!({"time": ["2026-11-01T14:30", "2026-11-01T14:45"], "precipitation": [0.1, 0.5]});
        assert_eq!(rain_in(&data), Some(30.0));
        data["current"]["weather_code"] = json!(61);
        assert_eq!(rain_in(&data), None, "já chovendo");
    }

    /// As frases dos painéis não deixam uma palavra sozinha na segunda linha (como o "12 h." da
    /// chuva), e a chance de chuva sobre o céu não usa o ciano, que some no azul.
    #[test]
    fn tile_phrases_break_well_and_rain_chance_reads() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let probe = Canvas::new(1, 1, 1.0);
        let t = Tempo::sample("detalhes", 0, 0.0, Mode::G);
        let iw = (W - 2.0 * PAD - 8.0) / 2.0 - 24.0; // painel do grande, sem as margens de dentro
        let tiles = t.now_tiles();
        assert!(tiles.iter().any(|t| t.title == "CHUVA"));
        for tile in tiles {
            let lines = probe.wrap(&tile.sub, Face::Regular, 10.5, iw, 2);
            if lines.len() == 2 {
                assert!(lines[1].split_whitespace().count() >= 2, "{}: \"{}\" sozinho na segunda linha", tile.title, lines[1]);
            }
        }
        assert_ne!(POP_TEXT, RAIN_BLUE);
        assert!(POP_TEXT.0 > 190 && POP_TEXT.1 > 220, "a chance de chuva bem clara sobre o céu");
    }

    /// Todo painel de detalhe tem um desenho (a umidade e a chuva tinham só o número, com o meio
    /// vazio), a chuva traz as horas certas e, no painel baixo, a frase cabe numa linha ao lado
    /// do desenho, sem "...".
    #[test]
    fn every_tile_has_a_drawing_and_short_phrases_fit() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let probe = Canvas::new(1, 1, 1.0);
        let t = Tempo::sample("detalhes", 0, 0.0, Mode::G);
        let now = t.now_tiles();
        let day = Tempo::day_tiles(&t.day(3).unwrap());
        for tile in now.iter().chain(day.iter()) {
            assert!(!matches!(tile.visual, Visual::None), "{} sem desenho", tile.title);
        }
        // agora: as próximas 12 horas, sem as colunas do nascer e do pôr do sol
        let Some(Visual::Rain(pops)) = now.iter().find(|t| t.title == "CHUVA").map(|t| &t.visual) else { panic!("chuva sem barras") };
        assert_eq!(pops.len(), 12);
        // um dia: as 24 horas dele, com a chuva da tarde (70%) no sample
        let Some(Visual::Rain(pops)) = day.iter().find(|t| t.title == "CHUVA").map(|t| &t.visual) else { panic!("chuva do dia sem barras") };
        assert_eq!(pops.len(), 24);
        assert_eq!(pops.iter().max(), Some(&70));
        assert!(matches!(day.iter().find(|t| t.title == "VENTO").unwrap().visual, Visual::Wind(d, 31) if d == 200.0));
        // painel baixo (o mais estreito é o do pequeno)
        let (_, _, w, _) = Tempo::tile_slots(Mode::P)[0];
        let iw = w - 24.0;
        for tile in &now {
            if matches!(tile.visual, Visual::Wind(..)) {
                continue;
            }
            let sub = tile.sub_short.as_deref().unwrap_or(&tile.sub);
            assert!(probe.text_width(sub, Face::Regular, 10.5) <= iw, "{}: \"{sub}\" não cabe no painel baixo", tile.title);
        }
        // o clima guardado por uma versão antiga (sem a direção do vento do dia): sem bússola
        let mut old = t.day(3).unwrap();
        old.as_object_mut().unwrap().remove("wind_dir");
        assert!(matches!(Tempo::day_tiles(&old)[1].visual, Visual::None));
    }

    /// A temperatura que mudou rola como num hodômetro e o widget pede quadros até ela parar.
    #[test]
    fn temperature_rolls_when_it_changes() {
        let rolled = |ms: f32| {
            let mut t = Tempo::sample("inicio", 0, 0.0, Mode::P);
            let (before, now) = t.temp_change();
            t.temp_roll = Odometer::rolled(&before, &now, ms);
            t
        };
        assert!(rolled(100.0).animating(), "rolando, sem quadros");
        assert!(!rolled(2000.0).animating(), "terminou e continua pedindo quadros");
        assert!(!Tempo::sample("inicio", 0, 0.0, Mode::P).animating());
    }

    /// Desenha todas as vistas com a previsão de verdade (precisa de internet):
    /// cargo test tempo -- --ignored --nocapture
    #[test]
    #[ignore]
    fn draw_views() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let Fetch::Ok((_, _, _, place), data) = fetch_weather("São Paulo".into(), Some(-23.55), Some(-46.63), Some("São Paulo, São Paulo".into())) else {
            panic!("sem conexão");
        };
        let weather = process(&data, &place);
        assert_eq!(weather["hours"].as_array().unwrap().len(), NEXT_HOURS);
        assert_eq!(weather["day_info"].as_array().unwrap().len(), 7);
        assert_eq!(weather["day_info"][1]["hours"].as_array().unwrap().len(), 24);
        let cfg = json!({"tempo": {"city": "São Paulo", "place": place}, "app": {}});
        let cache = json!({"weather": weather, "weather_time": 0});
        let views = [
            (Mode::M, View::Main, 0, 0.0, "m"),
            (Mode::M, View::Main, 0, 8.0, "m_rolado"),
            (Mode::G, View::Main, 0, 0.0, "g"),
            (Mode::P, View::Details, 0, 0.0, "p_detalhes"),
            (Mode::P, View::Details, 3, 0.0, "p_detalhes_uv"),
            (Mode::M, View::Details, 0, 0.0, "m_detalhes"),
            (Mode::M, View::Details, 2, 0.0, "m_detalhes_3"),
            (Mode::G, View::Details, 0, 0.0, "g_detalhes"),
            (Mode::M, View::Day(1), 0, 0.0, "m_dia"),
            (Mode::G, View::Day(0), 0, 0.0, "g_hoje"),
            (Mode::G, View::Day(2), 0, 0.0, "g_dia"),
        ];
        for (mode, view, page, scroll, name) in views {
            let mut t = Tempo::new(&cfg, &cache);
            t.mode_seen.set(mode);
            if let View::Day(d) = view {
                t.open_day(d, None);
            }
            t.view.set(view);
            t.page.set(page);
            t.main_scroll = Scroll::at(scroll);
            let l = Layout::of(mode);
            let mut c = Canvas::new((l.w * 1.25) as u32, (l.h * 1.25) as u32, 1.25);
            c.fill_round_rect(0.0, 0.0, l.w, l.h, l.radius, Rgba::rgb(40, 40, 50));
            t.draw(&mut c, mode, &Env { cfg: &cfg, key: "tempo" });
            let path = std::env::temp_dir().join(format!("tempo_{name}.png"));
            crate::gfx::save_png(&c.finish(), &path).unwrap();
            println!("{}", path.display());
        }
    }

    #[test]
    fn details_pages_draw_in_every_size() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let cfg = json!({"tempo": {}, "app": {}});
        for mode in Mode::ALL {
            for page in 0..Tempo::pages(mode) {
                for view in ["detalhes", "dia1"] {
                    let t = Tempo::sample(view, page, 0.0, mode);
                    let l = Layout::of(mode);
                    let mut c = Canvas::new((l.w * 1.25) as u32, (l.h * 1.25) as u32, 1.25);
                    t.draw(&mut c, mode, &Env { cfg: &cfg, key: "tempo" });
                    let img = c.finish();
                    assert!(img.px.chunks(4).any(|p| p[3] > 0), "{mode:?} {view} página {page}");
                }
            }
        }
        // os painéis de agora têm cada um o seu desenho
        let t = Tempo::sample("detalhes", 0, 0.0, Mode::G);
        let tiles = t.now_tiles();
        assert!(matches!(tiles[0].visual, Visual::Feels(..)));
        assert!(matches!(tiles[2].visual, Visual::Wind(..)));
        assert!(matches!(tiles[3].visual, Visual::Uv(_)));
        assert!(matches!(tiles[4].visual, Visual::Sun(..)));
    }

    #[test]
    fn entry_animation_reaches_the_end() {
        // ao abrir os detalhes, o próximo quadro já está na hora (e não foge para o futuro),
        // e os ticks seguem até o quadro final, com os painéis inteiros
        let mut t = Tempo::sample("inicio", 0, 0.0, Mode::M);
        t.next_weather = None; // sem buscar a previsão no teste
        t.view.set(View::Details);
        t.mark_view();
        let first = t.next_tick().unwrap();
        assert!(first <= Instant::now(), "o primeiro quadro tem de estar na hora");
        assert_eq!(t.next_tick(), Some(first), "o prazo não pode andar sozinho");
        let (mut cfg, mut cache, mut out) = (json!({"tempo": {}, "app": {}}), json!({}), Vec::new());
        let mut ctx = Ctx { cfg: &mut cfg, cache: &mut cache, key: "tempo", out: &mut out };
        assert!(t.tick(&mut ctx), "redesenha durante a entrada");
        assert!(t.next_tick().unwrap() > Instant::now() - Duration::from_millis(1));
        // depois que a entrada acaba: um último quadro, e então para
        t.view_at.set(Instant::now() - Duration::from_secs(2));
        assert!(t.tick(&mut ctx), "quadro final");
        assert!(!t.tick(&mut ctx));
        assert_eq!(t.tile_in(5), 1.0);
    }

    #[test]
    fn clicks_match_the_drawing() {
        let cfg = json!({"tempo": {"city": "Florianópolis"}, "app": {"atalhos_clique": true}});
        let env = Env { cfg: &cfg, key: "tempo" };
        for mode in [Mode::M, Mode::G] {
            let t = Tempo::sample("inicio", 0, 0.0, mode);
            // o topo abre os detalhes; cada coluna da faixa é a sua hora
            assert_eq!(t.hit(60.0, 40.0, mode, &env), Some(DETAILS), "{mode:?}");
            let (x0, x1) = main_strip_bounds(mode);
            let col = (x1 - x0) / HOURS as f32;
            for i in 0..HOURS {
                let x = x0 + col * (i as f32 + 0.5);
                assert_eq!(t.hit(x, main_hours_y(mode) + 30.0, mode, &env), Some(HOUR + i as u32), "{mode:?} hora {i}");
            }
        }
        // grande: cada linha desenhada é o seu dia
        let t = Tempo::sample("inicio", 0, 0.0, Mode::G);
        let (top, row) = t.days_geom();
        assert!(top + row * G_DAYS as f32 <= LARGE_H - 12.0);
        for i in 0..G_DAYS {
            assert_eq!(t.hit(160.0, top + row * (i as f32 + 0.5), Mode::G, &env), Some(DAY + i as u32), "dia {i}");
        }
    }

    /// Desenha cada clima (médio e grande, com a camada animada) em PNGs na pasta temporária:
    /// cargo test --release sky_sheet -- --ignored --nocapture
    #[test]
    #[ignore]
    fn sky_sheet() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let cfg = json!({"tempo": {}, "app": {}});
        let skies = [
            ("limpo", 0, 13.5, false),
            ("por_do_sol", 0, 18.6, false),
            ("aurora", 1, 4.9, false),
            ("noite", 0, 22.0, false),
            ("poucas", 2, 10.0, false),
            ("nublado", 3, 11.0, false),
            ("neblina", 45, 8.0, false),
            ("garoa", 53, 21.0, false),
            ("chuva", 63, 14.0, false),
            ("forte", 65, 16.0, false),
            ("trovoada", 95, 16.0, false),
            ("granizo", 99, 15.0, false),
            ("neve", 73, 10.0, false),
            ("ventania", 1, 12.0, true),
        ];
        for (name, code, hour, windy) in skies {
            for mode in [Mode::M, Mode::G] {
                let t = Tempo::sample_sky(code, hour, windy, mode);
                let l = Layout::of(mode);
                let mut c = Canvas::new((l.w * 1.5) as u32, (l.h * 1.5) as u32, 1.5);
                t.draw(&mut c, mode, &Env { cfg: &cfg, key: "tempo" });
                t.overlay(&mut c, mode, 0.0, 0.0);
                let path = std::env::temp_dir().join(format!("ceu_{name}_{}.png", mode.code()));
                crate::gfx::save_png(&c.finish(), &path).unwrap();
                println!("{}", path.display());
            }
        }
    }

    #[test]
    fn sunrise_and_sunset_join_the_hours() {
        // às 14:15, o pôr do sol (18:52) entra logo depois das 18h, e o nascer de amanhã (5:20)
        // depois das 5h; a faixa continua com 24 colunas
        let w = process(&sample_data(), "Florianópolis");
        let hours = w["hours"].as_array().unwrap();
        assert_eq!(hours.len(), NEXT_HOURS);
        let at = |label: &str| hours.iter().position(|h| h[0] == label).unwrap_or_else(|| panic!("{label}"));
        assert_eq!(at("18:52"), at("18h") + 1);
        assert_eq!(hours[at("18:52")][1], "sunset");
        assert_eq!(hours[at("18:52")][2], "Pôr do sol");
        assert_eq!(at("5:20"), at("5h") + 1);
        assert_eq!(hours[at("5:20")][1], "sunrise");
        assert_eq!(hours[at("5:20")][5], 1, "o nascer é de amanhã");
        // clicar na coluna do pôr do sol abre o dia dele, na hora dele
        let cfg = json!({"tempo": {"city": "Florianópolis"}, "app": {}});
        let mut t = Tempo::sample("inicio", 0, 0.0, Mode::M);
        let (mut c, mut cache, mut out) = (cfg.clone(), json!({}), Vec::new());
        let mut ctx = Ctx { cfg: &mut c, cache: &mut cache, key: "tempo", out: &mut out };
        t.act(HOUR + at("18:52") as u32, &mut ctx);
        assert_eq!(t.view.get(), View::Day(0));
        // a vista de um dia não muda (as colunas são as 24 horas)
        assert_eq!(t.day_hours(0).len(), 24);
    }

    #[test]
    fn the_line_follows_the_weather() {
        let hours = |codes: &[i64]| json!(codes.iter().enumerate().map(|(i, c)| json!([if i == 0 { "Agora".to_string() } else { format!("{}h", 14 + i) }, "x", 20, c, 0])).collect::<Vec<_>>());
        let line = |codes: &[i64], gusts| sky_line(&hours(codes), gusts).map(|l| (l.text, l.icon));
        let short = |codes: &[i64], gusts| sky_line(&hours(codes), gusts).map(|l| l.short);
        assert_eq!(short(&[95, 95, 63, 3], None).as_deref(), Some("Trovoada até 16h"));
        assert_eq!(short(&[73, 73], None).as_deref(), Some("Neve por horas"));
        assert_eq!(short(&[1, 1], Some(71)).as_deref(), Some("Vento de 71 km/h"));
        assert_eq!(short(&[1, 1, 61], None).as_deref(), Some("Chuva às 16h"));
        assert_eq!(line(&[95, 95, 63, 3], None), Some(("Trovoadas até 16h, depois chuva".into(), "thunder")));
        assert_eq!(line(&[63, 3], None), Some(("Chuva até 15h, depois nublado".into(), "rain")));
        assert_eq!(line(&[73, 73], None), Some(("Neve nas próximas horas".into(), "snow")));
        assert_eq!(line(&[45, 2], None), Some(("Neblina até 15h, depois parcialmente nublado".into(), "fog")));
        assert_eq!(line(&[1, 1], Some(71)), Some(("Ventos de até 71 km/h".into(), "wind")));
        assert_eq!(line(&[1, 1, 71], None), Some(("Neve a partir das 16h".into(), "snow")));
        assert_eq!(line(&[1, 1, 61], None), Some(("Chuva a partir das 16h".into(), "rain")));
        assert_eq!(line(&[0, 1, 2], None), None);
    }

    #[test]
    fn windy_and_new_icons() {
        let mut data = sample_data();
        data["current"]["wind_gusts_10m"] = json!(80.0);
        let w = process(&data, "Florianópolis");
        assert_eq!((w["desc"].as_str(), w["icon"].as_str(), w["windy"].as_bool()), (Some("Ventania"), Some("wind"), Some(true)));
        assert_eq!(w["rain_line"], "Ventos de até 80 km/h");
        // chovendo, ventar não vira ventania
        data["current"]["weather_code"] = json!(63);
        assert_eq!(process(&data, "x")["windy"], false);
        assert_eq!(weather_icon(65, true), "heavy_rain");
        assert_eq!(weather_icon(99, true), "hail");
        assert_eq!(weather_icon(95, true), "thunder");
    }

    /// Na ilha, os avisos do Tempo nunca ficam cortados com "…": a versão curta (sem a cidade, com
    /// a condição curta) sempre cabe, mesmo com a temperatura mais larga.
    #[test]
    fn island_alert_texts_are_never_cut() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let c = Canvas::new(1, 1, 1.0);
        let (descs, lines) = all_island_texts();
        let medium = iu::size(IslandForm::Medium);
        let medium_room = medium.w - 74.0 - 86.0;
        // a linha de baixo: "Agora -18° · Quase limpo", ou o aviso das próximas horas (trovoada agora)
        for (_, short) in &descs {
            let sub = format!("Agora -18° · {short}");
            assert!(c.text_width(&sub, Face::Regular, 11.5) <= medium_room, "{sub:?}");
        }
        for (_, short) in &lines {
            assert!(c.text_width(short, Face::Regular, 11.5) <= medium_room, "{short:?}");
        }
        // a pílula esticada, sem a cidade, com o valor mais largo
        let banner = iu::size(IslandForm::Banner);
        let r = (0.0, 0.0, banner.w, banner.h);
        for text in ["Trovoada chegando", "Chuva chegando", "Amanhã esfria", "Amanhã esquenta"] {
            for value in ["agora", "60 min", "−12°", "23h"] {
                assert!(c.text_width(text, Face::Regular, 12.0) <= iu::compact_room(&c, 46.0, r, value), "{text:?} com {value:?}");
            }
        }
    }

    /// Nenhuma condição do tempo fica cortada com "…": no médio e no grande cabe ao lado da
    /// temperatura (mesmo uma larga, de três algarismos com sinal); no pequeno, em até duas linhas.
    #[test]
    fn conditions_are_never_cut() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let c = Canvas::new(1, 1, 1.0);
        let mut descs: Vec<&str> = (0..=99).map(wmo).filter(|d| *d != "—").collect();
        descs.push("Ventania");
        let small_w = SMALL - 32.0;
        for temp in [24, -18] {
            let mut t = Tempo::sample("inicio", 0, 0.0, Mode::M);
            t.weather.as_mut().unwrap()["temp"] = json!(temp);
            for big in [false, true] {
                let max_w = t.top_desc_w(&c, big);
                for d in &descs {
                    let w = c.text_width(d, Face::Semibold, top_desc_size(big));
                    assert!(w <= max_w, "{d:?} com {temp}° ({w} > {max_w}, grande: {big})");
                }
            }
        }
        for d in &descs {
            let lines = c.wrap(d, Face::Semibold, COMPACT_DESC_SIZE, small_w, 2);
            assert!(lines.len() <= 2);
            for l in &lines {
                assert!(c.text_width(l, Face::Semibold, COMPACT_DESC_SIZE) <= small_w, "{d:?}: {l:?} não cabe no pequeno");
            }
        }
        // o caso que aparecia cortado: no pequeno, em duas linhas
        assert_eq!(c.wrap(wmo(1), Face::Semibold, COMPACT_DESC_SIZE, small_w, 2), ["Predominantemente", "limpo"]);
        // "Pôr do sol" e "Nascer" na faixa de horas do médio e do grande (o grande mostrava "Pôr do s…")
        for mode in [Mode::M, Mode::G] {
            let (x0, x1) = main_strip_bounds(mode);
            let col = (x1 - x0) / HOURS as f32;
            for name in ["Pôr do sol", "Nascer"] {
                let size = event_label_size(&c, name, col - 2.0);
                assert!(c.text_width(name, Face::Semibold, size) <= col - 2.0, "{name:?} não cabe na coluna ({mode:?})");
            }
        }
    }

    #[test]
    fn the_sky_redraws_every_minute_and_storms_animate() {
        let (mut cfg, mut cache, mut out) = (json!({"tempo": {}, "app": {}}), json!({}), Vec::new());
        let mut ctx = Ctx { cfg: &mut cfg, cache: &mut cache, key: "tempo", out: &mut out };
        let mut t = Tempo::sample("inicio", 0, 0.0, Mode::M);
        t.next_weather = None;
        assert!(t.next_tick().unwrap() <= Instant::now() + SKY_EVERY);
        assert!(!t.tick(&mut ctx));
        t.sky_at.set(Instant::now() - Duration::from_millis(1));
        assert!(t.tick(&mut ctx), "redesenha o céu");
        assert!(t.sky_at.get() > Instant::now());
        // céu limpo e nublado ficam parados; chuva, neve, vento e trovoada animam
        assert_eq!(Tempo::sample_sky(3, 10.0, false, Mode::M).overlay_fps(), 0);
        assert_eq!(Tempo::sample_sky(45, 10.0, false, Mode::M).overlay_fps(), 0);
        for code in [53, 63, 65, 73, 95, 99] {
            assert_eq!(Tempo::sample_sky(code, 10.0, false, Mode::M).overlay_fps(), RAIN_FPS, "{code}");
        }
        assert_eq!(Tempo::sample_sky(1, 10.0, true, Mode::M).overlay_fps(), RAIN_FPS);
        // o tremor do trovão obedece à opção
        let storm = Tempo::sample_sky(95, 19.0, false, Mode::M);
        assert!(storm.storm.flash(Instant::now()) > 0.5);
        storm.shake_on.set(false);
        assert_eq!(storm.shake(), (0.0, 0.0));
        assert!(!shakes(&json!({"tempo": {"tremer": false}})));
        assert!(shakes(&json!({"tempo": {}})));
    }

    #[test]
    fn short_names() {
        assert_eq!(short_city("São José dos Pinhais, Paraná"), "São José dos Pinhais");
        assert_eq!(short_city("Florianópolis"), "Florianópolis");
        assert_eq!(capitalize("ter"), "Ter");
        assert_eq!(capitalize("sáb"), "Sáb");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn small_helpers() {
        assert_eq!(hhmm("2026-11-01T05:07"), "5:07");
        assert_eq!(wind_from(317.0), "noroeste");
        assert_eq!(wind_from(0.0), "norte");
        assert_eq!(wind_from(359.0), "norte");
        assert_eq!(uv_level(9.45), "Muito alto");
        assert_eq!(weekday("2026-11-01"), 0, "1º de novembro de 2026 é domingo");
        assert_eq!(Tempo::pages(Mode::P), 6);
        assert_eq!(Tempo::pages(Mode::M), 3);
        assert_eq!(Tempo::pages(Mode::G), 1);
    }
}
