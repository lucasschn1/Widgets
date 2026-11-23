//! A cotação do dólar e do bitcoin na ilha: um aviso quando um deles varia forte no dia (o dólar
//! mais de 1,5%, o bitcoin mais de 5%), com o preço e um gráfico pequeno do dia. Some com o OK;
//! só avisa de novo se a variação voltar para perto de zero e cruzar o limite outra vez.
//! Os preços vêm da AwesomeAPI (economia.awesomeapi.com.br), de 5 em 5 minutos.

use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{draw_buttons, hit, Clock, OK, OPEN};
use crate::data::config::Get;
use crate::gfx::{Anchor, Canvas, Face};
use crate::ui::theme::{Rgba, FG, MUTED, WHITE};
use crate::widgets::island_ui as iu;
use crate::widgets::{alert_hash, on_island, Ctx, Env, IslandForm, IslandItem, Kind, Mode, Rect4, Request};

pub(super) const URL: &str = "https://economia.awesomeapi.com.br/json/last/USD-BRL,BTC-BRL";
const EVERY: Duration = Duration::from_secs(5 * 60);
const UP: Rgba = Rgba::rgb(48, 209, 88);
const DOWN: Rgba = Rgba::rgb(255, 92, 80);
/// a variação precisa voltar para menos que isto do limite para poder avisar de novo
const REARM: f64 = 0.7;
/// pontos do gráfico do dia (de 5 em 5 minutos: um dia inteiro)
const MAX_SAMPLES: usize = 300;
/// de quanto em quanto tempo ver se é hora de buscar (e, buscando, se a resposta chegou)
const CHECK_EVERY: Duration = Duration::from_secs(30);
const FETCH_POLL: Duration = Duration::from_millis(300);

/// Um ativo acompanhado: o código (no cache), a chave na resposta da AwesomeAPI, o nome, o
/// limite que avisa (variação em %) e as casas decimais do preço.
struct AssetInfo {
    code: &'static str,
    field: &'static str,
    name: &'static str,
    limit: f64,
    decimals: usize,
}

const ASSETS: [AssetInfo; 2] = [
    AssetInfo { code: "USD", field: "USDBRL", name: "Dólar", limit: 1.5, decimals: 2 },
    AssetInfo { code: "BTC", field: "BTCBRL", name: "Bitcoin", limit: 5.0, decimals: 0 },
];

/// O que se sabe de um ativo hoje.
#[derive(Clone, Debug, PartialEq)]
struct Asset {
    bid: f64,
    pct: f64,
    high: f64,
    low: f64,
    /// os preços de hoje: (horário, preço)
    samples: Vec<(f64, f64)>,
    /// quantas vezes cruzou o limite hoje (muda o aviso)
    count: u32,
    /// pode avisar (a variação está perto de zero)
    armed: bool,
}

impl Default for Asset {
    fn default() -> Asset {
        Asset { bid: 0.0, pct: 0.0, high: 0.0, low: 0.0, samples: Vec::new(), count: 0, armed: true }
    }
}

pub struct Quote {
    clock: Clock,
    assets: [Asset; 2],
    /// o dia (local) dos dados
    day: i64,
    updated: f64,
    ok: Vec<String>,
    pending: Option<Receiver<Option<Value>>>,
    check_at: Instant,
    next_fetch: Instant,
}

/// "R$ 5,94", "R$ 369.262"
pub fn brl(value: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value.abs());
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    let sign = if value < 0.0 { "−" } else { "" };
    if frac.is_empty() { format!("R$ {sign}{grouped}") } else { format!("R$ {sign}{grouped},{frac}") }
}

/// "+1,6%", "−6,4%"
pub fn pct(change: f64) -> String {
    let sign = if change < 0.0 { "−" } else { "+" };
    format!("{sign}{:.1}%", change.abs()).replace('.', ",")
}

impl Quote {
    pub fn new(cache: &Value) -> Quote {
        let mut quote = Quote {
            clock: Clock::default(),
            assets: [Asset::default(), Asset::default()],
            day: 0,
            updated: 0.0,
            ok: Vec::new(),
            pending: None,
            check_at: Instant::now(),
            next_fetch: Instant::now(),
        };
        quote.restore(&cache["cotacao"]);
        quote
    }

    /// O dólar e o bitcoin com estas variações (e um dia de preços), às 14:30.
    #[cfg(test)]
    pub fn sample(usd: f64, btc: f64) -> Quote {
        let now = crate::data::util::days_from_civil(2026, 11, 23) as f64 * 86400.0 + 17.5 * 3600.0;
        let mut quote = Quote::new(&Value::Null);
        quote.clock = Clock::at(now, -3.0 * 3600.0);
        // 61 preços de 5 em 5 minutos, andando até a variação pedida (com uma ondulação)
        for (i, (base, change)) in [(5.94, usd), (369262.0, btc)].into_iter().enumerate() {
            for step in 0..=60 {
                let along = step as f64 / 60.0;
                let price = base * (1.0 + change / 100.0 * along + 0.002 * (along * 17.0).sin());
                quote.receive_one(i, price, change * along, base * 1.03, base * 0.97, now - (60 - step) as f64 * 300.0);
            }
        }
        quote
    }

    /// Lê o que ficou guardado no cache.
    fn restore(&mut self, saved: &Value) {
        self.day = saved.i64_of("dia").unwrap_or(0);
        self.updated = saved.f64_of("em").unwrap_or(0.0);
        self.ok = saved["ok"].as_array().map(|keys| keys.iter().filter_map(|key| key.as_str().map(String::from)).collect()).unwrap_or_default();
        for (i, info) in ASSETS.iter().enumerate() {
            let asset = &saved[info.code];
            if !asset.is_object() {
                continue;
            }
            self.assets[i] = Asset {
                bid: asset.f64_of("preco").unwrap_or(0.0),
                pct: asset.f64_of("variacao").unwrap_or(0.0),
                high: asset.f64_of("max").unwrap_or(0.0),
                low: asset.f64_of("min").unwrap_or(0.0),
                samples: asset["dia"].as_array().map(|list| list.iter().filter_map(|point| Some((point[0].as_f64()?, point[1].as_f64()?))).collect()).unwrap_or_default(),
                count: asset.i64_of("avisos").unwrap_or(0) as u32,
                armed: asset.bool_of("pronto").unwrap_or(true),
            };
        }
    }

    /// O que guardar no cache.
    fn save(&self) -> Value {
        let mut saved = json!({"dia": self.day, "em": self.updated, "ok": self.ok});
        for (i, info) in ASSETS.iter().enumerate() {
            let asset = &self.assets[i];
            saved[info.code] = json!({"preco": asset.bid, "variacao": asset.pct, "max": asset.high, "min": asset.low, "avisos": asset.count, "pronto": asset.armed,
                "dia": asset.samples.iter().map(|&(at, price)| json!([at, price])).collect::<Vec<_>>()});
        }
        saved
    }

    /// Um preço novo do ativo `i`: guarda no gráfico do dia e vê se cruzou o limite.
    fn receive_one(&mut self, i: usize, bid: f64, pct: f64, high: f64, low: f64, at: f64) {
        let day = self.clock.day(at);
        if day != self.day {
            // dia novo: o gráfico e os avisos recomeçam
            self.day = day;
            for asset in &mut self.assets {
                asset.samples.clear();
                asset.count = 0;
                asset.armed = true;
            }
            self.ok.clear();
        }
        let limit = ASSETS[i].limit;
        let asset = &mut self.assets[i];
        (asset.bid, asset.pct, asset.high, asset.low) = (bid, pct, high, low);
        asset.samples.push((at, bid));
        if asset.samples.len() > MAX_SAMPLES {
            asset.samples.remove(0);
        }
        if pct.abs() >= limit {
            if asset.armed {
                asset.armed = false;
                asset.count += 1;
            }
        } else if pct.abs() < limit * REARM {
            asset.armed = true;
        }
        self.updated = at;
    }

    /// Lê a resposta da AwesomeAPI (os números vêm como texto).
    fn receive(&mut self, data: &Value, at: f64) {
        for (i, info) in ASSETS.iter().enumerate() {
            let answer = &data[info.field];
            let num = |key: &str| answer.str_of(key).and_then(|text| text.parse::<f64>().ok());
            if let (Some(bid), Some(change)) = (num("bid"), num("pctChange")) {
                self.receive_one(i, bid, change, num("high").unwrap_or(bid), num("low").unwrap_or(bid), at);
            }
        }
    }

    /// A chave do aviso do ativo `i` (muda a cada vez que ele cruza o limite no dia).
    fn key(&self, i: usize) -> String {
        format!("{}|{}|{}", ASSETS[i].code, self.day, self.assets[i].count)
    }

    /// Os ativos com aviso agora (o dólar primeiro): acima do limite, ainda não dispensados.
    fn alerts(&self) -> Vec<usize> {
        let today = self.clock.day(self.clock.now()) == self.day;
        (0..ASSETS.len())
            .filter(|&i| {
                let asset = &self.assets[i];
                today && asset.count > 0 && asset.pct.abs() >= ASSETS[i].limit && !self.ok.contains(&self.key(i))
            })
            .collect()
    }

    /// A cor da variação do ativo `i`: verde subindo, vermelho caindo.
    fn color(&self, i: usize) -> Rgba {
        if self.assets[i].pct < 0.0 { DOWN } else { UP }
    }

    /// "Dólar subiu 1,6% hoje"
    fn headline(&self, i: usize) -> String {
        let asset = &self.assets[i];
        let verb = if asset.pct < 0.0 { "caiu" } else { "subiu" };
        format!("{} {verb} {}% hoje", ASSETS[i].name, format!("{:.1}", asset.pct.abs()).replace('.', ","))
    }
}

/// A cor da carteira do dólar.
const WALLET: Rgba = Rgba::rgb(64, 196, 120);
const BITCOIN: Rgba = Rgba::rgb(247, 147, 26);

/// Uma carteira (o dólar) com `size` de largura: a nota aparecendo atrás, o corpo e o fecho.
pub(super) fn wallet(c: &mut Canvas, cx: f32, cy: f32, size: f32) {
    // a nota saindo por cima
    c.fill_round_rect(cx - 0.36 * size, cy - 0.44 * size, cx + 0.30 * size, cy - 0.12 * size, 0.06 * size, Rgba::rgb(170, 232, 190));
    // o corpo
    c.fill_round_rect(cx - 0.5 * size, cy - 0.26 * size, cx + 0.5 * size, cy + 0.40 * size, 0.13 * size, WALLET);
    c.fill_round_rect(cx - 0.5 * size, cy - 0.26 * size, cx + 0.5 * size, cy - 0.16 * size, 0.05 * size, Rgba(255, 255, 255, 60));
    // a aba do fecho, com o botão
    c.fill_round_rect(cx + 0.10 * size, cy - 0.04 * size, cx + 0.56 * size, cy + 0.24 * size, 0.09 * size, Rgba::rgb(36, 140, 82));
    c.fill_circle(cx + 0.33 * size, cy + 0.10 * size, 0.06 * size, Rgba::rgb(220, 255, 230));
}

/// A moeda do bitcoin: o círculo laranja com o ₿.
pub(super) fn bitcoin(c: &mut Canvas, cx: f32, cy: f32, r: f32) {
    c.fill_circle(cx, cy, r, BITCOIN);
    c.text(cx + r * 0.04, cy, "₿", Face::Semibold, r * 1.35, WHITE, Anchor::MM, None);
}

/// O símbolo do ativo (a carteira do dólar, a moeda do bitcoin) num círculo na cor da variação.
fn badge(c: &mut Canvas, cx: f32, cy: f32, r: f32, i: usize, color: Rgba) {
    c.fill_circle(cx, cy, r, color.alpha(0.2));
    if i == 0 {
        wallet(c, cx, cy, r * 1.15);
    } else {
        bitcoin(c, cx, cy, r * 0.68);
    }
}

/// O gráfico do dia no retângulo (x0, y0, x1, y1).
fn sparkline(c: &mut Canvas, x0: f32, y0: f32, x1: f32, y1: f32, samples: &[(f64, f64)], color: Rgba) {
    if samples.len() < 2 {
        return;
    }
    let (lo, hi) = samples.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &(_, price)| (lo.min(price), hi.max(price)));
    let (t0, t1) = (samples[0].0, samples[samples.len() - 1].0);
    // (o preço parado o dia todo ainda desenha uma linha, sem dividir por zero)
    let span = (hi - lo).max(hi.abs() * 1e-4).max(1e-9);
    let pts: Vec<(f32, f32)> = samples
        .iter()
        .map(|&(at, price)| (x0 + (x1 - x0) * ((at - t0) / (t1 - t0).max(1.0)) as f32, y1 - (y1 - y0) * ((price - lo) / span) as f32))
        .collect();
    c.line(&pts, color, 1.6);
    if let Some(&(x, y)) = pts.last() {
        c.fill_circle(x, y, 2.6, color);
    }
}

impl Kind for Quote {
    fn key(&self) -> &'static str {
        "cotacao"
    }
    fn title(&self) -> &'static str {
        super::title("cotacao")
    }
    fn draw(&self, _c: &mut Canvas, _mode: Mode, _env: &Env) {}

    fn next_tick(&self) -> Option<Instant> {
        Some(self.check_at)
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        let wall = Instant::now();
        self.check_at = wall + CHECK_EVERY;
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(data) => {
                    self.pending = None;
                    if let Some(data) = data {
                        self.receive(&data, self.clock.now());
                        ctx.cache["cotacao"] = self.save();
                        ctx.out.push(Request::SaveCache);
                    }
                    return true;
                }
                Err(TryRecvError::Empty) => {
                    self.check_at = wall + FETCH_POLL;
                    return false;
                }
                Err(TryRecvError::Disconnected) => self.pending = None,
            }
        }
        // busca para o aviso e para a ilha em repouso (se ela mostra as cotações)
        let wanted = on_island(ctx.cfg, "cotacao") || super::rest::wants(ctx.cfg, super::rest::Look::Quotes);
        if wall >= self.next_fetch && wanted {
            self.next_fetch = wall + EVERY;
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(crate::data::util::fetch_json(URL).ok());
                crate::platform::events::wake();
            });
            self.pending = Some(rx);
            self.check_at = wall + FETCH_POLL;
        }
        false
    }

    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        let i = *self.alerts().first()?;
        Some(IslandItem::alert(self.color(i), alert_hash(&self.key(i))))
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, _env: &Env) {
        let alerts = self.alerts();
        let Some(&i) = alerts.first() else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let asset = &self.assets[i];
        let (name, decimals) = (ASSETS[i].name, ASSETS[i].decimals);
        let color = self.color(i);
        match form {
            IslandForm::Bubble => badge(c, x + w / 2.0, cy, 13.0, i, color),
            IslandForm::Compact => {
                badge(c, x + 26.0, cy, 12.0, i, color);
                iu::compact_text(c, x + 46.0, r, &format!("{name} {}", brl(asset.bid, decimals)), &pct(asset.pct), color);
            }
            IslandForm::Banner => {
                badge(c, x + 26.0, cy, 12.0, i, color);
                iu::compact_text(c, x + 46.0, r, &self.headline(i), &brl(asset.bid, decimals), color);
            }
            IslandForm::Medium => {
                badge(c, x + 39.0, cy, 20.0, i, color);
                let max_w = w - 74.0 - 86.0;
                // o mínimo e o máximo do dia, se couberem (no bitcoin, com os milhares, não cabem)
                let full = format!("{} · mín. {} · máx. {}", brl(asset.bid, decimals), brl(asset.low, decimals).trim_start_matches("R$ "), brl(asset.high, decimals).trim_start_matches("R$ "));
                let sub = if c.text_width(&full, Face::Regular, 11.5) <= max_w { full } else { format!("{} agora", brl(asset.bid, decimals)) };
                iu::two_lines(c, x + 74.0, cy, max_w, &self.headline(i), &sub, color);
                draw_buttons(c, form, r, hot, None);
            }
            IslandForm::Large => {
                iu::header(c, r, "COTAÇÃO", color, &format!("às {}", self.clock.hhmm(self.updated)), MUTED);
                // as duas, uma em cada linha: nome, preço, variação e o gráfico do dia
                for (row, which) in [0usize, 1].into_iter().enumerate() {
                    let asset = &self.assets[which];
                    let ry = y + 58.0 + 52.0 * row as f32;
                    let col = self.color(which);
                    let strong = alerts.contains(&which);
                    badge(c, x + 36.0, ry + 4.0, 14.0, which, col);
                    c.text(x + 60.0, ry - 6.0, ASSETS[which].name, Face::Regular, 11.5, MUTED, Anchor::LM, None);
                    c.text(x + 60.0, ry + 12.0, &brl(asset.bid, ASSETS[which].decimals), Face::Semibold, if strong { 17.0 } else { 15.0 }, FG, Anchor::LM, Some(130.0));
                    c.text(x + 222.0, ry + 4.0, &pct(asset.pct), Face::Semibold, 13.0, col, Anchor::RM, None);
                    sparkline(c, x + 236.0, ry - 10.0, x + w - 24.0, ry + 16.0, &asset.samples, col);
                }
                draw_buttons(c, form, r, hot, Some("Gráfico"));
            }
        }
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        hit(form, x, y, w, h, true)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            OK => {
                for i in self.alerts() {
                    self.ok.push(self.key(i));
                }
                ctx.cache["cotacao"] = self.save();
                ctx.out.push(Request::SaveCache);
            }
            OPEN => {
                let i = self.alerts().first().copied().unwrap_or(0);
                let search = if i == 0 { "dólar hoje" } else { "bitcoin em reais" };
                ctx.open(format!("https://www.google.com/search?q={}", crate::data::util::urlencode(search)));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(cfg: &Value) -> Env<'_> {
        Env { cfg, key: "cotacao" }
    }

    #[test]
    fn money_reads_in_portuguese() {
        assert_eq!(brl(5.9389, 2), "R$ 5,94");
        assert_eq!(brl(369262.4, 0), "R$ 369.262");
        assert_eq!(brl(1234567.0, 0), "R$ 1.234.567");
        assert_eq!(brl(12.0, 2), "R$ 12,00");
        assert_eq!(pct(1.62), "+1,6%");
        assert_eq!(pct(-6.44), "−6,4%");
    }

    /// Avisa ao cruzar o limite (1,5% no dólar, 5% no bitcoin), uma vez só enquanto a variação
    /// continua alta; perto de zero de novo, pode avisar outra vez (um aviso novo). O OK
    /// dispensa; um dia novo recomeça.
    #[test]
    fn alerts_on_strong_moves_only() {
        let cfg = json!({"app": {}});
        let now = crate::data::util::days_from_civil(2026, 11, 23) as f64 * 86400.0 + 15.0 * 3600.0;
        let mut q = Quote::new(&Value::Null);
        q.clock = Clock::at(now, -3.0 * 3600.0);
        q.receive_one(0, 5.9, 0.9, 5.9, 5.9, now);
        q.receive_one(1, 370000.0, 4.0, 1.0, 1.0, now);
        assert!(q.island_item(&env(&cfg)).is_none(), "abaixo dos limites");
        q.receive_one(0, 6.0, 1.6, 6.0, 5.9, now + 300.0);
        let first = q.island_item(&env(&cfg)).expect("o dólar passou de 1,5%");
        assert!(first.alert);
        q.receive_one(0, 6.01, 1.8, 6.0, 5.9, now + 600.0);
        assert_eq!(q.island_item(&env(&cfg)).map(|i| i.alert_id), Some(first.alert_id), "o mesmo aviso enquanto continua alta");
        q.receive_one(0, 5.98, 1.2, 6.0, 5.9, now + 900.0);
        assert!(q.island_item(&env(&cfg)).is_none(), "voltou para baixo do limite: some");
        q.receive_one(0, 6.02, 1.7, 6.0, 5.9, now + 1200.0);
        let back_up = q.island_item(&env(&cfg)).expect("subiu de novo");
        assert_eq!(back_up.alert_id, first.alert_id, "sem ter chegado perto de zero, é o mesmo aviso (a ilha não estica de novo)");
        q.receive_one(0, 5.95, 0.5, 6.0, 5.9, now + 1500.0);
        q.receive_one(0, 6.05, 1.9, 6.0, 5.9, now + 1800.0);
        let second = q.island_item(&env(&cfg)).expect("cruzou de novo");
        assert_ne!(second.alert_id, first.alert_id);
        // o bitcoin caindo mais de 5%
        q.receive_one(1, 350000.0, -5.2, 1.0, 1.0, now + 1800.0);
        assert_eq!(q.alerts(), vec![0, 1]);
        let mut out = Vec::new();
        let (mut cfg2, mut cache) = (cfg.clone(), json!({}));
        q.act(OK, &mut Ctx { cfg: &mut cfg2, cache: &mut cache, key: "cotacao", out: &mut out });
        assert!(q.island_item(&env(&cfg)).is_none(), "o OK dispensa os dois");
        // guardado e lido de volta (o app fechou e abriu)
        let back = Quote::new(&cache);
        assert_eq!(back.assets, q.assets);
        assert_eq!(back.ok, q.ok);
        // dia seguinte: recomeça
        q.clock = Clock::at(now + 86400.0, -3.0 * 3600.0);
        q.receive_one(0, 6.1, 1.6, 6.1, 6.0, now + 86400.0);
        assert!(q.island_item(&env(&cfg)).is_some());
        assert_eq!(q.assets[0].samples.len(), 1, "o gráfico recomeça no dia novo");
    }

    #[test]
    fn reads_the_api_answer() {
        let data = json!({"USDBRL": {"bid": "5.9389", "pctChange": "1.62", "high": "5.95", "low": "5.85"}, "BTCBRL": {"bid": "369262", "pctChange": "-1.769", "high": "379309", "low": "365765"}});
        let mut q = Quote::new(&Value::Null);
        let now = crate::data::util::days_from_civil(2026, 11, 23) as f64 * 86400.0 + 15.0 * 3600.0;
        q.clock = Clock::at(now, -3.0 * 3600.0);
        q.receive(&data, now);
        assert_eq!(q.assets[0].bid, 5.9389);
        assert_eq!(q.assets[1].pct, -1.769);
        assert_eq!(q.alerts(), vec![0]);
        assert_eq!(q.headline(0), "Dólar subiu 1,6% hoje");
    }
}
