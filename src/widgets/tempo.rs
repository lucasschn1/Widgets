//! Widget de Tempo ("céu vivo"): o fundo é o próprio céu (de dia, de noite, nublado, chovendo),
//! com chuva ou neve caindo de verdade, as próximas horas e os próximos dias.

use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{Ctx, Env, Kind, Layout, MenuItem, Mode, NotifyAction, Status, SMALL, SMALL_RADIUS};
use crate::config::Get;
use crate::drawing::weather_art;
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::*;
use crate::util;
use crate::win;

const W: f32 = 320.0;
const PAD: f32 = 16.0;
const M_H: f32 = 150.0;
const HOURS_Y: f32 = 80.0;
const HOURS: usize = 6;
const DAY_H: f32 = 30.0;
const G_LINE_Y: f32 = 150.0;
const G_DAYS_Y: f32 = 186.0;
const G_DAYS: usize = 5;
const G_H: f32 = G_DAYS_Y + DAY_H * G_DAYS as f32 + 12.0;

const WEATHER_EVERY: Duration = Duration::from_secs(15 * 60);
const WEATHER_RETRY: Duration = Duration::from_secs(2 * 60);
const RAIN_FPS: u32 = 30;

// ações
const OPEN_FORECAST: u32 = 3;
pub const UPDATE_WEATHER: u32 = 4;
pub const ASK_CITY: u32 = 5;

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
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day,precipitation\
         &hourly=temperature_2m,weather_code,is_day&forecast_hours=12&minutely_15=precipitation&forecast_minutely_15=6\
         &daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=7"
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

fn round(v: &Value) -> i64 {
    v.as_f64().unwrap_or(0.0).round() as i64
}

/// Próximas horas: [rótulo, ícone, temperatura, código].
fn hours(data: &Value) -> Value {
    let hourly = &data["hourly"];
    let empty = Vec::new();
    let times = hourly["time"].as_array().unwrap_or(&empty);
    let temps = hourly["temperature_2m"].as_array().unwrap_or(&empty);
    let codes = hourly["weather_code"].as_array().unwrap_or(&empty);
    let days = hourly["is_day"].as_array().unwrap_or(&empty);
    let mut out = Vec::new();
    for i in 0..times.len().min(temps.len()).min(codes.len()) {
        let stamp = times[i].as_str().unwrap_or("");
        let label = if i == 0 { "Agora".to_string() } else { format!("{}h", stamp.get(11..13).and_then(|h| h.parse::<i32>().ok()).unwrap_or(0)) };
        let code = codes[i].as_i64().unwrap_or(0);
        let is_day = days.get(i).and_then(Value::as_i64).unwrap_or(1) != 0;
        out.push(json!([label, weather_icon(code, is_day), round(&temps[i]), code]));
    }
    Value::Array(out)
}

/// Próximos dias: [rótulo, ícone, mínima, máxima].
fn days(data: &Value) -> Value {
    let daily = &data["daily"];
    let empty = Vec::new();
    let times = daily["time"].as_array().unwrap_or(&empty);
    let names = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
    let mut out = Vec::new();
    for (i, t) in times.iter().enumerate() {
        let label = if i == 0 {
            "Hoje".to_string()
        } else {
            // dia da semana: 1970-01-01 foi uma quinta
            t.as_str().and_then(util::parse_date).map(|d| names[((d % 7 + 11) % 7) as usize].to_string()).unwrap_or_default()
        };
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

pub struct Tempo {
    weather: Option<Value>,
    weather_msg: String,
    next_weather: Option<Instant>,
    pending: Option<Receiver<Fetch>>,
    poll_at: Instant,
    status: Status,
    started: Instant,
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
                let cur = &data["current"];
                let daily = &data["daily"];
                let code = cur.i64_of("weather_code").unwrap_or(0);
                let is_day = cur.i64_of("is_day").unwrap_or(1) != 0;
                let hours = hours(&data);
                let weather = json!({
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
                    "rain_line": rain_line(&hours),
                    "hours": hours,
                    "days": days(&data),
                });
                self.status.error = None;
                self.status.updated = Some(win::now());
                ctx.cache["weather"] = weather.clone();
                ctx.cache["weather_time"] = json!(self.status.updated);
                ctx.out.push(super::Request::SaveCache);
                self.weather = Some(weather);
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
        if mode == Mode::P { SMALL } else { W }
    }

    fn height(&self, mode: Mode) -> f32 {
        match mode {
            Mode::P => SMALL,
            Mode::M => M_H,
            Mode::G => G_H,
        }
    }

    fn radius(mode: Mode) -> f32 {
        match mode {
            Mode::P => SMALL_RADIUS,
            Mode::M => 24.0,
            Mode::G => 26.0,
        }
    }

    fn temp_text(&self) -> String {
        self.weather.as_ref().and_then(|w| w.i64_of("temp")).map(|t| format!("{t}°")).unwrap_or_else(|| "--°".into())
    }

    fn minmax(&self) -> String {
        match self.weather.as_ref().map(|w| (w.i64_of("max"), w.i64_of("min"))) {
            Some((Some(max), Some(min))) => format!("Máx. {max}°  Mín. {min}°"),
            _ => String::new(),
        }
    }

    fn city(&self, env: &Env) -> String {
        let wcfg = env.wcfg();
        self.weather.as_ref().and_then(|w| w.str_of("place")).or(wcfg.str_of("place")).or(wcfg.str_of("city")).unwrap_or("Tempo").to_string()
    }

    /// Fundo: o céu, com estrelas nas noites limpas.
    fn draw_sky(&self, c: &mut Canvas, mode: Mode) {
        let (w, h) = (Self::width(mode), self.height(mode));
        let (top, bottom) = sky(self.code(), self.is_day());
        c.gradient_round_rect(0.0, 0.0, w, h, Self::radius(mode), &[(0.0, top.alpha(0.94)), (1.0, bottom.alpha(0.94))], 0.0, h.min(G_H));
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

    /// Cidade, temperatura grande e, à direita, ícone, condição e máxima/mínima.
    fn draw_top(&self, c: &mut Canvas, env: &Env) {
        let city = self.city(env);
        c.text(PAD, 13.0, &city, Face::Semibold, 12.5, FG, Anchor::LA, Some(170.0));
        c.text(PAD - 2.0, 30.0, &self.temp_text(), Face::Light, 40.0, FG, Anchor::LA, None);
        match &self.weather {
            Some(wx) => {
                let size = 30.0;
                c.image(&weather_art(wx.str_of("icon").unwrap_or("clouds"), (size * c.s).round() as u32), W - PAD - size + 2.0, 10.0);
                c.text(W - PAD, 44.0, wx.str_of("desc").unwrap_or(""), Face::Semibold, 12.5, FG, Anchor::RA, Some(150.0));
                c.text(W - PAD, 61.0, &self.minmax(), Face::Regular, 11.5, MUTED, Anchor::RA, None);
            }
            None => {
                c.text(W - PAD, 44.0, &self.weather_msg, Face::Semibold, 12.0, FG, Anchor::RA, Some(170.0));
            }
        }
    }

    fn hour_columns() -> Vec<f32> {
        let col = (W - 2.0 * PAD + 8.0) / HOURS as f32;
        (0..HOURS).map(|i| PAD - 4.0 + col * (i as f32 + 0.5)).collect()
    }

    fn draw_hours(&self, c: &mut Canvas) {
        let hours = self.weather.as_ref().and_then(|w| w["hours"].as_array().cloned()).unwrap_or_default();
        if hours.is_empty() {
            return;
        }
        let icon = 24.0;
        for (cx, hour) in Self::hour_columns().into_iter().zip(hours) {
            let label = hour[0].as_str().unwrap_or("");
            c.text(cx, HOURS_Y + 4.0, label, Face::Regular, 10.5, MUTED, Anchor::MA, None);
            c.image(&weather_art(hour[1].as_str().unwrap_or("clouds"), (icon * c.s).round() as u32), cx - icon / 2.0, HOURS_Y + 21.0);
            c.text(cx, HOURS_Y + 49.0, &format!("{}°", hour[2].as_i64().unwrap_or(0)), Face::Semibold, 12.0, FG, Anchor::MA, None);
        }
    }

    /// Lista de dias com a faixa de mínima a máxima (e o ponto de agora em "Hoje").
    fn draw_days(&self, c: &mut Canvas, y0: f32, count: usize) {
        let days = self.weather.as_ref().and_then(|w| w["days"].as_array().cloned()).unwrap_or_default();
        let days: Vec<Value> = days.into_iter().take(count).collect();
        if days.is_empty() {
            return;
        }
        let lo = days.iter().filter_map(|d| d[2].as_i64()).min().unwrap_or(0) as f32;
        let hi = days.iter().filter_map(|d| d[3].as_i64()).max().unwrap_or(1) as f32;
        let span = (hi - lo).max(1.0);
        let (bx0, bx1) = (128.0, W - PAD - 34.0);
        for (i, d) in days.iter().enumerate() {
            let y = y0 + DAY_H * i as f32;
            let cy = y + DAY_H / 2.0;
            c.hline_px(PAD, W - PAD, y, WHITE.alpha(0.12));
            c.text(PAD, cy, d[0].as_str().unwrap_or(""), Face::Regular, 12.5, FG, Anchor::LM, None);
            let icon = 20.0;
            c.image(&weather_art(d[1].as_str().unwrap_or("clouds"), (icon * c.s).round() as u32), 62.0, cy - icon / 2.0);
            let (mn, mx) = (d[2].as_i64().unwrap_or(0) as f32, d[3].as_i64().unwrap_or(0) as f32);
            c.text(bx0 - 8.0, cy, &format!("{mn}°"), Face::Regular, 12.5, MUTED, Anchor::RM, None);
            c.text(W - PAD, cy, &format!("{mx}°"), Face::Semibold, 12.5, FG, Anchor::RM, None);
            c.fill_round_rect(bx0, cy - 2.5, bx1, cy + 2.5, 2.5, Rgba(0, 0, 0, 60));
            let (a, b) = (bx0 + (bx1 - bx0) * (mn - lo) / span, bx0 + (bx1 - bx0) * (mx - lo) / span);
            c.gradient_h(a, cy - 2.5, b.max(a + 5.0), cy + 2.5, 2.5, &[(0.0, Rgba::rgb(90, 200, 250)), (0.6, Rgba::rgb(255, 214, 10)), (1.0, Rgba::rgb(255, 159, 10))], bx0, bx1);
            if i == 0 {
                if let Some(t) = self.weather.as_ref().and_then(|w| w.i64_of("temp")) {
                    let x = bx0 + (bx1 - bx0) * ((t as f32 - lo) / span).clamp(0.0, 1.0);
                    c.fill_circle(x, cy, 4.5, Rgba(0, 0, 0, 90));
                    c.fill_circle(x, cy, 3.2, WHITE);
                }
            }
        }
    }

    /// Pequeno (quadrado): cidade, temperatura grande e, embaixo, ícone, condição e máx./mín.
    fn draw_compact(&self, c: &mut Canvas, env: &Env) {
        let (x, w) = (14.0, SMALL - 28.0);
        let city = self.city(env);
        let city = city.split(',').next().unwrap_or(&city).to_string();
        c.text(x, 14.0, &city, Face::Semibold, 12.5, FG, Anchor::LA, Some(w));
        c.text(x - 2.0, 30.0, &self.temp_text(), Face::Light, 42.0, FG, Anchor::LA, None);
        match &self.weather {
            Some(wx) => {
                let size = 24.0;
                c.image(&weather_art(wx.str_of("icon").unwrap_or("clouds"), (size * c.s).round() as u32), x - 2.0, 88.0);
                c.text(x, 115.0, wx.str_of("desc").unwrap_or(""), Face::Semibold, 12.0, FG, Anchor::LA, Some(w));
                if let (Some(max), Some(min)) = (wx.i64_of("max"), wx.i64_of("min")) {
                    c.text(x, 132.0, &format!("Máx. {max}°  Mín. {min}°"), Face::Regular, 11.0, MUTED, Anchor::LA, Some(w));
                }
            }
            None => {
                for (i, line) in c.wrap(&self.weather_msg, Face::Regular, 11.5, w, 3).iter().enumerate() {
                    c.text(x, 96.0 + 16.0 * i as f32, line, Face::Regular, 11.5, MUTED, Anchor::LA, Some(w));
                }
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
}

impl Kind for Tempo {
    fn key(&self) -> &'static str {
        "tempo"
    }
    fn title(&self) -> &'static str {
        "Tempo"
    }
    fn default_mode(&self) -> Mode {
        Mode::M
    }

    fn layout(&self, mode: Mode) -> Layout {
        Layout { w: Self::width(mode), h: self.height(mode), radius: Self::radius(mode) }
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, env: &Env) {
        self.draw_sky(c, mode);
        match mode {
            Mode::P => self.draw_compact(c, env),
            Mode::M => {
                self.draw_top(c, env);
                self.draw_hours(c);
            }
            Mode::G => {
                self.draw_top(c, env);
                self.draw_hours(c);
                let line = self.weather.as_ref().and_then(|w| w.str_of("rain_line").map(String::from).or(w.str_of("details").map(String::from)));
                if let Some(text) = line {
                    c.fill_round_rect(PAD - 4.0, G_LINE_Y, W - PAD + 4.0, G_LINE_Y + 26.0, 9.0, Rgba(0, 0, 0, 46));
                    c.text(PAD + 6.0, G_LINE_Y + 13.0, &text, Face::Regular, 11.5, FG, Anchor::LM, Some(W - 2.0 * PAD - 12.0));
                }
                self.draw_days(c, G_DAYS_Y, G_DAYS);
            }
        }
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
        let _ = x;
        if env.wcfg().str_of("city").is_none_or(str::is_empty) {
            return Some(ASK_CITY);
        }
        if self.weather.is_none() {
            return self.status.error.as_ref().map(|_| UPDATE_WEATHER);
        }
        let _ = (y, mode);
        env.shortcuts().then_some(OPEN_FORECAST)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            OPEN_FORECAST => {
                let url = Self::forecast_url(&ctx.env());
                ctx.open(url);
            }
            UPDATE_WEATHER => self.update_weather(ctx),
            ASK_CITY => ctx.out.push(super::Request::Ask("city")),
            _ => {}
        }
    }

    fn tooltip(&self, _x: f32, y: f32, mode: Mode, env: &Env) -> Option<String> {
        let Some(wx) = &self.weather else {
            return self.status.error.as_ref().map(|_| "Clique para tentar de novo".to_string());
        };
        let mut lines = vec![format!("{}  ·  {}", wx.str_of("desc").unwrap_or(""), self.temp_text()), wx.str_of("details").unwrap_or("").to_string()];
        if let Some(t) = self.status.updated {
            lines.push(format!("Atualizado {}", util::time_ago(t)));
        }
        let _ = (y, mode);
        if env.shortcuts() {
            lines.push("Clique para ver a previsão completa".into());
        }
        Some(lines.join("\n"))
    }

    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        vec![MenuItem::Cmd("Atualizar".into(), UPDATE_WEATHER), MenuItem::Cmd("Mudar cidade…".into(), ASK_CITY)]
    }

    fn next_tick(&self) -> Option<Instant> {
        let mut next = self.next_weather;
        if self.pending.is_some() {
            next = Some(next.map_or(self.poll_at, |n| n.min(self.poll_at)));
        }
        next
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        let now = Instant::now();
        let mut changed = false;
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
