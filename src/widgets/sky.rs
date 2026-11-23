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
use crate::ui::theme::{Rgba, WHITE};

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
            _ if code >= 95 => Weather::Storm,
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

    /// As cores e as nuvens deste clima.
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
    count: f32,
    min: usize,
    size: (f32, f32),
    alpha: f32,
    y: (f32, f32),
    dark: f32,
}

/// Uma camada de nuvens, na ordem de `Layer` (os pares por extenso: de `size0` a `size1`, de
/// `y0` a `y1`).
#[allow(clippy::too_many_arguments)]
const fn layer(count: f32, min: usize, size0: f32, size1: f32, alpha: f32, y0: f32, y1: f32, dark: f32) -> Layer {
    Layer { count, min, size: (size0, size1), alpha, y: (y0, y1), dark }
}

const FEW: [Layer; 1] = [layer(2.4, 1, 0.5, 0.8, 0.92, 0.05, 0.4, 0.0)];
/// nublado: três camadas, as de trás maiores e mais escuras
const OVERCAST: [Layer; 3] = [layer(4.0, 2, 1.3, 1.8, 0.75, -0.2, 0.2, 0.35), layer(4.0, 2, 0.9, 1.3, 0.8, 0.0, 0.45, 0.15), layer(3.0, 1, 0.55, 0.85, 0.85, 0.15, 0.7, 0.0)];
const DRIZZLE: [Layer; 2] = [layer(4.0, 2, 1.2, 1.7, 0.7, -0.2, 0.2, 0.3), layer(3.0, 1, 0.8, 1.2, 0.7, 0.0, 0.4, 0.1)];
const RAIN: [Layer; 2] = [layer(5.0, 2, 1.3, 1.8, 0.8, -0.25, 0.15, 0.4), layer(4.0, 2, 0.9, 1.3, 0.75, -0.05, 0.35, 0.2)];
const HEAVY: [Layer; 2] = [layer(5.0, 3, 1.4, 2.0, 0.85, -0.3, 0.15, 0.45), layer(4.0, 2, 1.0, 1.4, 0.8, -0.1, 0.35, 0.25)];
const SNOW: [Layer; 1] = [layer(4.0, 2, 1.3, 1.8, 0.55, -0.25, 0.12, 0.2)];
const WINDY: [Layer; 1] = [layer(3.0, 1, 0.45, 0.7, 0.9, 0.05, 0.45, 0.0)];

/// A cor de um 0xRRGGBB.
fn hex(rgb: u32) -> Rgba {
    Rgba::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Sobe suave de 0 (em `from`) a 1 (em `to`), sem quinas nas pontas (o "smoothstep").
fn smooth(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Números pseudoaleatórios repetíveis (as mesmas nuvens, estrelas e gotas a cada quadro).
struct Rnd(u32);

impl Rnd {
    /// O próximo número, de 0 a 1.
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

/// Uma cor-chave do céu limpo: `offset` horas depois da âncora (0 = meia-noite, 1 = nascer do
/// sol, 2 = meio do dia, 3 = pôr do sol, 4 = a meia-noite seguinte), o topo e a base.
#[derive(Clone, Copy)]
struct SkyKey {
    anchor: u8,
    offset: f32,
    top: u32,
    bottom: u32,
}

const fn key(anchor: u8, offset: f32, top: u32, bottom: u32) -> SkyKey {
    SkyKey { anchor, offset, top, bottom }
}

/// As cores-chave do céu limpo ao longo do dia.
const KEYS: [SkyKey; 12] = [
    key(0, 0.0, 0x0a1430, 0x1b2a55),
    key(1, -1.6, 0x0c1736, 0x22305e),
    key(1, -0.7, 0x2b3670, 0x9a6f9c), // aurora
    key(1, 0.0, 0x4566b0, 0xf0a07c),  // nascer do sol
    key(1, 1.1, 0x3a80d4, 0xa9cdef),
    key(2, 0.0, 0x2675d6, 0x66a8eb), // meio-dia
    key(3, -2.4, 0x357ccd, 0x8bb9e6),
    key(3, -0.9, 0x4a6ab6, 0xf2b56e), // tarde dourada
    key(3, 0.0, 0x38408c, 0xf27a52),  // pôr do sol
    key(3, 0.7, 0x222a62, 0x8d4d7c),  // crepúsculo
    key(3, 1.5, 0x111a40, 0x2b3566),
    key(4, 0.0, 0x0a1430, 0x1b2a55),
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
            .map(|key| {
                let base = match key.anchor {
                    0 => 0.0,
                    1 => self.rise,
                    2 => (self.rise + self.set) / 2.0,
                    3 => self.set,
                    _ => 24.0,
                };
                // as horas nunca voltam (com o nascer e o pôr do sol muito perto, as chaves se
                // juntam)
                prev = (base + key.offset).clamp(prev, 24.0);
                prev
            })
            .collect();
        for i in 0..KEYS.len() - 1 {
            let (h0, h1) = (hours[i], hours[i + 1]);
            if self.now >= h0 && self.now <= h1 {
                let between = if h1 > h0 { smooth(0.0, 1.0, (self.now - h0) / (h1 - h0)) } else { 1.0 };
                let (from, to) = (KEYS[i], KEYS[i + 1]);
                return (hex(from.top).mix(hex(to.top), between), hex(from.bottom).mix(hex(to.bottom), between));
            }
        }
        (hex(KEYS[0].top), hex(KEYS[0].bottom))
    }
}

/// As cores do céu (topo, base) neste clima e nesta hora.
pub fn colors(weather: Weather, clock: &Clock) -> (Rgba, Rgba) {
    let (top, bottom) = clock.clear_sky();
    let look = weather.look();
    if look.cloud <= 0.0 {
        return (top, bottom);
    }
    let daylight = clock.daylight();
    let [day_top, day_bottom, night_top, night_bottom] = look.sky.map(hex);
    (top.mix(night_top.mix(day_top, daylight), look.cloud), bottom.mix(night_bottom.mix(day_bottom, daylight), look.cloud))
}

// ---------- a camada de baixo ----------

/// Só o degradê do céu (sem a previsão ainda: nada de nuvens, estrelas ou luz).
pub fn draw_plain(c: &mut Canvas, w: f32, h: f32, r: f32, weather: Weather, clock: &Clock) {
    let (top, bottom) = colors(weather, clock);
    c.gradient_round_rect(0.0, 0.0, w, h, r, &[(0.0, top.alpha(0.94)), (1.0, bottom.alpha(0.94))], 0.0, h);
}

/// O céu inteiro atrás do conteúdo: cores, luz do sol ou da lua, estrelas, nuvens e neblina.
pub fn draw_back(c: &mut Canvas, w: f32, h: f32, r: f32, weather: Weather, clock: &Clock) {
    draw_plain(c, w, h, r, weather, clock);
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
    // uma luz a `along` (0 a 1) do caminho de um lado ao outro do céu
    let light = |c: &mut Canvas, along: f32, col: Rgba, amount: f32, rad: f32| {
        if amount <= 0.005 {
            return;
        }
        let x = -w * 0.08 + w * 1.16 * along;
        let y = -12.0 + (1.0 - (PI * along.clamp(0.0, 1.0)).sin()) * h.min(170.0) * 0.55;
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
        let (x, y, r, brightness) = (rnd.f() * w, rnd.f() * zone * 0.9, 0.4 + rnd.f() * 0.8, 0.25 + rnd.f() * 0.5);
        c.fill_circle(x, y, r, WHITE.alpha(brightness * amount));
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
    let shade = |col: Rgba, factor: f32| Rgba::rgb((col.0 as f32 * factor) as u8, (col.1 as f32 * factor) as u8, (col.2 as f32 * factor) as u8);
    let top = shade(Rgba::rgb(80, 88, 110).mix(Rgba::rgb(210, 215, 226), day), look.dark).mix(clear_top, 0.1);
    let bottom = shade(Rgba::rgb(40, 46, 62).mix(Rgba::rgb(132, 142, 160), day), look.dark).mix(clear_bottom, if weather.lightning() { 0.09 } else { 0.3 });
    let zone = h.min(230.0);
    let mut rnd = Rnd(3);
    for layer in look.layers {
        let count = ((layer.count * w / 320.0 * if h > 200.0 { 1.4 } else { 1.0 }).round() as usize).max(layer.min);
        let (layer_top, layer_bottom) = (top.mix(bottom, layer.dark), bottom.mix(Rgba::rgb(0, 0, 0), layer.dark * 0.6));
        for _ in 0..count {
            let shape = (rnd.f() * 6.0) as usize % 6;
            let size = layer.size.0 + rnd.f() * (layer.size.1 - layer.size.0);
            let (cw, ch) = (180.0 * size, 100.0 * size);
            let x = rnd.f() * (w + cw * 1.2) - cw * 0.6;
            let y = (layer.y.0 + (layer.y.1 - layer.y.0) * rnd.f()) * zone - ch * 0.35;
            // em meia resolução e em tamanhos arredondados (desfocada, esticar não aparece, e o
            // desfoque custa 4 vezes menos e se repete entre nuvens parecidas)
            let img = cloud_sprite(shape, layer_top, layer_bottom, (size * c.s * 5.0).round() / 10.0);
            let s = c.s;
            c.image_px(&img, ((x - cw * 0.1) * s).round(), (y * s).round(), (cw * 1.2 * s).round(), (ch * 1.2 * s).round(), layer.alpha);
        }
    }
}

/// Uma bolha de nuvem: o centro, os raios e se é a base achatada (um retângulo arredondado).
struct Puff {
    x: f32,
    y: f32,
    rx: f32,
    ry: f32,
    base: bool,
}

/// As bolhas da nuvem `shape` (0 a 5), num quadro de 180 × 100: a base e de 4 a 6 bolhas em
/// cima, maiores no meio.
fn puffs(shape: usize) -> Vec<Puff> {
    let mut seed: u64 = 11 + shape as u64 * 97;
    let mut random = || {
        seed = seed * 16807 % 2_147_483_647;
        seed as f32 / 2_147_483_647.0
    };
    let mut out = vec![Puff { x: 90.0, y: 74.0, rx: 78.0, ry: 20.0, base: true }];
    let count = 4 + (random() * 3.0) as usize;
    for i in 0..count {
        let x = 30.0 + 120.0 * (i as f32 + 0.5) / count as f32 + (random() - 0.5) * 14.0;
        let rad = 18.0 + random() * 16.0 + (1.0 - (i as f32 - (count as f32 - 1.0) / 2.0).abs() / count as f32) * 14.0;
        out.push(Puff { x, y: 74.0 - rad * 0.55, rx: rad, ry: rad, base: false });
    }
    out
}

/// Qual nuvem pronta serve: a forma, as cores (arredondadas) e a escala (em centésimos).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SpriteKey {
    shape: usize,
    top: u32,
    bottom: u32,
    scale: u32,
}

/// nuvens prontas guardadas, no máximo (passando disso, o guardado recomeça do zero)
const SPRITES_MAX: usize = 64;

thread_local! {
    /// nuvens prontas (desenhar e desfocar custa; elas só mudam de cor devagar)
    static SPRITES: RefCell<HashMap<SpriteKey, Image>> = RefCell::new(HashMap::new());
}

/// Uma nuvem desfocada nas bordas (216 × 120 unidades com a margem do desfoque) na escala `scale`
/// (pixels por unidade), com o degradê de `top` a `bottom`.
fn cloud_sprite(shape: usize, top: Rgba, bottom: Rgba, scale: f32) -> Image {
    // as cores em passos de 6: um tom a mais ou a menos usa a mesma nuvem
    let rounded = |col: Rgba| ((col.0 / 6) as u32) << 16 | ((col.1 / 6) as u32) << 8 | (col.2 / 6) as u32;
    let key = SpriteKey { shape, top: rounded(top), bottom: rounded(bottom), scale: (scale * 100.0).round() as u32 };
    if let Some(img) = SPRITES.with(|sprites| sprites.borrow().get(&key).cloned()) {
        return img;
    }
    let mut c = Canvas::new((216.0 * scale).ceil() as u32, (120.0 * scale).ceil() as u32, scale);
    let fill = Fill::VGrad(24.0, 106.0, top, bottom);
    for Puff { x, y, rx, ry, base } in puffs(shape) {
        let (x, y) = (x + 18.0, y + 10.0);
        if base {
            c.fill_round_rect_with(x - rx, y - ry, x + rx, y + ry, ry, fill);
        } else {
            c.fill_circle_with(x, y, rx, fill);
        }
    }
    let mut img = c.finish();
    let (iw, ih) = (img.w, img.h);
    crate::gfx::imgops::blur(&mut img.px, iw, ih, 4, 2.2 * scale);
    SPRITES.with(|sprites| {
        let mut sprites = sprites.borrow_mut();
        if sprites.len() > SPRITES_MAX {
            sprites.clear();
        }
        sprites.insert(key, img.clone());
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

/// o primeiro raio da trovoada vem 1,5 s depois de ela começar; os outros, de 3 a 8 s depois
/// do anterior
const FIRST_STRIKE_MS: u64 = 1500;
const STRIKE_EVERY_MS: u64 = 3000;
const STRIKE_JITTER_MS: f32 = 5000.0;
/// o trovão chega de 0,7 a 1,6 s depois da luz
const THUNDER_AFTER_MS: u64 = 700;
const THUNDER_JITTER_MS: f32 = 900.0;
/// os raios ficam guardados por 4 s (o clarão e o tremor já passaram)
const STRIKE_KEEP_MS: f32 = 4000.0;

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
    // uma piscada: um sino em volta de `center` ms, com esta largura e esta força
    let pulse = |center: f32, width: f32, strength: f32| strength * (-((t - center) / width).powi(2)).exp();
    let tail = if t > 0.0 && t < 600.0 { 0.25 * (-t / 260.0).exp() } else { 0.0 };
    (pulse(0.0, 35.0, 1.0) + pulse(110.0, 30.0, 0.7) + pulse(230.0, 45.0, 0.5) + tail).clamp(0.0, 1.0)
}

impl Storm {
    /// O próximo número pseudoaleatório (0 a 1), repetível como o `Rnd`.
    fn rnd(&self) -> f32 {
        let next = self.seed.get().wrapping_mul(1_103_515_245).wrapping_add(12345);
        self.seed.set(next);
        (next >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Um raio agora, num cartão w × h: um traço que se divide ao meio várias vezes (cada metade
    /// desviando um pouco), com alguns galhos.
    pub fn strike(&self, w: f32, h: f32, now: Instant) {
        let x = (0.15 + self.rnd() * 0.7) * w;
        let y1 = (h * (0.55 + self.rnd() * 0.35)).min(240.0);
        let spread = w.min(200.0) * 0.35;
        let mut pts = vec![(x, -4.0), (x + (self.rnd() - 0.5) * spread, y1)];
        // cinco rodadas: cada trecho ganha um ponto no meio, desviado (menos a cada rodada)
        for round in 0..5 {
            let mut next = Vec::with_capacity(pts.len() * 2);
            for i in 0..pts.len() - 1 {
                let (from, to) = (pts[i], pts[i + 1]);
                next.push(from);
                next.push(((from.0 + to.0) / 2.0 + (self.rnd() - 0.5) * spread / (round as f32 + 1.3), (from.1 + to.1) / 2.0 + (self.rnd() - 0.5) * 4.0));
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
                for step in 1..=5 {
                    let along = step as f32 / 5.0;
                    branch.push((bx + dir * len * along + (self.rnd() - 0.5) * 5.0, by + len * along * 0.9 + (self.rnd() - 0.5) * 3.0));
                }
                branches.push(branch);
            }
            i += 5 + (self.rnd() * 5.0) as usize;
        }
        // o trovão chega depois da luz
        let thunder = now + Duration::from_millis(THUNDER_AFTER_MS + (self.rnd() * THUNDER_JITTER_MS) as u64);
        let wobble = self.rnd();
        let mut list = self.strikes.borrow_mut();
        list.retain(|strike| ms(now, strike.at) < STRIKE_KEEP_MS);
        list.push(Strike { at: now, thunder, wobble, main: pts, branches });
    }

    /// A cada quadro da trovoada: cai o próximo raio quando chega a hora.
    fn tick(&self, w: f32, h: f32, now: Instant) {
        match self.next.get() {
            None => self.next.set(Some(now + Duration::from_millis(FIRST_STRIKE_MS))),
            Some(due) if now >= due => {
                self.strike(w, h, now);
                self.next.set(Some(now + Duration::from_millis(STRIKE_EVERY_MS + (self.rnd() * STRIKE_JITTER_MS) as u64)));
            }
            _ => {}
        }
    }

    /// O clarão agora (0 a 1).
    pub fn flash(&self, now: Instant) -> f32 {
        self.strikes.borrow().iter().map(|strike| flash_at(ms(now, strike.at))).fold(0.0, f32::max)
    }

    /// O tremor do trovão agora (deslocamento em pontos; some em menos de um segundo).
    pub fn shake(&self, now: Instant) -> (f32, f32) {
        let (mut x, mut y) = (0.0, 0.0);
        for strike in self.strikes.borrow().iter().filter(|strike| now >= strike.thunder) {
            let since = ms(now, strike.thunder);
            if since > 800.0 {
                continue;
            }
            let strength = 1.8 * (-since / 220.0).exp();
            x += (since * 0.09 + strike.wobble * 9.0).sin() * strength;
            y += (since * 0.13 + strike.wobble * 5.0).cos() * strength * 0.6;
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
    secs: f32,
}

/// O que cai ou sopra por cima do cartão (w × h, canto em ox, oy), e os raios. `t`: segundos
/// desde que a animação começou (as gotas são calculadas por ele: sem estado).
#[allow(clippy::too_many_arguments)]
pub fn draw_front(c: &mut Canvas, ox: f32, oy: f32, w: f32, h: f32, r: f32, weather: Weather, t: f32, storm: &Storm) {
    if !weather.moves() {
        return;
    }
    let now = Instant::now();
    let pane = Pane { ox, oy, w, h, secs: t };
    c.push_round_clip(ox, oy, ox + w, oy + h, r);
    if weather.lightning() {
        storm.tick(w, h, now);
        bolts(c, &pane, r, storm, now);
    }
    let area = w * h;
    let mut rnd = Rnd(99);
    match weather {
        Weather::Drizzle => streaks(c, &pane, &mut rnd, Drops { count: area / 650.0, speed: (80.0, 130.0), length: (3.0, 6.0), slant: 0.08, width: 0.9, alpha: 0.34 }),
        Weather::Rain => {
            // duas profundidades: as gotas de trás finas e apagadas, as da frente longas e claras
            streaks(c, &pane, &mut rnd, Drops { count: area / 1300.0, speed: (150.0, 210.0), length: (5.0, 9.0), slant: 0.15, width: 0.7, alpha: 0.2 });
            streaks(c, &pane, &mut rnd, Drops { count: area / 1500.0, speed: (240.0, 320.0), length: (11.0, 19.0), slant: 0.18, width: 1.3, alpha: 0.38 });
            splashes(c, &pane, &mut rnd, w / 12.0, 0.55);
        }
        Weather::Heavy | Weather::Storm => {
            streaks(c, &pane, &mut rnd, Drops { count: area / 1000.0, speed: (200.0, 260.0), length: (6.0, 11.0), slant: 0.26, width: 0.7, alpha: 0.24 });
            streaks(c, &pane, &mut rnd, Drops { count: area / 1100.0, speed: (300.0, 400.0), length: (14.0, 24.0), slant: 0.3, width: 1.4, alpha: 0.42 });
            splashes(c, &pane, &mut rnd, w / 6.0, 0.6);
        }
        Weather::Snow => snow(c, &pane, &mut rnd),
        Weather::Hail => {
            streaks(c, &pane, &mut rnd, Drops { count: area / 2400.0, speed: (220.0, 300.0), length: (8.0, 14.0), slant: 0.12, width: 0.8, alpha: 0.22 });
            hail(c, &pane, &mut rnd);
        }
        Weather::Wind => wind(c, &pane, &mut rnd),
        _ => {}
    }
    c.pop_clip();
}

/// O clarão no cartão inteiro e os raios (com um brilho em volta, que pisca junto).
fn bolts(c: &mut Canvas, pane: &Pane, r: f32, storm: &Storm, now: Instant) {
    let flash = storm.flash(now);
    if flash > 0.01 {
        c.fill_round_rect(pane.ox, pane.oy, pane.ox + pane.w, pane.oy + pane.h, r, Rgba(215, 215, 255, (flash * 0.32 * 255.0) as u8));
    }
    let rgba = |red: u8, green: u8, blue: u8, alpha: f32| Rgba(red, green, blue, (alpha.clamp(0.0, 1.0) * 255.0) as u8);
    for strike in storm.strikes.borrow().iter() {
        let age = ms(now, strike.at);
        let glow = (flash_at(age) * 1.4).min(1.0);
        if age > 320.0 || glow < 0.03 {
            continue;
        }
        let shift = |pts: &[(f32, f32)]| pts.iter().map(|&(x, y)| (pane.ox + x, pane.oy + y)).collect::<Vec<_>>();
        let main = shift(&strike.main);
        c.line(&main, rgba(190, 190, 255, glow * 0.12), 9.0);
        c.line(&main, rgba(200, 200, 255, glow * 0.35), 4.0);
        c.line(&main, rgba(255, 255, 255, glow), 1.5);
        for branch in &strike.branches {
            let branch = shift(branch);
            c.line(&branch, rgba(200, 200, 255, glow * 0.15), 3.5);
            c.line(&branch, rgba(235, 235, 255, glow * 0.8), 0.9);
        }
    }
}

/// Uma chuva de traços retos: quantas gotas, a faixa de velocidade (pt/s) e de comprimento, a
/// inclinação, a espessura e a opacidade.
struct Drops {
    count: f32,
    speed: (f32, f32),
    length: (f32, f32),
    slant: f32,
    width: f32,
    alpha: f32,
}

/// As gotas em traços retos, caindo e voltando ao topo (calculadas pelo tempo, sem estado).
fn streaks(c: &mut Canvas, pane: &Pane, rnd: &mut Rnd, drops: Drops) {
    let count = (drops.count.round() as usize).max(4);
    let mut segs = Vec::with_capacity(count);
    for _ in 0..count {
        let (x0, y0, speed_pick, length_pick) = (rnd.f(), rnd.f(), rnd.f(), rnd.f());
        let speed = drops.speed.0 + speed_pick * (drops.speed.1 - drops.speed.0);
        let len = drops.length.0 + length_pick * (drops.length.1 - drops.length.0);
        let y = (y0 * (pane.h + len) + pane.secs * speed) % (pane.h + len) - len;
        let x = (x0 * (pane.w + 40.0) - pane.secs * speed * drops.slant).rem_euclid(pane.w + 40.0) - 20.0;
        segs.push((pane.ox + x, pane.oy + y, pane.ox + x - len * drops.slant, pane.oy + y + len));
    }
    c.segments(&segs, Rgba(205, 222, 255, (drops.alpha * 255.0) as u8), drops.width);
}

/// Respingos no pé do cartão: um arquinho que abre e some.
fn splashes(c: &mut Canvas, pane: &Pane, rnd: &mut Rnd, count: f32, alpha: f32) {
    for _ in 0..count.round() as usize {
        let (x0, phase, period, depth) = (rnd.f(), rnd.f(), 0.5 + rnd.f() * 0.6, rnd.f());
        let cycle = ((pane.secs + phase * period) % period) / period;
        // só aparece na primeira parte de cada ciclo
        if cycle > 0.45 {
            continue;
        }
        let open = cycle / 0.45;
        let (cx, cy, rx, ry) = (pane.ox + x0 * pane.w, pane.oy + pane.h - 3.0 - depth * 8.0, 1.0 + open * 5.0, 0.5 + open * 1.4);
        let pts: Vec<(f32, f32)> = (0..=6)
            .map(|i| {
                let angle = PI + PI * i as f32 / 6.0;
                (cx + rx * angle.cos(), cy + ry * angle.sin())
            })
            .collect();
        c.line(&pts, Rgba(215, 228, 255, (alpha * (1.0 - open) * 255.0) as u8), 0.8);
    }
}

/// Uma camada de flocos: quantos, as faixas de raio e de velocidade, a opacidade e se são macios
/// (com a borda esfumada).
struct Flakes {
    count: f32,
    radius: (f32, f32),
    speed: (f32, f32),
    alpha: f32,
    soft: bool,
}

/// Neve: flocos pequenos ao fundo e grandes e macios na frente, balançando enquanto caem.
fn snow(c: &mut Canvas, pane: &Pane, rnd: &mut Rnd) {
    let area = pane.w * pane.h;
    let layers = [
        Flakes { count: area / 650.0, radius: (0.7, 1.3), speed: (12.0, 22.0), alpha: 0.6, soft: false },
        Flakes { count: area / 2600.0, radius: (1.8, 3.0), speed: (26.0, 40.0), alpha: 0.85, soft: true },
    ];
    for Flakes { count, radius, speed, alpha, soft } in layers {
        for _ in 0..count.round() as usize {
            let (x0, y0, speed_pick, radius_pick, phase) = (rnd.f(), rnd.f(), rnd.f(), rnd.f(), rnd.f());
            let fall = speed.0 + speed_pick * (speed.1 - speed.0);
            let rad = radius.0 + radius_pick * (radius.1 - radius.0);
            let y = (y0 * (pane.h + 10.0) + pane.secs * fall) % (pane.h + 10.0) - 5.0;
            let sway = (pane.secs * 0.9 + phase * 6.28).sin() * if soft { 8.0 } else { 4.0 };
            let x = (x0 * pane.w + sway + pane.secs * 3.0).rem_euclid(pane.w);
            let (x, y) = (pane.ox + x, pane.oy + y);
            if soft {
                c.fill_radial(x, y, rad, rad, &[(0.0, WHITE.alpha(alpha)), (0.6, WHITE.alpha(alpha * 0.6)), (1.0, WHITE.alpha(0.0))]);
            } else {
                c.fill_circle(x, y, rad, WHITE.alpha(alpha));
            }
        }
    }
}

/// Granizo: pedrinhas que caem rápido (com um rastro) e quicam no pé do cartão.
fn hail(c: &mut Canvas, pane: &Pane, rnd: &mut Rnd) {
    for _ in 0..(pane.w * pane.h / 1100.0).round() as usize {
        let (x0, phase, speed_pick, radius_pick) = (rnd.f(), rnd.f(), rnd.f(), rnd.f());
        let side = if rnd.f() < 0.5 { -1.0 } else { 1.0 };
        let speed = 240.0 + speed_pick * 90.0;
        // cada pedra: cai, quica e some por um instante (`period` segundos ao todo)
        let fall = (pane.h + 6.0) / speed;
        let bounce = 0.34;
        let period = fall + bounce + 0.15;
        let at = (pane.secs + phase * period) % period;
        let rad = 1.3 + radius_pick;
        let mut x = x0 * pane.w;
        let (y, alpha) = if at < fall {
            let y = -6.0 + at * speed;
            c.line(&[(pane.ox + x, pane.oy + y - 9.0), (pane.ox + x, pane.oy + y)], Rgba(230, 240, 255, 64), rad);
            (y, 0.9)
        } else if at < fall + bounce {
            let hop = (at - fall) / bounce;
            x += hop * 7.0 * side;
            (pane.h - 4.0 - 64.0 * hop * (1.0 - hop) * (1.0 - hop * 0.4), 0.9 * (1.0 - hop * 0.6))
        } else {
            continue;
        };
        c.fill_circle(pane.ox + x, pane.oy + y, rad, Rgba(242, 248, 255, (alpha * 255.0) as u8));
    }
}

/// Ventania: rajadas em linhas finas e onduladas que atravessam o cartão, mais fortes no meio.
fn wind(c: &mut Canvas, pane: &Pane, rnd: &mut Rnd) {
    let zone = pane.h.min(230.0);
    for _ in 0..(pane.w * pane.h / 6000.0).round() as usize + 3 {
        let (x0, y0, speed_pick, length_pick, phase) = (rnd.f(), rnd.f(), rnd.f(), rnd.f(), rnd.f());
        let speed = 130.0 + speed_pick * 90.0;
        let len = 45.0 + length_pick * 55.0;
        let span = pane.w + len + 80.0;
        let x = (x0 * span + pane.secs * speed) % span - len - 40.0;
        let y = 14.0 + y0 * (zone - 28.0);
        let wave = (pane.secs * 2.2 + phase * 6.28).sin() * 3.0;
        let ctrl = [(x, y), (x + len * 0.33, y - 4.0 + wave), (x + len * 0.66, y + 4.0 - wave), (x + len, y + wave * 0.5)];
        // o ponto a `along` (0 a 1) da curva de Bézier de `ctrl`
        let at = |along: f32| {
            let rest = 1.0 - along;
            let weight = [rest * rest * rest, 3.0 * rest * rest * along, 3.0 * rest * along * along, along * along * along];
            (0..4).fold((pane.ox, pane.oy), |acc, i| (acc.0 + ctrl[i].0 * weight[i], acc.1 + ctrl[i].1 * weight[i]))
        };
        const PIECES: usize = 6;
        for i in 0..PIECES {
            let (f0, f1) = (i as f32 / PIECES as f32, (i + 1) as f32 / PIECES as f32);
            let mid = (f0 + f1) / 2.0;
            // mais forte a 60% do caminho, sumindo nas pontas
            let alpha = if mid < 0.6 { 0.4 * mid / 0.6 } else { 0.4 * (1.0 - mid) / 0.4 };
            c.line(&[at(f0), at(f1)], WHITE.alpha(alpha), 1.1);
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

    // TEMPORÁRIO (legibilidade): impressão digital do céu com o tempo fixo.
    #[test]
    #[ignore]
    fn zz_sky_fingerprint() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        fn hash(img: &Image) -> u64 {
            img.px.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3))
        }
        let mut out = String::new();
        for w in Weather::ALL {
            for h in [3.0, 6.2, 12.0, 18.2, 21.0] {
                for (cw, ch) in [(320.0, 154.0), (320.0, 330.0), (154.0, 154.0)] {
                    let mut c = Canvas::new((cw * 1.25) as u32, (ch * 1.25) as u32, 1.25);
                    draw_back(&mut c, cw, ch, 26.0, w, &Clock::new(h, 6.2, 18.2));
                    out += &format!("back {w:?} {h} {cw}x{ch} {:x}
", hash(&c.finish()));
                }
            }
            if !w.lightning() {
                for t in [0.0, 3.7, 12.5] {
                    let mut c = Canvas::new(400, 193, 1.25);
                    draw_front(&mut c, 0.0, 0.0, 320.0, 154.0, 26.0, w, t, &Storm::default());
                    out += &format!("front {w:?} {t} {:x}
", hash(&c.finish()));
                }
            }
        }
        let storm = Storm::default();
        let t0 = Instant::now();
        storm.strike(320.0, 154.0, t0);
        storm.strike(320.0, 330.0, t0 + Duration::from_millis(100));
        for ms in [0u64, 30, 120, 250, 400] {
            let now = t0 + Duration::from_millis(ms);
            let mut c = Canvas::new(400, 413, 1.25);
            bolts(&mut c, &Pane { ox: 2.0, oy: 3.0, w: 320.0, h: 330.0, secs: 1.0 }, 26.0, &storm, now);
            out += &format!("bolts {ms} {:x} {} {:?}
", hash(&c.finish()), storm.flash(now), storm.shake(now + Duration::from_millis(900)));
        }
        let path = std::env::var("FINGERPRINT").unwrap_or_else(|_| "sky.txt".into());
        std::fs::write(path, out).unwrap();
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
