//! Astronomia do dia a dia, calculada no próprio PC: onde estão o Sol e a Lua, quando o Sol se
//! põe, quando a Lua nasce, as luas cheias e os eclipses da Lua (com o começo e o fim de cada
//! fase), e as passagens da Estação Espacial que dá para ver a olho nu (com a órbita baixada da
//! internet, calculada pelo SGP4).
//!
//! As fórmulas são as de "Astronomical Algorithms" (Jean Meeus), na versão curta: o Sol e a Lua
//! ficam a menos de um minuto de arco do lugar certo, o que dá horários com erro de um ou dois
//! minutos. Os horários são segundos desde 1970 (UTC), os ângulos em graus.
//!
//! Nas contas, as letras são as do livro (`t` em séculos, `l0`, `m`, `mp`, `f`...), para dar para
//! conferir uma com a outra.

use std::f64::consts::PI;

/// graus para radianos
const D: f64 = PI / 180.0;
/// Raio da Terra no equador (km).
const EARTH_KM: f64 = 6378.137;

/// Dia juliano de um horário (segundos desde 1970).
pub fn jd(unix: f64) -> f64 {
    unix / 86400.0 + 2440587.5
}

/// Séculos julianos desde o ano 2000.
fn centuries(unix: f64) -> f64 {
    (jd(unix) - 2451545.0) / 36525.0
}

/// O ângulo entre 0 e 360 graus.
fn norm(deg: f64) -> f64 {
    deg.rem_euclid(360.0)
}

/// Inclinação do eixo da Terra (graus).
fn obliquity(t: f64) -> f64 {
    23.439291 - 0.0130042 * t
}

/// Longitude aparente do Sol na eclíptica (graus).
pub fn sun_longitude(unix: f64) -> f64 {
    let t = centuries(unix);
    let l0 = 280.46646 + 36000.76983 * t + 0.0003032 * t * t;
    let m = (357.52911 + 35999.05029 * t - 0.0001537 * t * t) * D;
    let c = (1.914602 - 0.004817 * t - 0.000014 * t * t) * m.sin() + (0.019993 - 0.000101 * t) * (2.0 * m).sin() + 0.000289 * (3.0 * m).sin();
    let omega = (125.04 - 1934.136 * t) * D;
    norm(l0 + c - 0.00569 - 0.00478 * omega.sin())
}

/// Da eclíptica (longitude, latitude) para o equador (ascensão reta, declinação), em radianos.
fn equatorial(lon: f64, lat: f64, t: f64) -> (f64, f64) {
    let (l, b, e) = (lon * D, lat * D, obliquity(t) * D);
    let ra = (l.sin() * e.cos() - b.tan() * e.sin()).atan2(l.cos());
    let dec = (b.sin() * e.cos() + b.cos() * e.sin() * l.sin()).asin();
    (ra, dec)
}

/// Tempo sideral de Greenwich (radianos).
pub fn gmst(unix: f64) -> f64 {
    let d = jd(unix) - 2451545.0;
    let t = d / 36525.0;
    norm(280.46061837 + 360.98564736629 * d + 0.000387933 * t * t - t * t * t / 38710000.0) * D
}

/// (altura, azimute) de um ponto do céu visto de (lat, lon): a altura acima do horizonte e o
/// azimute a partir do norte, girando para o leste.
fn alt_az(ra: f64, dec: f64, unix: f64, lat: f64, lon: f64) -> (f64, f64) {
    let h = gmst(unix) + lon * D - ra;
    let phi = lat * D;
    let alt = (phi.sin() * dec.sin() + phi.cos() * dec.cos() * h.cos()).asin();
    let az = (-h.sin()).atan2(dec.tan() * phi.cos() - phi.sin() * h.cos());
    (alt / D, norm(az / D))
}

/// (altura, azimute) do Sol.
pub fn sun_alt_az(unix: f64, lat: f64, lon: f64) -> (f64, f64) {
    let (ra, dec) = equatorial(sun_longitude(unix), 0.0, centuries(unix));
    alt_az(ra, dec, unix, lat, lon)
}

/// Um termo das séries da Lua (Meeus, tabelas 47.A e 47.B): os múltiplos de D, M, M' e F que
/// formam o argumento e os coeficientes (em 1e-6 grau; a distância, em metros).
struct MoonTerm {
    d: i8,
    m: i8,
    mp: i8,
    f: i8,
    /// na longitude (ou na latitude, em `MOON_B`)
    coef: f64,
    /// na distância (só em `MOON_LR`)
    dist: f64,
}

const fn lon_term(d: i8, m: i8, mp: i8, f: i8, coef: f64, dist: f64) -> MoonTerm {
    MoonTerm { d, m, mp, f, coef, dist }
}

const fn lat_term(d: i8, m: i8, mp: i8, f: i8, coef: f64) -> MoonTerm {
    MoonTerm { d, m, mp, f, coef, dist: 0.0 }
}

/// Termos da longitude e da distância da Lua.
const MOON_LR: [MoonTerm; 32] = [
    lon_term(0, 0, 1, 0, 6288774.0, -20905355.0),
    lon_term(2, 0, -1, 0, 1274027.0, -3699111.0),
    lon_term(2, 0, 0, 0, 658314.0, -2955968.0),
    lon_term(0, 0, 2, 0, 213618.0, -569925.0),
    lon_term(0, 1, 0, 0, -185116.0, 48888.0),
    lon_term(0, 0, 0, 2, -114332.0, -3149.0),
    lon_term(2, 0, -2, 0, 58793.0, 246158.0),
    lon_term(2, -1, -1, 0, 57066.0, -152138.0),
    lon_term(2, 0, 1, 0, 53322.0, -170733.0),
    lon_term(2, -1, 0, 0, 45758.0, -204586.0),
    lon_term(0, 1, -1, 0, -40923.0, -129620.0),
    lon_term(1, 0, 0, 0, -34720.0, 108743.0),
    lon_term(0, 1, 1, 0, -30383.0, 104755.0),
    lon_term(2, 0, 0, -2, 15327.0, 10321.0),
    lon_term(0, 0, 1, 2, -12528.0, 0.0),
    lon_term(0, 0, 1, -2, 10980.0, 79661.0),
    lon_term(4, 0, -1, 0, 10675.0, -34782.0),
    lon_term(0, 0, 3, 0, 10034.0, -23210.0),
    lon_term(4, 0, -2, 0, 8548.0, -21636.0),
    lon_term(2, 1, -1, 0, -7888.0, 24208.0),
    lon_term(2, 1, 0, 0, -6766.0, 30824.0),
    lon_term(1, 0, -1, 0, -5163.0, -8379.0),
    lon_term(1, 1, 0, 0, 4987.0, -16675.0),
    lon_term(2, -1, 1, 0, 4036.0, -12831.0),
    lon_term(2, 0, 2, 0, 3994.0, -10445.0),
    lon_term(4, 0, 0, 0, 3861.0, -11650.0),
    lon_term(2, 0, -3, 0, 3665.0, 14403.0),
    lon_term(0, 1, -2, 0, -2689.0, -7003.0),
    lon_term(2, 0, -1, 2, -2602.0, 0.0),
    lon_term(2, -1, -2, 0, 2390.0, 10056.0),
    lon_term(1, 0, 1, 0, -2348.0, 6322.0),
    lon_term(2, -2, 0, 0, 2236.0, -9884.0),
];

/// Termos da latitude da Lua.
const MOON_B: [MoonTerm; 20] = [
    lat_term(0, 0, 0, 1, 5128122.0),
    lat_term(0, 0, 1, 1, 280602.0),
    lat_term(0, 0, 1, -1, 277693.0),
    lat_term(2, 0, 0, -1, 173237.0),
    lat_term(2, 0, -1, 1, 55413.0),
    lat_term(2, 0, -1, -1, 46271.0),
    lat_term(2, 0, 0, 1, 32573.0),
    lat_term(0, 0, 2, 1, 17198.0),
    lat_term(2, 0, 1, -1, 9266.0),
    lat_term(0, 0, 2, -1, 8822.0),
    lat_term(2, -1, 0, -1, 8216.0),
    lat_term(2, 0, -2, -1, 4324.0),
    lat_term(2, 0, 1, 1, 4200.0),
    lat_term(2, 1, 0, -1, -3359.0),
    lat_term(2, -1, -1, 1, 2463.0),
    lat_term(2, -1, 0, 1, 2211.0),
    lat_term(2, -1, -1, -1, 2065.0),
    lat_term(0, 1, -1, -1, -1870.0),
    lat_term(4, 0, -1, -1, 1828.0),
    lat_term(0, 1, 0, 1, -1794.0),
];

/// A Lua vista do centro da Terra.
#[derive(Clone, Copy, Debug)]
pub struct MoonPosition {
    /// longitude e latitude na eclíptica (graus)
    pub lon: f64,
    pub lat: f64,
    pub dist_km: f64,
}

/// Onde está a Lua, vista do centro da Terra.
pub fn moon_ecliptic(unix: f64) -> MoonPosition {
    let t = centuries(unix);
    let lp = norm(218.3164477 + 481267.88123421 * t);
    let d = norm(297.8501921 + 445267.1114034 * t);
    let m = norm(357.5291092 + 35999.0502909 * t);
    let mp = norm(134.9633964 + 477198.8675055 * t);
    let f = norm(93.2720950 + 483202.0175233 * t);
    let e = 1.0 - 0.002516 * t - 0.0000074 * t * t;
    // o argumento de um termo e a correção pela excentricidade da órbita da Terra (nos termos
    // com M)
    let arg = |term: &MoonTerm| (term.d as f64 * d + term.m as f64 * m + term.mp as f64 * mp + term.f as f64 * f) * D;
    let ecc = |term: &MoonTerm| e.powi(term.m.unsigned_abs() as i32);
    let (mut sl, mut sr, mut sb) = (0.0, 0.0, 0.0);
    for term in &MOON_LR {
        let x = arg(term);
        sl += term.coef * ecc(term) * x.sin();
        sr += term.dist * ecc(term) * x.cos();
    }
    for term in &MOON_B {
        sb += term.coef * ecc(term) * arg(term).sin();
    }
    let a1 = (119.75 + 131.849 * t) * D;
    let a2 = (53.09 + 479264.290 * t) * D;
    let a3 = (313.45 + 481266.484 * t) * D;
    sl += 3958.0 * a1.sin() + 1962.0 * (lp * D - f * D).sin() + 318.0 * a2.sin();
    sb += -2235.0 * (lp * D).sin() + 382.0 * a3.sin() + 175.0 * (a1 - f * D).sin() + 175.0 * (a1 + f * D).sin() + 127.0 * (lp * D - mp * D).sin()
        - 115.0 * (lp * D + mp * D).sin();
    MoonPosition { lon: norm(lp + sl / 1e6), lat: sb / 1e6, dist_km: 385000.56 + sr / 1000.0 }
}

/// (altura, azimute) da Lua (do centro da Terra; perto do horizonte, a de verdade fica até um
/// grau mais baixa, o que `MOON_RISE` já desconta).
pub fn moon_alt_az(unix: f64, lat: f64, lon: f64) -> (f64, f64) {
    let moon = moon_ecliptic(unix);
    let (ra, dec) = equatorial(moon.lon, moon.lat, centuries(unix));
    alt_az(ra, dec, unix, lat, lon)
}

/// Altura do centro do Sol quando a borda de cima toca o horizonte (com a refração).
pub const SUN_SET: f64 = -0.833;
/// Altura (do centro da Terra) da Lua nascendo: a paralaxe, a refração e o raio dela.
pub const MOON_RISE: f64 = 0.125;

/// O primeiro horário entre `from` e `to` em que `f` passa por zero, subindo (`rising`) ou
/// descendo, com precisão de um segundo. Procura de `step` em `step` segundos.
pub fn crossing(f: impl Fn(f64) -> f64, from: f64, to: f64, step: f64, rising: bool) -> Option<f64> {
    let mut t0 = from;
    let mut v0 = f(t0);
    while t0 < to {
        let t1 = (t0 + step).min(to);
        let v1 = f(t1);
        let crossed = if rising { v0 < 0.0 && v1 >= 0.0 } else { v0 > 0.0 && v1 <= 0.0 };
        if crossed {
            // busca binária entre `before` (do lado de antes) e `after` (já do outro lado)
            let (mut before, mut after, mut v_before) = (t0, t1, v0);
            while after - before > 1.0 {
                let mid = (before + after) / 2.0;
                let v_mid = f(mid);
                if (v_mid >= 0.0) == (v_before >= 0.0) {
                    before = mid;
                    v_before = v_mid;
                } else {
                    after = mid;
                }
            }
            return Some(after);
        }
        t0 = t1;
        v0 = v1;
    }
    None
}

/// O próximo pôr do sol depois de `after`.
pub fn next_sunset(after: f64, lat: f64, lon: f64) -> Option<f64> {
    crossing(|t| sun_alt_az(t, lat, lon).0 - SUN_SET, after, after + 36.0 * 3600.0, 600.0, false)
}

/// O próximo nascer da Lua depois de `after`.
pub fn next_moonrise(after: f64, lat: f64, lon: f64) -> Option<f64> {
    crossing(|t| moon_alt_az(t, lat, lon).0 - MOON_RISE, after, after + 36.0 * 3600.0, 600.0, true)
}

/// Quanto a Lua está do lado oposto ao Sol, de -180 a 180 graus (0 = lua cheia).
fn opposition(unix: f64) -> f64 {
    let elong = norm(moon_ecliptic(unix).lon - sun_longitude(unix));
    (elong - 180.0 + 180.0).rem_euclid(360.0) - 180.0
}

/// As luas cheias entre `from` e `to`.
pub fn full_moons(from: f64, to: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut t = from;
    while t < to {
        // de 6 em 6 horas a Lua anda uns 3 graus: só procura perto da oposição
        let t1 = t + 6.0 * 3600.0;
        let (now, later) = (opposition(t), opposition(t1));
        if now < 0.0 && later >= 0.0 && now > -90.0 {
            if let Some(at) = crossing(opposition, t, t1, 6.0 * 3600.0, true) {
                out.push(at);
            }
        }
        t = t1;
    }
    out
}

/// O tipo de eclipse da Lua.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EclipseKind {
    /// só uma parte da Lua entra na sombra da Terra
    Partial,
    /// a Lua inteira na sombra (fica vermelha)
    Total,
}

/// Um eclipse da Lua: o máximo e quando a Lua entra e sai da sombra (e, no total, quando fica
/// inteira nela).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Eclipse {
    pub kind: EclipseKind,
    pub peak: f64,
    /// entra e sai da sombra
    pub start: f64,
    pub end: f64,
    /// fica inteira na sombra e começa a sair (só no total)
    pub total: Option<(f64, f64)>,
}

impl Eclipse {
    /// Quanto da Lua está na sombra em `t` (0 a 1, aproximado pela distância ao centro dela).
    pub fn covered(&self, t: f64) -> f64 {
        let Shadow { sep, umbra, moon } = shadow(t);
        ((umbra + moon - sep) / (2.0 * moon)).clamp(0.0, 1.0)
    }
}

/// A Lua e a sombra da Terra num momento, em graus.
struct Shadow {
    /// a distância da Lua ao centro da sombra
    sep: f64,
    /// o raio da sombra
    umbra: f64,
    /// o raio da Lua
    moon: f64,
}

/// A Lua e a sombra da Terra em `unix`.
fn shadow(unix: f64) -> Shadow {
    let moon = moon_ecliptic(unix);
    let anti = sun_longitude(unix) + 180.0;
    let sep = ((moon.lat * D).cos() * ((moon.lon - anti) * D).cos()).clamp(-1.0, 1.0).acos() / D;
    let parallax = (EARTH_KM / moon.dist_km).asin() / D;
    let moon_radius = (1737.4 / moon.dist_km).asin() / D;
    // a sombra fica 2% maior por causa do ar (a regra de Danjon)
    let umbra = 1.02 * (parallax + 0.00244 - 0.2666);
    Shadow { sep, umbra, moon: moon_radius }
}

/// O eclipse da Lua na lua cheia `full` (None: a Lua passa por cima ou por baixo da sombra).
/// Os eclipses penumbrais (quase invisíveis) não contam.
pub fn lunar_eclipse(full: f64) -> Option<Eclipse> {
    // o máximo: onde a Lua passa mais perto do centro da sombra (perto da lua cheia), por busca
    // ternária
    let (mut lo, mut hi) = (full - 3.0 * 3600.0, full + 3.0 * 3600.0);
    while hi - lo > 10.0 {
        let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        if shadow(m1).sep < shadow(m2).sep {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let peak = (lo + hi) / 2.0;
    let Shadow { sep, umbra, moon } = shadow(peak);
    if sep >= umbra + moon {
        return None;
    }
    // zero quando a borda da Lua toca a sombra: `inside` 1, por fora; -1, por dentro
    let edge = |inside: f64| move |t: f64| {
        let now = shadow(t);
        now.sep - (now.umbra + inside * now.moon)
    };
    let span = 4.0 * 3600.0;
    let start = crossing(edge(1.0), peak - span, peak, 120.0, false)?;
    let end = crossing(edge(1.0), peak, peak + span, 120.0, true)?;
    let total = if sep < umbra - moon {
        Some((crossing(edge(-1.0), peak - span, peak, 120.0, false)?, crossing(edge(-1.0), peak, peak + span, 120.0, true)?))
    } else {
        None
    };
    let kind = if total.is_some() { EclipseKind::Total } else { EclipseKind::Partial };
    Some(Eclipse { kind, peak, start, end, total })
}

/// Os oito pontos cardeais, para dizer "olhe para noroeste".
const COMPASS: [&str; 8] = ["norte", "nordeste", "leste", "sudeste", "sul", "sudoeste", "oeste", "noroeste"];

/// "noroeste" (o ponto cardeal mais perto do azimute).
pub fn compass(az: f64) -> &'static str {
    COMPASS[((norm(az) + 22.5) / 45.0) as usize % 8]
}

// ---------- a Estação Espacial ----------

/// A órbita da ISS (as duas linhas do TLE), pronta para calcular.
pub struct Orbit {
    constants: sgp4::Constants,
    /// a época da órbita (segundos desde 1970)
    epoch: f64,
}

impl Orbit {
    /// A órbita das duas linhas do TLE (None se elas não leem).
    pub fn from_tle(line1: &str, line2: &str) -> Option<Orbit> {
        let elements = sgp4::Elements::from_tle(None, line1.trim().as_bytes(), line2.trim().as_bytes()).ok()?;
        let epoch = elements.datetime.and_utc().timestamp_millis() as f64 / 1000.0;
        let constants = sgp4::Constants::from_elements(&elements).ok()?;
        Some(Orbit { constants, epoch })
    }

    /// Onde a ISS está em `unix` (km, no referencial TEME: o equador do dia, sem girar com a
    /// Terra).
    pub fn position(&self, unix: f64) -> Option<[f64; 3]> {
        let prediction = self.constants.propagate(sgp4::MinutesSinceEpoch((unix - self.epoch) / 60.0)).ok()?;
        Some(prediction.position)
    }

    /// A época da órbita (segundos desde 1970).
    #[cfg(test)]
    pub fn epoch(&self) -> f64 {
        self.epoch
    }
}

/// A direção do Sol (vetor unitário) no mesmo referencial da órbita.
fn sun_vector(unix: f64) -> [f64; 3] {
    let (ra, dec) = equatorial(sun_longitude(unix), 0.0, centuries(unix));
    [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()]
}

/// A ISS em `r` está no sol (fora da sombra da Terra, um cilindro atrás dela)?
pub fn sunlit(r: [f64; 3], sun: [f64; 3]) -> bool {
    let along = r[0] * sun[0] + r[1] * sun[1] + r[2] * sun[2];
    if along >= 0.0 {
        return true;
    }
    let perp = [r[0] - along * sun[0], r[1] - along * sun[1], r[2] - along * sun[2]];
    (perp[0] * perp[0] + perp[1] * perp[1] + perp[2] * perp[2]).sqrt() > EARTH_KM
}

/// (altura, azimute) de um ponto `r` (km, TEME) visto de (lat, lon), no nível do mar.
pub fn look(r: [f64; 3], unix: f64, lat: f64, lon: f64) -> (f64, f64) {
    let (phi, theta) = (lat * D, gmst(unix) + lon * D);
    // o observador no mesmo referencial (elipsoide WGS84)
    let e2 = 0.00669437999014;
    // o raio de curvatura da Terra naquela latitude
    let n = EARTH_KM / (1.0 - e2 * phi.sin() * phi.sin()).sqrt();
    let observer = [n * phi.cos() * theta.cos(), n * phi.cos() * theta.sin(), n * (1.0 - e2) * phi.sin()];
    let delta = [r[0] - observer[0], r[1] - observer[1], r[2] - observer[2]];
    let south = phi.sin() * theta.cos() * delta[0] + phi.sin() * theta.sin() * delta[1] - phi.cos() * delta[2];
    let east = -theta.sin() * delta[0] + theta.cos() * delta[1];
    let up = phi.cos() * theta.cos() * delta[0] + phi.cos() * theta.sin() * delta[1] + phi.sin() * delta[2];
    let range = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    ((up / range).asin() / D, norm(east.atan2(-south) / D))
}

/// Uma passagem visível da ISS: o trecho em que ela está alta o bastante, no sol, com o céu já
/// escuro para quem olha.
#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    pub start: f64,
    pub end: f64,
    /// o ponto mais alto
    pub peak: f64,
    pub max_alt: f64,
    /// o caminho no céu, de 10 em 10 segundos
    pub track: Vec<TrackPoint>,
}

/// Um ponto do caminho da ISS no céu: o horário, a altura e o azimute (em graus).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackPoint {
    pub at: f64,
    pub alt: f64,
    pub az: f64,
}

impl Pass {
    /// (altura, azimute) em `t`, entre os pontos do caminho.
    pub fn at(&self, t: f64) -> (f64, f64) {
        let track = &self.track;
        let next = track.iter().position(|point| point.at >= t).unwrap_or(track.len() - 1);
        if next == 0 {
            return (track[0].alt, track[0].az);
        }
        let (from, to) = (track[next - 1], track[next]);
        let along = ((t - from.at) / (to.at - from.at).max(1.0)).clamp(0.0, 1.0);
        // o azimute pelo lado mais curto (de 350° para 10° passa pelo norte)
        let mut daz = to.az - from.az;
        if daz > 180.0 {
            daz -= 360.0;
        } else if daz < -180.0 {
            daz += 360.0;
        }
        (from.alt + (to.alt - from.alt) * along, norm(from.az + daz * along))
    }
    /// De que lado ela aparece (azimute).
    pub fn start_az(&self) -> f64 {
        self.track[0].az
    }
    /// De que lado ela some (azimute).
    pub fn end_az(&self) -> f64 {
        self.track[self.track.len() - 1].az
    }
}

/// Altura mínima para contar (perto do horizonte, prédios e árvores escondem).
const PASS_MIN_ALT: f64 = 10.0;
/// O ponto mais alto precisa passar disto para valer o aviso ("bem alta").
pub const PASS_GOOD_ALT: f64 = 25.0;
/// O céu escuro o bastante: o Sol este tanto abaixo do horizonte.
const DARK_SUN: f64 = -6.0;

/// As passagens visíveis da ISS entre `from` e `from + hours`, vistas de (lat, lon).
pub fn visible_passes(orbit: &Orbit, lat: f64, lon: f64, from: f64, hours: f64) -> Vec<Pass> {
    let mut out = Vec::new();
    let mut cur: Vec<TrackPoint> = Vec::new();
    let mut t = from;
    let to = from + hours * 3600.0;
    while t <= to {
        let Some(position) = orbit.position(t) else { break };
        let (alt, az) = look(position, t, lat, lon);
        // longe do horizonte (do outro lado da Terra), passos largos: a altura muda uns 4° por
        // minuto perto do horizonte
        let step = if alt < -20.0 {
            120.0
        } else if alt < 0.0 {
            20.0
        } else {
            10.0
        };
        let ok = alt >= PASS_MIN_ALT && sunlit(position, sun_vector(t)) && sun_alt_az(t, lat, lon).0 <= DARK_SUN;
        let seen = ok.then_some(TrackPoint { at: t, alt, az });
        match seen {
            Some(point) => cur.push(point),
            None if cur.len() >= 3 => {
                let track = std::mem::take(&mut cur);
                // o ponto mais alto: (horário, altura)
                let top = track.iter().copied().fold((0.0, f64::MIN), |best, point| if point.alt > best.1 { (point.at, point.alt) } else { best });
                if top.1 >= PASS_GOOD_ALT {
                    out.push(Pass { start: track[0].at, end: track[track.len() - 1].at, peak: top.0, max_alt: top.1, track });
                }
            }
            None => cur.clear(),
        }
        t += step;
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::util::days_from_civil;

    /// Segundos desde 1970 de uma data e hora UTC.
    fn utc(y: i64, mo: u32, d: u32, h: u32, mi: u32) -> f64 {
        days_from_civil(y, mo, d) as f64 * 86400.0 + h as f64 * 3600.0 + mi as f64 * 60.0
    }

    fn near(a: f64, b: f64, mins: f64) -> bool {
        (a - b).abs() <= mins * 60.0
    }

    /// No equador, no equinócio, o Sol se põe por volta das 18:10 (UTC, longitude 0); em
    /// Londres, no solstício de junho, às 20:21 UTC.
    #[test]
    fn sunsets_land_on_known_times() {
        let eq = next_sunset(utc(2026, 3, 20, 12, 0), 0.0, 0.0).unwrap();
        assert!(near(eq, utc(2026, 3, 20, 18, 10), 5.0), "equador: {}", (eq - utc(2026, 3, 20, 0, 0)) / 3600.0);
        let london = next_sunset(utc(2026, 6, 21, 12, 0), 51.48, 0.0).unwrap();
        assert!(near(london, utc(2026, 6, 21, 20, 21), 4.0), "Londres: {}", (london - utc(2026, 6, 21, 0, 0)) / 3600.0);
        // ao meio-dia, o Sol no norte em São Paulo no inverno de lá (junho)
        let (alt, az) = sun_alt_az(utc(2026, 6, 21, 15, 10), -23.55, -46.63);
        assert!(alt > 40.0 && (az < 20.0 || az > 340.0), "{alt} {az}");
    }

    /// Os eclipses da Lua de 2025 e 2026 (NASA): o tipo e o máximo, a meia hora; e uma lua
    /// cheia sem eclipse.
    #[test]
    fn lunar_eclipses_match_nasa() {
        let known = [
            (utc(2025, 3, 14, 6, 59), EclipseKind::Total),
            (utc(2025, 9, 7, 18, 12), EclipseKind::Total),
            (utc(2026, 3, 3, 11, 34), EclipseKind::Total),
            (utc(2026, 8, 28, 4, 13), EclipseKind::Partial),
        ];
        for (peak, kind) in known {
            let full = full_moons(peak - 3.0 * 86400.0, peak + 3.0 * 86400.0);
            assert_eq!(full.len(), 1, "uma lua cheia perto de {peak}");
            assert!(near(full[0], peak, 45.0), "lua cheia longe do máximo: {}", (full[0] - peak) / 60.0);
            let e = lunar_eclipse(full[0]).unwrap_or_else(|| panic!("sem eclipse em {peak}"));
            assert_eq!(e.kind, kind, "{peak}");
            assert!(near(e.peak, peak, 30.0), "máximo {} min fora", (e.peak - peak) / 60.0);
            assert!(e.start < e.peak && e.peak < e.end && e.end - e.start < 4.5 * 3600.0);
            if let Some((a, b)) = e.total {
                assert!(e.start < a && a < e.peak && e.peak < b && b < e.end);
            }
            assert!(e.covered(e.peak) > 0.8 && e.covered(e.start - 600.0) == 0.0);
        }
        // a lua cheia de 24 de novembro de 2026 passa longe da sombra
        let nov = full_moons(utc(2026, 11, 22, 0, 0), utc(2026, 11, 27, 0, 0));
        assert_eq!(nov.len(), 1);
        assert!(lunar_eclipse(nov[0]).is_none());
        // uma por mês, mais ou menos
        let n = full_moons(utc(2026, 1, 1, 0, 0), utc(2027, 1, 1, 0, 0)).len();
        assert!((12..=13).contains(&n), "{n} luas cheias em 2026");
    }

    /// A lua cheia nasce perto do pôr do sol, do lado oposto do céu.
    #[test]
    fn full_moon_rises_opposite_the_sunset() {
        let (lat, lon) = (-23.55, -46.63);
        let full = full_moons(utc(2026, 11, 22, 0, 0), utc(2026, 11, 27, 0, 0))[0];
        let set = next_sunset(full - 18.0 * 3600.0, lat, lon).unwrap();
        let rise = next_moonrise(set - 6.0 * 3600.0, lat, lon).unwrap();
        assert!(near(rise, set, 90.0), "a lua cheia nasce {} min do pôr do sol", (rise - set) / 60.0);
        let (_, sun_az) = sun_alt_az(set, lat, lon);
        let (_, moon_az) = moon_alt_az(rise, lat, lon);
        let opposite = (norm(moon_az - sun_az) - 180.0).abs();
        assert!(opposite < 20.0, "sol {sun_az} lua {moon_az}");
    }

    #[test]
    fn compass_names() {
        assert_eq!(compass(0.0), "norte");
        assert_eq!(compass(359.0), "norte");
        assert_eq!(compass(90.0), "leste");
        assert_eq!(compass(135.0), "sudeste");
        assert_eq!(compass(250.0), "oeste");
        assert_eq!(compass(300.0), "noroeste");
    }

    /// A sombra da Terra: atrás dela (do lado oposto ao Sol) e perto do eixo, sem sol; ao lado,
    /// ou do lado do Sol, no sol.
    #[test]
    fn earth_shadow() {
        let sun = [1.0, 0.0, 0.0];
        assert!(!sunlit([-6800.0, 0.0, 0.0], sun));
        assert!(!sunlit([-6800.0, 3000.0, 0.0], sun));
        assert!(sunlit([-6800.0, 6500.0, 0.0], sun));
        assert!(sunlit([6800.0, 0.0, 0.0], sun));
        assert!(sunlit([0.0, 0.0, 6800.0], sun));
    }

    /// Uma órbita de verdade (20/11/2026): a ISS a uns 420 km de altura, e as passagens
    /// visíveis sobre São Paulo, de noite, subindo até o ponto mais alto e descendo.
    #[test]
    fn iss_passes_look_right() {
        let orbit = Orbit::from_tle(
            "1 25544U 98067A   26324.17237002  .00010040  00000+0  18678-3 0  9996",
            "2 25544  51.6318 248.1735 0007426  45.4169 314.7424 15.49502307595936",
        )
        .unwrap();
        let t0 = orbit.epoch();
        for k in 0..20 {
            let r = orbit.position(t0 + k as f64 * 600.0).unwrap();
            let h = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt() - EARTH_KM;
            assert!((380.0..460.0).contains(&h), "altura {h} km");
        }
        let (lat, lon) = (-23.55, -46.63);
        let passes = visible_passes(&orbit, lat, lon, t0, 4.0 * 24.0);
        for p in &passes {
            let mins = (p.end - p.start) / 60.0;
            assert!((0.3..=8.0).contains(&mins), "{mins} min");
            assert!(p.max_alt >= PASS_GOOD_ALT && p.max_alt <= 90.0);
            assert!(p.start <= p.peak && p.peak <= p.end);
            assert!(sun_alt_az(p.start, lat, lon).0 <= DARK_SUN);
            let (a, _) = p.at((p.start + p.end) / 2.0);
            assert!(a >= PASS_MIN_ALT - 0.5);
        }
    }
}

