//! Widget de clima e sistema (CPU, memória e disco).

use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{Ctx, Env, Hot, Kind, Layout, MenuItem, Mode, NotifyAction, Status};
use crate::config::Get;
use crate::drawing::{self, weather_art};
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::*;
use crate::util;
use crate::win;

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 318.0;
const PAD: f32 = 20.0;
const RADIUS: f32 = 26.0;
const RING_D: f32 = 72.0;
const RING_W: f32 = 8.0;
const ICON: f32 = 54.0;
const COMPACT_ICON: f32 = 34.0;
const COMPACT_H: f32 = 64.0;
const COMPACT_RADIUS: f32 = 22.0;
const MEDIUM_H: f32 = 150.0; // pequeno + as próximas horas
const HOURS: usize = 6;
const C_DIVIDER_X: f32 = 136.0; // entre o tempo e os anéis
const COMPACT_RING_D: f32 = 40.0;
const COMPACT_RING_W: f32 = 5.0;
const RING_FULL: Rgba = Rgba::rgb(255, 69, 58);

const WEATHER_EVERY: Duration = Duration::from_secs(15 * 60);
const WEATHER_RETRY: Duration = Duration::from_secs(2 * 60);
const SYSTEM_EVERY: Duration = Duration::from_secs(2);

// ações
const OPEN_TASKMGR: u32 = 1;
const OPEN_STORAGE: u32 = 2;
const OPEN_FORECAST: u32 = 3;
pub const UPDATE_WEATHER: u32 = 4;
pub const ASK_CITY: u32 = 5;

fn ring_color(name: &str) -> Rgba {
    match name {
        "CPU" => Rgba::rgb(10, 132, 255),
        "RAM" => Rgba::rgb(191, 90, 242),
        _ => Rgba::rgb(48, 209, 88),
    }
}

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
    Offline(String),
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
            Err(e) => return Fetch::Offline(e),
        },
    };
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day,precipitation\
         &hourly=temperature_2m,weather_code,is_day&forecast_hours=8&minutely_15=precipitation&forecast_minutely_15=6\
         &daily=temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=1"
    );
    match util::fetch_json(&url) {
        Ok(data) => Fetch::Ok((city, lat, lon, place), data),
        Err(e) => Fetch::Offline(e),
    }
}

pub struct System {
    weather: Option<Value>,
    weather_msg: String,
    metrics: Vec<(&'static str, f32, String)>,
    last_cpu: (u64, u64),
    next_system: Instant,
    next_weather: Option<Instant>,
    pending: Option<Receiver<Fetch>>,
    /// quando olhar de novo se a busca do clima terminou
    poll_at: Instant,
    status: Status,
}

impl System {
    pub fn new(cfg: &Value, cache: &Value) -> System {
        let wcfg = &cfg["sistema"];
        let place = wcfg.str_of("place").or(wcfg.str_of("city"));
        // abre com o último clima guardado, enquanto busca o atual
        let cached = cache.get("weather").filter(|w| place.is_some() && w.str_of("place") == place).cloned();
        let updated = if cached.is_some() { cache.f64_of("weather_time") } else { None };
        let mut s = System {
            weather: cached,
            weather_msg: "Carregando…".into(),
            metrics: Vec::new(),
            last_cpu: win::cpu_times(),
            next_system: Instant::now(),
            next_weather: None,
            pending: None,
            poll_at: Instant::now(),
            status: Status { updated, ..Default::default() },
        };
        s.update_system();
        s
    }

    fn update_system(&mut self) {
        let (idle, total) = win::cpu_times();
        let (last_idle, last_total) = self.last_cpu;
        self.last_cpu = (idle, total);
        let d_total = total.saturating_sub(last_total);
        let cpu = if d_total > 0 { 1.0 - idle.saturating_sub(last_idle) as f64 / d_total as f64 } else { 0.0 };
        let cpu = cpu.clamp(0.0, 1.0);
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let (used, total_mem) = win::memory();
        let (dused, dtotal, _) = win::disk_usage("C:\\");
        self.metrics = vec![
            ("CPU", cpu as f32, format!("{} / {cores} núcleos", br(cpu * cores as f64, 1))),
            ("RAM", (used as f64 / total_mem as f64) as f32, format!("{} / {:.0} GB", br(win::gb(used), 1), win::gb(total_mem))),
            ("Disco", (dused as f64 / dtotal as f64) as f32, format!("{:.0} / {:.0} GB", win::gb(dused), win::gb(dtotal))),
        ];
        self.next_system = Instant::now() + SYSTEM_EVERY;
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
            Fetch::Offline(_) => {
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
                let r = |v: &Value| v.as_f64().unwrap_or(0.0).round() as i64;
                let weather = json!({
                    "place": place,
                    "temp": format!("{}°", r(&cur["temperature_2m"])),
                    "icon": weather_icon(code, cur.i64_of("is_day").unwrap_or(1) != 0),
                    "desc": wmo(code),
                    "minmax": format!("Máx. {}°   Mín. {}°", r(&daily["temperature_2m_max"][0]), r(&daily["temperature_2m_min"][0])),
                    "details": format!("Sensação {}°  ·  Umidade {}%  ·  Vento {} km/h",
                        r(&cur["apparent_temperature"]), r(&cur["relative_humidity_2m"]), r(&cur["wind_speed_10m"])),
                    "hours": hours(&data),
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
                let place = self.weather.as_ref().and_then(|w| w.str_of("place")).unwrap_or("").split(',').next().unwrap_or("").to_string();
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

    fn lay(mode: Mode) -> (f32, f32, f32, f32, f32, Vec<(f32, f32)>) {
        // (w, h, raio, diâmetro do anel, espessura, centros dos anéis)
        if mode != Mode::G {
            let h = if mode == Mode::P { COMPACT_H } else { MEDIUM_H };
            // anéis encostados na direita, com a mesma margem do cartão
            let rings = (0..3).map(|i| (WIDTH - 36.0 - 54.0 * (2 - i) as f32, COMPACT_H / 2.0)).collect();
            return (WIDTH, h, COMPACT_RADIUS, COMPACT_RING_D, COMPACT_RING_W, rings);
        }
        let col = (WIDTH - 2.0 * PAD) / 3.0;
        let rings = (0..3).map(|i| (PAD + col * (i as f32 + 0.5), 188.0 + RING_D / 2.0)).collect();
        (WIDTH, HEIGHT, RADIUS, RING_D, RING_W, rings)
    }

    fn ring_at(&self, x: f32, y: f32, mode: Mode) -> Option<(f32, f32, f32, usize)> {
        let (.., d, _, rings) = Self::lay(mode);
        rings.iter().enumerate().take(self.metrics.len()).find_map(|(i, &(cx, cy))| {
            (((x - cx).powi(2) + (y - cy).powi(2)).sqrt() <= d / 2.0 + 2.0).then_some((cx, cy, d / 2.0, i))
        })
    }

    fn weather_rect(mode: Mode) -> (f32, f32, f32, f32) {
        if mode != Mode::G {
            (8.0, 8.0, C_DIVIDER_X - 6.0, COMPACT_H - 8.0)
        } else {
            (PAD - 6.0, 34.0, PAD + 112.0, 94.0)
        }
    }

    fn on_weather(x: f32, y: f32, mode: Mode) -> bool {
        let (x0, y0, x1, y1) = Self::weather_rect(mode);
        let on_icon = mode == Mode::G && (WIDTH - PAD - ICON..=WIDTH - PAD + 4.0).contains(&x) && (12.0..=12.0 + ICON).contains(&y);
        ((x0..=x1).contains(&x) && (y0..=y1).contains(&y)) || on_icon
    }

    fn on_hours(x: f32, y: f32, mode: Mode) -> bool {
        mode == Mode::M && (COMPACT_H + 4.0..=MEDIUM_H - 4.0).contains(&y) && (8.0..=WIDTH - 8.0).contains(&x)
    }

    fn draw_compact(&self, c: &mut Canvas, mode: Mode) {
        let cy = COMPACT_H / 2.0;
        let mut x = 16.0;
        if let Some(wx) = &self.weather {
            let size = COMPACT_ICON;
            c.image(&weather_art(wx.str_of("icon").unwrap_or("clouds"), (size * c.s).round() as u32), 14.0, cy - size / 2.0);
            x = 14.0 + size + 6.0;
        }
        let temp = self.weather.as_ref().and_then(|w| w.str_of("temp")).unwrap_or("--°");
        c.text(x, cy, temp, Face::Light, 26.0, FG, Anchor::LM, None);
        c.vline_px(C_DIVIDER_X, 18.0, COMPACT_H - 18.0, DIVIDER);
        let (.., d, wd, rings) = Self::lay(mode);
        for (&(rx, ry), (name, frac, _)) in rings.iter().zip(&self.metrics) {
            let color = if *frac >= 0.9 { RING_FULL } else { ring_color(name) };
            drawing::ring(c, rx, ry, d, wd, *frac, color);
            c.text(rx, ry, &format!("{}%", (frac * 100.0).round()), Face::Semibold, 10.0, FG, Anchor::MM, None);
        }
        if mode == Mode::M {
            self.draw_hours(c);
        }
    }

    fn hour_columns() -> Vec<f32> {
        let col = (WIDTH - 32.0) / HOURS as f32;
        (0..HOURS).map(|i| 16.0 + col * (i as f32 + 0.5)).collect()
    }

    /// Faixa com a previsão das próximas horas (tamanho médio).
    fn draw_hours(&self, c: &mut Canvas) {
        c.hline_px(PAD, WIDTH - PAD, COMPACT_H, DIVIDER);
        let hours = self.weather.as_ref().and_then(|w| w["hours"].as_array().cloned()).unwrap_or_default();
        if hours.is_empty() {
            let msg = if self.weather.is_none() { self.weather_msg.as_str() } else { "Previsão por hora indisponível" };
            c.text(WIDTH / 2.0, (COMPACT_H + MEDIUM_H) / 2.0, msg, Face::Regular, 12.0, MUTED, Anchor::MM, Some(WIDTH - 40.0));
            return;
        }
        let icon = 28.0;
        for (cx, hour) in Self::hour_columns().into_iter().zip(hours) {
            let (label, name, temp) = (hour[0].as_str().unwrap_or(""), hour[1].as_str().unwrap_or("clouds"), hour[2].as_str().unwrap_or(""));
            c.text(cx, COMPACT_H + 10.0, label, Face::Regular, 10.5, MUTED, Anchor::MA, None);
            c.image(&weather_art(name, (icon * c.s).round() as u32), cx - icon / 2.0, COMPACT_H + 30.0);
            c.text(cx, COMPACT_H + 64.0, temp, Face::Semibold, 12.0, FG, Anchor::MA, None);
        }
    }

    fn draw_full(&self, c: &mut Canvas, env: &Env) {
        let inner = WIDTH - 2.0 * PAD;
        let wcfg = env.wcfg();
        let city = self
            .weather
            .as_ref()
            .and_then(|w| w.str_of("place"))
            .or(wcfg.str_of("place"))
            .or(wcfg.str_of("city"))
            .unwrap_or("Clima")
            .to_string();
        c.text(PAD, 18.0, &city, Face::Semibold, 14.0, FG, Anchor::LA, Some(inner - ICON - 4.0));
        let temp = self.weather.as_ref().and_then(|w| w.str_of("temp")).unwrap_or("--°");
        c.text(PAD - 2.0, 34.0, temp, Face::Light, 48.0, FG, Anchor::LA, None);
        if let Some(wx) = &self.weather {
            let size = ICON;
            c.image(&weather_art(wx.str_of("icon").unwrap_or("clouds"), (size * c.s).round() as u32), WIDTH - PAD - size + 4.0, 12.0);
            c.text(PAD, 100.0, wx.str_of("desc").unwrap_or(""), Face::Semibold, 13.0, FG, Anchor::LA, Some(inner));
            c.text(PAD, 120.0, wx.str_of("minmax").unwrap_or(""), Face::Regular, 12.0, MUTED, Anchor::LA, None);
            c.text(PAD, 145.0, wx.str_of("details").unwrap_or(""), Face::Regular, 11.0, MUTED, Anchor::LA, Some(inner));
        } else {
            c.text(PAD, 100.0, &self.weather_msg, Face::Semibold, 13.0, FG, Anchor::LA, Some(inner));
        }
        c.hline_px(PAD, WIDTH - PAD, 172.0, DIVIDER);
        let (.., d, wd, rings) = Self::lay(Mode::G);
        for (&(cx, cy), (name, frac, detail)) in rings.iter().zip(&self.metrics) {
            let color = if *frac >= 0.9 { RING_FULL } else { ring_color(name) };
            drawing::ring(c, cx, cy, d, wd, *frac, color);
            c.text(cx, cy, &format!("{}%", (frac * 100.0).round()), Face::Semibold, 15.0, FG, Anchor::MM, None);
            c.text(cx, cy + RING_D / 2.0 + 10.0, name, Face::Semibold, 12.0, FG, Anchor::MA, None);
            c.text(cx, cy + RING_D / 2.0 + 27.0, detail, Face::Regular, 11.0, MUTED, Anchor::MA, None);
        }
    }

    fn forecast_url(env: &Env) -> String {
        let w = env.wcfg();
        let place = w.str_of("place").or(w.str_of("city")).unwrap_or("");
        format!("https://www.google.com/search?q={}", util::urlencode(&format!("previsão do tempo {place}")))
    }
}

/// Minutos desde a meia-noite de "2026-10-31T14:15" (o horário local que a Open-Meteo devolve).
fn local_minutes(stamp: &str) -> Option<f64> {
    let days = util::parse_date(stamp)? as f64;
    let h: f64 = stamp.get(11..13)?.parse().ok()?;
    let m: f64 = stamp.get(14..16)?.parse().ok()?;
    Some(days * 1440.0 + h * 60.0 + m)
}

fn hours(data: &Value) -> Value {
    let hourly = &data["hourly"];
    let empty = Vec::new();
    let times = hourly["time"].as_array().unwrap_or(&empty);
    let temps = hourly["temperature_2m"].as_array().unwrap_or(&empty);
    let codes = hourly["weather_code"].as_array().unwrap_or(&empty);
    let days = hourly["is_day"].as_array().unwrap_or(&empty);
    let mut out = Vec::new();
    for i in 0..HOURS.min(times.len()).min(temps.len()).min(codes.len()) {
        let stamp = times[i].as_str().unwrap_or("");
        let label = if i == 0 { "Agora".to_string() } else { format!("{}h", stamp.get(11..13).and_then(|h| h.parse::<i32>().ok()).unwrap_or(0)) };
        let code = codes[i].as_i64().unwrap_or(0);
        let is_day = days.get(i).and_then(Value::as_i64).unwrap_or(1) != 0;
        out.push(json!([label, weather_icon(code, is_day), format!("{}°", temps[i].as_f64().unwrap_or(0.0).round() as i64)]));
    }
    Value::Array(out)
}

impl Kind for System {
    fn key(&self) -> &'static str {
        "sistema"
    }
    fn title(&self) -> &'static str {
        "Clima e Sistema"
    }

    fn layout(&self, mode: Mode) -> Layout {
        let (w, h, radius, ..) = Self::lay(mode);
        Layout { w, h, radius }
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, env: &Env) {
        if mode == Mode::G {
            self.draw_full(c, env);
        } else {
            self.draw_compact(c, mode);
        }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        let shortcuts = env.shortcuts();
        if let Some((.., i)) = self.ring_at(x, y, mode) {
            if shortcuts {
                return Some(if self.metrics[i].0 == "Disco" { OPEN_STORAGE } else { OPEN_TASKMGR });
            }
        }
        if Self::on_weather(x, y, mode) || Self::on_hours(x, y, mode) {
            if env.wcfg().str_of("city").is_none_or(str::is_empty) {
                return Some(ASK_CITY);
            }
            if self.weather.is_none() {
                return self.status.error.as_ref().map(|_| UPDATE_WEATHER);
            }
            return shortcuts.then_some(OPEN_FORECAST);
        }
        None
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            OPEN_TASKMGR => ctx.open("taskmgr.exe"),
            OPEN_STORAGE => ctx.open("ms-settings:storagesense"),
            OPEN_FORECAST => {
                let url = Self::forecast_url(&ctx.env());
                ctx.open(url);
            }
            UPDATE_WEATHER => self.update_weather(ctx),
            ASK_CITY => ctx.out.push(super::Request::Ask("city")),
            _ => {}
        }
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<Hot> {
        if let Some((cx, cy, r, _)) = self.ring_at(x, y, mode) {
            return Some((cx - r - 4.0, cy - r - 4.0, cx + r + 4.0, cy + r + 4.0, r + 4.0));
        }
        if Self::on_hours(x, y, mode) {
            return Some((8.0, COMPACT_H + 4.0, WIDTH - 8.0, MEDIUM_H - 6.0, 14.0));
        }
        if Self::on_weather(x, y, mode) {
            if mode == Mode::G && x > PAD + 112.0 {
                return Some((WIDTH - PAD - ICON, 10.0, WIDTH - PAD + 4.0, 14.0 + ICON, 14.0));
            }
            let (x0, y0, x1, y1) = Self::weather_rect(mode);
            return Some((x0, y0, x1, y1, 12.0));
        }
        None
    }

    fn tooltip(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<String> {
        if let Some((.., i)) = self.ring_at(x, y, mode) {
            let (name, frac, detail) = &self.metrics[i];
            let action = if *name == "Disco" { "as configurações de armazenamento" } else { "o Gerenciador de Tarefas" };
            let mut lines = vec![format!("{name}  ·  {}%", (frac * 100.0).round()), detail.clone()];
            if env.shortcuts() {
                lines.push(format!("Clique para abrir {action}"));
            }
            return Some(lines.join("\n"));
        }
        let wx = self.weather.as_ref();
        if Self::on_hours(x, y, mode) {
            return wx.map(|_| "Próximas horas\nClique para ver a previsão completa".to_string());
        }
        if Self::on_weather(x, y, mode) {
            let Some(wx) = wx else {
                return self.status.error.as_ref().map(|_| "Clique para tentar de novo".to_string());
            };
            let mut lines = vec![
                format!("{}  ·  {}", wx.str_of("desc").unwrap_or(""), wx.str_of("temp").unwrap_or("")),
                wx.str_of("minmax").unwrap_or("").to_string(),
            ];
            if mode != Mode::G {
                lines.push(wx.str_of("details").unwrap_or("").to_string());
            }
            if let Some(t) = self.status.updated {
                lines.push(format!("Atualizado {}", util::time_ago(t)));
            }
            if env.shortcuts() {
                lines.push("Clique para ver a previsão".into());
            }
            return Some(lines.join("\n"));
        }
        None
    }

    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        vec![MenuItem::Cmd("Atualizar clima".into(), UPDATE_WEATHER), MenuItem::Cmd("Mudar cidade…".into(), ASK_CITY)]
    }

    fn next_tick(&self) -> Option<Instant> {
        let mut next = self.next_system;
        if let Some(w) = self.next_weather {
            next = next.min(w);
        }
        if self.pending.is_some() {
            next = next.min(self.poll_at);
        }
        Some(next)
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
                    self.receive(Fetch::Offline("falha inesperada".into()), ctx);
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.poll_at = now + Duration::from_millis(200),
            }
        }
        if self.next_weather.is_some_and(|t| now >= t) {
            self.update_weather(ctx);
            changed = true;
        }
        if now >= self.next_system {
            self.update_system();
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
