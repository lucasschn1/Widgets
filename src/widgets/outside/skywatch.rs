//! O céu na ilha: o pôr do sol chegando, a lua cheia (e os eclipses da Lua) e a Estação
//! Espacial passando. Tudo calculado para a cidade do widget Tempo (ver `crate::astro`).

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{cached_hours, cached_weather, draw_buttons, hit, location, Clock, OK};
use crate::astro::{self, Eclipse, EclipseKind, Orbit, Pass};
use crate::config::Get;
use crate::drawing::weather_art;
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::theme::{Rgba, FG, MUTED, WHITE};
use crate::widgets::island_ui as iu;
use crate::widgets::{alert_hash, on_island, sky, Ctx, Env, IslandForm, IslandItem, Kind, Mode, Rect4, Request};

/// "a leste", "ao norte"
pub(super) fn toward(az: f64) -> String {
    match astro::compass(az) {
        "norte" => "ao norte".into(),
        "sul" => "ao sul".into(),
        name => format!("a {name}"),
    }
}

/// "12 min", "1 min", "agora"
fn minutes_until(secs: f64) -> String {
    let m = (secs / 60.0).ceil() as i64;
    if m < 1 { "agora".into() } else { format!("{m} min") }
}

// ---------- o pôr do sol ----------

const SUN: Rgba = Rgba::rgb(255, 159, 67);
/// o aviso aparece este tanto antes do pôr do sol
const SUN_LEAD: f64 = 15.0 * 60.0;

pub struct Sunset {
    clock: Clock,
    /// o próximo pôr do sol já calculado: (lat, lon, calculado a partir de, horário)
    next: Cell<Option<(f64, f64, f64, f64)>>,
    /// o clima de agora (do Tempo), para o céu da ilha aberta
    weather: (i64, bool),
    check_at: Instant,
    ok: Vec<String>,
    sky: RefCell<Option<((u32, u32, i64, i64), Image)>>,
}

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
        let noon = crate::util::days_from_civil(2026, 11, 20) as f64 * 86400.0 + 15.0 * 3600.0;
        let set = astro::next_sunset(noon, -23.55, -46.63).unwrap();
        Sunset { clock: Clock::at(set - 600.0, -3.0 * 3600.0), ..Sunset::new() }
    }

    fn sunset(&self, lat: f64, lon: f64) -> Option<f64> {
        let now = self.clock.now();
        // vale enquanto o relógio está entre o cálculo e o pôr do sol (se o relógio do PC voltar,
        // calcula de novo)
        if let Some((a, b, from, s)) = self.next.get() {
            if a == lat && b == lon && from <= now && s >= now {
                return Some(s);
            }
        }
        let s = astro::next_sunset(now, lat, lon)?;
        self.next.set(Some((lat, lon, now, s)));
        Some(s)
    }

    /// O aviso de agora: dos 15 minutos antes até o sol se pôr.
    fn alert(&self, cfg: &Value) -> Option<SunAlert> {
        let (lat, lon, place) = location(cfg)?;
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
    fn sky_at(&self, c: &Canvas, at: f64) -> std::cell::Ref<'_, Option<((u32, u32, i64, i64), Image)>> {
        let (w, h, r) = iu::size(IslandForm::Large);
        let local = |t: f64| ((t + self.clock.offset()).rem_euclid(86400.0) / 3600.0) as f32;
        let key = ((w * c.s).round() as u32, (h * c.s).round() as u32, self.weather.0, (at / 60.0) as i64);
        if self.sky.borrow().as_ref().is_none_or(|(k, _)| *k != key) {
            let mut sc = Canvas::new(key.0, key.1, c.s);
            let clock = sky::Clock::new(local(at - 4.0 * 60.0), 6.0, local(at));
            sky::draw_back(&mut sc, w, h, r, sky::Weather::of(self.weather.0, self.weather.1), &clock);
            *self.sky.borrow_mut() = Some((key, sc.finish()));
        }
        self.sky.borrow()
    }
}

/// O sol descendo até a linha do horizonte (`p`: 0 = 15 minutos antes, 1 = no pôr do sol).
fn setting_sun(c: &mut Canvas, cx: f32, cy: f32, size: f32, p: f32) {
    let img = weather_art("sun", (size * c.s).round() as u32);
    let horizon = cy + size * 0.24;
    c.push_round_clip(cx - size, cy - size, cx + size, horizon, 0.0);
    c.image(&img, cx - size / 2.0, cy - size / 2.0 + size * 0.28 * p.clamp(0.0, 1.0));
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
        self.check_at = Instant::now() + Duration::from_secs(10 * 60);
        if let Some(w) = cached_weather(ctx.cache) {
            self.weather = w;
        }
        false
    }

    fn island_item(&self, env: &Env) -> Option<IslandItem> {
        let a = self.alert(env.cfg)?;
        Some(IslandItem::alert(SUN, alert_hash(&a.key)))
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        let Some(a) = self.alert(env.cfg) else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let left = a.at - self.clock.now();
        let p = (1.0 - left / SUN_LEAD) as f32;
        let time = self.clock.hhmm(a.at);
        match form {
            IslandForm::Bubble => setting_sun(c, x + w / 2.0, cy, 30.0, p),
            IslandForm::Compact | IslandForm::Banner => {
                setting_sun(c, x + 26.0, cy, 28.0, p);
                if form == IslandForm::Banner {
                    iu::compact_text(c, x + 46.0, r, &format!("Pôr do sol às {time}"), &minutes_until(left), SUN);
                } else {
                    iu::compact_text(c, x + 46.0, r, "Pôr do sol", &time, SUN);
                }
            }
            IslandForm::Medium => {
                setting_sun(c, x + 39.0, cy, 48.0, p);
                let sub = format!("Em {}, {}", minutes_until(left), toward(a.az));
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &format!("Pôr do sol às {time}"), &sub, SUN);
                draw_buttons(c, form, r, hot, None);
            }
            IslandForm::Large => {
                {
                    let sky = self.sky_at(c, a.at);
                    if let Some((_, img)) = sky.as_ref() {
                        c.image_px(img, (x * c.s).round(), (y * c.s).round(), img.w as f32, img.h as f32, 1.0);
                    }
                }
                iu::header(c, r, "PÔR DO SOL", SUN, &a.place, MUTED);
                c.text(x + 22.0, y + 58.0, &format!("O sol se põe às {time}"), Face::Semibold, 20.0, FG, Anchor::LM, Some(w - 44.0));
                let sub = format!("Em {}. Olhe para o {}.", minutes_until(left), astro::compass(a.az));
                c.text(x + 22.0, y + 82.0, &sub, Face::Regular, 12.0, MUTED, Anchor::LM, Some(w - 44.0));
                setting_sun(c, x + 46.0, y + 140.0, 56.0, p);
                draw_buttons(c, form, r, hot, None);
            }
        }
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        hit(form, x, y, w, h, false)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        if action == OK {
            if let Some(a) = self.alert(ctx.cfg) {
                self.ok.push(a.key);
            }
        }
    }
}

// ---------- a lua cheia e os eclipses ----------

const MOON: Rgba = Rgba::rgb(236, 228, 200);
const COPPER: Rgba = Rgba::rgb(232, 112, 72);

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
fn visible(e: &Eclipse, lat: f64, lon: f64) -> bool {
    let mut t = e.start;
    while t <= e.end {
        if astro::moon_alt_az(t, lat, lon).0 > astro::MOON_RISE + 2.0 && astro::sun_alt_az(t, lat, lon).0 < -6.0 {
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
        let eclipse = astro::lunar_eclipse(full).filter(|e| visible(e, lat, lon));
        let (day, show, hide, key) = match eclipse {
            Some(e) => {
                // a noite em que ele acontece (de madrugada: a noite anterior)
                let day = clock.day(e.start - 12.0 * 3600.0);
                (day, evening(day).min(e.start - 1800.0), e.end, format!("eclipse|{day}"))
            }
            None => {
                // a noite mais perto da lua cheia
                let day = ((full + clock.offset()) / 86400.0 - 21.0 / 24.0).round() as i64;
                (day, evening(day), clock.midnight(day + 1), format!("lua|{day}"))
            }
        };
        let rise = astro::next_moonrise(clock.midnight(day) + 12.0 * 3600.0, lat, lon).map(|t| (t, astro::moon_alt_az(t, lat, lon).1));
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
        let at = crate::util::days_from_civil(2026, 11, 24) as f64 * 86400.0 + 23.0 * 3600.0;
        Moon::sample_at(at)
    }

    /// O eclipse total de 14/03/2025, visto de São Paulo: na noite anterior, ou no máximo.
    #[cfg(test)]
    pub fn sample_eclipse(now: bool) -> Moon {
        let day = crate::util::days_from_civil(2025, 3, 14) as f64 * 86400.0;
        Moon::sample_at(if now { day + 7.0 * 3600.0 } else { day - 2.0 * 3600.0 })
    }

    /// A noite de agora (se a ilha mostra alguma).
    fn tonight(&self) -> Option<&Night> {
        let now = self.clock.now();
        self.nights.iter().find(|n| now >= n.show && now < n.hide && !self.ok.contains(&n.key))
    }

    /// O eclipse já começou?
    fn live(&self, n: &Night) -> Option<Eclipse> {
        n.eclipse.filter(|e| self.clock.now() >= e.start)
    }

    fn rise_line(&self, n: &Night) -> String {
        match n.rise {
            Some((t, az)) if t > self.clock.now() => format!("Nasce às {} {}", self.clock.hhmm(t), toward(az)),
            Some((t, az)) => format!("Nasceu às {}, {}", self.clock.hhmm(t), toward(az)),
            None => "No céu esta noite".into(),
        }
    }
}

fn eclipse_name(e: &Eclipse) -> &'static str {
    match e.kind {
        EclipseKind::Total => "Eclipse total da Lua",
        EclipseKind::Partial => "Eclipse parcial da Lua",
    }
}

/// A Lua cheia, com os mares (e, com `glow`, um brilho em volta); `covered` (0 a 1) é quanto
/// dela está na sombra da Terra (a sombra é avermelhada; inteira na sombra, a Lua fica cor de
/// cobre).
fn draw_moon(c: &mut Canvas, cx: f32, cy: f32, r: f32, covered: f32, glow: bool) {
    if glow {
        for (k, a) in [(1.7, 7u8), (1.35, 13), (1.15, 20)] {
            c.fill_circle(cx, cy, r * k, Rgba(240, 232, 200, (a as f32 * (1.0 - covered * 0.7)) as u8));
        }
    }
    c.fill_circle(cx, cy, r, Rgba::rgb(238, 233, 216));
    for (dx, dy, k, a) in [(-0.32, -0.22, 0.30, 36u8), (0.18, -0.30, 0.22, 30), (0.05, 0.25, 0.28, 32), (-0.25, 0.32, 0.16, 26), (0.38, 0.12, 0.14, 24)] {
        c.fill_circle(cx + dx * r, cy + dy * r, k * r, Rgba(120, 110, 96, a));
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

/// As estrelas do fundo da ilha aberta (sempre as mesmas).
fn stars(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
    let mut seed = 7u32;
    for _ in 0..34 {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let sx = x + 14.0 + (seed >> 8) as f32 % (w - 28.0);
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let sy = y + 10.0 + (seed >> 8) as f32 % (h - 20.0);
        let a = 40 + (seed >> 4) % 90;
        c.fill_circle(sx, sy, 0.8, Rgba(255, 255, 255, a as u8));
    }
}

/// A linha do tempo do eclipse: na sombra (cobre claro), inteira na sombra (cobre) e agora.
fn draw_timeline(c: &mut Canvas, clock: &Clock, x0: f32, x1: f32, y: f32, e: &Eclipse, now: f64) {
    let px = |t: f64| x0 + (x1 - x0) * ((t - e.start) / (e.end - e.start)).clamp(0.0, 1.0) as f32;
    c.fill_round_rect(x0, y - 3.0, x1, y + 3.0, 3.0, COPPER.alpha(0.4));
    if let Some((a, b)) = e.total {
        c.fill_round_rect(px(a), y - 3.0, px(b), y + 3.0, 3.0, COPPER);
    }
    c.fill_circle(px(e.peak), y, 2.0, WHITE.alpha(0.7));
    if now >= e.start && now <= e.end {
        c.fill_circle(px(now), y, 6.0, WHITE.alpha(0.25));
        c.fill_circle(px(now), y, 3.5, WHITE);
    }
    c.text(x0, y + 10.0, &clock.hhmm(e.start), Face::Regular, 10.5, MUTED, Anchor::LA, None);
    c.text(px(e.peak), y + 10.0, &clock.hhmm(e.peak), Face::Semibold, 10.5, FG, Anchor::MA, None);
    c.text(x1, y + 10.0, &clock.hhmm(e.end), Face::Regular, 10.5, MUTED, Anchor::RA, None);
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
        self.check_at = Instant::now() + Duration::from_secs(30 * 60);
        let Some((lat, lon, _)) = location(ctx.cfg) else {
            self.nights.clear();
            return false;
        };
        let now = self.clock.now();
        if self.for_loc != Some((lat, lon)) || now - self.computed > 6.0 * 3600.0 {
            (self.nights, self.fulls) = nights(&self.clock, lat, lon, now);
            self.for_loc = Some((lat, lon));
            self.computed = now;
        }
        false
    }

    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        let n = self.tonight()?;
        Some(match (n.eclipse, self.live(n)) {
            (_, Some(_)) => IslandItem::live(COPPER),
            (Some(_), None) => IslandItem::alert(COPPER, alert_hash(&n.key)),
            (None, _) => IslandItem::alert(MOON, alert_hash(&n.key)),
        })
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        let Some(n) = self.tonight() else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let now = self.clock.now();
        let covered = self.live(n).map_or(0.0, |e| e.covered(now) as f32);
        let place = location(env.cfg).map(|l| l.2).unwrap_or_default();
        match form {
            IslandForm::Bubble => draw_moon(c, x + w / 2.0, cy, 11.0, covered, false),
            IslandForm::Compact | IslandForm::Banner => {
                draw_moon(c, x + 26.0, cy, 10.5, covered, false);
                let (label, value, color) = match (n.eclipse, self.live(n)) {
                    (_, Some(_)) if covered >= 0.999 => ("Eclipse: Lua na sombra".to_string(), "total".to_string(), COPPER),
                    (_, Some(_)) => ("Eclipse agora".to_string(), format!("{:.0}%", covered * 100.0), COPPER),
                    (Some(e), None) if form == IslandForm::Banner => (format!("{} hoje", eclipse_name(&e)), self.clock.hhmm(e.peak), COPPER),
                    (Some(e), None) => ("Eclipse da Lua".to_string(), self.clock.hhmm(e.peak), COPPER),
                    (None, _) if form == IslandForm::Banner => match n.rise {
                        Some((t, _)) if t > now => ("Lua cheia nasce às".to_string(), self.clock.hhmm(t), MOON),
                        _ => ("Lua cheia no céu esta noite".to_string(), String::new(), MOON),
                    },
                    (None, _) => match n.rise {
                        Some((t, _)) if t > now => ("Lua cheia".to_string(), self.clock.hhmm(t), MOON),
                        _ => ("Lua cheia".to_string(), "no céu".to_string(), MOON),
                    },
                };
                iu::compact_text(c, x + 46.0, r, &label, &value, color);
            }
            IslandForm::Medium => {
                draw_moon(c, x + 39.0, cy, 19.0, covered, false);
                let (title, sub, color) = match (n.eclipse, self.live(n)) {
                    (_, Some(e)) => {
                        let (_, az) = location(env.cfg).map(|l| astro::moon_alt_az(now, l.0, l.1)).unwrap_or((0.0, 0.0));
                        let title = if covered >= 0.999 { format!("Lua vermelha, {}", toward(az)) } else { format!("Eclipse: {:.0}% na sombra", covered * 100.0) };
                        (title, format!("Máximo {} · fim {}", self.clock.hhmm(e.peak), self.clock.hhmm(e.end)), COPPER)
                    }
                    (Some(e), None) => (format!("{} hoje", eclipse_name(&e)), format!("Começa {} · máximo {}", self.clock.hhmm(e.start), self.clock.hhmm(e.peak)), COPPER),
                    (None, _) => ("Lua cheia hoje".to_string(), self.rise_line(n), MOON),
                };
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &title, &sub, color);
                draw_buttons(c, form, r, hot, None);
            }
            IslandForm::Large => {
                let (_, _, lr) = iu::size(IslandForm::Large);
                c.gradient_round_rect(x, y, x + w, y + h, lr, &[(0.0, Rgba::rgb(6, 10, 30)), (1.0, Rgba::rgb(26, 32, 72))], y, y + h);
                stars(c, x, y, w, h);
                let shown = match n.eclipse {
                    Some(e) if self.live(n).is_none() => e.covered(e.peak) as f32,
                    _ => covered,
                };
                draw_moon(c, x + w - 70.0, y + 100.0, 40.0, shown, true);
                let color = if n.eclipse.is_some() { COPPER } else { MOON };
                iu::header(c, r, "LUA", color, &place, MUTED);
                let text_w = w - 44.0 - 112.0;
                match (n.eclipse, self.live(n)) {
                    (Some(e), live) => {
                        let title = if live.is_some() { "Eclipse agora".to_string() } else { format!("{} hoje", eclipse_name(&e)) };
                        c.text(x + 22.0, y + 58.0, &title, Face::Semibold, 18.0, FG, Anchor::LM, Some(text_w));
                        let sub = match e.total {
                            Some((a, b)) => format!("Vermelha das {} às {}", self.clock.hhmm(a), self.clock.hhmm(b)),
                            None => format!("Até {:.0}% na sombra às {}", e.covered(e.peak) * 100.0, self.clock.hhmm(e.peak)),
                        };
                        c.text(x + 22.0, y + 82.0, &sub, Face::Regular, 12.0, MUTED, Anchor::LM, Some(text_w));
                        draw_timeline(c, &self.clock, x + 22.0, x + 22.0 + text_w, y + 118.0, &e, now);
                    }
                    (None, _) => {
                        c.text(x + 22.0, y + 58.0, "Lua cheia hoje", Face::Semibold, 20.0, FG, Anchor::LM, Some(text_w));
                        c.text(x + 22.0, y + 82.0, &self.rise_line(n), Face::Regular, 12.0, MUTED, Anchor::LM, Some(text_w));
                        if let Some(next) = self.fulls.iter().find(|&&f| f > n.hide + 86400.0) {
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
            if let Some(key) = self.tonight().map(|n| n.key.clone()) {
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

/// O que a busca devolve: a órbita (linha 1, linha 2, quando foi baixada), as passagens, para
/// onde e quando foram calculadas.
type Job = (Option<(String, String, f64)>, Vec<Pass>, (f64, f64), f64);

pub struct Station {
    clock: Clock,
    tle: Option<(String, String, f64)>,
    passes: Vec<Pass>,
    for_loc: Option<(f64, f64)>,
    computed: f64,
    failed_at: f64,
    /// a previsão do Tempo: (dia a partir de `hours_day`, hora, código do tempo)
    hours: Vec<(i64, i64, i64)>,
    hours_day: i64,
    pending: Option<Receiver<Job>>,
    check_at: Instant,
    ok: Vec<String>,
}

/// Baixa a órbita da ISS (o TLE da Celestrak).
fn fetch_tle(now: f64) -> Option<(String, String, f64)> {
    let (status, text) = crate::util::fetch_text(TLE_URL, &[]).ok()?;
    if status >= 400 {
        return None;
    }
    let l1 = text.lines().find(|l| l.starts_with("1 "))?.trim().to_string();
    let l2 = text.lines().find(|l| l.starts_with("2 "))?.trim().to_string();
    Orbit::from_tle(&l1, &l2)?;
    Some((l1, l2, now))
}

impl Station {
    pub fn new(cache: &Value) -> Station {
        let c = &cache["iss"];
        let tle = match (c.str_of("l1"), c.str_of("l2"), c.f64_of("at")) {
            (Some(a), Some(b), Some(at)) => Some((a.to_string(), b.to_string(), at)),
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
        let start = crate::util::days_from_civil(2026, 11, 23) as f64 * 86400.0 + 22.0 * 3600.0 + 42.0 * 60.0;
        let mut s = Station::new(&Value::Null);
        s.passes = vec![sample_pass(start)];
        s.clock = Clock::at(if live { start + 150.0 } else { start - 180.0 }, -3.0 * 3600.0);
        s
    }

    /// O céu limpo na hora `t`, pela previsão do Tempo (sem previsão: limpo).
    fn clear(&self, t: f64) -> bool {
        let d = self.clock.day(t) - self.hours_day;
        let h = ((t + self.clock.offset()).rem_euclid(86400.0) / 3600.0) as i64;
        self.hours.iter().find(|x| x.0 == d && x.1 == h).is_none_or(|x| x.2 <= 2)
    }

    fn pass_key(p: &Pass) -> String {
        format!("iss|{}", (p.start / 60.0).round() as i64)
    }

    /// A passagem que a ilha mostra agora: dos 5 minutos antes até ela sumir.
    fn current(&self) -> Option<&Pass> {
        let now = self.clock.now();
        self.passes.iter().find(|p| now >= p.start - ISS_LEAD && now <= p.end && self.clear(p.start) && !self.ok.contains(&Station::pass_key(p)))
    }

    fn passing(&self, p: &Pass) -> bool {
        self.clock.now() >= p.start
    }
}

#[cfg(test)]
pub fn sample_pass(start: f64) -> Pass {
    let track: Vec<(f64, f64, f64)> = (0..=36)
        .map(|k| {
            let f = k as f64 / 36.0;
            (start + k as f64 * 10.0, 10.0 + 52.0 * (f * std::f64::consts::PI).sin(), (300.0 + 180.0 * f).rem_euclid(360.0))
        })
        .collect();
    Pass { start, end: start + 360.0, peak: start + 180.0, max_alt: 62.0, track }
}

/// A estação (os painéis solares e o módulo no meio).
pub(super) fn draw_station(c: &mut Canvas, cx: f32, cy: f32, s: f32) {
    c.fill_round_rect(cx - s, cy - 0.3 * s, cx - 0.34 * s, cy + 0.3 * s, 0.06 * s, ISS_BLUE.alpha(0.9));
    c.fill_round_rect(cx + 0.34 * s, cy - 0.3 * s, cx + s, cy + 0.3 * s, 0.06 * s, ISS_BLUE.alpha(0.9));
    c.fill_round_rect(cx - 0.36 * s, cy - 0.05 * s, cx + 0.36 * s, cy + 0.05 * s, 0.04 * s, FG.alpha(0.75));
    c.fill_round_rect(cx - 0.15 * s, cy - 0.22 * s, cx + 0.15 * s, cy + 0.22 * s, 0.08 * s, FG);
}

/// O céu visto de cima (o norte para cima): o caminho da passagem, de onde ela aparece até onde
/// some, e onde está agora.
fn draw_dome(c: &mut Canvas, cx: f32, cy: f32, rr: f32, p: &Pass, now: Option<f64>) {
    c.fill_circle(cx, cy, rr, Rgba(255, 255, 255, 12));
    c.stroke_circle(cx, cy, rr, Rgba(255, 255, 255, 46), 1.0);
    for k in [1.0 / 3.0, 2.0 / 3.0] {
        c.stroke_circle(cx, cy, rr * k, Rgba(255, 255, 255, 18), 1.0);
    }
    for (label, az) in [("N", 0.0f32), ("L", 90.0), ("S", 180.0), ("O", 270.0)] {
        let a = az.to_radians();
        c.text(cx + (rr + 8.0) * a.sin(), cy - (rr + 8.0) * a.cos(), label, Face::Semibold, 9.5, MUTED, Anchor::MM, None);
    }
    let pt = |alt: f64, az: f64| {
        let d = rr * (1.0 - (alt / 90.0) as f32).clamp(0.0, 1.0);
        let a = (az as f32).to_radians();
        (cx + d * a.sin(), cy - d * a.cos())
    };
    let pts: Vec<(f32, f32)> = p.track.iter().map(|&(_, alt, az)| pt(alt, az)).collect();
    c.line(&pts, ISS_BLUE.alpha(0.6), 2.0);
    if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
        c.stroke_circle(a.0, a.1, 3.5, ISS_BLUE, 1.5);
        c.fill_circle(b.0, b.1, 2.5, ISS_BLUE.alpha(0.6));
    }
    if let Some(t) = now {
        let (alt, az) = p.at(t);
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
        self.check_at = wall + Duration::from_secs(60);
        let now = self.clock.now();
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok((tle, passes, at, computed)) => {
                    self.pending = None;
                    if tle.as_ref().map(|t| t.2) != self.tle.as_ref().map(|t| t.2) {
                        if let Some((l1, l2, fetched)) = &tle {
                            ctx.cache["iss"] = json!({"l1": l1, "l2": l2, "at": fetched});
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
                    self.check_at = wall + Duration::from_millis(300);
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
        let visible: Vec<Value> = self.passes.iter().filter(|p| p.end > now && self.clear(p.start)).map(|p| json!([p.start, p.start_az(), p.max_alt])).collect();
        ctx.cache["iss_visiveis"] = json!(visible);
        let Some((lat, lon, _)) = location(ctx.cfg) else { return false };
        let old_tle = self.tle.as_ref().is_none_or(|t| now - t.2 > TLE_MAX_AGE);
        let due = old_tle || self.for_loc != Some((lat, lon)) || now - self.computed > PASSES_EVERY;
        if due && now - self.failed_at > RETRY {
            let (tx, rx) = channel();
            let keep = self.tle.clone();
            std::thread::spawn(move || {
                let tle = if old_tle { fetch_tle(now).or(keep) } else { keep };
                let passes = tle.as_ref().and_then(|t| Orbit::from_tle(&t.0, &t.1)).map(|o| astro::visible_passes(&o, lat, lon, now - 600.0, 48.0)).unwrap_or_default();
                let _ = tx.send((tle, passes, (lat, lon), now));
                crate::events::wake();
            });
            self.pending = Some(rx);
            self.check_at = wall + Duration::from_millis(300);
        }
        false
    }

    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        let p = self.current()?;
        Some(if self.passing(p) {
            // passando: vai para a pílula (são só uns minutos)
            IslandItem { hurry: true, ..IslandItem::live(ISS_BLUE) }
        } else {
            IslandItem::alert(ISS_BLUE, alert_hash(&Station::pass_key(p)))
        })
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        let Some(p) = self.current() else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let now = self.clock.now();
        let passing = self.passing(p);
        let (alt, az) = p.at(now);
        let (start, end) = (self.clock.hhmm(p.start), self.clock.hhmm(p.end));
        let from = astro::compass(p.start_az());
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
                    (format!("Estação Espacial às {start}"), format!("De {} para {}, até {:.0}°", from, astro::compass(p.end_az()), p.max_alt))
                };
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &title, &sub, ISS_BLUE);
                draw_buttons(c, form, r, hot, None);
            }
            IslandForm::Large => {
                let place = location(env.cfg).map(|l| l.2).unwrap_or_default();
                iu::header(c, r, "ESTAÇÃO ESPACIAL", ISS_BLUE, &place, MUTED);
                draw_dome(c, x + 86.0, y + 114.0, 58.0, p, passing.then_some(now));
                let tx = x + 172.0;
                let tw = w - 172.0 - 20.0;
                let title = if passing { "Passando agora".to_string() } else { format!("Às {start}, em {}", minutes_until(p.start - now)) };
                c.text(tx, y + 60.0, &title, Face::Semibold, 16.0, FG, Anchor::LM, Some(tw));
                let lines = [
                    format!("Aparece {start}, {}", toward(p.start_az())),
                    format!("Mais alta {}, a {:.0}°", self.clock.hhmm(p.peak), p.max_alt),
                    format!("Some {end}, {}", toward(p.end_az())),
                ];
                for (k, line) in lines.iter().enumerate() {
                    c.text(tx, y + 86.0 + 19.0 * k as f32, line, Face::Regular, 12.0, MUTED, Anchor::LM, Some(tw));
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
        if self.current().is_some_and(|p| self.passing(p)) { 4 } else { 0 }
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
        s.hours = vec![(0, 19, 3)];
        assert!(s.island_item(&env(&cfg, "iss")).is_none(), "nublado");
        s.hours = vec![(0, 19, 1)];
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
