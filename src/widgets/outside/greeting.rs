//! As saudações na ilha: "bom dia", "boa tarde" e "boa noite", uma vez em cada período do dia,
//! na primeira vez que a pessoa usa o PC nele (nunca com ninguém olhando). Ficam uns segundos e
//! somem sozinhas, com o tempo de agora (do widget Tempo), o dólar e o bitcoin e, de noite, a lua
//! cheia ou a Estação Espacial passando, se for a noite delas.
//!
//! Não atrapalham: em tela cheia ou no foco do Pomodoro, esperam. Um período que passou sem a
//! pessoa no PC fica para trás (ligou às 14 h: vem direto o "boa tarde").

use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::quote::{bitcoin, brl, pct, wallet, URL as QUOTES_URL};
use super::skywatch::{draw_station, toward};
use super::{location, Clock, OK};
use crate::astro;
use crate::config::Get;
use crate::drawing::weather_art;
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::{ease_in_out, Rgba, FG, MUTED};
use crate::widgets::island_ui as iu;
use crate::widgets::{alert_hash, on_island, Ctx, Env, IslandForm, IslandItem, Kind, Mode, Rect4, Request};

/// a saudação fica este tanto na ilha (esticada nos primeiros 4 s), alternando o tempo e as
/// cotações (e, de noite, a lua e a ISS), cada um por `PAGE_FOR`
const SHOW_FOR: f64 = 28.0;
const PAGE_FOR: f64 = 7.0;
/// a troca de uma página para a outra: a de saída sobe e apaga, a nova sobe e acende
const TURN: f64 = 0.4;
const UP: Rgba = Rgba::rgb(48, 209, 88);
const DOWN: Rgba = Rgba::rgb(255, 92, 80);
/// "usando o PC": mexeu no teclado ou no mouse há menos que isto
const ACTIVE: f64 = 20.0;
/// as cotações valem por este tempo (depois, busca de novo antes do "bom dia")
const QUOTES_FRESH: f64 = 2.0 * 3600.0;
/// espera as cotações chegarem no máximo este tanto
const QUOTES_WAIT: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Period {
    Morning,
    Afternoon,
    Night,
}

impl Period {
    fn code(self) -> &'static str {
        match self {
            Period::Morning => "manha",
            Period::Afternoon => "tarde",
            Period::Night => "noite",
        }
    }
    fn from_code(s: &str) -> Option<Period> {
        [Period::Morning, Period::Afternoon, Period::Night].into_iter().find(|p| p.code() == s)
    }
    /// "Bom dia" (para a prévia das saudações)
    #[cfg(test)]
    pub fn hello_text(self) -> &'static str {
        self.hello()
    }
    fn hello(self) -> &'static str {
        match self {
            Period::Morning => "Bom dia",
            Period::Afternoon => "Boa tarde",
            Period::Night => "Boa noite",
        }
    }
    fn color(self) -> Rgba {
        match self {
            Period::Morning => Rgba::rgb(255, 200, 92),
            Period::Afternoon => Rgba::rgb(255, 159, 67),
            Period::Night => Rgba::rgb(160, 168, 255),
        }
    }
    fn art(self) -> &'static str {
        match self {
            Period::Night => "moon",
            _ => "sun",
        }
    }
}

/// Uma saudação pronta: o texto já montado na hora em que apareceu.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    period: Period,
    at: f64,
    day: i64,
    weather: Weather,
    /// (nome, preço, variação desde ontem): o dólar e o bitcoin
    quotes: Vec<(&'static str, f64, f64)>,
    /// de noite: a lua cheia e a ISS, (nome, hora, frase completa): ("ISS", "19:42", "A ISS
    /// passa às 19:42, olhe para o noroeste")
    sky: Vec<(String, String, String)>,
}

/// O que a saudação mostra de cada vez (troca a cada `PAGE_FOR`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Weather,
    Quotes,
    Sky,
}

/// O tempo na saudação, do que o Tempo guardou.
#[derive(Clone, Debug, Default, PartialEq)]
struct Weather {
    now: Option<i64>,
    max: Option<i64>,
    /// a hora em que começa a chover hoje
    rain: Option<i64>,
    /// amanhã: (máxima, "céu limpo")
    tomorrow: Option<(i64, String)>,
}

impl Shown {
    /// A frase do tempo, completa: "18° agora, máx. 27°, chuva às 15h"; de noite, "amanhã 22°,
    /// céu limpo".
    fn weather_line(&self) -> Option<String> {
        let w = &self.weather;
        if self.period == Period::Night {
            return w.tomorrow.as_ref().map(|(max, desc)| if desc.is_empty() { format!("amanhã {max}°") } else { format!("amanhã {max}°, {desc}") });
        }
        let mut parts = Vec::new();
        if let Some(t) = w.now {
            parts.push(if self.period == Period::Morning { format!("{t}° agora") } else { format!("{t}°") });
        }
        if let Some(m) = w.max {
            parts.push(format!("máx. {m}°"));
        }
        if let Some(h) = w.rain {
            parts.push(format!("chuva às {h}h"));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }

    /// As páginas que se alternam: o tempo, as cotações e (de noite) o céu.
    fn pages(&self) -> Vec<Page> {
        let mut pages = vec![Page::Weather];
        if !self.quotes.is_empty() {
            pages.push(Page::Quotes);
        }
        if !self.sky.is_empty() {
            pages.push(Page::Sky);
        }
        pages
    }

    /// A página `elapsed` segundos depois de aparecer e, logo depois de uma troca, a anterior e
    /// quanto a troca já andou (0 a 1).
    fn page_at(&self, elapsed: f64) -> (Page, Option<(Page, f32)>) {
        let pages = self.pages();
        let k = (elapsed.max(0.0) / PAGE_FOR).floor() as usize;
        let into = elapsed - k as f64 * PAGE_FOR;
        let turn = (k > 0 && pages.len() > 1 && into < TURN).then(|| (pages[(k - 1) % pages.len()], (into / TURN) as f32));
        (pages[k % pages.len()], turn)
    }

    /// A frase do tempo na pílula esticada (cabe numa linha): "Bom dia · 18° agora, máx. 27°".
    fn banner(&self) -> String {
        let hello = self.period.hello();
        let w = &self.weather;
        let rest = match (self.period, w.now, w.max) {
            (Period::Night, ..) => self.weather_line(),
            (Period::Morning, Some(t), Some(m)) => Some(format!("{t}° agora, máx. {m}°")),
            (_, Some(t), Some(m)) => Some(format!("{t}°, máx. {m}°")),
            _ => self.weather_line(),
        };
        match rest {
            Some(r) => format!("{hello} · {r}"),
            None => format!("{hello}!"),
        }
    }

    /// O céu desta noite, curto: "Lua cheia às 18:57 · ISS às 21:00".
    fn sky_short(&self) -> String {
        let one = |(name, at, _): &(String, String, String)| if at == "hoje" { format!("{name} hoje") } else { format!("{name} às {at}") };
        self.sky.iter().map(one).collect::<Vec<_>>().join(" · ")
    }
}

/// O símbolo de cada cotação: a carteira (o dólar) e a moeda do bitcoin.
fn symbol(c: &mut Canvas, k: usize, cx: f32, cy: f32, r: f32) {
    if k == 0 {
        wallet(c, cx, cy, r * 2.0);
    } else {
        bitcoin(c, cx, cy, r);
    }
}

/// "5,94", "369 mil" (sem o R$: o símbolo do lado já diz o que é)
fn short_price(v: f64) -> String {
    if v >= 10000.0 {
        format!("{:.0} mil", v / 1000.0)
    } else {
        format!("{v:.2}").replace('.', ",")
    }
}

/// O preço inteiro: "R$ 5,94", "R$ 369.262".
fn long_price(k: usize, v: f64) -> String {
    brl(v, if k == 0 { 2 } else { 0 })
}

/// Uma página da saudação na pílula (fechada, esticada ou média).
fn draw_page(c: &mut Canvas, form: IslandForm, r: Rect4, s: &Shown, page: Page) {
    let (x, y, w, h) = r;
    let cy = y + h / 2.0;
    let color = s.period.color();
    let hello = s.period.hello();
    let art = |c: &mut Canvas, name: &str, cx: f32, cy: f32, size: f32| {
        let img = weather_art(name, (size * c.s).round() as u32);
        c.image(&img, cx - size / 2.0, cy - size / 2.0);
    };
    let col = |p: f64| if p < 0.0 { DOWN } else { UP };
    match page {
        Page::Weather => match form {
            IslandForm::Compact => {
                art(c, s.period.art(), x + 26.0, cy, 26.0);
                let value = match s.period {
                    Period::Night => s.weather.tomorrow.as_ref().map(|t| format!("amanhã {}°", t.0)),
                    _ => s.weather.now.map(|t| format!("{t}°")),
                };
                iu::compact_text(c, x + 46.0, r, hello, value.as_deref().unwrap_or(""), color);
            }
            IslandForm::Banner => {
                art(c, s.period.art(), x + 26.0, cy, 26.0);
                iu::compact_text(c, x + 46.0, r, &s.banner(), "", color);
            }
            _ => {
                art(c, s.period.art(), x + 39.0, cy, 46.0);
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &format!("{hello}!"), &s.weather_line().unwrap_or_default(), color);
            }
        },
        Page::Quotes => match form {
            // o dólar e o bitcoin lado a lado: o símbolo, o preço e a variação
            IslandForm::Compact | IslandForm::Banner => {
                // um depois do outro, cada um do tamanho que precisa, centralizados
                let parts: Vec<(usize, String, String, f64)> = s
                    .quotes
                    .iter()
                    .enumerate()
                    .take(2)
                    .map(|(k, &(_, price, p))| (k, if form == IslandForm::Banner { long_price(k, price) } else { short_price(price) }, pct(p), p))
                    .collect();
                let gap = if form == IslandForm::Banner { 20.0 } else { 12.0 };
                let width = |c: &Canvas, text: &str, change: &str| 25.0 + c.text_width(text, Face::Semibold, 12.5) + 4.0 + c.text_width(change, Face::Semibold, 11.5);
                let total: f32 = parts.iter().map(|(_, t, ch, _)| width(c, t, ch)).sum::<f32>() + gap * (parts.len().max(1) - 1) as f32;
                let mut x0 = x + ((w - total) / 2.0).max(14.0);
                for (k, text, change, p) in &parts {
                    symbol(c, *k, x0 + 10.0, cy, 9.5);
                    let tw = c.text(x0 + 25.0, cy, text, Face::Semibold, 12.5, FG, Anchor::LM, None);
                    c.text(x0 + 25.0 + tw + 4.0, cy, change, Face::Semibold, 11.5, col(*p), Anchor::LM, None);
                    x0 += width(c, text, change) + gap;
                }
            }
            // uma em cada linha, com o nome
            _ => {
                for (k, &(name, price, p)) in s.quotes.iter().enumerate().take(2) {
                    let ry = cy + if k == 0 { -12.0 } else { 12.0 };
                    symbol(c, k, x + 34.0, ry, 9.5);
                    c.text(x + 52.0, ry, name, Face::Regular, 12.0, MUTED, Anchor::LM, None);
                    c.text(x + 112.0, ry, &long_price(k, price), Face::Semibold, 13.0, FG, Anchor::LM, None);
                    c.text(x + w - 92.0, ry, &pct(p), Face::Semibold, 12.0, col(p), Anchor::RM, None);
                }
            }
        },
        Page::Sky => {
            let (name, at, _) = &s.sky[0];
            let icon = |c: &mut Canvas, cx: f32, size: f32| {
                if name == "ISS" {
                    draw_station(c, cx, cy, size * 0.46);
                } else {
                    art(c, "moon", cx, cy, size);
                }
            };
            match form {
                IslandForm::Compact => {
                    icon(c, x + 26.0, 26.0);
                    iu::compact_text(c, x + 46.0, r, &format!("{name} esta noite"), at, color);
                }
                IslandForm::Banner => {
                    icon(c, x + 26.0, 26.0);
                    iu::compact_text(c, x + 46.0, r, &format!("Esta noite · {}", s.sky_short()), "", color);
                }
                _ => {
                    icon(c, x + 39.0, 46.0);
                    iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, "Esta noite", &s.sky_short(), color);
                }
            }
        }
    }
}

pub struct Greeting {
    clock: Clock,
    showing: Option<Shown>,
    /// os períodos já cumprimentados: (dia, períodos)
    done: (i64, Vec<Period>),
    /// cotações buscadas pela própria saudação: (horário, [(preço, variação)])
    quotes: Option<(f64, [(f64, f64); 2])>,
    fetching: Option<(Receiver<Option<Value>>, Instant)>,
    /// quando tentou buscar as cotações (sem conexão, não fica tentando a cada 5 s)
    quotes_tried: f64,
    check_at: Instant,
    /// nos testes: (segundos parado, tela cheia)
    fake: Option<(f64, bool)>,
}

/// O período de `t` (None de madrugada). A noite começa às 18 h, ou no fim do expediente se ele
/// for antes (de segunda a sexta, com o fim do expediente ligado), logo depois do "bom descanso".
pub fn period_at(clock: &Clock, t: f64, cfg: &Value) -> Option<Period> {
    let secs = (t + clock.offset()).rem_euclid(86400.0);
    let mut night: f64 = 18.0 * 3600.0;
    if clock.weekday(t) < 5 && on_island(cfg, "expediente") {
        night = night.min(super::workday::end_of(cfg) + 15.0);
    }
    if secs < 6.0 * 3600.0 {
        None
    } else if secs < 12.0 * 3600.0 {
        Some(Period::Morning)
    } else if secs < night {
        Some(Period::Afternoon)
    } else {
        Some(Period::Night)
    }
}

/// O Pomodoro está no foco (correndo)?
fn focusing(cfg: &Value, now: f64) -> bool {
    let st = &cfg["pomodoro"]["estado"];
    st.str_of("fase") == Some("foco") && st.f64_of("fim").is_some_and(|end| end > now)
}

/// "Céu limpo" → "céu limpo"
fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_lowercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// O tempo para a saudação, do que o Tempo guardou.
fn weather_of(cache: &Value) -> Weather {
    let w = &cache["weather"];
    if !w.is_object() {
        return Weather::default();
    }
    let tomorrow = &w["day_info"][1];
    let tomorrow = tomorrow.i64_of("max").map(|max| (max, tomorrow.str_of("desc").map(lower_first).unwrap_or_default()));
    // a primeira hora de hoje com chuva (sem estar chovendo agora)
    let is_rain = |code: i64| (51..68).contains(&code) || (80..83).contains(&code) || code >= 95;
    let hours = w["hours"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let rain = if w.i64_of("code").is_some_and(is_rain) {
        None
    } else {
        hours.iter().filter(|h| h[2].is_i64() && h[5].as_i64() == Some(0)).skip(1).find(|h| h[3].as_i64().is_some_and(is_rain)).and_then(|h| h[6].as_i64())
    };
    Weather { now: w.i64_of("temp"), max: w.i64_of("max"), rain, tomorrow }
}

impl Greeting {
    pub fn new(cache: &Value) -> Greeting {
        let c = &cache["saudacao"];
        let done = (c.i64_of("dia").unwrap_or(0), c["feitas"].as_array().map(|l| l.iter().filter_map(|v| v.as_str().and_then(Period::from_code)).collect()).unwrap_or_default());
        Greeting { clock: Clock::default(), showing: None, done, quotes: None, fetching: None, quotes_tried: f64::MIN, check_at: Instant::now(), fake: None }
    }

    /// A saudação de um período já na ilha, com o tempo e as cotações de exemplo.
    #[cfg(test)]
    pub fn sample(period: Period) -> Greeting {
        let monday = crate::util::days_from_civil(2026, 11, 23) as f64 * 86400.0;
        let hour = match period {
            Period::Morning => 8.0,
            Period::Afternoon => 14.0,
            Period::Night => 19.0,
        };
        let now = monday + (hour + 3.0) * 3600.0;
        let mut g = Greeting::new(&Value::Null);
        g.clock = Clock::at(now, -3.0 * 3600.0);
        g.fake = Some((1.0, false));
        g.quotes = Some((now, [(5.94, 0.3), (369262.0, -1.8)]));
        let cfg = json!({"tempo": {"lat": -23.55, "lon": -46.63, "place": "São Paulo"}, "app": {}});
        let mut cache = sample_cache(now);
        let mut out = Vec::new();
        let mut c = cfg.clone();
        g.tick(&mut Ctx { cfg: &mut c, cache: &mut cache, key: "saudacao", out: &mut out });
        g
    }

    /// Na prévia das saudações: o relógio `secs` segundos depois de ela aparecer.
    #[cfg(test)]
    pub fn preview_at(&mut self, secs: f64) {
        if let Some(at) = self.showing.as_ref().map(|s| s.at) {
            self.clock = Clock::at(at + secs.clamp(0.0, SHOW_FOR - 0.01), self.clock.offset());
        }
    }

    /// A saudação pronta para aparecer agora, se for a hora (e a pessoa estiver no PC, sem tela
    /// cheia nem foco do Pomodoro).
    fn due(&self, cfg: &Value) -> Option<Period> {
        let now = self.clock.now();
        let period = period_at(&self.clock, now, cfg)?;
        if self.done.0 == self.clock.day(now) && self.done.1.contains(&period) {
            return None;
        }
        let (idle, fullscreen) = self.fake.unwrap_or_else(|| (crate::win::idle_secs(), !crate::win::fullscreen_screens().is_empty()));
        (idle < ACTIVE && !fullscreen && !focusing(cfg, now)).then_some(period)
    }

    /// As cotações de agora: as do aviso de cotação (se recentes) ou as buscadas aqui.
    fn quotes_now(&self, cache: &Value) -> Option<[(f64, f64); 2]> {
        let now = self.clock.now();
        let c = &cache["cotacao"];
        if c.f64_of("em").is_some_and(|t| now - t < QUOTES_FRESH) {
            let q = |code: &str| Some((c[code].f64_of("preco")?, c[code].f64_of("variacao")?));
            if let (Some(a), Some(b)) = (q("USD"), q("BTC")) {
                return Some([a, b]);
            }
        }
        self.quotes.filter(|q| now - q.0 < QUOTES_FRESH).map(|q| q.1)
    }

    /// Monta a saudação do período com o que se sabe agora.
    fn compose(&self, period: Period, cfg: &Value, cache: &Value) -> Shown {
        let now = self.clock.now();
        let weather = weather_of(cache);
        let quotes = match self.quotes_now(cache) {
            Some([usd, btc]) => vec![("Dólar", usd.0, usd.1), ("Bitcoin", btc.0, btc.1)],
            None => Vec::new(),
        };
        let mut sky = Vec::new();
        if period == Period::Night {
            if let Some((lat, lon, _)) = location(cfg) {
                let day = self.clock.day(now);
                let midnight = self.clock.midnight(day + 1);
                // lua cheia esta noite (a lua cheia mais perto das 21 h de hoje)
                let tonight = self.clock.midnight(day) + 21.0 * 3600.0;
                if astro::full_moons(tonight - 86400.0, tonight + 86400.0).iter().any(|&f| (f - tonight).abs() <= 12.0 * 3600.0) {
                    let line = match astro::next_moonrise(self.clock.midnight(day) + 12.0 * 3600.0, lat, lon) {
                        Some(t) if t > now => ("Lua cheia".to_string(), self.clock.hhmm(t), format!("Lua cheia nasce às {}, {}", self.clock.hhmm(t), toward(astro::moon_alt_az(t, lat, lon).1))),
                        _ => ("Lua cheia".to_string(), "hoje".to_string(), "Lua cheia no céu esta noite".to_string()),
                    };
                    sky.push(line);
                }
                // a Estação Espacial passando antes da meia-noite
                let passes = cache["iss_visiveis"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                if let Some(p) = passes.iter().find(|p| p[0].as_f64().is_some_and(|t| t > now && t < midnight)) {
                    let (t, az) = (p[0].as_f64().unwrap_or(now), p[1].as_f64().unwrap_or(0.0));
                    let at = self.clock.hhmm(t);
                    let long = format!("A ISS passa às {at}, olhe para o {}", astro::compass(az));
                    sky.push(("ISS".to_string(), at, long));
                }
            }
        }
        Shown { period, at: now, day: self.clock.day(now), weather, quotes, sky }
    }

    fn visible(&self) -> Option<&Shown> {
        self.showing.as_ref().filter(|s| self.clock.now() - s.at < SHOW_FOR)
    }

    fn save(&self, cache: &mut Value) {
        cache["saudacao"] = json!({"dia": self.done.0, "feitas": self.done.1.iter().map(|p| p.code()).collect::<Vec<_>>()});
    }
}

impl Kind for Greeting {
    fn key(&self) -> &'static str {
        "saudacao"
    }
    fn title(&self) -> &'static str {
        super::title("saudacao")
    }
    fn draw(&self, _c: &mut Canvas, _mode: Mode, _env: &Env) {}

    fn next_tick(&self) -> Option<Instant> {
        Some(self.check_at)
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        let wall = Instant::now();
        self.check_at = wall + Duration::from_secs(5);
        let now = self.clock.now();
        if self.showing.as_ref().is_some_and(|s| now - s.at >= SHOW_FOR) {
            self.showing = None;
        }
        // as cotações que a própria saudação foi buscar
        if let Some((rx, since)) = &self.fetching {
            match rx.try_recv() {
                Ok(data) => {
                    self.fetching = None;
                    let q = data.and_then(|d| {
                        let one = |k: &str| Some((d[k].str_of("bid")?.parse::<f64>().ok()?, d[k].str_of("pctChange")?.parse::<f64>().ok()?));
                        Some([one("USDBRL")?, one("BTCBRL")?])
                    });
                    if let Some(q) = q {
                        self.quotes = Some((now, q));
                    }
                }
                Err(TryRecvError::Empty) if since.elapsed() < QUOTES_WAIT => {
                    self.check_at = wall + Duration::from_millis(300);
                    return false;
                }
                _ => self.fetching = None,
            }
        }
        if !on_island(ctx.cfg, "saudacao") {
            return false;
        }
        let day = self.clock.day(now);
        if self.done.0 != day {
            self.done = (day, Vec::new());
        }
        let Some(period) = self.due(ctx.cfg) else { return false };
        // com as cotações: se não há uma recente, busca antes (espera até 4 s)
        if self.quotes_now(ctx.cache).is_none() && self.fetching.is_none() && now - self.quotes_tried > 10.0 * 60.0 {
            self.quotes_tried = now;
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::util::fetch_json(QUOTES_URL).ok());
                crate::events::wake();
            });
            self.fetching = Some((rx, wall));
            self.check_at = wall + Duration::from_millis(300);
            return false;
        }
        self.showing = Some(self.compose(period, ctx.cfg, ctx.cache));
        // os períodos de antes ficam para trás (ligou às 14 h: nada de "bom dia")
        for p in [Period::Morning, Period::Afternoon, Period::Night] {
            if !self.done.1.contains(&p) {
                self.done.1.push(p);
            }
            if p == period {
                break;
            }
        }
        self.save(ctx.cache);
        ctx.out.push(Request::SaveCache);
        self.check_at = wall + Duration::from_secs(1);
        true
    }

    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        let s = self.visible()?;
        Some(IslandItem::alert(s.period.color(), alert_hash(&format!("saudacao|{}|{}", s.day, s.period.code()))))
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, _env: &Env) {
        let Some(s) = self.visible() else { return };
        let (x, y, w, h) = r;
        let color = s.period.color();
        let hello = s.period.hello();
        match form {
            IslandForm::Bubble => {
                let img = weather_art(s.period.art(), (28.0 * c.s).round() as u32);
                c.image(&img, x + w / 2.0 - 14.0, y + h / 2.0 - 14.0);
            }
            IslandForm::Large => {
                let date = {
                    const DAYS: [&str; 7] = ["segunda", "terça", "quarta", "quinta", "sexta", "sábado", "domingo"];
                    format!("{}, {}", DAYS[self.clock.weekday(s.at) as usize], self.clock.date_text(s.at))
                };
                iu::header(c, r, &hello.to_uppercase(), color, &date, MUTED);
                let img = weather_art(s.period.art(), (48.0 * c.s).round() as u32);
                c.image(&img, x + w - 78.0, y + 42.0);
                c.text(x + 22.0, y + 60.0, &format!("{hello}!"), Face::Semibold, 22.0, FG, Anchor::LM, Some(w - 120.0));
                let mut ly = y + 88.0;
                if let Some(wl) = s.weather_line() {
                    let mut t = wl;
                    if let Some(f) = t.get_mut(0..1) {
                        f.make_ascii_uppercase();
                    }
                    c.text(x + 22.0, ly, &t, Face::Regular, 12.5, MUTED, Anchor::LM, Some(w - 44.0));
                    ly += 22.0;
                }
                for (k, &(name, price, p)) in s.quotes.iter().enumerate() {
                    symbol(c, k, x + 31.0, ly + 6.0, 9.0);
                    c.text(x + 48.0, ly + 6.0, name, Face::Regular, 12.0, MUTED, Anchor::LM, None);
                    c.text(x + 110.0, ly + 6.0, &long_price(k, price), Face::Semibold, 14.0, FG, Anchor::LM, None);
                    let col = if p < 0.0 { DOWN } else { UP };
                    c.text(x + 236.0, ly + 6.0, &format!("{} desde ontem", pct(p)), Face::Regular, 12.0, col, Anchor::LM, None);
                    ly += 24.0;
                }
                for (_, _, line) in &s.sky {
                    c.text(x + 22.0, ly + 6.0, line, Face::Regular, 12.5, FG, Anchor::LM, Some(w - 44.0));
                    ly += 22.0;
                }
                super::draw_buttons(c, form, r, hot, None);
            }
            _ => {
                // o tempo e as cotações (e, de noite, o céu) se alternando
                let (page, turn) = s.page_at(self.clock.now() - s.at);
                match turn {
                    Some((prev, t)) => {
                        let a = ease_in_out(t);
                        c.push_opacity(1.0 - a);
                        draw_page(c, form, (x, y - 9.0 * a, w, h), s, prev);
                        c.pop_clip();
                        c.push_opacity(a);
                        draw_page(c, form, (x, y + 9.0 * (1.0 - a), w, h), s, page);
                        c.pop_clip();
                    }
                    None => draw_page(c, form, r, s, page),
                }
                if form == IslandForm::Medium {
                    super::draw_buttons(c, form, r, hot, None);
                }
            }
        }
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        super::hit(form, x, y, w, h, false)
    }

    /// Na troca de página, quadros lisos; fora dela, o relógio da ilha basta.
    fn island_fps(&self, form: IslandForm) -> u32 {
        let Some(s) = self.visible() else { return 0 };
        if matches!(form, IslandForm::Bubble | IslandForm::Large) || s.pages().len() < 2 {
            return 0;
        }
        let into = (self.clock.now() - s.at).rem_euclid(PAGE_FOR);
        if into > PAGE_FOR - 0.3 || into < TURN + 0.1 { 60 } else { 4 }
    }

    /// OK: a saudação some (até o próximo período).
    fn act(&mut self, action: u32, _ctx: &mut Ctx) {
        if action == OK {
            self.showing = None;
        }
    }
}

/// O que o Tempo e a ISS deixam guardado (para os exemplos e os testes): 18° agora, máxima de
/// 27°, chuva às 15 h; amanhã 22° e céu limpo; a ISS passando às 19:42.
#[cfg(test)]
pub fn sample_cache(now: f64) -> Value {
    let hours: Vec<Value> = (8..20).map(|h| json!([format!("{h}h"), "sun", 20, if h == 15 { 61 } else { 1 }, 0, 0, h])).collect();
    json!({
        "weather": {"temp": 18, "max": 27, "code": 1, "hours": hours, "day_info": [{"max": 27, "desc": "Chuva"}, {"max": 22, "desc": "Céu limpo"}]},
        "iss_visiveis": [[now + 2.0 * 3600.0, 315.0, 62.0]],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Value {
        json!({"tempo": {"lat": -23.55, "lon": -46.63, "place": "São Paulo"}, "app": {}})
    }

    /// Uma segunda-feira, na hora local `hour`.
    fn at(hour: f64) -> f64 {
        crate::util::days_from_civil(2026, 11, 23) as f64 * 86400.0 + (hour + 3.0) * 3600.0
    }

    fn run(g: &mut Greeting, cfg: &Value, cache: &mut Value) -> bool {
        let mut out = Vec::new();
        let mut c = cfg.clone();
        let r = g.tick(&mut Ctx { cfg: &mut c, cache, key: "saudacao", out: &mut out });
        r
    }

    #[test]
    fn periods_of_the_day() {
        let clock = Clock::at(0.0, -3.0 * 3600.0);
        let cfg = cfg();
        assert_eq!(period_at(&clock, at(5.5), &cfg), None);
        assert_eq!(period_at(&clock, at(6.0), &cfg), Some(Period::Morning));
        assert_eq!(period_at(&clock, at(12.5), &cfg), Some(Period::Afternoon));
        // segunda, com o expediente até as 17:00: a noite começa logo depois do "bom descanso"
        assert_eq!(period_at(&clock, at(17.0), &cfg), Some(Period::Afternoon));
        assert_eq!(period_at(&clock, at(17.01), &cfg), Some(Period::Night));
        let no_work = json!({"expediente": {"na_ilha": false}, "app": {}});
        assert_eq!(period_at(&clock, at(17.5), &no_work), Some(Period::Afternoon));
        // sábado: às 18 h
        assert_eq!(period_at(&clock, at(17.5) + 5.0 * 86400.0, &cfg), Some(Period::Afternoon));
        assert_eq!(period_at(&clock, at(18.0) + 5.0 * 86400.0, &cfg), Some(Period::Night));
    }

    /// Uma vez em cada período, só com a pessoa no PC (sem tela cheia nem foco do Pomodoro);
    /// some sozinha em uns segundos; ligou o PC de tarde: nada de "bom dia".
    #[test]
    fn once_per_period_with_someone_there() {
        let cfg = cfg();
        let mut cache = sample_cache(at(8.0));
        cache["cotacao"] = json!({"em": at(7.9), "USD": {"preco": 5.94, "variacao": 0.3}, "BTC": {"preco": 369262.0, "variacao": -1.8}});
        let mut g = Greeting::new(&Value::Null);
        // 8 h, ninguém no PC
        g.clock = Clock::at(at(8.0), -3.0 * 3600.0);
        g.fake = Some((600.0, false));
        assert!(!run(&mut g, &cfg, &mut cache) && g.island_item(&Env { cfg: &cfg, key: "saudacao" }).is_none(), "ninguém olhando");
        // mexeu no mouse, mas em tela cheia
        g.fake = Some((2.0, true));
        assert!(!run(&mut g, &cfg, &mut cache));
        // agora sim
        g.fake = Some((2.0, false));
        assert!(run(&mut g, &cfg, &mut cache));
        let item = g.island_item(&Env { cfg: &cfg, key: "saudacao" }).expect("bom dia");
        assert!(item.alert);
        let s = g.visible().unwrap();
        assert_eq!(s.period, Period::Morning);
        assert_eq!(s.weather_line().as_deref(), Some("18° agora, máx. 27°, chuva às 15h"));
        assert_eq!(s.banner(), "Bom dia · 18° agora, máx. 27°");
        assert_eq!(s.quotes.len(), 2);
        // some sozinha
        g.clock = Clock::at(at(8.0) + SHOW_FOR + 0.5, -3.0 * 3600.0);
        run(&mut g, &cfg, &mut cache);
        assert!(g.island_item(&Env { cfg: &cfg, key: "saudacao" }).is_none());
        // a manhã inteira, nada mais
        g.clock = Clock::at(at(11.0), -3.0 * 3600.0);
        assert!(!run(&mut g, &cfg, &mut cache));
        // guardado: o app fechou e abriu, continua sem repetir
        let mut back = Greeting::new(&cache);
        back.clock = g.clock;
        back.fake = Some((1.0, false));
        assert!(!run(&mut back, &cfg, &mut cache));
        // de tarde, no foco do Pomodoro: espera
        let focus = json!({"tempo": cfg["tempo"], "pomodoro": {"estado": {"fase": "foco", "fim": at(14.5)}}, "app": {}});
        back.clock = Clock::at(at(14.0), -3.0 * 3600.0);
        cache["cotacao"]["em"] = json!(at(13.9));
        assert!(!run(&mut back, &focus, &mut cache));
        assert!(run(&mut back, &cfg, &mut cache), "boa tarde");
        assert_eq!(back.visible().unwrap().banner(), "Boa tarde · 18°, máx. 27°");
        // PC ligado só de noite, num dia novo: vem o "boa noite" direto, com a ISS
        let mut night = Greeting::new(&cache);
        night.clock = Clock::at(at(19.0) + 86400.0, -3.0 * 3600.0);
        cache["cotacao"]["em"] = json!(at(18.9) + 86400.0);
        night.fake = Some((1.0, false));
        cache["iss_visiveis"] = json!([[at(20.5) + 86400.0, 315.0, 60.0]]);
        assert!(run(&mut night, &cfg, &mut cache));
        let s = night.visible().unwrap();
        assert_eq!(s.period, Period::Night);
        assert_eq!(s.weather_line().as_deref(), Some("amanhã 22°, céu limpo"));
        assert_eq!(s.banner(), "Boa noite · amanhã 22°, céu limpo");
        assert!(s.sky.iter().any(|l| l.2.contains("ISS passa às 20:30")), "{:?}", s.sky);
        // 24/11/2026 é noite de lua cheia: a lua vem primeiro, a ISS depois
        assert_eq!(s.sky_short(), "Lua cheia hoje · ISS às 20:30");
        assert_eq!(s.pages(), vec![Page::Weather, Page::Quotes, Page::Sky], "de noite também com as cotações");
        assert!(night.done.1.contains(&Period::Morning), "a manhã ficou para trás");
    }

    /// Na noite da lua cheia (24/11/2026), o "boa noite" conta.
    #[test]
    fn good_night_tells_the_full_moon() {
        let cfg = cfg();
        let mut cache = sample_cache(0.0);
        cache["iss_visiveis"] = json!([]);
        let mut g = Greeting::new(&Value::Null);
        g.clock = Clock::at(at(19.0) + 86400.0, -3.0 * 3600.0);
        g.fake = Some((1.0, false));
        g.quotes = Some((g.clock.now(), [(5.94, 0.3), (369262.0, -1.8)]));
        assert!(run(&mut g, &cfg, &mut cache));
        assert!(g.visible().unwrap().sky.iter().any(|l| l.2.starts_with("Lua cheia")), "{:?}", g.visible().unwrap().sky);
    }

    /// O tempo por 7 s, as cotações por 7 s, e de novo, até o tempo da saudação acabar (ou o OK);
    /// na troca, a página de antes ainda aparece saindo.
    #[test]
    fn pages_take_turns_every_7_seconds() {
        let mut g = Greeting::sample(Period::Morning);
        let start = g.visible().unwrap().at;
        assert_eq!(g.visible().unwrap().pages(), vec![Page::Weather, Page::Quotes]);
        let off = -3.0 * 3600.0;
        for (secs, page, turning) in [(1.0, Page::Weather, false), (6.9, Page::Weather, false), (7.1, Page::Quotes, true), (8.0, Page::Quotes, false), (14.2, Page::Weather, true), (22.0, Page::Quotes, false)] {
            g.clock = Clock::at(start + secs, off);
            let (p, turn) = g.visible().unwrap().page_at(secs);
            assert_eq!((p, turn.is_some()), (page, turning), "{secs} s");
        }
        // quadros lisos só perto da troca
        g.clock = Clock::at(start + 7.1, off);
        assert_eq!(g.island_fps(IslandForm::Compact), 60);
        g.clock = Clock::at(start + 3.0, off);
        assert_eq!(g.island_fps(IslandForm::Compact), 4);
        g.clock = Clock::at(start + SHOW_FOR - 0.5, off);
        assert!(g.visible().is_some(), "ainda na ilha perto do fim");
        g.clock = Clock::at(start + SHOW_FOR + 0.1, off);
        assert!(g.visible().is_none(), "o tempo da saudação acabou");
        // o OK fecha antes
        let mut g = Greeting::sample(Period::Afternoon);
        let (mut cfg, mut cache, mut out) = (cfg(), json!({}), Vec::new());
        g.act(OK, &mut Ctx { cfg: &mut cfg, cache: &mut cache, key: "saudacao", out: &mut out });
        assert!(g.visible().is_none());
        assert_eq!(short_price(369262.0), "369 mil");
        assert_eq!(short_price(5.9389), "5,94");
    }

    /// Desligada nas Configurações, nada.
    #[test]
    fn off_means_off() {
        let cfg = json!({"saudacao": {"na_ilha": false}, "app": {}});
        let mut cache = sample_cache(0.0);
        let mut g = Greeting::new(&Value::Null);
        g.clock = Clock::at(at(9.0), -3.0 * 3600.0);
        g.fake = Some((1.0, false));
        assert!(!run(&mut g, &cfg, &mut cache));
    }
}
