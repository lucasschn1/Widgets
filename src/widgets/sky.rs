//! O céu do widget de Tempo, como no app Tempo do iPhone e do Mac.
//!
//! - A camada de baixo, desenhada com o resto do widget (e refeita a cada minuto): as cores do céu
//!   pela hora do dia (do nascer ao pôr do sol da cidade) e pelo clima, a luz do sol ou da lua
//!   andando pelo céu, as estrelas, as nuvens (paradas) e a neblina.
//! - A camada de cima, só nos climas em que algo cai ou sopra (30 quadros por segundo, por cima
//!   do texto): garoa, chuva com respingos, neve, granizo quicando, rajadas de vento e, na
//!   trovoada, os raios, o clarão e o tremor do trovão.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::f32::consts::PI;
use std::time::{Duration, Instant};

use crate::gfx::{Canvas, Fill, Image};
use crate::theme::{Rgba, WHITE};

/// O clima, nos mesmos grupos do app Tempo da Apple.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weather {
    Clear,
    Partly,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    Heavy,
    Storm,
    Snow,
    Hail,
    Wind,
}

impl Weather {
    #[cfg(test)]
    pub const ALL: [Weather; 11] = [
        Weather::Clear,
        Weather::Partly,
        Weather::Cloudy,
        Weather::Fog,
        Weather::Drizzle,
        Weather::Rain,
        Weather::Heavy,
        Weather::Storm,
        Weather::Snow,
        Weather::Hail,
        Weather::Wind,
    ];

    /// Do código de tempo da Open-Meteo (WMO). `windy`: ventando forte agora (a previsão não tem
    /// um código para ventania; só conta com o céu sem chuva).
    pub fn of(code: i64, windy: bool) -> Weather {
        match code {
            0..=3 if windy => Weather::Wind,
            0 | 1 => Weather::Clear,
            2 => Weather::Partly,
            45 | 48 => Weather::Fog,
            51..=57 => Weather::Drizzle,
            65 | 67 | 82 => Weather::Heavy,
            61..=67 | 80..=82 => Weather::Rain,
            71..=77 | 85 | 86 => Weather::Snow,
            96 | 99 => Weather::Hail,
            c if c >= 95 => Weather::Storm,
            _ => Weather::Cloudy,
        }
    }

    /// Raios e trovões (a trovoada, com ou sem granizo).
    pub fn lightning(self) -> bool {
        matches!(self, Weather::Storm | Weather::Hail)
    }

    /// Algo cai ou sopra por cima: a camada animada.
    pub fn moves(self) -> bool {
        !matches!(self, Weather::Clear | Weather::Partly | Weather::Cloudy | Weather::Fog)
    }

    fn look(self) -> Look {
        let look = |cloud, sky, dark, layers| Look { cloud, sky, dark, layers };
        match self {
            Weather::Clear => look(0.0, [0; 4], 1.0, &[]),
            Weather::Partly => look(0.15, [0x4a80c4, 0x8db6e0, 0x141d36, 0x2c385a], 1.0, &FEW),
            Weather::Cloudy => look(0.94, [0x6b7d92, 0x9eabb9, 0x1d2330, 0x363d4d], 1.0, &OVERCAST),
            Weather::Fog => look(0.96, [0x88919b, 0xb0b7be, 0x262b33, 0x40464f], 1.0, &[]),
            Weather::Drizzle => look(0.94, [0x5f7185, 0x909eae, 0x1a202b, 0x323947], 0.9, &DRIZZLE),
            Weather::Rain => look(0.96, [0x4b5b6e, 0x78889b, 0x141a25, 0x2c3443], 0.78, &RAIN),
            Weather::Heavy => look(0.98, [0x3a4757, 0x5f6d7f, 0x10141c, 0x252c38], 0.65, &HEAVY),
            Weather::Storm => look(1.0, [0x2a2e40, 0x4d5166, 0x12121e, 0x2c2c40], 0.55, &HEAVY),
            Weather::Snow => look(0.95, [0x7d91a8, 0xaebdcc, 0x222c3b, 0x435064], 0.95, &SNOW),
            Weather::Hail => look(0.97, [0x4f5f72, 0x7d8c9e, 0x151b26, 0x2d3544], 0.7, &RAIN),
            Weather::Wind => look(0.3, [0x4f7bb0, 0x8aaed3, 0x141c33, 0x2b3655], 1.0, &WINDY),
        }
    }
}

/// Como um clima pinta o céu.
struct Look {
    /// o quanto as cores do clima tomam o lugar das do céu limpo (0 a 1)
    cloud: f32,
    /// as cores do clima: dia (topo, base) e noite (topo, base)
    sky: [u32; 4],
    /// o quanto as nuvens escurecem (1 = claras)
    dark: f32,
    layers: &'static [Layer],
}

/// Uma camada de nuvens: quantas (num cartão de 320 de largura), o tamanho, a opacidade, a faixa
/// de altura em que ficam (frações da parte de cima do cartão) e o quanto são mais escuras.
struct Layer {
    n: f32,
    min: usize,
    size: (f32, f32),
    alpha: f32,
    y: (f32, f32),
    dark: f32,
}

#[allow(clippy::too_many_arguments)]
const fn layer(n: f32, min: usize, s0: f32, s1: f32, alpha: f32, y0: f32, y1: f32, dark: f32) -> Layer {
    Layer { n, min, size: (s0, s1), alpha, y: (y0, y1), dark }
}

const FEW: [Layer; 1] = [layer(2.4, 1, 0.5, 0.8, 0.92, 0.05, 0.4, 0.0)];
/// nublado: três camadas, as de trás maiores e mais escuras
const OVERCAST: [Layer; 3] = [layer(4.0, 2, 1.3, 1.8, 0.75, -0.2, 0.2, 0.35), layer(4.0, 2, 0.9, 1.3, 0.8, 0.0, 0.45, 0.15), layer(3.0, 1, 0.55, 0.85, 0.85, 0.15, 0.7, 0.0)];
const DRIZZLE: [Layer; 2] = [layer(4.0, 2, 1.2, 1.7, 0.7, -0.2, 0.2, 0.3), layer(3.0, 1, 0.8, 1.2, 0.7, 0.0, 0.4, 0.1)];
const RAIN: [Layer; 2] = [layer(5.0, 2, 1.3, 1.8, 0.8, -0.25, 0.15, 0.4), layer(4.0, 2, 0.9, 1.3, 0.75, -0.05, 0.35, 0.2)];
const HEAVY: [Layer; 2] = [layer(5.0, 3, 1.4, 2.0, 0.85, -0.3, 0.15, 0.45), layer(4.0, 2, 1.0, 1.4, 0.8, -0.1, 0.35, 0.25)];
const SNOW: [Layer; 1] = [layer(4.0, 2, 1.3, 1.8, 0.55, -0.25, 0.12, 0.2)];
const WINDY: [Layer; 1] = [layer(3.0, 1, 0.45, 0.7, 0.9, 0.05, 0.45, 0.0)];

fn hex(v: u32) -> Rgba {
    Rgba::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Números pseudoaleatórios repetíveis (as mesmas nuvens, estrelas e gotas a cada quadro).
struct Rnd(u32);

impl Rnd {
    fn f(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

// ---------- a hora do dia ----------

/// A hora na cidade e o nascer e o pôr do sol de hoje, em horas (de 0 a 24).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub now: f32,
    pub rise: f32,
    pub set: f32,
}

/// As cores-chave do céu limpo: (âncora, horas a partir dela, topo, base). Âncoras: 0 = meia-noite,
/// 1 = nascer do sol, 2 = meio do dia, 3 = pôr do sol, 4 = a meia-noite seguinte.
const KEYS: [(u8, f32, u32, u32); 12] = [
    (0, 0.0, 0x0a1430, 0x1b2a55),
    (1, -1.6, 0x0c1736, 0x22305e),
    (1, -0.7, 0x2b3670, 0x9a6f9c), // aurora
    (1, 0.0, 0x4566b0, 0xf0a07c),  // nascer do sol
    (1, 1.1, 0x3a80d4, 0xa9cdef),
    (2, 0.0, 0x2675d6, 0x66a8eb), // meio-dia
    (3, -2.4, 0x357ccd, 0x8bb9e6),
    (3, -0.9, 0x4a6ab6, 0xf2b56e), // tarde dourada
    (3, 0.0, 0x38408c, 0xf27a52),  // pôr do sol
    (3, 0.7, 0x222a62, 0x8d4d7c),  // crepúsculo
    (3, 1.5, 0x111a40, 0x2b3566),
    (4, 0.0, 0x0a1430, 0x1b2a55),
];

impl Clock {
    /// `now`, `rise` e `set` em horas; o nascer e o pôr do sol ficam em horários possíveis
    /// (perto dos polos, a previsão pode não ter um ou outro).
    pub fn new(now: f32, rise: f32, set: f32) -> Clock {
        let rise = rise.clamp(2.0, 11.5);
        Clock { now: now.rem_euclid(24.0), rise, set: set.clamp(12.5, 22.5).max(rise + 1.0) }
    }

    /// Quanto é dia: 0 de noite, 1 de dia, suave no nascer e no pôr do sol.
    pub fn daylight(&self) -> f32 {
        smooth(self.rise - 0.7, self.rise + 0.6, self.now) * (1.0 - smooth(self.set - 0.4, self.set + 0.9, self.now))
    }

    /// O céu limpo agora: (topo, base).
    pub fn clear_sky(&self) -> (Rgba, Rgba) {
        let mut prev = 0.0f32;
        let hours: Vec<f32> = KEYS
            .iter()
            .map(|&(anchor, off, _, _)| {
                let base = match anchor {
                    0 => 0.0,
                    1 => self.rise,
                    2 => (self.rise + self.set) / 2.0,
                    3 => self.set,
                    _ => 24.0,
                };
                prev = (base + off).clamp(prev, 24.0);
                prev
            })
            .collect();
        for i in 0..KEYS.len() - 1 {
            let (h0, h1) = (hours[i], hours[i + 1]);
            if self.now >= h0 && self.now <= h1 {
                let t = if h1 > h0 { smooth(0.0, 1.0, (self.now - h0) / (h1 - h0)) } else { 1.0 };
                let (a, b) = (KEYS[i], KEYS[i + 1]);
                return (hex(a.2).mix(hex(b.2), t), hex(a.3).mix(hex(b.3), t));
            }
        }
        (hex(KEYS[0].2), hex(KEYS[0].3))
    }
}

/// As cores do céu (topo, base) neste clima e nesta hora.
pub fn colors(weather: Weather, clock: &Clock) -> (Rgba, Rgba) {
    let (top, bottom) = clock.clear_sky();
    let look = weather.look();
    if look.cloud <= 0.0 {
        return (top, bottom);
    }
    let d = clock.daylight();
    let [dt, db, nt, nb] = look.sky.map(hex);
    (top.mix(nt.mix(dt, d), look.cloud), bottom.mix(nb.mix(db, d), look.cloud))
}

// ---------- a camada de baixo ----------

/// O céu inteiro atrás do conteúdo: cores, luz do sol ou da lua, estrelas, nuvens e neblina.
pub fn draw_back(c: &mut Canvas, w: f32, h: f32, r: f32, weather: Weather, clock: &Clock) {
    let (top, bottom) = colors(weather, clock);
    c.gradient_round_rect(0.0, 0.0, w, h, r, &[(0.0, top.alpha(0.94)), (1.0, bottom.alpha(0.94))], 0.0, h);
    let look = weather.look();
    c.push_round_clip(0.0, 0.0, w, h, r);
    lights(c, w, h, look.cloud, clock);
    stars(c, w, h, look.cloud, clock.daylight());
    clouds(c, w, h, weather, &look, clock);
    if weather == Weather::Fog {
        fog(c, w, h, r, clock.daylight());
    }
    c.pop_clip();
}

/// A luz do sol (e, de noite, a da lua) andando pelo céu: sai da esquerda, fica no alto no meio
/// do dia e desce à direita, mais laranja perto do horizonte. Some com o céu fechado.
fn lights(c: &mut Canvas, w: f32, h: f32, cloud: f32, clock: &Clock) {
    let clear = (1.0 - cloud).powf(1.5);
    if clear < 0.01 {
        return;
    }
    let light = |c: &mut Canvas, f: f32, col: Rgba, amount: f32, rad: f32| {
        if amount <= 0.005 {
            return;
        }
        let x = -w * 0.08 + w * 1.16 * f;
        let y = -12.0 + (1.0 - (PI * f.clamp(0.0, 1.0)).sin()) * h.min(170.0) * 0.55;
        c.fill_radial(x, y, rad, rad, &[(0.0, col.alpha(amount)), (0.35, col.alpha(amount * 0.45)), (1.0, col.alpha(0.0))]);
    };
    let up = (clock.now - clock.rise) / (clock.set - clock.rise);
    if (-0.06..1.06).contains(&up) {
        let horizon = 1.0 - (PI * up.clamp(0.0, 1.0)).sin();
        let col = Rgba::rgb(255, 220, 140).mix(Rgba::rgb(255, 140, 70), smooth(0.45, 0.95, horizon));
        light(c, up, col, (0.42 + 0.2 * horizon) * clear * smooth(-0.06, 0.02, up) * (1.0 - smooth(0.98, 1.06, up)), 190.0);
    }
    let night = 24.0 - (clock.set - clock.rise);
    let moon = (clock.now - clock.set).rem_euclid(24.0) / night;
    if moon > 0.0 && moon < 1.0 {
        light(c, moon, Rgba::rgb(200, 212, 255), 0.22 * clear * smooth(0.0, 0.08, moon) * (1.0 - smooth(0.92, 1.0, moon)), 140.0);
    }
}

/// Estrelas: aparecem no crepúsculo e somem na aurora; com nuvens, ficam escondidas.
fn stars(c: &mut Canvas, w: f32, h: f32, cloud: f32, day: f32) {
    let amount = (1.0 - day) * (1.0 - cloud) * smooth(0.15, 0.6, 1.0 - day);
    if amount < 0.01 {
        return;
    }
    let zone = h.min(160.0);
    let mut rnd = Rnd(7);
    for _ in 0..(w * zone / 900.0) as usize {
        let (x, y, r, a) = (rnd.f() * w, rnd.f() * zone * 0.9, 0.4 + rnd.f() * 0.8, 0.25 + rnd.f() * 0.5);
        c.fill_circle(x, y, r, WHITE.alpha(a * amount));
    }
}

/// Nuvens com volume, paradas, em camadas (as de trás maiores e mais escuras). Cada uma é mais
/// clara em cima e mais escura embaixo, e a base pega a cor do céu que ela esconde (dourada no
/// fim da tarde, rosada no nascer, azul-escura de noite).
fn clouds(c: &mut Canvas, w: f32, h: f32, weather: Weather, look: &Look, clock: &Clock) {
    if look.layers.is_empty() {
        return;
    }
    let day = clock.daylight();
    let (clear_top, clear_bottom) = clock.clear_sky();
    let shade = |col: Rgba, k: f32| Rgba::rgb((col.0 as f32 * k) as u8, (col.1 as f32 * k) as u8, (col.2 as f32 * k) as u8);
    let top = shade(Rgba::rgb(80, 88, 110).mix(Rgba::rgb(210, 215, 226), day), look.dark).mix(clear_top, 0.1);
    let bottom = shade(Rgba::rgb(40, 46, 62).mix(Rgba::rgb(132, 142, 160), day), look.dark).mix(clear_bottom, if weather.lightning() { 0.09 } else { 0.3 });
    let zone = h.min(230.0);
    let mut rnd = Rnd(3);
    for l in look.layers {
        let n = ((l.n * w / 320.0 * if h > 200.0 { 1.4 } else { 1.0 }).round() as usize).max(l.min);
        let (lt, lb) = (top.mix(bottom, l.dark), bottom.mix(Rgba::rgb(0, 0, 0), l.dark * 0.6));
        for _ in 0..n {
            let v = (rnd.f() * 6.0) as usize % 6;
            let k = l.size.0 + rnd.f() * (l.size.1 - l.size.0);
            let (cw, ch) = (180.0 * k, 100.0 * k);
            let x = rnd.f() * (w + cw * 1.2) - cw * 0.6;
            let y = (l.y.0 + (l.y.1 - l.y.0) * rnd.f()) * zone - ch * 0.35;
            let img = cloud_sprite(v, lt, lb, k * c.s);
            c.image_px(&img, ((x - cw * 0.1) * c.s).round(), (y * c.s).round(), img.w as f32, img.h as f32, l.alpha);
        }
    }
}

/// As bolhas de uma nuvem (180 × 100): (x, y, raio x, raio y, é a base achatada).
fn puffs(v: usize) -> Vec<(f32, f32, f32, f32, bool)> {
    let mut seed: u64 = 11 + v as u64 * 97;
    let mut r = || {
        seed = seed * 16807 % 2_147_483_647;
        seed as f32 / 2_147_483_647.0
    };
    let mut out = vec![(90.0, 74.0, 78.0, 20.0, true)];
    let k = 4 + (r() * 3.0) as usize;
    for i in 0..k {
        let x = 30.0 + 120.0 * (i as f32 + 0.5) / k as f32 + (r() - 0.5) * 14.0;
        let rad = 18.0 + r() * 16.0 + (1.0 - (i as f32 - (k as f32 - 1.0) / 2.0).abs() / k as f32) * 14.0;
        out.push((x, 74.0 - rad * 0.55, rad, rad, false));
    }
    out
}

thread_local! {
    /// nuvens prontas (desenhar e desfocar custa; elas só mudam de cor devagar)
    static SPRITES: RefCell<HashMap<(usize, u32, u32, u32), Image>> = RefCell::new(HashMap::new());
}

/// Uma nuvem desfocada nas bordas (216 × 120 unidades com a margem do desfoque) na escala `scale`
/// (pixels por unidade), com o degradê de `top` a `bottom`.
fn cloud_sprite(v: usize, top: Rgba, bottom: Rgba, scale: f32) -> Image {
    let q = |c: Rgba| ((c.0 / 6) as u32) << 16 | ((c.1 / 6) as u32) << 8 | (c.2 / 6) as u32;
    let key = (v, q(top), q(bottom), (scale * 100.0).round() as u32);
    if let Some(img) = SPRITES.with(|m| m.borrow().get(&key).cloned()) {
        return img;
    }
    let mut c = Canvas::new((216.0 * scale).ceil() as u32, (120.0 * scale).ceil() as u32, scale);
    let f = Fill::VGrad(24.0, 106.0, top, bottom);
    for (x, y, rx, ry, base) in puffs(v) {
        let (x, y) = (x + 18.0, y + 10.0);
        if base {
            c.fill_round_rect_with(x - rx, y - ry, x + rx, y + ry, ry, f);
        } else {
            c.fill_circle_with(x, y, rx, f);
        }
    }
    let mut img = c.finish();
    let (iw, ih) = (img.w, img.h);
    crate::imgops::blur(&mut img.px, iw, ih, 4, 2.2 * scale);
    SPRITES.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 64 {
            m.clear();
        }
        m.insert(key, img.clone());
    });
    img
}

/// Neblina: um véu mais forte embaixo e faixas de névoa paradas.
fn fog(c: &mut Canvas, w: f32, h: f32, r: f32, day: f32) {
    let col = Rgba::rgb(60, 66, 76).mix(Rgba::rgb(225, 229, 234), day);
    c.gradient_round_rect(0.0, 0.0, w, h, r, &[(0.0, col.alpha(0.05)), (1.0, col.alpha(0.28))], 0.0, h);
    let mut rnd = Rnd(41);
    let bands = (4.0 + h / 60.0).round() as usize;
    for i in 0..bands {
        let cx = rnd.f() * w;
        let cy = (0.15 + 0.85 * (i as f32 + rnd.f() * 0.6) / bands as f32) * h;
        let (rx, ry) = (w * (0.5 + rnd.f() * 0.5), 10.0 + rnd.f() * 14.0);
        c.fill_radial(cx, cy, rx, ry, &[(0.0, col.alpha(0.2)), (1.0, col.alpha(0.0))]);
    }
}

// ---------- os raios ----------

/// Os raios da trovoada: o próximo vem de 3 a 8 s depois do anterior, num lugar diferente.
pub struct Storm {
    next: Cell<Option<Instant>>,
    strikes: RefCell<Vec<Strike>>,
    seed: Cell<u32>,
}

/// Um raio: quando caiu, quando chega o trovão, o traço (em unidades do cartão) e os galhos.
struct Strike {
    at: Instant,
    thunder: Instant,
    wobble: f32,
    main: Vec<(f32, f32)>,
    branches: Vec<Vec<(f32, f32)>>,
}

impl Default for Storm {
    fn default() -> Storm {
        Storm { next: Cell::new(None), strikes: RefCell::new(Vec::new()), seed: Cell::new(0x9e37_79b9) }
    }
}

/// Milissegundos de `at` até `now`.
fn ms(now: Instant, at: Instant) -> f32 {
    now.saturating_duration_since(at).as_secs_f32() * 1000.0
}

/// O clarão `t` ms depois do raio (0 a 1): três piscadas que somem.
fn flash_at(t: f32) -> f32 {
    let pulse = |c: f32, w: f32, a: f32| a * (-((t - c) / w).powi(2)).exp();
    let tail = if t > 0.0 && t < 600.0 { 0.25 * (-t / 260.0).exp() } else { 0.0 };
    (pulse(0.0, 35.0, 1.0) + pulse(110.0, 30.0, 0.7) + pulse(230.0, 45.0, 0.5) + tail).clamp(0.0, 1.0)
}

impl Storm {
    fn rnd(&self) -> f32 {
        let s = self.seed.get().wrapping_mul(1_103_515_245).wrapping_add(12345);
        self.seed.set(s);
        (s >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Um raio agora, num cartão w × h: um traço que se divide ao meio várias vezes (cada metade
    /// desviando um pouco), com alguns galhos.
    pub fn strike(&self, w: f32, h: f32, now: Instant) {
        let x = (0.15 + self.rnd() * 0.7) * w;
        let y1 = (h * (0.55 + self.rnd() * 0.35)).min(240.0);
        let spread = w.min(200.0) * 0.35;
        let mut pts = vec![(x, -4.0), (x + (self.rnd() - 0.5) * spread, y1)];
        for k in 0..5 {
            let mut next = Vec::with_capacity(pts.len() * 2);
            for i in 0..pts.len() - 1 {
                let (a, b) = (pts[i], pts[i + 1]);
                next.push(a);
                next.push(((a.0 + b.0) / 2.0 + (self.rnd() - 0.5) * spread / (k as f32 + 1.3), (a.1 + b.1) / 2.0 + (self.rnd() - 0.5) * 4.0));
            }
            next.push(pts[pts.len() - 1]);
            pts = next;
        }
        let mut branches = Vec::new();
        let mut i = 4;
        while i + 6 < pts.len() {
            if self.rnd() >= 0.55 {
                let (bx, by) = pts[i];
                let dir = if self.rnd() < 0.5 { -1.0 } else { 1.0 };
                let len = 18.0 + self.rnd() * 26.0;
                let mut branch = vec![(bx, by)];
                for j in 1..=5 {
                    let f = j as f32 / 5.0;
                    branch.push((bx + dir * len * f + (self.rnd() - 0.5) * 5.0, by + len * f * 0.9 + (self.rnd() - 0.5) * 3.0));
                }
                branches.push(branch);
            }
            i += 5 + (self.rnd() * 5.0) as usize;
        }
        // o trovão chega depois da luz
        let thunder = now + Duration::from_millis(700 + (self.rnd() * 900.0) as u64);
        let wobble = self.rnd();
        let mut list = self.strikes.borrow_mut();
        list.retain(|s| ms(now, s.at) < 4000.0);
        list.push(Strike { at: now, thunder, wobble, main: pts, branches });
    }

    /// A cada quadro da trovoada: cai o próximo raio quando chega a hora.
    fn tick(&self, w: f32, h: f32, now: Instant) {
        match self.next.get() {
            None => self.next.set(Some(now + Duration::from_millis(1500))),
            Some(t) if now >= t => {
                self.strike(w, h, now);
                self.next.set(Some(now + Duration::from_millis(3000 + (self.rnd() * 5000.0) as u64)));
            }
            _ => {}
        }
    }

    /// O clarão agora (0 a 1).
    pub fn flash(&self, now: Instant) -> f32 {
        self.strikes.borrow().iter().map(|s| flash_at(ms(now, s.at))).fold(0.0, f32::max)
    }

    /// O tremor do trovão agora (deslocamento em pontos; some em menos de um segundo).
    pub fn shake(&self, now: Instant) -> (f32, f32) {
        let (mut x, mut y) = (0.0, 0.0);
        for s in self.strikes.borrow().iter().filter(|s| now >= s.thunder) {
            let t = ms(now, s.thunder);
            if t > 800.0 {
                continue;
            }
            let a = 1.8 * (-t / 220.0).exp();
            x += (t * 0.09 + s.wobble * 9.0).sin() * a;
            y += (t * 0.13 + s.wobble * 5.0).cos() * a * 0.6;
        }
        (x, y)
    }
}

// ---------- a camada de cima ----------

/// Onde desenhar: o canto do cartão, o tamanho e os segundos desde que a animação começou.
struct Pane {
    ox: f32,
    oy: f32,
    w: f32,
    h: f32,
    t: f32,
}

/// O que cai ou sopra por cima do cartão (w × h, canto em ox, oy), e os raios. `t`: segundos
/// desde que a animação começou (as gotas são calculadas por ele: sem estado).
#[allow(clippy::too_many_arguments)]
pub fn draw_front(c: &mut Canvas, ox: f32, oy: f32, w: f32, h: f32, r: f32, weather: Weather, t: f32, storm: &Storm) {
    if !weather.moves() {
        return;
    }
    let now = Instant::now();
    let p = Pane { ox, oy, w, h, t };
    c.push_round_clip(ox, oy, ox + w, oy + h, r);
    if weather.lightning() {
        storm.tick(w, h, now);
        bolts(c, &p, r, storm, now);
    }
    let area = w * h;
    let mut rnd = Rnd(99);
    match weather {
        Weather::Drizzle => streaks(c, &p, &mut rnd, area / 650.0, (80.0, 130.0), (3.0, 6.0), 0.08, 0.9, 0.34),
        Weather::Rain => {
            // duas profundidades: as gotas de trás finas e apagadas, as da frente longas e claras
            streaks(c, &p, &mut rnd, area / 1300.0, (150.0, 210.0), (5.0, 9.0), 0.15, 0.7, 0.2);
            streaks(c, &p, &mut rnd, area / 1500.0, (240.0, 320.0), (11.0, 19.0), 0.18, 1.3, 0.38);
            splashes(c, &p, &mut rnd, w / 12.0, 0.55);
        }
        Weather::Heavy | Weather::Storm => {
            streaks(c, &p, &mut rnd, area / 1000.0, (200.0, 260.0), (6.0, 11.0), 0.26, 0.7, 0.24);
            streaks(c, &p, &mut rnd, area / 1100.0, (300.0, 400.0), (14.0, 24.0), 0.3, 1.4, 0.42);
            splashes(c, &p, &mut rnd, w / 6.0, 0.6);
        }
        Weather::Snow => snow(c, &p, &mut rnd),
        Weather::Hail => {
            streaks(c, &p, &mut rnd, area / 2400.0, (220.0, 300.0), (8.0, 14.0), 0.12, 0.8, 0.22);
            hail(c, &p, &mut rnd);
        }
        Weather::Wind => wind(c, &p, &mut rnd),
        _ => {}
    }
    c.pop_clip();
}

/// O clarão no cartão inteiro e os raios (com um brilho em volta, que pisca junto).
fn bolts(c: &mut Canvas, p: &Pane, r: f32, storm: &Storm, now: Instant) {
    let flash = storm.flash(now);
    if flash > 0.01 {
        c.fill_round_rect(p.ox, p.oy, p.ox + p.w, p.oy + p.h, r, Rgba(215, 215, 255, (flash * 0.32 * 255.0) as u8));
    }
    let col = |r: u8, g: u8, b: u8, a: f32| Rgba(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8);
    for s in storm.strikes.borrow().iter() {
        let age = ms(now, s.at);
        let a = (flash_at(age) * 1.4).min(1.0);
        if age > 320.0 || a < 0.03 {
            continue;
        }
        let shift = |pts: &[(f32, f32)]| pts.iter().map(|&(x, y)| (p.ox + x, p.oy + y)).collect::<Vec<_>>();
        let main = shift(&s.main);
        c.line(&main, col(190, 190, 255, a * 0.12), 9.0);
        c.line(&main, col(200, 200, 255, a * 0.35), 4.0);
        c.line(&main, col(255, 255, 255, a), 1.5);
        for b in &s.branches {
            let b = shift(b);
            c.line(&b, col(200, 200, 255, a * 0.15), 3.5);
            c.line(&b, col(235, 235, 255, a * 0.8), 0.9);
        }
    }
}

/// `n` gotas em traços retos: velocidade `v` (pt/s), comprimento `l`, inclinação `slant`.
#[allow(clippy::too_many_arguments)]
fn streaks(c: &mut Canvas, p: &Pane, rnd: &mut Rnd, n: f32, v: (f32, f32), l: (f32, f32), slant: f32, width: f32, alpha: f32) {
    let count = (n.round() as usize).max(4);
    let mut segs = Vec::with_capacity(count);
    for _ in 0..count {
        let (x0, y0, sp, ln) = (rnd.f(), rnd.f(), rnd.f(), rnd.f());
        let speed = v.0 + sp * (v.1 - v.0);
        let len = l.0 + ln * (l.1 - l.0);
        let y = (y0 * (p.h + len) + p.t * speed) % (p.h + len) - len;
        let x = (x0 * (p.w + 40.0) - p.t * speed * slant).rem_euclid(p.w + 40.0) - 20.0;
        segs.push((p.ox + x, p.oy + y, p.ox + x - len * slant, p.oy + y + len));
    }
    c.segments(&segs, Rgba(205, 222, 255, (alpha * 255.0) as u8), width);
}

/// Respingos no pé do cartão: um arquinho que abre e some.
fn splashes(c: &mut Canvas, p: &Pane, rnd: &mut Rnd, n: f32, alpha: f32) {
    for _ in 0..n.round() as usize {
        let (x0, ph, per, yy) = (rnd.f(), rnd.f(), 0.5 + rnd.f() * 0.6, rnd.f());
        let u = ((p.t + ph * per) % per) / per;
        if u > 0.45 {
            continue;
        }
        let k = u / 0.45;
        let (cx, cy, rx, ry) = (p.ox + x0 * p.w, p.oy + p.h - 3.0 - yy * 8.0, 1.0 + k * 5.0, 0.5 + k * 1.4);
        let pts: Vec<(f32, f32)> = (0..=6)
            .map(|i| {
                let a = PI + PI * i as f32 / 6.0;
                (cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect();
        c.line(&pts, Rgba(215, 228, 255, (alpha * (1.0 - k) * 255.0) as u8), 0.8);
    }
}

/// Neve: flocos pequenos ao fundo e grandes e macios na frente, balançando enquanto caem.
fn snow(c: &mut Canvas, p: &Pane, rnd: &mut Rnd) {
    let area = p.w * p.h;
    for (n, r0, r1, v0, v1, a, soft) in [(area / 650.0, 0.7, 1.3, 12.0, 22.0, 0.6, false), (area / 2600.0, 1.8, 3.0, 26.0, 40.0, 0.85, true)] {
        for _ in 0..n.round() as usize {
            let (x0, y0, sp, rr, ph) = (rnd.f(), rnd.f(), rnd.f(), rnd.f(), rnd.f());
            let v = v0 + sp * (v1 - v0);
            let rad = r0 + rr * (r1 - r0);
            let y = (y0 * (p.h + 10.0) + p.t * v) % (p.h + 10.0) - 5.0;
            let sway = (p.t * 0.9 + ph * 6.28).sin() * if soft { 8.0 } else { 4.0 };
            let x = (x0 * p.w + sway + p.t * 3.0).rem_euclid(p.w);
            let (x, y) = (p.ox + x, p.oy + y);
            if soft {
                c.fill_radial(x, y, rad, rad, &[(0.0, WHITE.alpha(a)), (0.6, WHITE.alpha(a * 0.6)), (1.0, WHITE.alpha(0.0))]);
            } else {
                c.fill_circle(x, y, rad, WHITE.alpha(a));
            }
        }
    }
}

/// Granizo: pedrinhas que caem rápido (com um rastro) e quicam no pé do cartão.
fn hail(c: &mut Canvas, p: &Pane, rnd: &mut Rnd) {
    for _ in 0..(p.w * p.h / 1100.0).round() as usize {
        let (x0, ph, sp, rr) = (rnd.f(), rnd.f(), rnd.f(), rnd.f());
        let side = if rnd.f() < 0.5 { -1.0 } else { 1.0 };
        let v = 240.0 + sp * 90.0;
        let fall = (p.h + 6.0) / v;
        let bounce = 0.34;
        let per = fall + bounce + 0.15;
        let u = (p.t + ph * per) % per;
        let rad = 1.3 + rr;
        let mut x = x0 * p.w;
        let (y, a) = if u < fall {
            let y = -6.0 + u * v;
            c.line(&[(p.ox + x, p.oy + y - 9.0), (p.ox + x, p.oy + y)], Rgba(230, 240, 255, 64), rad);
            (y, 0.9)
        } else if u < fall + bounce {
            let b = (u - fall) / bounce;
            x += b * 7.0 * side;
            (p.h - 4.0 - 64.0 * b * (1.0 - b) * (1.0 - b * 0.4), 0.9 * (1.0 - b * 0.6))
        } else {
            continue;
        };
        c.fill_circle(p.ox + x, p.oy + y, rad, Rgba(242, 248, 255, (a * 255.0) as u8));
    }
}

/// Ventania: rajadas em linhas finas e onduladas que atravessam o cartão, mais fortes no meio.
fn wind(c: &mut Canvas, p: &Pane, rnd: &mut Rnd) {
    let zone = p.h.min(230.0);
    for _ in 0..(p.w * p.h / 6000.0).round() as usize + 3 {
        let (x0, y0, sp, ln, ph) = (rnd.f(), rnd.f(), rnd.f(), rnd.f(), rnd.f());
        let v = 130.0 + sp * 90.0;
        let len = 45.0 + ln * 55.0;
        let span = p.w + len + 80.0;
        let x = (x0 * span + p.t * v) % span - len - 40.0;
        let y = 14.0 + y0 * (zone - 28.0);
        let wav = (p.t * 2.2 + ph * 6.28).sin() * 3.0;
        let ctrl = [(x, y), (x + len * 0.33, y - 4.0 + wav), (x + len * 0.66, y + 4.0 - wav), (x + len, y + wav * 0.5)];
        let at = |f: f32| {
            let g = 1.0 - f;
            let k = [g * g * g, 3.0 * g * g * f, 3.0 * g * f * f, f * f * f];
            (0..4).fold((p.ox, p.oy), |acc, i| (acc.0 + ctrl[i].0 * k[i], acc.1 + ctrl[i].1 * k[i]))
        };
        const PIECES: usize = 6;
        for i in 0..PIECES {
            let (f0, f1) = (i as f32 / PIECES as f32, (i + 1) as f32 / PIECES as f32);
            let mid = (f0 + f1) / 2.0;
            let a = if mid < 0.6 { 0.4 * mid / 0.6 } else { 0.4 * (1.0 - mid) / 0.4 };
            c.line(&[at(f0), at(f1)], WHITE.alpha(a), 1.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist(a: Rgba, b: Rgba) -> f32 {
        ((a.0 as f32 - b.0 as f32).powi(2) + (a.1 as f32 - b.1 as f32).powi(2) + (a.2 as f32 - b.2 as f32).powi(2)).sqrt()
    }

    #[test]
    fn codes_become_the_apple_groups() {
        assert_eq!(Weather::of(0, false), Weather::Clear);
        assert_eq!(Weather::of(2, false), Weather::Partly);
        assert_eq!(Weather::of(3, false), Weather::Cloudy);
        assert_eq!(Weather::of(45, false), Weather::Fog);
        assert_eq!(Weather::of(53, false), Weather::Drizzle);
        assert_eq!(Weather::of(63, false), Weather::Rain);
        assert_eq!(Weather::of(80, false), Weather::Rain);
        assert_eq!(Weather::of(65, false), Weather::Heavy);
        assert_eq!(Weather::of(82, false), Weather::Heavy);
        assert_eq!(Weather::of(73, false), Weather::Snow);
        assert_eq!(Weather::of(95, false), Weather::Storm);
        assert_eq!(Weather::of(99, false), Weather::Hail);
        // ventania só com o céu sem chuva
        assert_eq!(Weather::of(1, true), Weather::Wind);
        assert_eq!(Weather::of(63, true), Weather::Rain);
        assert!(Weather::Storm.lightning() && Weather::Hail.lightning() && !Weather::Rain.lightning());
        assert!(!Weather::Cloudy.moves() && !Weather::Fog.moves() && Weather::Wind.moves() && Weather::Snow.moves());
    }

    #[test]
    fn the_sky_follows_the_day() {
        let at = |h: f32| Clock::new(h, 6.2, 18.2);
        assert_eq!(at(0.0).daylight(), 0.0);
        assert_eq!(at(12.0).daylight(), 1.0);
        assert!(at(6.2).daylight() > 0.2 && at(6.2).daylight() < 0.8, "nascendo");
        // meio-dia azul; pôr do sol com a base alaranjada; noite escura
        let (top, bottom) = at(12.2).clear_sky();
        assert!(top.2 > top.0 + 100 && bottom.2 > bottom.0, "{top:?} {bottom:?}");
        let (_, bottom) = at(18.2).clear_sky();
        assert!(bottom.0 > 200 && bottom.0 > bottom.2 + 80, "{bottom:?}");
        let (top, _) = at(23.0).clear_sky();
        assert!(top.0 < 30 && top.1 < 40, "{top:?}");
        // sem pulos: minuto a minuto, a cor anda pouco
        let mut prev = at(0.0).clear_sky();
        for m in 1..1440 {
            let now = at(m as f32 / 60.0).clear_sky();
            assert!(dist(now.0, prev.0) < 12.0 && dist(now.1, prev.1) < 12.0, "salto às {m} min");
            prev = now;
        }
        // horários absurdos da previsão viram horários possíveis
        let c = Clock::new(25.0, 0.0, 30.0);
        assert!(c.now < 24.0 && c.rise >= 2.0 && c.set <= 22.5);
    }

    #[test]
    fn cloudy_does_not_look_clear() {
        // o nublado é cinza, longe do azul do céu limpo
        for h in [10.0, 15.0] {
            let clk = Clock::new(h, 6.2, 18.2);
            let (clear, _) = colors(Weather::Clear, &clk);
            let (cloudy, _) = colors(Weather::Cloudy, &clk);
            assert!(dist(clear, cloudy) > 40.0, "{h}h: {clear:?} {cloudy:?}");
            let spread = cloudy.0.max(cloudy.1).max(cloudy.2) - cloudy.0.min(cloudy.1).min(cloudy.2);
            assert!(spread < 50, "cinza, não azul: {cloudy:?}");
        }
        // a trovoada é o céu mais escuro
        let clk = Clock::new(14.0, 6.2, 18.2);
        let light = |w| {
            let (t, b) = colors(w, &clk);
            t.0 as u32 + t.1 as u32 + t.2 as u32 + b.0 as u32 + b.1 as u32 + b.2 as u32
        };
        for w in Weather::ALL {
            assert!(light(Weather::Storm) <= light(w), "{w:?}");
        }
    }

    #[test]
    fn lightning_flashes_then_thunder_shakes() {
        let storm = Storm::default();
        let t0 = Instant::now();
        storm.strike(320.0, 154.0, t0);
        assert!(storm.flash(t0) > 0.9);
        assert!(storm.flash(t0 + Duration::from_millis(1500)) < 0.01, "o clarão some");
        // o raio fica dentro do cartão (de lado) e desce do topo
        let list = storm.strikes.borrow();
        let s = &list[0];
        assert!(s.main.iter().all(|&(x, y)| (-60.0..380.0).contains(&x) && (-10.0..250.0).contains(&y)));
        assert!(s.main[0].1 < 0.0 && s.main.len() > 20);
        let thunder = s.thunder;
        assert!(thunder > t0 + Duration::from_millis(600), "a luz chega antes do som");
        // antes do trovão, parado; logo depois, treme; depois de um segundo, parado de novo
        assert_eq!(storm.shake(thunder - Duration::from_millis(10)), (0.0, 0.0));
        let (x, y) = storm.shake(thunder + Duration::from_millis(30));
        assert!(x.abs() + y.abs() > 0.3 && x.abs() < 3.0 && y.abs() < 3.0);
        assert_eq!(storm.shake(thunder + Duration::from_millis(900)), (0.0, 0.0));
    }

    #[test]
    fn every_weather_draws() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let storm = Storm::default();
        storm.strike(320.0, 154.0, Instant::now());
        for w in Weather::ALL {
            for h in [3.0, 6.2, 12.0, 18.2, 21.0] {
                let mut c = Canvas::new(400, 193, 1.25);
                draw_back(&mut c, 320.0, 154.0, 26.0, w, &Clock::new(h, 6.2, 18.2));
                draw_front(&mut c, 0.0, 0.0, 320.0, 154.0, 26.0, w, 12.5, &storm);
                let img = c.finish();
                assert!(img.px.chunks(4).filter(|p| p[3] > 200).count() > 20_000, "{w:?} {h}h: o céu cobre o cartão");
            }
        }
    }
}
