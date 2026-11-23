//! O céu na ilha: o pôr do sol chegando, a lua cheia (e os eclipses da Lua) e a Estação
//! Espacial passando. Tudo calculado para a cidade do widget Tempo (ver `crate::data::astro`).

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{cached_hours, cached_weather, draw_buttons, hit, location, Clock, HourCode, Place, OK};
use crate::data::astro::{self, Eclipse, EclipseKind, Orbit, Pass};
use crate::data::config::Get;
use crate::gfx::drawing::weather_art;
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::ui::theme::{Rgba, FG, MUTED, WHITE};
use crate::widgets::island_ui as iu;
use crate::widgets::{alert_hash, on_island, sky, Ctx, Env, IslandForm, IslandItem, Kind, Mode, Rect4, Request};

/// "a leste", "ao norte"
pub(super) fn toward(azimuth: f64) -> String {
    match astro::compass(azimuth) {
        "norte" => "ao norte".into(),
        "sul" => "ao sul".into(),
        name => format!("a {name}"),
    }
}

/// "12 min", "1 min", "agora"
fn minutes_until(secs: f64) -> String {
    let minutes = (secs / 60.0).ceil() as i64;
    if minutes < 1 { "agora".into() } else { format!("{minutes} min") }
}

// ---------- o pôr do sol ----------

const SUN: Rgba = Rgba::rgb(255, 159, 67);
/// o aviso aparece este tanto antes do pôr do sol
const SUN_LEAD: f64 = 15.0 * 60.0;
/// de quanto em quanto tempo ler de novo o clima guardado pelo Tempo
const WEATHER_EVERY: Duration = Duration::from_secs(10 * 60);

/// O próximo pôr do sol já calculado: para onde, a partir de quando e o horário dele.
#[derive(Clone, Copy)]
struct SunsetCalc {
    lat: f64,
    lon: f64,
    from: f64,
    at: f64,
}

/// De que é o céu guardado: o tamanho em pixels, o clima e o minuto do pôr do sol.
#[derive(Clone, Copy, PartialEq)]
struct SkyKey {
    w_px: u32,
    h_px: u32,
    weather: i64,
    minute: i64,
}

pub struct Sunset {
    clock: Clock,
    next: Cell<Option<SunsetCalc>>,
    /// o clima de agora (do Tempo), para o céu da ilha aberta
    weather: (i64, bool),
    check_at: Instant,
    ok: Vec<String>,
    /// o céu do pôr do sol já desenhado (para a ilha aberta)
    sky: RefCell<Option<(SkyKey, Image)>>,
}

/// O aviso do pôr do sol: quando, a chave do OK, o azimute (para onde olhar) e a cidade.
struct SunAlert {
    at: f64,
    key: String,
    az: f64,
    place: String,
}

impl Sunset {
    pub fn new() -> Sunset {
        Sunset { clock: Clock::default(), next: Cell::new(None), weather: (0, false), check_at: Instant::now(), ok: Vec::new(), sky: RefCell::new(None) }
    }

    /// Dez minutos antes do pôr do sol em São Paulo (20/11/2026).
    #[cfg(test)]
    pub fn sample() -> Sunset {
        let noon = crate::data::util::days_from_civil(2026, 11, 20) as f64 * 86400.0 + 15.0 * 3600.0;
        let set = astro::next_sunset(noon, -23.55, -46.63).unwrap();
        Sunset { clock: Clock::at(set - 600.0, -3.0 * 3600.0), ..Sunset::new() }
    }

    /// O próximo pôr do sol em (lat, lon).
    fn sunset(&self, lat: f64, lon: f64) -> Option<f64> {
        let now = self.clock.now();
        // vale enquanto o relógio está entre o cálculo e o pôr do sol (se o relógio do PC voltar,
        // calcula de novo)
        if let Some(calc) = self.next.get() {
            if calc.lat == lat && calc.lon == lon && calc.from <= now && calc.at >= now {
                return Some(calc.at);
            }
        }
        let at = astro::next_sunset(now, lat, lon)?;
        self.next.set(Some(SunsetCalc { lat, lon, from: now, at }));
        Some(at)
    }

    /// O aviso de agora: dos 15 minutos antes até o sol se pôr.
    fn alert(&self, cfg: &Value) -> Option<SunAlert> {
        let Place { lat, lon, name: place } = location(cfg)?;
        let at = self.sunset(lat, lon)?;
        let now = self.clock.now();
        if now < at - SUN_LEAD || now >= at {
            return None;
        }
        let key = format!("sol|{}", self.clock.day(at));
        if self.ok.contains(&key) {
            return None;
        }
        Some(SunAlert { at, key, az: astro::sun_alt_az(at, lat, lon).1, place })
    }

    /// O céu previsto para o pôr do sol (guardado; refeito quando muda o tamanho ou o clima).
    fn sky_at(&self, c: &Canvas, at: f64) -> std::cell::Ref<'_, Option<(SkyKey, Image)>> {
        let iu::FormSize { w, h, radius: r } = iu::size(IslandForm::Large);
        let local = |t: f64| ((t + self.clock.offset()).rem_euclid(86400.0) / 3600.0) as f32;
        let key = SkyKey { w_px: (w * c.s).round() as u32, h_px: (h * c.s).round() as u32, weather: self.weather.0, minute: (at / 60.0) as i64 };
        if self.sky.borrow().as_ref().is_none_or(|(saved, _)| *saved != key) {
            let mut sky_canvas = Canvas::new(key.w_px, key.h_px, c.s);
            // o céu de 4 minutos antes do pôr do sol (o mais bonito), com o sol se pondo na hora
            let clock = sky::Clock::new(local(at - 4.0 * 60.0), 6.0, local(at));
            sky::draw_back(&mut sky_canvas, w, h, r, sky::Weather::of(self.weather.0, self.weather.1), &clock);
            *self.sky.borrow_mut() = Some((key, sky_canvas.finish()));
        }
        self.sky.borrow()
    }
}

/// O sol descendo até a linha do horizonte (`progress`: 0 = 15 minutos antes, 1 = no pôr do sol).
fn setting_sun(c: &mut Canvas, cx: f32, cy: f32, size: f32, progress: f32) {
    let img = weather_art("sun", (size * c.s).round() as u32);
    let horizon = cy + size * 0.24;
    c.push_round_clip(cx - size, cy - size, cx + size, horizon, 0.0);
    c.image(&img, cx - size / 2.0, cy - size / 2.0 + size * 0.28 * progress.clamp(0.0, 1.0));
    c.pop_clip();
    c.fill_round_rect(cx - size * 0.44, horizon - 0.75, cx + size * 0.44, horizon + 0.75, 0.75, SUN.alpha(0.9));
}

impl Kind for Sunset {
    fn key(&self) -> &'static str {
        "sol"
    }
    fn title(&self) -> &'static str {
        super::title("sol")
    }
    fn draw(&self, _c: &mut Canvas, _mode: Mode, _env: &Env) {}

    fn next_tick(&self) -> Option<Instant> {
        Some(self.check_at)
    }

    /// Lê o clima guardado pelo Tempo (para as cores do céu na ilha aberta).
    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        self.check_at = Instant::now() + WEATHER_EVERY;
        if let Some(weather) = cached_weather(ctx.cache) {
            self.weather = weather;
        }
        false
    }

    fn island_item(&self, env: &Env) -> Option<IslandItem> {
        let alert = self.alert(env.cfg)?;
        Some(IslandItem::alert(SUN, alert_hash(&alert.key)))
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        let Some(alert) = self.alert(env.cfg) else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let left = alert.at - self.clock.now();
        let progress = (1.0 - left / SUN_LEAD) as f32;
        let time = self.clock.hhmm(alert.at);
        match form {
            IslandForm::Bubble => setting_sun(c, x + w / 2.0, cy, 30.0, progress),
            IslandForm::Compact | IslandForm::Banner => {
                setting_sun(c, x + 26.0, cy, 28.0, progress);
                if form == IslandForm::Banner {
                    iu::compact_text(c, x + 46.0, r, &format!("Pôr do sol às {time}"), &minutes_until(left), SUN);
                } else {
                    iu::compact_text(c, x + 46.0, r, "Pôr do sol", &time, SUN);
                }
            }
            IslandForm::Medium => {
                setting_sun(c, x + 39.0, cy, 48.0, progress);
                let sub = format!("Em {}, {}", minutes_until(left), toward(alert.az));
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &format!("Pôr do sol às {time}"), &sub, SUN);
                draw_buttons(c, form, r, hot, None);
            }
            IslandForm::Large => {
                {
                    let sky = self.sky_at(c, alert.at);
                    if let Some((_, img)) = sky.as_ref() {
                        c.image_px(img, (x * c.s).round(), (y * c.s).round(), img.w as f32, img.h as f32, 1.0);
                    }
                }
                iu::header(c, r, "PÔR DO SOL", SUN, &alert.place, MUTED);
                c.text(x + 22.0, y + 58.0, &format!("O sol se põe às {time}"), Face::Semibold, 20.0, FG, Anchor::LM, Some(w - 44.0));
                let sub = format!("Em {}. Olhe para o {}.", minutes_until(left), astro::compass(alert.az));
                c.text(x + 22.0, y + 82.0, &sub, Face::Regular, 12.0, MUTED, Anchor::LM, Some(w - 44.0));
                setting_sun(c, x + 46.0, y + 140.0, 56.0, progress);
                draw_buttons(c, form, r, hot, None);
            }
        }
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        hit(form, x, y, w, h, false)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        if action == OK {
            if let Some(alert) = self.alert(ctx.cfg) {
                self.ok.push(alert.key);
            }
        }
    }
}

// ---------- a lua cheia e os eclipses ----------

const MOON: Rgba = Rgba::rgb(236, 228, 200);
const COPPER: Rgba = Rgba::rgb(232, 112, 72);
/// de quanto em quanto tempo conferir as noites (e a cidade); elas são refeitas a cada 6 h
const NIGHTS_CHECK: Duration = Duration::from_secs(30 * 60);
const NIGHTS_EVERY: f64 = 6.0 * 3600.0;
/// o céu escuro o bastante para ver o eclipse: o Sol este tanto abaixo do horizonte
const DARK_SUN: f64 = -6.0;

/// Uma noite de lua cheia (ou de eclipse): quando a ilha mostra e o que.
#[derive(Clone, Debug)]
struct Night {
    show: f64,
    hide: f64,
    /// a Lua nascendo: (horário, azimute)
    rise: Option<(f64, f64)>,
    eclipse: Option<Eclipse>,
    key: String,
}

pub struct Moon {
    clock: Clock,
    nights: Vec<Night>,
    /// a próxima lua cheia depois da desta noite (para a ilha aberta)
    fulls: Vec<f64>,
    for_loc: Option<(f64, f64)>,
    computed: f64,
    check_at: Instant,
    ok: Vec<String>,
}

/// O eclipse dá para ver daqui: a Lua acima do horizonte, com o céu escuro, em algum momento
/// enquanto ela está na sombra.
fn visible(eclipse: &Eclipse, lat: f64, lon: f64) -> bool {
    // de 5 em 5 minutos
    let mut t = eclipse.start;
    while t <= eclipse.end {
        if astro::moon_alt_az(t, lat, lon).0 > astro::MOON_RISE + 2.0 && astro::sun_alt_az(t, lat, lon).0 < DARK_SUN {
            return true;
        }
        t += 300.0;
    }
    false
}

/// As noites de lua cheia (e de eclipse) dos próximos 40 dias.
fn nights(clock: &Clock, lat: f64, lon: f64, from: f64) -> (Vec<Night>, Vec<f64>) {
    let fulls = astro::full_moons(from - 2.0 * 86400.0, from + 40.0 * 86400.0);
    // o começo da noite: às 19 h, ou no pôr do sol se for depois
    let evening = |day: i64| {
        let noon = clock.midnight(day) + 12.0 * 3600.0;
        let set = astro::next_sunset(noon, lat, lon).unwrap_or(noon + 6.0 * 3600.0);
        set.max(clock.midnight(day) + 19.0 * 3600.0)
    };
    let mut out = Vec::new();
    for &full in &fulls {
        let eclipse = astro::lunar_eclipse(full).filter(|eclipse| visible(eclipse, lat, lon));
        let (day, show, hide, key) = match eclipse {
            Some(eclipse) => {
                // a noite em que ele acontece (de madrugada: a noite anterior), desde meia hora
                // antes de ele começar
                let day = clock.day(eclipse.start - 12.0 * 3600.0);
                (day, evening(day).min(eclipse.start - 1800.0), eclipse.end, format!("eclipse|{day}"))
            }
            None => {
                // a noite mais perto da lua cheia
                let day = ((full + clock.offset()) / 86400.0 - 21.0 / 24.0).round() as i64;
                (day, evening(day), clock.midnight(day + 1), format!("lua|{day}"))
            }
        };
        let rise = astro::next_moonrise(clock.midnight(day) + 12.0 * 3600.0, lat, lon).map(|rise| (rise, astro::moon_alt_az(rise, lat, lon).1));
        out.push(Night { show, hide, rise, eclipse, key });
    }
    (out, fulls)
}

impl Moon {
    pub fn new() -> Moon {
        Moon { clock: Clock::default(), nights: Vec::new(), fulls: Vec::new(), for_loc: None, computed: 0.0, check_at: Instant::now(), ok: Vec::new() }
    }

    #[cfg(test)]
    fn sample_at(now: f64) -> Moon {
        let clock = Clock::at(now, -3.0 * 3600.0);
        let (nights, fulls) = nights(&clock, -23.55, -46.63, now);
        Moon { clock, nights, fulls, for_loc: Some((-23.55, -46.63)), computed: now, ..Moon::new() }
    }

    /// A noite da lua cheia de 24/11/2026 em São Paulo, às 20 h.
    #[cfg(test)]
    pub fn sample_full() -> Moon {
        let at = crate::data::util::days_from_civil(2026, 11, 24) as f64 * 86400.0 + 23.0 * 3600.0;
        Moon::sample_at(at)
    }

    /// O eclipse total de 14/03/2025, visto de São Paulo: na noite anterior, ou no máximo.
    #[cfg(test)]
    pub fn sample_eclipse(now: bool) -> Moon {
        let day = crate::data::util::days_from_civil(2025, 3, 14) as f64 * 86400.0;
        Moon::sample_at(if now { day + 7.0 * 3600.0 } else { day - 2.0 * 3600.0 })
    }

    /// A noite de agora (se a ilha mostra alguma).
    fn tonight(&self) -> Option<&Night> {
        let now = self.clock.now();
        self.nights.iter().find(|night| now >= night.show && now < night.hide && !self.ok.contains(&night.key))
    }

    /// O eclipse já começou?
    fn live(&self, night: &Night) -> Option<Eclipse> {
        night.eclipse.filter(|eclipse| self.clock.now() >= eclipse.start)
    }

    /// "Nasce às 19:12 a leste" (ou "Nasceu às ...", se já nasceu).
    fn rise_line(&self, night: &Night) -> String {
        match night.rise {
            Some((at, az)) if at > self.clock.now() => format!("Nasce às {} {}", self.clock.hhmm(at), toward(az)),
            Some((at, az)) => format!("Nasceu às {}, {}", self.clock.hhmm(at), toward(az)),
            None => "No céu esta noite".into(),
        }
    }
}

/// "Eclipse total da Lua" ou "Eclipse parcial da Lua".
fn eclipse_name(eclipse: &Eclipse) -> &'static str {
    match eclipse.kind {
        EclipseKind::Total => "Eclipse total da Lua",
        EclipseKind::Partial => "Eclipse parcial da Lua",
    }
}

/// A Lua cheia, com os mares (e, com `glow`, um brilho em volta); `covered` (0 a 1) é quanto
/// dela está na sombra da Terra (a sombra é avermelhada; inteira na sombra, a Lua fica cor de
/// cobre).
fn draw_moon(c: &mut Canvas, cx: f32, cy: f32, r: f32, covered: f32, glow: bool) {
    if glow {
        // três círculos cada vez maiores e mais fracos: (tamanho em raios, opacidade)
        for (size, alpha) in [(1.7, 7u8), (1.35, 13), (1.15, 20)] {
            c.fill_circle(cx, cy, r * size, Rgba(240, 232, 200, (alpha as f32 * (1.0 - covered * 0.7)) as u8));
        }
    }
    c.fill_circle(cx, cy, r, Rgba::rgb(238, 233, 216));
    for sea in SEAS {
        c.fill_circle(cx + sea.dx * r, cy + sea.dy * r, sea.size * r, Rgba(120, 110, 96, sea.alpha));
    }
    if covered > 0.0 {
        c.push_round_clip(cx - r, cy - r, cx + r, cy + r, r);
        if covered >= 0.999 {
            c.fill_circle(cx, cy, r, Rgba(150, 52, 28, 190));
        } else {
            let big = 2.7 * r;
            let dist = big + r - 2.0 * r * covered;
            c.fill_circle(cx - dist * 0.97, cy - dist * 0.24, big, Rgba(92, 28, 16, 225));
        }
        c.pop_clip();
    }
}

/// Um mar da Lua (uma mancha mais escura), em raios da Lua a partir do centro.
struct Sea {
    dx: f32,
    dy: f32,
    size: f32,
    alpha: u8,
}

const SEAS: [Sea; 5] = [
    Sea { dx: -0.32, dy: -0.22, size: 0.30, alpha: 36 },
    Sea { dx: 0.18, dy: -0.30, size: 0.22, alpha: 30 },
    Sea { dx: 0.05, dy: 0.25, size: 0.28, alpha: 32 },
    Sea { dx: -0.25, dy: 0.32, size: 0.16, alpha: 26 },
    Sea { dx: 0.38, dy: 0.12, size: 0.14, alpha: 24 },
];

/// As estrelas do fundo da ilha aberta (sempre as mesmas).
fn stars(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
    let mut seed = 7u32;
    for _ in 0..34 {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let sx = x + 14.0 + (seed >> 8) as f32 % (w - 28.0);
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let sy = y + 10.0 + (seed >> 8) as f32 % (h - 20.0);
        let alpha = 40 + (seed >> 4) % 90;
        c.fill_circle(sx, sy, 0.8, Rgba(255, 255, 255, alpha as u8));
    }
}

/// A linha do tempo do eclipse: na sombra (cobre claro), inteira na sombra (cobre) e agora.
fn draw_timeline(c: &mut Canvas, clock: &Clock, x0: f32, x1: f32, y: f32, eclipse: &Eclipse, now: f64) {
    let px = |t: f64| x0 + (x1 - x0) * ((t - eclipse.start) / (eclipse.end - eclipse.start)).clamp(0.0, 1.0) as f32;
    c.fill_round_rect(x0, y - 3.0, x1, y + 3.0, 3.0, COPPER.alpha(0.4));
    if let Some((total_from, total_to)) = eclipse.total {
        c.fill_round_rect(px(total_from), y - 3.0, px(total_to), y + 3.0, 3.0, COPPER);
    }
    c.fill_circle(px(eclipse.peak), y, 2.0, WHITE.alpha(0.7));
    if now >= eclipse.start && now <= eclipse.end {
        c.fill_circle(px(now), y, 6.0, WHITE.alpha(0.25));
        c.fill_circle(px(now), y, 3.5, WHITE);
    }
    c.text(x0, y + 10.0, &clock.hhmm(eclipse.start), Face::Regular, 10.5, MUTED, Anchor::LA, None);
    c.text(px(eclipse.peak), y + 10.0, &clock.hhmm(eclipse.peak), Face::Semibold, 10.5, FG, Anchor::MA, None);
    c.text(x1, y + 10.0, &clock.hhmm(eclipse.end), Face::Regular, 10.5, MUTED, Anchor::RA, None);
}

impl Kind for Moon {
    fn key(&self) -> &'static str {
        "lua"
    }
    fn title(&self) -> &'static str {
        super::title("lua")
    }
    fn draw(&self, _c: &mut Canvas, _mode: Mode, _env: &Env) {}

    fn next_tick(&self) -> Option<Instant> {
        Some(self.check_at)
    }

    /// Refaz as noites de lua cheia de tempos em tempos (e quando muda a cidade).
    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        self.check_at = Instant::now() + NIGHTS_CHECK;
        let Some(Place { lat, lon, .. }) = location(ctx.cfg) else {
            self.nights.clear();
            return false;
        };
        let now = self.clock.now();
        if self.for_loc != Some((lat, lon)) || now - self.computed > NIGHTS_EVERY {
            (self.nights, self.fulls) = nights(&self.clock, lat, lon, now);
            self.for_loc = Some((lat, lon));
            self.computed = now;
        }
        false
    }

    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        let night = self.tonight()?;
        Some(match (night.eclipse, self.live(night)) {
            (_, Some(_)) => IslandItem::live(COPPER),
            (Some(_), None) => IslandItem::alert(COPPER, alert_hash(&night.key)),
            (None, _) => IslandItem::alert(MOON, alert_hash(&night.key)),
        })
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        let Some(night) = self.tonight() else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let now = self.clock.now();
        let covered = self.live(night).map_or(0.0, |eclipse| eclipse.covered(now) as f32);
        let place = location(env.cfg).map(|place| place.name).unwrap_or_default();
        match form {
            IslandForm::Bubble => draw_moon(c, x + w / 2.0, cy, 11.0, covered, false),
            IslandForm::Compact | IslandForm::Banner => {
                draw_moon(c, x + 26.0, cy, 10.5, covered, false);
                let (label, value, color) = match (night.eclipse, self.live(night)) {
                    (_, Some(_)) if covered >= 0.999 => ("Eclipse: Lua na sombra".to_string(), "total".to_string(), COPPER),
                    (_, Some(_)) => ("Eclipse agora".to_string(), format!("{:.0}%", covered * 100.0), COPPER),
                    (Some(eclipse), None) if form == IslandForm::Banner => (format!("{} hoje", eclipse_name(&eclipse)), self.clock.hhmm(eclipse.peak), COPPER),
                    (Some(eclipse), None) => ("Eclipse da Lua".to_string(), self.clock.hhmm(eclipse.peak), COPPER),
                    (None, _) if form == IslandForm::Banner => match night.rise {
                        Some((rise, _)) if rise > now => ("Lua cheia nasce às".to_string(), self.clock.hhmm(rise), MOON),
                        _ => ("Lua cheia no céu esta noite".to_string(), String::new(), MOON),
                    },
                    (None, _) => match night.rise {
                        Some((rise, _)) if rise > now => ("Lua cheia".to_string(), self.clock.hhmm(rise), MOON),
                        _ => ("Lua cheia".to_string(), "no céu".to_string(), MOON),
                    },
                };
                iu::compact_text(c, x + 46.0, r, &label, &value, color);
            }
            IslandForm::Medium => {
                draw_moon(c, x + 39.0, cy, 19.0, covered, false);
                let (title, sub, color) = match (night.eclipse, self.live(night)) {
                    (_, Some(eclipse)) => {
                        let (_, az) = location(env.cfg).map(|place| astro::moon_alt_az(now, place.lat, place.lon)).unwrap_or((0.0, 0.0));
                        let title = if covered >= 0.999 { format!("Lua vermelha, {}", toward(az)) } else { format!("Eclipse: {:.0}% na sombra", covered * 100.0) };
                        (title, format!("Máximo {} · fim {}", self.clock.hhmm(eclipse.peak), self.clock.hhmm(eclipse.end)), COPPER)
                    }
                    (Some(eclipse), None) => (format!("{} hoje", eclipse_name(&eclipse)), format!("Começa {} · máximo {}", self.clock.hhmm(eclipse.start), self.clock.hhmm(eclipse.peak)), COPPER),
                    (None, _) => ("Lua cheia hoje".to_string(), self.rise_line(night), MOON),
                };
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &title, &sub, color);
                draw_buttons(c, form, r, hot, None);
            }
            IslandForm::Large => {
                let iu::FormSize { radius: lr, .. } = iu::size(IslandForm::Large);
                c.gradient_round_rect(x, y, x + w, y + h, lr, &[(0.0, Rgba::rgb(6, 10, 30)), (1.0, Rgba::rgb(26, 32, 72))], y, y + h);
                stars(c, x, y, w, h);
                // antes de o eclipse começar, a Lua como ela vai ficar no máximo
                let shown = match night.eclipse {
                    Some(eclipse) if self.live(night).is_none() => eclipse.covered(eclipse.peak) as f32,
                    _ => covered,
                };
                draw_moon(c, x + w - 70.0, y + 100.0, 40.0, shown, true);
                let color = if night.eclipse.is_some() { COPPER } else { MOON };
                iu::header(c, r, "LUA", color, &place, MUTED);
                let text_w = w - 44.0 - 112.0;
                match (night.eclipse, self.live(night)) {
                    (Some(eclipse), live) => {
                        let title = if live.is_some() { "Eclipse agora".to_string() } else { format!("{} hoje", eclipse_name(&eclipse)) };
                        c.text(x + 22.0, y + 58.0, &title, Face::Semibold, 18.0, FG, Anchor::LM, Some(text_w));
                        let sub = match eclipse.total {
                            Some((total_from, total_to)) => format!("Vermelha das {} às {}", self.clock.hhmm(total_from), self.clock.hhmm(total_to)),
                            None => format!("Até {:.0}% na sombra às {}", eclipse.covered(eclipse.peak) * 100.0, self.clock.hhmm(eclipse.peak)),
                        };
                        c.text(x + 22.0, y + 82.0, &sub, Face::Regular, 12.0, MUTED, Anchor::LM, Some(text_w));
                        draw_timeline(c, &self.clock, x + 22.0, x + 22.0 + text_w, y + 118.0, &eclipse, now);
                    }
                    (None, _) => {
                        c.text(x + 22.0, y + 58.0, "Lua cheia hoje", Face::Semibold, 20.0, FG, Anchor::LM, Some(text_w));
                        c.text(x + 22.0, y + 82.0, &self.rise_line(night), Face::Regular, 12.0, MUTED, Anchor::LM, Some(text_w));
                        if let Some(next) = self.fulls.iter().find(|&&full| full > night.hide + 86400.0) {
                            c.text(x + 22.0, y + 104.0, &format!("A próxima: {}", self.clock.date_text(*next)), Face::Regular, 12.0, MUTED, Anchor::LM, Some(text_w));
                        }
                    }
                }
                draw_buttons(c, form, r, hot, None);
            }
        }
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        hit(form, x, y, w, h, false)
    }

    /// Com o eclipse andando, a sombra anda devagar: um quadro a cada poucos segundos basta.
    fn island_fps(&self, _form: IslandForm) -> u32 {
        0
    }

    fn act(&mut self, action: u32, _ctx: &mut Ctx) {
        if action == OK {
            if let Some(key) = self.tonight().map(|night| night.key.clone()) {
                self.ok.push(key);
            }
        }
    }
}

// ---------- a Estação Espacial ----------

const ISS_BLUE: Rgba = Rgba::rgb(110, 200, 255);
/// o aviso aparece este tanto antes da passagem
const ISS_LEAD: f64 = 5.0 * 60.0;
const TLE_URL: &str = "https://celestrak.org/NORAD/elements/gp.php?CATNR=25544&FORMAT=tle";
/// a órbita muda um pouco a cada dia (a ISS freia no ar e sobe de vez em quando)
const TLE_MAX_AGE: f64 = 12.0 * 3600.0;
const PASSES_EVERY: f64 = 3.0 * 3600.0;
/// sem conexão: tenta de novo depois deste tempo
const RETRY: f64 = 15.0 * 60.0;
/// de quanto em quanto tempo conferir as passagens (e, calculando, se a conta acabou)
const STATION_CHECK: Duration = Duration::from_secs(60);
const JOB_POLL: Duration = Duration::from_millis(300);
/// as passagens calculadas cobrem as próximas 48 horas (a partir de 10 minutos atrás)
const PASSES_HOURS: f64 = 48.0;

/// A órbita da ISS: as duas linhas do TLE e quando foi baixada.
#[derive(Clone)]
struct Tle {
    line1: String,
    line2: String,
    fetched: f64,
}

/// O que a busca devolve: a órbita, as passagens, para onde e quando foram calculadas.
struct Job {
    tle: Option<Tle>,
    passes: Vec<Pass>,
    at: (f64, f64),
    computed: f64,
}

pub struct Station {
    clock: Clock,
    tle: Option<Tle>,
    passes: Vec<Pass>,
    for_loc: Option<(f64, f64)>,
    computed: f64,
    failed_at: f64,
    /// a previsão do Tempo (os dias contados a partir de `hours_day`)
    hours: Vec<HourCode>,
    hours_day: i64,
    pending: Option<Receiver<Job>>,
    check_at: Instant,
    ok: Vec<String>,
}

/// Baixa a órbita da ISS (o TLE da Celestrak).
fn fetch_tle(now: f64) -> Option<Tle> {
    let (status, text) = crate::data::util::fetch_text(TLE_URL, &[]).ok()?;
    if status >= 400 {
        return None;
    }
    let line1 = text.lines().find(|line| line.starts_with("1 "))?.trim().to_string();
    let line2 = text.lines().find(|line| line.starts_with("2 "))?.trim().to_string();
    Orbit::from_tle(&line1, &line2)?;
    Some(Tle { line1, line2, fetched: now })
}

impl Station {
    pub fn new(cache: &Value) -> Station {
        let saved = &cache["iss"];
        let tle = match (saved.str_of("l1"), saved.str_of("l2"), saved.f64_of("at")) {
            (Some(line1), Some(line2), Some(fetched)) => Some(Tle { line1: line1.to_string(), line2: line2.to_string(), fetched }),
            _ => None,
        };
        Station {
            clock: Clock::default(),
            tle,
            passes: Vec::new(),
            for_loc: None,
            computed: 0.0,
            failed_at: f64::MIN,
            hours: Vec::new(),
            hours_day: 0,
            pending: None,
            check_at: Instant::now(),
            ok: Vec::new(),
        }
    }

    /// Uma passagem de exemplo às 19:42 (de noroeste a sudeste, subindo até 62°): 3 minutos
    /// antes, ou no meio dela.
    #[cfg(test)]
    pub fn sample(live: bool) -> Station {
        let start = crate::data::util::days_from_civil(2026, 11, 23) as f64 * 86400.0 + 22.0 * 3600.0 + 42.0 * 60.0;
        let mut station = Station::new(&Value::Null);
        station.passes = vec![sample_pass(start)];
        station.clock = Clock::at(if live { start + 150.0 } else { start - 180.0 }, -3.0 * 3600.0);
        station
    }

    /// O céu limpo na hora `t`, pela previsão do Tempo (sem previsão: limpo; até o código 2,
    /// "parcialmente nublado", ainda conta).
    fn clear(&self, t: f64) -> bool {
        let day = self.clock.day(t) - self.hours_day;
        let hour = ((t + self.clock.offset()).rem_euclid(86400.0) / 3600.0) as i64;
        self.hours.iter().find(|forecast| forecast.day == day && forecast.hour == hour).is_none_or(|forecast| forecast.code <= 2)
    }

    /// A chave do OK de uma passagem (o minuto em que ela começa).
    fn pass_key(pass: &Pass) -> String {
        format!("iss|{}", (pass.start / 60.0).round() as i64)
    }

    /// A passagem que a ilha mostra agora: dos 5 minutos antes até ela sumir.
    fn current(&self) -> Option<&Pass> {
        let now = self.clock.now();
        self.passes.iter().find(|pass| now >= pass.start - ISS_LEAD && now <= pass.end && self.clear(pass.start) && !self.ok.contains(&Station::pass_key(pass)))
    }

    /// Ela já está passando (e não só chegando)?
    fn passing(&self, pass: &Pass) -> bool {
        self.clock.now() >= pass.start
    }
}

/// Uma passagem de exemplo: 6 minutos de noroeste a sudeste, subindo até 62°.
#[cfg(test)]
pub fn sample_pass(start: f64) -> Pass {
    use crate::data::astro::TrackPoint;
    let track: Vec<TrackPoint> = (0..=36)
        .map(|step| {
            let along = step as f64 / 36.0;
            TrackPoint { at: start + step as f64 * 10.0, alt: 10.0 + 52.0 * (along * std::f64::consts::PI).sin(), az: (300.0 + 180.0 * along).rem_euclid(360.0) }
        })
        .collect();
    Pass { start, end: start + 360.0, peak: start + 180.0, max_alt: 62.0, track }
}

/// A estação (os painéis solares e o módulo no meio), com `size` do centro à ponta do painel.
pub(super) fn draw_station(c: &mut Canvas, cx: f32, cy: f32, size: f32) {
    c.fill_round_rect(cx - size, cy - 0.3 * size, cx - 0.34 * size, cy + 0.3 * size, 0.06 * size, ISS_BLUE.alpha(0.9));
    c.fill_round_rect(cx + 0.34 * size, cy - 0.3 * size, cx + size, cy + 0.3 * size, 0.06 * size, ISS_BLUE.alpha(0.9));
    c.fill_round_rect(cx - 0.36 * size, cy - 0.05 * size, cx + 0.36 * size, cy + 0.05 * size, 0.04 * size, FG.alpha(0.75));
    c.fill_round_rect(cx - 0.15 * size, cy - 0.22 * size, cx + 0.15 * size, cy + 0.22 * size, 0.08 * size, FG);
}

/// O céu visto de cima (o norte para cima): o caminho da passagem, de onde ela aparece até onde
/// some, e onde está agora.
fn draw_dome(c: &mut Canvas, cx: f32, cy: f32, radius: f32, pass: &Pass, now: Option<f64>) {
    c.fill_circle(cx, cy, radius, Rgba(255, 255, 255, 12));
    c.stroke_circle(cx, cy, radius, Rgba(255, 255, 255, 46), 1.0);
    for ring in [1.0 / 3.0, 2.0 / 3.0] {
        c.stroke_circle(cx, cy, radius * ring, Rgba(255, 255, 255, 18), 1.0);
    }
    for (label, az) in [("N", 0.0f32), ("L", 90.0), ("S", 180.0), ("O", 270.0)] {
        let angle = az.to_radians();
        c.text(cx + (radius + 8.0) * angle.sin(), cy - (radius + 8.0) * angle.cos(), label, Face::Semibold, 9.5, MUTED, Anchor::MM, None);
    }
    // o ponto do céu na tela: no meio, o alto (90°); na borda, o horizonte
    let pt = |alt: f64, az: f64| {
        let from_center = radius * (1.0 - (alt / 90.0) as f32).clamp(0.0, 1.0);
        let angle = (az as f32).to_radians();
        (cx + from_center * angle.sin(), cy - from_center * angle.cos())
    };
    let pts: Vec<(f32, f32)> = pass.track.iter().map(|point| pt(point.alt, point.az)).collect();
    c.line(&pts, ISS_BLUE.alpha(0.6), 2.0);
    if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
        c.stroke_circle(first.0, first.1, 3.5, ISS_BLUE, 1.5);
        c.fill_circle(last.0, last.1, 2.5, ISS_BLUE.alpha(0.6));
    }
    if let Some(t) = now {
        let (alt, az) = pass.at(t);
        let (x, y) = pt(alt, az);
        c.fill_circle(x, y, 8.0, WHITE.alpha(0.18));
        c.fill_circle(x, y, 4.0, WHITE);
    }
}

impl Kind for Station {
    fn key(&self) -> &'static str {
        "iss"
    }
    fn title(&self) -> &'static str {
        super::title("iss")
    }
    fn draw(&self, _c: &mut Canvas, _mode: Mode, _env: &Env) {}

    fn next_tick(&self) -> Option<Instant> {
        Some(self.check_at)
    }

    /// Baixa a órbita (duas vezes por dia) e calcula as passagens visíveis das próximas 48 horas
    /// numa thread; lê a previsão do Tempo para saber se o céu vai estar limpo.
    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        let wall = Instant::now();
        self.check_at = wall + STATION_CHECK;
        let now = self.clock.now();
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(Job { tle, passes, at, computed }) => {
                    self.pending = None;
                    if tle.as_ref().map(|tle| tle.fetched) != self.tle.as_ref().map(|tle| tle.fetched) {
                        if let Some(Tle { line1, line2, fetched }) = &tle {
                            ctx.cache["iss"] = json!({"l1": line1, "l2": line2, "at": fetched});
                            ctx.out.push(Request::SaveCache);
                        }
                    }
                    if tle.is_none() {
                        self.failed_at = now;
                    }
                    self.tle = tle;
                    self.passes = passes;
                    self.for_loc = Some(at);
                    self.computed = computed;
                    return true;
                }
                Err(TryRecvError::Empty) => {
                    self.check_at = wall + JOB_POLL;
                    return false;
                }
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.failed_at = now;
                }
            }
        }
        if !on_island(ctx.cfg, "iss") {
            return false;
        }
        self.hours = cached_hours(ctx.cache);
        self.hours_day = self.clock.day(now);
        // as passagens que dá para ver (o "boa noite" avisa junto): (início, azimute, altura)
        let visible: Vec<Value> = self.passes.iter().filter(|pass| pass.end > now && self.clear(pass.start)).map(|pass| json!([pass.start, pass.start_az(), pass.max_alt])).collect();
        ctx.cache["iss_visiveis"] = json!(visible);
        let Some(Place { lat, lon, .. }) = location(ctx.cfg) else { return false };
        let old_tle = self.tle.as_ref().is_none_or(|tle| now - tle.fetched > TLE_MAX_AGE);
        let due = old_tle || self.for_loc != Some((lat, lon)) || now - self.computed > PASSES_EVERY;
        if due && now - self.failed_at > RETRY {
            let (tx, rx) = channel();
            let keep = self.tle.clone();
            std::thread::spawn(move || {
                let tle = if old_tle { fetch_tle(now).or(keep) } else { keep };
                let passes = tle
                    .as_ref()
                    .and_then(|tle| Orbit::from_tle(&tle.line1, &tle.line2))
                    .map(|orbit| astro::visible_passes(&orbit, lat, lon, now - 600.0, PASSES_HOURS))
                    .unwrap_or_default();
                let _ = tx.send(Job { tle, passes, at: (lat, lon), computed: now });
                crate::platform::events::wake();
            });
            self.pending = Some(rx);
            self.check_at = wall + JOB_POLL;
        }
        false
    }

    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        let pass = self.current()?;
        Some(if self.passing(pass) {
            // passando: vai para a pílula (são só uns minutos)
            IslandItem { hurry: true, ..IslandItem::live(ISS_BLUE) }
        } else {
            IslandItem::alert(ISS_BLUE, alert_hash(&Station::pass_key(pass)))
        })
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        let Some(pass) = self.current() else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let now = self.clock.now();
        let passing = self.passing(pass);
        let (alt, az) = pass.at(now);
        let (start, end) = (self.clock.hhmm(pass.start), self.clock.hhmm(pass.end));
        let from = astro::compass(pass.start_az());
        match form {
            IslandForm::Bubble => draw_station(c, x + w / 2.0, cy, 13.0),
            IslandForm::Compact | IslandForm::Banner => {
                draw_station(c, x + 26.0, cy, 12.0);
                if passing {
                    iu::compact_text(c, x + 46.0, r, &format!("ISS: olhe para o {}", astro::compass(az)), &format!("{alt:.0}°"), ISS_BLUE);
                } else if form == IslandForm::Banner {
                    iu::compact_text(c, x + 46.0, r, &format!("A ISS passa às {start}, olhe para o {from}"), "", ISS_BLUE);
                } else {
                    iu::compact_text(c, x + 46.0, r, "Estação Espacial", &start, ISS_BLUE);
                }
            }
            IslandForm::Medium => {
                draw_station(c, x + 39.0, cy, 20.0);
                let (title, sub) = if passing {
                    (format!("Olhe para o {}", astro::compass(az)), format!("{alt:.0}° de altura · some às {end}"))
                } else {
                    (format!("Estação Espacial às {start}"), format!("De {} para {}, até {:.0}°", from, astro::compass(pass.end_az()), pass.max_alt))
                };
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &title, &sub, ISS_BLUE);
                draw_buttons(c, form, r, hot, None);
            }
            IslandForm::Large => {
                let place = location(env.cfg).map(|place| place.name).unwrap_or_default();
                iu::header(c, r, "ESTAÇÃO ESPACIAL", ISS_BLUE, &place, MUTED);
                draw_dome(c, x + 86.0, y + 114.0, 58.0, pass, passing.then_some(now));
                let tx = x + 172.0;
                let tw = w - 172.0 - 20.0;
                let title = if passing { "Passando agora".to_string() } else { format!("Às {start}, em {}", minutes_until(pass.start - now)) };
                c.text(tx, y + 60.0, &title, Face::Semibold, 16.0, FG, Anchor::LM, Some(tw));
                let lines = [
                    format!("Aparece {start}, {}", toward(pass.start_az())),
                    format!("Mais alta {}, a {:.0}°", self.clock.hhmm(pass.peak), pass.max_alt),
                    format!("Some {end}, {}", toward(pass.end_az())),
                ];
                for (index, line) in lines.iter().enumerate() {
                    c.text(tx, y + 86.0 + 19.0 * index as f32, line, Face::Regular, 12.0, MUTED, Anchor::LM, Some(tw));
                }
                draw_buttons(c, form, r, hot, None);
            }
        }
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        hit(form, x, y, w, h, false)
    }

    /// Passando, o ponto anda no céu (e a altura muda): alguns quadros por segundo.
    fn island_fps(&self, _form: IslandForm) -> u32 {
        if self.current().is_some_and(|pass| self.passing(pass)) { 4 } else { 0 }
    }

    fn act(&mut self, action: u32, _ctx: &mut Ctx) {
        if action == OK {
            if let Some(key) = self.current().map(Station::pass_key) {
                self.ok.push(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Value {
        json!({"tempo": {"lat": -23.55, "lon": -46.63, "place": "São Paulo, Brasil"}, "app": {}})
    }

    fn env<'a>(cfg: &'a Value, key: &'static str) -> Env<'a> {
        Env { cfg, key }
    }

    /// O pôr do sol aparece 15 minutos antes, some quando o sol se põe e com o OK; sem a cidade
    /// do Tempo, nada.
    #[test]
    fn sunset_shows_in_the_last_15_minutes() {
        let cfg = cfg();
        let mut s = Sunset::sample();
        let set = s.sunset(-23.55, -46.63).unwrap();
        let off = -3.0 * 3600.0;
        // 18:3x em São Paulo
        let local = ((set + off).rem_euclid(86400.0)) / 3600.0;
        assert!((18.3..18.8).contains(&local), "pôr do sol às {local}");
        for (dt, shows) in [(-20.0 * 60.0, false), (-14.0 * 60.0, true), (-30.0, true), (5.0, false), (3600.0, false)] {
            s.clock = Clock::at(set + dt, off);
            assert_eq!(s.island_item(&env(&cfg, "sol")).is_some(), shows, "{dt} s do pôr do sol");
        }
        s.clock = Clock::at(set - 300.0, off);
        let item = s.island_item(&env(&cfg, "sol")).unwrap();
        assert!(item.alert);
        let mut out = Vec::new();
        let mut cfg2 = cfg.clone();
        let mut cache = json!({});
        s.act(OK, &mut Ctx { cfg: &mut cfg2, cache: &mut cache, key: "sol", out: &mut out });
        assert!(s.island_item(&env(&cfg, "sol")).is_none(), "o OK dispensa até amanhã");
        assert!(Sunset::sample().island_item(&env(&json!({"app": {}}), "sol")).is_none(), "sem cidade");
    }

    /// A noite de lua cheia: das 19 h (ou do pôr do sol) até a meia-noite, com a hora em que ela
    /// nasce; fora disso, nada.
    #[test]
    fn full_moon_night() {
        let cfg = cfg();
        let m = Moon::sample_full();
        let n = m.tonight().expect("noite de lua cheia");
        assert!(n.eclipse.is_none());
        let off = -3.0 * 3600.0;
        let show = ((n.show + off).rem_euclid(86400.0)) / 3600.0;
        assert!((19.0..19.5).contains(&show), "aparece às {show}");
        assert_eq!(((n.hide + off).rem_euclid(86400.0)) as i64, 0, "some à meia-noite");
        let (rise, az) = n.rise.unwrap();
        assert!((rise - n.show).abs() < 2.0 * 3600.0);
        assert!((45.0..135.0).contains(&az), "nasce a leste: {az}");
        assert!(m.island_item(&env(&cfg, "lua")).unwrap().alert);
        // uma semana depois, nada
        let mut later = Moon::sample_full();
        later.clock = Clock::at(m.clock.now() + 7.0 * 86400.0, off);
        assert!(later.tonight().is_none());
    }

    /// O eclipse de 14/03/2025 em São Paulo: aviso na noite anterior, ao vivo durante (com a Lua
    /// na sombra), e some no fim.
    #[test]
    fn lunar_eclipse_night() {
        let cfg = cfg();
        let before = Moon::sample_eclipse(false);
        let n = before.tonight().expect("noite do eclipse");
        let e = n.eclipse.expect("eclipse visível de São Paulo");
        assert_eq!(e.kind, EclipseKind::Total);
        assert!(before.island_item(&env(&cfg, "lua")).unwrap().alert);
        let during = Moon::sample_eclipse(true);
        let item = during.island_item(&env(&cfg, "lua")).unwrap();
        assert!(!item.alert, "durante: ao vivo");
        assert!(during.live(during.tonight().unwrap()).is_some());
        let mut after = Moon::sample_eclipse(true);
        after.clock = Clock::at(e.end + 60.0, -3.0 * 3600.0);
        assert!(after.tonight().is_none(), "some quando o eclipse acaba");
    }

    /// A ISS: aviso 5 minutos antes, na pílula enquanto passa, some no fim; com o céu fechado
    /// na hora, nada.
    #[test]
    fn station_pass_timing() {
        let cfg = cfg();
        let mut s = Station::sample(false);
        let p = s.passes[0].clone();
        let off = -3.0 * 3600.0;
        for (t, shows, alert) in [(p.start - 400.0, false, false), (p.start - 200.0, true, true), (p.start + 60.0, true, false), (p.end + 5.0, false, false)] {
            s.clock = Clock::at(t, off);
            let item = s.island_item(&env(&cfg, "iss"));
            assert_eq!(item.is_some(), shows, "{}", t - p.start);
            if let Some(item) = item {
                assert_eq!(item.alert, alert);
                assert_eq!(item.hurry, !alert, "passando, vai para a pílula");
            }
        }
        // céu encoberto às 19 h daquele dia
        s.clock = Clock::at(p.start - 200.0, off);
        s.hours_day = s.clock.day(p.start);
        s.hours = vec![HourCode { day: 0, hour: 19, code: 3 }];
        assert!(s.island_item(&env(&cfg, "iss")).is_none(), "nublado");
        s.hours = vec![HourCode { day: 0, hour: 19, code: 1 }];
        assert!(s.island_item(&env(&cfg, "iss")).is_some(), "limpo");
        assert_eq!(s.island_fps(IslandForm::Compact), 0);
        assert_eq!(Station::sample(true).island_fps(IslandForm::Large), 4);
    }

    #[test]
    fn directions_read_well() {
        assert_eq!(toward(0.0), "ao norte");
        assert_eq!(toward(90.0), "a leste");
        assert_eq!(toward(200.0), "ao sul");
        assert_eq!(toward(315.0), "a noroeste");
        assert_eq!(minutes_until(30.0), "1 min");
        assert_eq!(minutes_until(-5.0), "agora");
        assert_eq!(minutes_until(12.0 * 60.0), "12 min");
    }
}
