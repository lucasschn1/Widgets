//! Widget de Tempo ("céu vivo"): o fundo é o próprio céu (de dia, de noite, nublado, chovendo),
//! com chuva ou neve caindo de verdade, as próximas horas e os próximos dias.
//!
//! Navegável como o Tempo do Mac:
//! - a faixa de horas rola (roda do mouse ou setinhas nas pontas) pelas próximas 24 horas;
//! - clicar no tempo de agora abre os painéis de detalhes (sensação, umidade, vento, UV, sol e
//!   chuva), em páginas com pontinhos no pequeno e no médio;
//! - clicar num dia (no grande) ou numa hora abre o dia: as horas dele, chuva, vento, UV e o sol,
//!   com setas para o dia anterior e o seguinte.

use std::cell::Cell;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{Ctx, Env, Hot, Kind, Layout, MenuItem, Mode, NotifyAction, Status, LARGE_H, MEDIUM_H, SMALL, WIDE};
use crate::config::Get;
use crate::drawing::weather_art;
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::*;
use crate::util;
use crate::win;

const W: f32 = WIDE;
const PAD: f32 = 16.0;
const M_H: f32 = MEDIUM_H;
/// colunas de horas visíveis na faixa
const HOURS: usize = 6;
/// horas na faixa da tela principal (rolando)
const NEXT_HOURS: usize = 24;
const STRIP_H: f32 = 66.0;
// tela principal, com respiro (como o Tempo da Apple): margem maior, a temperatura solta das
// horas e, no grande, 5 dias em linhas altas
const M_PAD: f32 = 18.0;
const G_PAD: f32 = 20.0;
const M_HOURS_Y: f32 = 88.0;
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
const RAIN_BLUE: Rgba = Rgba::rgb(110, 200, 255);
/// faixa do índice UV: baixo, moderado, alto, muito alto e extremo
const UV_STOPS: [(f32, Rgba); 5] =
    [(0.0, Rgba::rgb(48, 209, 88)), (0.3, Rgba::rgb(255, 214, 10)), (0.55, Rgba::rgb(255, 159, 10)), (0.8, Rgba::rgb(255, 69, 58)), (1.0, Rgba::rgb(191, 90, 242))];
/// faixa de temperatura: do frio ao quente
const TEMP_STOPS: [(f32, Rgba); 4] = [(0.0, Rgba::rgb(90, 200, 250)), (0.45, Rgba::rgb(120, 220, 160)), (0.75, Rgba::rgb(255, 214, 10)), (1.0, Rgba::rgb(255, 120, 40))];

const WEATHER_EVERY: Duration = Duration::from_secs(15 * 60);
const WEATHER_RETRY: Duration = Duration::from_secs(2 * 60);
const RAIN_FPS: u32 = 30;

// ações
const OPEN_FORECAST: u32 = 3;
pub const UPDATE_WEATHER: u32 = 4;
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

/// Nome do ícone (ver weather_art) para um código de tempo da Open-Meteo.
pub fn weather_icon(code: i64, is_day: bool) -> &'static str {
    match code {
        0 | 1 => if is_day { "sun" } else { "moon" },
        2 => if is_day { "partly_day" } else { "partly_night" },
        3 => "clouds",
        45 | 48 => "fog",
        51..=57 => "drizzle",
        61..=67 | 80..=82 => "rain",
        71..=77 | 85 | 86 => "snow",
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

/// Degradê do céu (topo, base) para o tempo e a hora.
fn sky(code: i64, day: bool) -> (Rgba, Rgba) {
    let rgb = |r, g, b| Rgba::rgb(r, g, b);
    match (code, day) {
        (0 | 1, true) => (rgb(38, 117, 214), rgb(102, 168, 235)),
        (2, true) => (rgb(64, 118, 182), rgb(128, 160, 198)),
        (0..=2, false) => (rgb(10, 20, 46), rgb(34, 50, 92)),
        (3 | 45 | 48, true) => (rgb(88, 104, 126), rgb(138, 152, 170)),
        (3 | 45 | 48, false) => (rgb(26, 32, 46), rgb(52, 60, 78)),
        (c, true) if is_snow(c) => (rgb(110, 130, 160), rgb(176, 190, 210)),
        (c, false) if is_snow(c) => (rgb(36, 46, 70), rgb(70, 82, 108)),
        (c, true) if c >= 95 => (rgb(40, 42, 62), rgb(78, 80, 104)),
        (c, false) if c >= 95 => (rgb(18, 18, 30), rgb(44, 44, 64)),
        (_, true) => (rgb(46, 62, 92), rgb(98, 114, 140)),
        (_, false) => (rgb(16, 24, 42), rgb(40, 52, 76)),
    }
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
                let p = &found[0];
                (p["lat"].as_f64().unwrap_or(0.0), p["lon"].as_f64().unwrap_or(0.0), p.str_of("label").unwrap_or(&city).to_string())
            }
            Ok(_) => return Fetch::NotFound(format!("Cidade não encontrada: {}", city.split(',').next().unwrap_or("").trim())),
            Err(_) => return Fetch::Offline,
        },
    };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,dew_point_2m,weather_code,wind_speed_10m,wind_direction_10m,uv_index,is_day,precipitation\
         &hourly=temperature_2m,weather_code,is_day,precipitation_probability&minutely_15=precipitation&forecast_minutely_15=6\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,precipitation_probability_max,precipitation_sum,wind_speed_10m_max,uv_index_max\
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

/// "Chuva até 0h", "Chuva a partir das 15h" ou nada, olhando as próximas horas.
fn rain_line(hours: &Value) -> Option<String> {
    let list = hours.as_array()?;
    let code = |h: &Value| h[3].as_i64().unwrap_or(0);
    let label = |h: &Value| h[0].as_str().unwrap_or("").to_string();
    let raining = list.first().map(|h| is_rain(code(h))).unwrap_or(false);
    if raining {
        return Some(match list.iter().position(|h| !is_rain(code(h))) {
            Some(i) => format!("Chuva até {}, depois {}", label(&list[i]), wmo(code(&list[i])).to_lowercase()),
            None => "Chuva nas próximas horas".into(),
        });
    }
    list.iter().skip(1).find(|h| is_rain(code(h))).map(|h| format!("Chuva a partir das {}", label(h)))
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
    let hour_of = |i: usize| times[i].get(11..13).and_then(|h| h.parse::<i64>().ok()).unwrap_or(0);
    let entry = |i: usize, label: String| -> Value {
        let c = hourly["weather_code"][i].as_i64().unwrap_or(0);
        let day = hourly["is_day"][i].as_i64().unwrap_or(1) != 0;
        let pop = hourly["precipitation_probability"][i].as_i64().unwrap_or(0);
        let d = dates.iter().position(|date| !date.is_empty() && times[i].starts_with(date.as_str())).unwrap_or(0);
        json!([label, weather_icon(c, day), round(&hourly["temperature_2m"][i]), c, pop, d, hour_of(i)])
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
        .map(|(d, date)| {
            let day_hours: Vec<Value> = (0..times.len()).filter(|&i| times[i].starts_with(date.as_str())).map(|i| entry(i, format!("{}h", hour_of(i)))).collect();
            let name = match d {
                0 => "Hoje".to_string(),
                1 => "Amanhã".to_string(),
                _ => WEEKDAYS[weekday(date)].to_string(),
            };
            let dm = date.get(8..10).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
            let month = date.get(5..7).and_then(|s| s.parse::<usize>().ok()).unwrap_or(1).clamp(1, 12);
            let c = daily["weather_code"][d].as_i64().unwrap_or(0);
            json!({
                "name": name,
                "date": format!("{dm} de {}", MONTHS[month - 1]),
                "icon": weather_icon(c, true),
                "desc": wmo(c),
                "max": round(&daily["temperature_2m_max"][d]),
                "min": round(&daily["temperature_2m_min"][d]),
                "pop": daily["precipitation_probability_max"][d].as_i64().unwrap_or(0),
                "rain": daily["precipitation_sum"][d].as_f64().unwrap_or(0.0),
                "wind": round(&daily["wind_speed_10m_max"][d]),
                "uv": daily["uv_index_max"][d].as_f64().unwrap_or(0.0),
                "sunrise": daily["sunrise"][d].as_str().unwrap_or(""),
                "sunset": daily["sunset"][d].as_str().unwrap_or(""),
                "hours": day_hours,
            })
        })
        .collect();
    let next12 = Value::Array(hours.iter().take(12).cloned().collect());
    let now = json!({
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
    });
    json!({
        "place": place,
        "temp": round(&cur["temperature_2m"]),
        "code": code,
        "is_day": is_day,
        "icon": weather_icon(code, is_day),
        "desc": wmo(code),
        "max": round(&daily["temperature_2m_max"][0]),
        "min": round(&daily["temperature_2m_min"][0]),
        "details": format!("Sensação {}°  ·  Umidade {}%  ·  Vento {} km/h",
            round(&cur["apparent_temperature"]), round(&cur["relative_humidity_2m"]), round(&cur["wind_speed_10m"])),
        "rain_line": rain_line(&next12),
        "hours": hours,
        "days": days(data),
        "day_info": day_info,
        "now": now,
    })
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

    fn shown(&self) -> f32 {
        let t = ease(self.since.elapsed().as_secs_f32() * 1000.0 / SCROLL_MS);
        self.from + (self.to - self.from) * t
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
}

/// Um painel de detalhe: título, valor grande, frase embaixo e o desenho.
struct Tile {
    title: String,
    value: String,
    sub: String,
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
}

impl Tempo {
    pub fn new(cfg: &Value, cache: &Value) -> Tempo {
        let wcfg = &cfg["tempo"];
        let place = wcfg.str_of("place").or(wcfg.str_of("city"));
        // abre com o último clima guardado, enquanto busca o atual
        let cached = cache.get("weather").filter(|w| place.is_some() && w.str_of("place") == place && w.get("code").is_some()).cloned();
        let updated = if cached.is_some() { cache.f64_of("weather_time") } else { None };
        Tempo {
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
        }
    }

    pub fn update_weather(&mut self, ctx: &mut Ctx) {
        let w = ctx.cfg[ctx.key].clone();
        let Some(city) = w.str_of("city").filter(|c| !c.is_empty()).map(String::from) else {
            self.weather_msg = "Clique para escolher a cidade".into();
            self.next_weather = None;
            return;
        };
        if self.pending.is_some() {
            return;
        }
        let (lat, lon, place) = (w.f64_of("lat"), w.f64_of("lon"), w.str_of("place").map(String::from));
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(fetch_weather(city, lat, lon, place));
            crate::events::wake();
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
        let cur = &data["current"];
        if is_rain(cur.i64_of("weather_code").unwrap_or(0)) || cur.f64_of("precipitation").unwrap_or(0.0) > 0.0 {
            return;
        }
        let now = cur.str_of("time").and_then(local_minutes);
        let steps = &data["minutely_15"];
        let (Some(times), Some(amounts), Some(now)) = (steps["time"].as_array(), steps["precipitation"].as_array(), now) else { return };
        for (stamp, amount) in times.iter().zip(amounts) {
            let Some(t) = stamp.as_str().and_then(local_minutes) else { continue };
            let minutes = t - now;
            if minutes > 0.0 && minutes <= 60.0 && amount.as_f64().unwrap_or(0.0) >= 0.2 {
                let place = self.place_short();
                let query = format!("https://www.google.com/search?q={}", util::urlencode(&format!("previsão do tempo {place}")));
                ctx.notify(
                    "chuva",
                    "Chuva chegando",
                    format!("Deve começar a chover em cerca de {} min em {place}.", minutes.round()),
                    Some(NotifyAction::Open(query)),
                );
                return;
            }
        }
    }

    fn place_short(&self) -> String {
        self.weather.as_ref().and_then(|w| w.str_of("place")).unwrap_or("").split(',').next().unwrap_or("").to_string()
    }

    fn code(&self) -> i64 {
        self.weather.as_ref().and_then(|w| w.i64_of("code")).unwrap_or(3)
    }

    fn is_day(&self) -> bool {
        match self.weather.as_ref().and_then(|w| w.bool_of("is_day")) {
            Some(d) => d,
            None => (6..19).contains(&win::local_time().hour),
        }
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
        self.main_scroll.animating() || self.day_scroll.animating() || self.view_at.get().elapsed() < Duration::from_millis(700)
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

    fn dot_rects(mode: Mode) -> Vec<Rect4> {
        let n = Self::pages(mode);
        if n < 2 {
            return Vec::new();
        }
        let w = Self::width(mode);
        let y = dots_y(mode);
        let step = 14.0;
        let x0 = w / 2.0 - step * (n as f32 - 1.0) / 2.0;
        (0..n).map(|i| (x0 + step * i as f32 - 7.0, y - 7.0, x0 + step * i as f32 + 7.0, y + 7.0)).collect()
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
        let n = |k: &str| now.i64_of(k);
        let f = |k: &str| now.f64_of(k).unwrap_or(0.0);
        let s = |k: &str| now.str_of(k).unwrap_or("").to_string();
        let deg = |v: Option<i64>| v.map(|v| format!("{v}°")).unwrap_or_else(|| "--".into());
        let (temp, feels) = (n("temp").unwrap_or(0), n("feels").unwrap_or(0));
        let feels_sub = if feels > temp + 1 {
            format!("Parece mais quente que os {temp}° reais")
        } else if feels < temp - 1 {
            format!("Parece mais frio que os {temp}° reais")
        } else {
            "Parecida com a temperatura real".into()
        };
        // sol: o próximo acontecimento (nascer ou pôr) e quanto do dia já passou
        let (sunrise, sunset, now_min) = (s("sunrise"), s("sunset"), minute_of_day(&s("time")));
        let (rise_m, set_m) = (minute_of_day(&sunrise), minute_of_day(&sunset));
        let frac = |m: Option<i64>| m.map(|m| m as f32 / 1440.0);
        let path = Visual::Sun(frac(rise_m).unwrap_or(0.25), frac(set_m).unwrap_or(0.75), frac(now_min));
        let sun = match (now_min, rise_m, set_m) {
            (Some(t), Some(r), Some(_)) if t < r => Tile { title: "NASCER DO SOL".into(), value: hhmm(&sunrise), sub: format!("Pôr do sol: {}", hhmm(&sunset)), visual: path },
            (Some(t), Some(_), Some(st)) if t < st => Tile { title: "PÔR DO SOL".into(), value: hhmm(&sunset), sub: format!("Nascer do sol: {}", hhmm(&sunrise)), visual: path },
            _ => Tile { title: "NASCER DO SOL".into(), value: hhmm(&s("sunrise_next")), sub: format!("Pôr do sol: {}", hhmm(&sunset)), visual: path },
        };
        let rain_today = f("rain_today");
        vec![
            Tile { title: "SENSAÇÃO".into(), value: deg(n("feels")), sub: feels_sub, visual: Visual::Feels(temp, feels) },
            Tile { title: "UMIDADE".into(), value: n("humidity").map(|v| format!("{v}%")).unwrap_or_else(|| "--".into()), sub: format!("O ponto de orvalho está em {} agora.", deg(n("dew"))), visual: Visual::None },
            Tile { title: "VENTO".into(), value: format!("{} km/h", n("wind").unwrap_or(0)), sub: format!("Do {}", wind_from(f("wind_dir"))), visual: Visual::Wind(f("wind_dir"), n("wind").unwrap_or(0)) },
            Tile { title: "ÍNDICE UV".into(), value: format!("{}", f("uv").round()), sub: format!("Máximo de {} hoje", f("uv_max").round()), visual: Visual::Uv(f("uv")) },
            sun,
            Tile {
                title: "CHUVA".into(),
                value: format!("{}%", n("pop").unwrap_or(0)),
                sub: if rain_today >= 0.1 { format!("Chance nas próximas 12\u{a0}h. {}\u{a0}mm hoje.", br1(rain_today)) } else { "Chance nas próximas 12\u{a0}h.".into() },
                visual: Visual::None,
            },
        ]
    }

    fn day_tiles(day: &Value) -> Vec<Tile> {
        let n = |k: &str| day.i64_of(k).unwrap_or(0);
        let f = |k: &str| day.f64_of(k).unwrap_or(0.0);
        let s = |k: &str| day.str_of(k).unwrap_or("").to_string();
        let rain = f("rain");
        vec![
            Tile { title: "CHUVA".into(), value: format!("{}%", n("pop")), sub: if rain >= 0.1 { format!("{} mm no dia", br1(rain)) } else { "Sem chuva prevista".into() }, visual: Visual::None },
            Tile { title: "VENTO".into(), value: format!("{} km/h", n("wind")), sub: "Máximo do dia".into(), visual: Visual::None },
            Tile { title: "ÍNDICE UV".into(), value: format!("{}", f("uv").round()), sub: "Máximo do dia".into(), visual: Visual::Uv(f("uv")) },
            Tile {
                title: "NASCER DO SOL".into(),
                value: hhmm(&s("sunrise")),
                sub: format!("Pôr do sol: {}", hhmm(&s("sunset"))),
                visual: Visual::Sun(minute_of_day(&s("sunrise")).unwrap_or(360) as f32 / 1440.0, minute_of_day(&s("sunset")).unwrap_or(1080) as f32 / 1440.0, None),
            },
        ]
    }

    // ---------- desenho ----------

    /// Fundo: o céu, com estrelas nas noites limpas.
    fn draw_sky(&self, c: &mut Canvas, mode: Mode) {
        let (w, h) = (Self::width(mode), self.height(mode));
        let (top, bottom) = sky(self.code(), self.is_day());
        c.gradient_round_rect(0.0, 0.0, w, h, Self::radius(mode), &[(0.0, top.alpha(0.94)), (1.0, bottom.alpha(0.94))], 0.0, h);
        // a luz do sol (ou da lua) vindo do canto de cima, com o céu limpo
        if self.code() <= 2 {
            let (warm, amount) = if self.is_day() { (Rgba::rgb(255, 214, 130), 0.4) } else { (Rgba::rgb(200, 210, 255), 0.25) };
            c.push_round_clip(0.0, 0.0, w, h, Self::radius(mode));
            // muitos círculos bem fracos, para a luz sumir sem degraus
            let steps = 40;
            for i in 0..steps {
                let r = 160.0 - i as f32 * (150.0 / steps as f32);
                c.fill_circle(w - 26.0, -6.0, r, warm.alpha(amount / steps as f32));
            }
            c.pop_clip();
        }
        if !self.is_day() && self.code() <= 2 {
            let mut seed: u32 = 7;
            let mut rnd = || {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (seed >> 8) as f32 / (1u32 << 24) as f32
            };
            for _ in 0..(w * h.min(160.0) / 900.0) as usize {
                let (x, y, r, a) = (rnd() * w, rnd() * h.min(160.0) * 0.9, 0.4 + rnd() * 0.8, 0.25 + rnd() * 0.5);
                c.fill_circle(x, y, r, WHITE.alpha(a));
            }
        }
    }

    /// Faixa de horas que rola (`off` em colunas), com as setinhas nas pontas quando há mais.
    fn draw_strip(&self, c: &mut Canvas, list: &[Value], y: f32, off: f32) {
        if list.is_empty() {
            return;
        }
        let (x0, x1) = Self::strip_bounds();
        let col = (x1 - x0) / HOURS as f32;
        let icon = 22.0;
        c.push_round_clip(x0, y - 2.0, x1, y + STRIP_H, 0.0);
        let first = (off.floor() as usize).saturating_sub(1);
        for (i, h) in list.iter().enumerate().skip(first).take(HOURS + 2) {
            let cx = x0 + col * (i as f32 - off + 0.5);
            c.text(cx, y, h[0].as_str().unwrap_or(""), Face::Regular, 10.5, MUTED, Anchor::MA, None);
            c.image(&weather_art(h[1].as_str().unwrap_or("clouds"), (icon * c.s).round() as u32), cx - icon / 2.0, y + 15.0);
            // a chance de chuva fica junto da temperatura, menor, sem encostar no ícone
            let pop = h[4].as_i64().unwrap_or(0);
            if pop >= 20 {
                c.text(cx, y + 39.5, &format!("{pop}%"), Face::Semibold, 9.0, RAIN_BLUE, Anchor::MA, None);
            }
            c.text(cx, y + 50.0, &format!("{}°", h[2].as_i64().unwrap_or(0)), Face::Semibold, 12.5, FG, Anchor::MA, None);
        }
        c.pop_clip();
        let max = Self::max_scroll(list.len());
        let (l, r) = Self::chevron_rects(y);
        // setas pequenas e discretas, na margem (não ficam por cima das horas)
        let cy = y + 26.0;
        if off > 0.05 {
            let x = (l.0 + l.2) / 2.0 + 2.0;
            c.line(&[(x + 2.0, cy - 4.0), (x - 2.0, cy), (x + 2.0, cy + 4.0)], FG.alpha(0.55), 1.4);
        }
        if off < max - 0.05 {
            let x = (r.0 + r.2) / 2.0 - 2.0;
            c.line(&[(x - 2.0, cy - 4.0), (x + 2.0, cy), (x - 2.0, cy + 4.0)], FG.alpha(0.55), 1.4);
        }
    }

    // ---------- tela principal ----------

    /// Cabeçalho do médio e do grande: cidade e temperatura à esquerda; ícone, condição e
    /// máx./mín. à direita. `big`: grande (tudo um pouco maior e mais solto).
    fn draw_top(&self, c: &mut Canvas, env: &Env, big: bool) {
        let pad = if big { G_PAD } else { M_PAD };
        let (city_y, temp_y, temp_size) = if big { (18.0, 30.0, 54.0) } else { (16.0, 29.0, 40.0) };
        c.text(pad, city_y, &short_city(&self.city(env)), Face::Semibold, if big { 14.0 } else { 13.0 }, FG, Anchor::LA, Some(170.0));
        c.text(pad - if big { 4.0 } else { 3.0 }, temp_y, &self.temp_text(), Face::Light, temp_size, FG, Anchor::LA, None);
        match &self.weather {
            Some(wx) => {
                let size = if big { 26.0 } else { 22.0 };
                c.image(&weather_art(wx.str_of("icon").unwrap_or("clouds"), (size * c.s).round() as u32), W - pad - size, city_y - 2.0);
                let (dy, my) = if big { (48.0, 67.0) } else { (40.0, 57.0) };
                c.text(W - pad, dy, wx.str_of("desc").unwrap_or(""), Face::Semibold, if big { 13.5 } else { 12.5 }, FG, Anchor::RA, Some(150.0));
                c.text(W - pad, my, &self.minmax(), Face::Regular, if big { 12.0 } else { 11.0 }, MUTED, Anchor::RA, None);
            }
            None => {
                c.text(W - pad, 40.0, &self.weather_msg, Face::Semibold, 12.0, FG, Anchor::RA, Some(170.0));
            }
        }
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
        let (icon_y, temp_y) = if big { (17.0, 52.0) } else { (16.0, 43.0) };
        c.push_round_clip(x0, y - 2.0, x1, y + temp_y + 18.0, 0.0);
        let first = (off.floor() as usize).saturating_sub(1);
        for (i, h) in list.iter().enumerate().skip(first).take(HOURS + 2) {
            let cx = x0 + col * (i as f32 - off + 0.5);
            let label = h[0].as_str().unwrap_or("");
            c.text(cx, y, label, if label == "Agora" { Face::Semibold } else { Face::Regular }, 10.5, if label == "Agora" { FG } else { MUTED }, Anchor::MA, None);
            c.image(&weather_art(h[1].as_str().unwrap_or("clouds"), (icon * c.s).round() as u32), cx - icon / 2.0, y + icon_y);
            let pop = h[4].as_i64().unwrap_or(0);
            if big && pop >= 20 {
                c.text(cx, y + icon_y + icon + 0.5, &format!("{pop}%"), Face::Semibold, 9.0, RAIN_BLUE, Anchor::MA, None);
            }
            c.text(cx, y + temp_y, &format!("{}°", h[2].as_i64().unwrap_or(0)), Face::Semibold, if big { 13.0 } else { 12.5 }, FG, Anchor::MA, None);
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
                c.text(88.0, icon_cy + icon / 2.0 + 0.5, &format!("{pop}%"), Face::Semibold, 9.0, RAIN_BLUE, Anchor::MA, None);
            }
            let (mn, mx) = (d[2].as_i64().unwrap_or(0) as f32, d[3].as_i64().unwrap_or(0) as f32);
            c.text(bx0 - 10.0, cy, &format!("{mn}°"), Face::Regular, 13.0, MUTED, Anchor::RM, None);
            c.text(W - pad, cy, &format!("{mx}°"), Face::Semibold, 13.0, FG, Anchor::RM, None);
            c.fill_round_rect(bx0, cy - 2.5, bx1, cy + 2.5, 2.5, Rgba(0, 0, 0, 55));
            let (a, b) = (bx0 + (bx1 - bx0) * (mn - lo) / span, bx0 + (bx1 - bx0) * (mx - lo) / span);
            c.gradient_h(a, cy - 2.5, b.max(a + 5.0), cy + 2.5, 2.5, &[(0.0, Rgba::rgb(90, 200, 250)), (0.6, Rgba::rgb(255, 214, 10)), (1.0, Rgba::rgb(255, 159, 10))], bx0, bx1);
            if i == 0 {
                if let Some(t) = self.weather.as_ref().and_then(|w| w.i64_of("temp")) {
                    let x = bx0 + (bx1 - bx0) * ((t as f32 - lo) / span).clamp(0.0, 1.0);
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
        c.text(x - 3.0, 29.0, &self.temp_text(), Face::Light, 44.0, FG, Anchor::LA, None);
        match &self.weather {
            Some(wx) => {
                let size = 22.0;
                c.image(&weather_art(wx.str_of("icon").unwrap_or("clouds"), (size * c.s).round() as u32), SMALL - 16.0 - size, 14.0);
                c.text(x, 104.0, wx.str_of("desc").unwrap_or(""), Face::Semibold, 12.0, FG, Anchor::LA, Some(w));
                c.text(x, 121.0, &self.minmax(), Face::Regular, 11.0, MUTED, Anchor::LA, Some(w));
            }
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
                    c.text(G_PAD, cy, "\u{e753}", Face::Icons, 11.0, RAIN_BLUE, Anchor::LM, None);
                    c.text(G_PAD + 18.0, cy, &text, Face::Regular, 11.5, FG.alpha(0.85), Anchor::LM, Some(W - 2.0 * G_PAD - 18.0));
                    c.hline_px(G_PAD, W - G_PAD, G_RAIN.1, WHITE.alpha(0.09));
                }
                self.draw_days(c);
            }
        }
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

    fn draw_button(c: &mut Canvas, r: Rect4, left: bool, enabled: bool) {
        let (cx, cy) = ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0);
        c.fill_circle(cx, cy, 12.0, WHITE.alpha(if enabled { 0.16 } else { 0.06 }));
        let col = FG.alpha(if enabled { 1.0 } else { 0.35 });
        let d = if left { 1.0 } else { -1.0 };
        c.line(&[(cx + 2.5 * d, cy - 5.0), (cx - 2.5 * d, cy), (cx + 2.5 * d, cy + 5.0)], col, 1.8);
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
                for k in 0..8 {
                    let a = (k as f32 * 45.0).to_radians();
                    c.line(&[(cx + a.cos() * 3.6, cy + a.sin() * 3.6), (cx + a.cos() * 5.0, cy + a.sin() * 5.0)], col, 1.1);
                }
            }
            _ => {
                // nascer / pôr do sol: meio sol sobre o horizonte
                c.arc(cx, cy + 3.0, 3.2, 180.0, 360.0, col, 1.4);
                c.line(&[(cx - 5.0, cy + 3.5), (cx + 5.0, cy + 3.5)], col, 1.4);
                for deg in [-150.0f32, -90.0, -30.0] {
                    let a = deg.to_radians();
                    c.line(&[(cx + a.cos() * 4.8, cy + 3.0 + a.sin() * 4.8), (cx + a.cos() * 6.4, cy + 3.0 + a.sin() * 6.4)], col, 1.2);
                }
            }
        }
    }

    /// Um painel de detalhe, no estilo dos módulos do Tempo da Apple: o título pequeno com o ícone,
    /// o valor grande, um desenho próprio (bússola, caminho do sol, faixa do UV ou da sensação) e
    /// uma frase curta no pé. `a`: opacidade da entrada (0 a 1), que também o traz um pouco de baixo.
    fn draw_tile(c: &mut Canvas, t: &Tile, (x, y, w, h): Rect4, a: f32) {
        if a <= 0.01 {
            return;
        }
        let fa = |col: Rgba| Rgba(col.0, col.1, col.2, (col.3 as f32 * a) as u8);
        let y = y + (1.0 - a) * 6.0;
        // alto (grande) ou baixo (pequeno e médio)
        let tall = h >= 100.0;
        let r = if tall { 16.0 } else { 14.0 };
        c.fill_round_rect(x, y, x + w, y + h, r, fa(Rgba(0, 0, 0, 46)));
        c.stroke_round_rect(x + 0.5, y + 0.5, x + w - 0.5, y + h - 0.5, r, fa(WHITE.alpha(0.07)), 1.0 / c.s);
        let ix = x + 12.0;
        let iw = w - 24.0;
        let head = fa(FG.alpha(0.62));
        Self::draw_tile_icon(c, &t.title, ix - 1.0, y + 10.5, head);
        c.text(ix + 13.0, y + 11.0, &t.title, Face::Semibold, 9.5, head, Anchor::LA, Some(iw - 13.0));

        // a bússola do vento ocupa o painel alto inteiro, com a velocidade no meio
        if let (Visual::Wind(deg, speed), true) = (&t.visual, tall) {
            let (cx, cy) = (x + w / 2.0, y + 26.0 + (h - 34.0) / 2.0);
            let rad = ((h - 40.0) / 2.0).min(w / 2.0 - 14.0);
            Self::draw_compass(c, cx, cy, rad, *deg, true, &fa);
            c.text(cx, cy - 4.0, &speed.to_string(), Face::Semibold, 16.0, fa(FG), Anchor::MM, None);
            c.text(cx, cy + 10.0, "km/h", Face::Regular, 9.0, fa(FG.alpha(0.7)), Anchor::MM, None);
            return;
        }

        // o valor (a unidade, "km/h", vai menor ao lado do número)
        let (vsize, vy) = if tall { (30.0, y + 24.0) } else { (21.0, y + 23.0) };
        let (number, unit) = match t.value.split_once(' ') {
            Some((n, u)) => (n, Some(u)),
            None => (t.value.as_str(), None),
        };
        let room = if matches!(t.visual, Visual::Wind(..)) { 56.0 } else { 0.0 };
        let nw = c.text(ix - 1.0, vy, number, Face::Regular, vsize, fa(FG), Anchor::LA, Some(iw - room));
        if let Some(u) = unit {
            let base = vy + vsize * 0.98;
            c.text(ix - 1.0 + nw + 4.0, base, u, Face::Regular, if tall { 15.0 } else { 12.0 }, fa(FG.alpha(0.85)), Anchor::LS, Some((iw - room - nw - 4.0).max(10.0)));
        }
        // uma palavra logo abaixo do número (o nível do UV), como no Tempo da Apple
        let value_bottom = vy + vsize * 1.18;
        if let Visual::Uv(uv) = t.visual {
            if tall {
                c.text(ix, value_bottom + 1.0, uv_level(uv), Face::Semibold, 14.0, fa(FG), Anchor::LA, None);
            } else {
                c.text(ix - 1.0 + nw + 7.0, vy + vsize * 0.98, uv_level(uv), Face::Semibold, 12.0, fa(FG), Anchor::LS, None);
            }
        }

        // a frase no pé (até 2 linhas no alto; 1 no baixo quando há desenho)
        let has_art = !matches!(t.visual, Visual::None | Visual::Wind(..));
        let max_lines = if tall { 2 } else if has_art { 1 } else { 2 };
        // no painel baixo, a sensação diz só a temperatura real (a frase inteira não cabe)
        let sub = match t.visual {
            Visual::Feels(real, _) if !tall => format!("Real: {real}°"),
            _ => t.sub.clone(),
        };
        let lines = c.wrap(&sub, Face::Regular, 10.5, iw, max_lines);
        let n = lines.len() as f32;
        for (k, line) in lines.iter().enumerate() {
            let ly = y + h - 10.0 - 13.0 * (n - 1.0 - k as f32);
            c.text(ix, ly, line, Face::Regular, 10.5, fa(FG.alpha(0.8)), Anchor::LS, Some(iw));
        }
        let text_top = y + h - 10.0 - 13.0 * n - 4.0;

        match t.visual {
            Visual::Uv(uv) => {
                let by = text_top - 8.0;
                Self::draw_scale(c, ix, by, iw, (uv as f32 / 11.0).clamp(0.0, 1.0), None, &UV_STOPS, &fa);
            }
            Visual::Feels(real, feels) => {
                // faixa da temperatura: a real como um traço, a sensação como o ponto
                let lo = real.min(feels) as f32 - 8.0;
                let hi = real.max(feels) as f32 + 8.0;
                let pos = |v: i64| ((v as f32 - lo) / (hi - lo)).clamp(0.0, 1.0);
                let by = text_top - 8.0;
                Self::draw_scale(c, ix, by, iw, pos(feels), Some(pos(real)), &TEMP_STOPS, &fa);
            }
            Visual::Sun(rise, set, now) => {
                if tall {
                    Self::draw_sun_path(c, ix, value_bottom + 6.0, iw, (text_top - 4.0 - value_bottom - 6.0).max(12.0), rise, set, now, &fa);
                } else {
                    // baixo: a curva ao lado do horário
                    let x0 = ix + nw + 14.0;
                    Self::draw_sun_path(c, x0, vy + 3.0, (ix + iw - x0).max(30.0), vsize - 2.0, rise, set, now, &fa);
                }
            }
            Visual::Wind(deg, _) => {
                // baixo: a bússola pequena à direita do valor
                let rad = (h - 30.0).min(52.0) / 2.0;
                Self::draw_compass(c, x + w - 12.0 - rad, y + 18.0 + (h - 18.0) / 2.0 - 4.0, rad, deg, false, &fa);
            }
            Visual::None => {}
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
        Self::draw_button(c, Self::back_rect(), true, true);
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
        Self::draw_button(c, Self::back_rect(), true, true);
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
        let w = env.wcfg();
        let place = w.str_of("place").or(w.str_of("city")).unwrap_or("");
        format!("https://www.google.com/search?q={}", util::urlencode(&format!("previsão do tempo {place}")))
    }

    /// Chuva, garoa ou neve caindo (0: céu sem nada caindo).
    fn falling(&self) -> u32 {
        if self.weather.is_none() {
            return 0;
        }
        let code = self.code();
        if is_rain(code) || is_snow(code) { RAIN_FPS } else { 0 }
    }

    /// Clique na faixa de horas em `y`: setinhas (rolar) ou uma hora (`hours`: abre o dia dela).
    fn strip_hit(&self, x: f32, y: f32, strip_y: f32, len: usize, off: f32, hours: bool) -> Option<u32> {
        if !(strip_y - 4.0..strip_y + STRIP_H).contains(&y) {
            return None;
        }
        let (l, r) = Self::chevron_rects(strip_y);
        if x <= l.2 {
            return (off > 0.05).then_some(SCROLL_BACK);
        }
        if x >= r.0 {
            return (off < Self::max_scroll(len) - 0.05).then_some(SCROLL_FWD);
        }
        if !hours {
            return None;
        }
        let (x0, x1) = Self::strip_bounds();
        let i = ((x - x0) / ((x1 - x0) / HOURS as f32) + off).floor() as usize;
        (i < len).then_some(HOUR + i as u32)
    }
}

/// Tela principal: onde fica a faixa de horas, a altura dela e as pontas das colunas.
fn main_hours_y(mode: Mode) -> f32 {
    if mode == Mode::G { G_HOURS_Y } else { M_HOURS_Y }
}

fn main_strip_h(mode: Mode) -> f32 {
    if mode == Mode::G { 70.0 } else { 60.0 }
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

impl Kind for Tempo {
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
        self.draw_sky(c, mode);
        match self.view_for(mode) {
            View::Details if self.navigable() => return self.draw_details(c, mode, env),
            View::Day(d) if self.navigable() => return self.draw_day(c, mode, d),
            _ => {}
        }
        self.draw_main(c, mode, env);
    }

    /// Chuva ou neve caindo por cima do céu (as gotas são calculadas pela hora: sem estado).
    fn overlay(&self, c: &mut Canvas, mode: Mode, ox: f32, oy: f32) {
        if self.falling() == 0 {
            return;
        }
        let (w, h) = (Self::width(mode), self.height(mode));
        let code = self.code();
        let snow = is_snow(code);
        let light = (51..=55).contains(&code) || code == 61 || code == 80;
        let t = self.started.elapsed().as_secs_f32();
        let count = ((w * h / if light { 1600.0 } else { 900.0 }) as usize).max(12);
        c.push_round_clip(ox, oy, ox + w, oy + h, Self::radius(mode));
        let mut seed: u32 = 99;
        let mut rnd = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        for _ in 0..count {
            let (x0, y0, speed, len) = (rnd(), rnd(), rnd(), rnd());
            if snow {
                let v = 14.0 + speed * 16.0;
                let y = (y0 * (h + 10.0) + t * v) % (h + 10.0) - 5.0;
                let x = (x0 * w + (t * 0.8 + x0 * 9.0).sin() * 6.0).rem_euclid(w);
                c.fill_circle(ox + x, oy + y, 1.2 + len * 1.3, WHITE.alpha(0.55));
            } else {
                let v = 180.0 + speed * 120.0;
                let l = 7.0 + len * 9.0;
                let y = (y0 * (h + l) + t * v) % (h + l) - l;
                let x = (x0 * (w + 40.0) - t * v * 0.18).rem_euclid(w + 40.0) - 20.0;
                c.line(&[(ox + x, oy + y), (ox + x - l * 0.18, oy + y + l)], Rgba(205, 222, 255, if light { 60 } else { 85 }), 1.2);
            }
        }
        c.pop_clip();
    }

    fn overlay_fps(&self) -> u32 {
        self.falling()
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
            View::Main => {
                if mode == Mode::P {
                    return Some(DETAILS);
                }
                let hy = main_hours_y(mode);
                if y < hy - 4.0 {
                    return Some(DETAILS);
                }
                if y < hy + main_strip_h(mode) {
                    let (x0, x1) = main_strip_bounds(mode);
                    let i = ((x - x0) / ((x1 - x0) / HOURS as f32) + self.main_scroll.shown()).floor();
                    let len = self.next_hours().len();
                    return (x >= x0 && x <= x1 && i >= 0.0 && (i as usize) < len).then_some(HOUR + i as u32);
                }
                if mode == Mode::G {
                    if self.rain_line().is_some() && (G_RAIN.0..G_RAIN.1).contains(&y) {
                        return Some(DETAILS);
                    }
                    let (top, row) = self.days_geom();
                    if y >= top {
                        let i = ((y - top) / row) as usize;
                        return (i < G_DAYS.min(self.day_count())).then_some(DAY + i as u32);
                    }
                }
                None
            }
            View::Details => {
                if inside(Self::back_rect(), x, y) {
                    return Some(BACK);
                }
                if let Some(n) = Self::dot_rects(mode).into_iter().position(|r| inside(r, x, y)) {
                    return Some(PAGE + n as u32);
                }
                (Self::pages(mode) > 1).then_some(NEXT_PAGE)
            }
            View::Day(d) => {
                if inside(Self::back_rect(), x, y) {
                    return Some(BACK);
                }
                if inside(Self::prev_rect(), x, y) {
                    return (d > 0).then_some(DAY_PREV);
                }
                if inside(Self::next_rect(), x, y) {
                    return (d + 1 < self.day_count()).then_some(DAY_NEXT);
                }
                let strip_y = if mode == Mode::G { DAY_STRIP_G } else { DAY_STRIP_M };
                self.strip_hit(x, y, strip_y, self.day_hours(d).len(), self.day_scroll.shown(), false)
            }
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
        let mut next = self.next_weather;
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
        changed
    }

    fn status(&self) -> Status {
        self.status.clone()
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
        // a entrada dos painéis já terminou
        t.view_at.set(Instant::now() - Duration::from_secs(5));
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
