//! Widget do GitHub: o mapa de contribuições do último ano, em quadrados que ocupam o cartão (e o
//! aviso quando alguém pede sua revisão).

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::island_ui::{self as iu, SOFT};
use super::{alert_hash, Ctx, Env, Hot, IslandForm, IslandItem, Kind, Layout, MenuItem, Mode, NotifyAction, Rect4, Request, Status, LARGE_H, MEDIUM_H, SMALL, WIDE};
use crate::data::config::Get;
use crate::gfx::{Anchor, Canvas, Face};
use crate::ui::theme::*;
use crate::data::util::{self, MONTHS, WEEKDAYS};
use crate::platform::win;

const GITHUB_GREEN: Rgba = Rgba::rgb(63, 185, 80);

/// Um dia do mapa de contribuições (a ordem dos campos é a da ordenação: pelo dia).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DayCount {
    /// dias desde 1970
    day: i64,
    /// a cor do quadrado (0 a 4)
    level: i64,
    /// quantas contribuições
    count: i64,
}

#[derive(Clone)]
pub struct Data {
    user: String,
    /// os dias do mapa, em ordem
    days: Vec<DayCount>,
    total: i64,
}

impl Data {
    /// Para o cache: cada dia como ["2026-11-17", nível, quantidade].
    fn to_json(&self) -> Value {
        let days: Vec<Value> = self
            .days
            .iter()
            .map(|entry| {
                let date = util::civil_from_days(entry.day);
                json!([format!("{:04}-{:02}-{:02}", date.year, date.month, date.day), entry.level, entry.count])
            })
            .collect();
        json!({"user": self.user, "days": days, "total": self.total})
    }

    /// (o cache das versões com a lista de atividades também tem "rows": fica de fora)
    fn from_json(saved: &Value) -> Option<Data> {
        let days = saved["days"]
            .as_array()?
            .iter()
            .map(|entry| Some(DayCount { day: util::parse_date(entry[0].as_str()?)?, level: entry[1].as_i64()?, count: entry[2].as_i64()? }))
            .collect::<Option<Vec<_>>>()?;
        Some(Data { user: saved.str_of("user")?.into(), days, total: saved.i64_of("total").unwrap_or(0) })
    }
}

// ---------- rede (fora da thread da interface) ----------

/// Token do GitHub CLI (`gh`), se ele estiver instalado e logado; libera os repositórios privados.
fn gh_token() -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("gh")
        .args(["auth", "token"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: sem piscar um console
        .output()
        .ok()?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!token.is_empty()).then_some(token)
}

/// Um GET na API do GitHub (com o token, se houver).
fn api(path: &str, token: &str) -> Result<Value, String> {
    let auth = format!("Bearer {token}");
    let mut headers = vec![("Accept", "application/vnd.github+json")];
    if !token.is_empty() {
        headers.push(("Authorization", auth.as_str()));
    }
    util::fetch_json_with(&format!("https://api.github.com{path}"), &headers)
}

/// Valor de um atributo dentro de uma tag HTML (`name="valor"`).
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(&tag[start..end])
}

/// Por que a busca não trouxe o mapa.
enum Failure {
    NotFound(String),
    Offline,
    NoUser,
}

/// Dias do mapa de contribuições do último ano e o total.
fn contributions(user: &str) -> Result<(Vec<DayCount>, i64), Failure> {
    let url = format!("https://github.com/users/{}/contributions", util::urlencode(user));
    let (status, html) = util::fetch_text(&url, &[]).map_err(|_| Failure::Offline)?;
    if status == 404 {
        return Err(Failure::NotFound(format!("Usuário não encontrado: {user}")));
    }
    if status >= 400 {
        return Err(Failure::Offline);
    }
    // a quantidade de cada dia está no texto do tooltip ligado à célula ("3 contributions on…")
    let mut tips: HashMap<&str, i64> = HashMap::new();
    for chunk in html.split("<tool-tip").skip(1) {
        let Some(end) = chunk.find('>') else { continue };
        let (tag, rest) = (&chunk[..end], &chunk[end + 1..]);
        if let Some(id) = attr(tag, "for") {
            let text = rest.split("</tool-tip>").next().unwrap_or("");
            let digits: String = text.trim().chars().take_while(char::is_ascii_digit).collect();
            tips.insert(id, digits.parse().unwrap_or(0));
        }
    }
    let mut days = Vec::new();
    for chunk in html.split("<td").skip(1) {
        let Some(end) = chunk.find('>') else { continue };
        let tag = &chunk[..end];
        let Some(date) = attr(tag, "data-date").and_then(util::parse_date) else { continue };
        let level = attr(tag, "data-level").and_then(|level| level.parse().ok()).unwrap_or(0);
        let count = attr(tag, "id").and_then(|id| tips.get(id)).copied().unwrap_or(0);
        days.push(DayCount { day: date, level, count });
    }
    if days.is_empty() {
        // a página do GitHub mudou de formato
        return Err(Failure::Offline);
    }
    days.sort();
    // o total vem do texto "1,234 contributions in the last year" (sem ele, a soma dos dias)
    let total = html
        .find(" in the last year")
        .and_then(|at| {
            let before = &html[..at];
            let before = before.trim_end().strip_suffix("contributions").or(before.trim_end().strip_suffix("contribution"))?;
            let digits: String = before.trim_end().chars().rev().take_while(|ch| ch.is_ascii_digit() || *ch == ',' || *ch == '.').collect();
            digits.chars().rev().filter(char::is_ascii_digit).collect::<String>().parse().ok()
        })
        .unwrap_or_else(|| days.iter().map(|entry| entry.count).sum());
    Ok((days, total))
}

#[derive(Clone, Debug)]
struct Review {
    id: i64,
    title: String,
    url: String,
    repo: String,
}

/// O que uma busca bem-sucedida traz: o mapa, os pedidos de revisão (se pedidos e com token) e
/// o token usado (para as próximas buscas não chamarem o `gh` de novo).
struct Fetched {
    data: Data,
    reviews: Option<Vec<Review>>,
    token: String,
}

type FetchResult = Result<Fetched, Failure>;

/// Busca tudo (fora da thread da interface): descobre o usuário pelo token se ele não foi dado,
/// lê o mapa de contribuições e, com `want_reviews`, os pedidos de revisão abertos.
fn fetch(user: Option<String>, want_reviews: bool, token: Option<String>) -> FetchResult {
    let token = token.or_else(gh_token).unwrap_or_default();
    let user = match user.filter(|name| !name.is_empty()) {
        Some(name) => name,
        None => {
            if token.is_empty() {
                return Err(Failure::NoUser);
            }
            api("/user", &token).map_err(|_| Failure::Offline)?.str_of("login").ok_or(Failure::NoUser)?.to_string()
        }
    };
    let (days, total) = contributions(&user)?;
    let mut reviews = None;
    if want_reviews && !token.is_empty() {
        let query = util::urlencode(&format!("is:pr is:open review-requested:{user}"));
        if let Ok(found) = api(&format!("/search/issues?q={query}&per_page=20"), &token) {
            reviews = Some(
                found["items"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|item| Review {
                        id: item.i64_of("id").unwrap_or(0),
                        title: item.str_of("title").unwrap_or("").into(),
                        url: item.str_of("html_url").unwrap_or("").into(),
                        repo: item.str_of("repository_url").unwrap_or("").rsplit('/').next().unwrap_or("").into(),
                    })
                    .collect(),
            );
        }
    }
    Ok(Fetched { data: Data { user, days, total }, reviews, token })
}

// ---------- o widget ----------
//
// Só o mapa de contribuições, em quadrados de cantos arredondados que ocupam o cartão: quanto
// maior o widget, mais semanas e maiores os quadrados. Pequeno: 8 semanas e o total em cima.
// Médio: 18 semanas, com o rótulo e o total. Grande: o ano inteiro em três faixas de 18 semanas,
// cada uma com o período e quantas contribuições teve.

const W: f32 = WIDE;
const PAD: f32 = super::CARD_PAD;
const HEAD_Y: f32 = super::LABEL_Y;
/// espaço entre os quadrados
const GAP: f32 = 3.0;
/// linha de base do texto de cima (o total), alinhada ao rótulo "GITHUB"
const TOP_BASE: f32 = HEAD_Y + 9.5;

const P_WEEKS: i64 = 8;
const P_STEP: f32 = (SMALL - 2.0 * PAD + GAP) / P_WEEKS as f32;
const P_GRID_Y: f32 = SMALL - PAD - grid_h(P_STEP);

/// no médio e no grande, onde os quadrados podem começar (embaixo do rótulo)
const GRID_TOP: f32 = 28.0;
const M_WEEKS: i64 = 18;
/// o médio é baixo: os quadrados crescem até encostar na altura, e o bloco fica centrado
const M_STEP: f32 = {
    let by_width = (W - 2.0 * PAD + GAP) / M_WEEKS as f32;
    let by_height = (MEDIUM_H - PAD - GRID_TOP + GAP) / 7.0;
    if by_width < by_height { by_width } else { by_height }
};
const M_GRID_X: f32 = (W - (M_WEEKS as f32 * M_STEP - GAP)) / 2.0;
const M_GRID_Y: f32 = MEDIUM_H - PAD - grid_h(M_STEP);

const BANDS: i64 = 3;
const BAND_WEEKS: i64 = 18;
const G_STEP: f32 = (W - 2.0 * PAD + GAP) / BAND_WEEKS as f32;
/// onde começa a primeira faixa (embaixo do rótulo)
const G_BANDS_Y: f32 = 34.0;
/// a linha de cada faixa (período e total) até os quadrados dela
const BAND_HEAD: f32 = 17.0;
/// espaço entre as faixas: o que sobra, dividido igual
const BAND_GAP: f32 = (LARGE_H - PAD - G_BANDS_Y - BANDS as f32 * (BAND_HEAD + grid_h(G_STEP))) / (BANDS - 1) as f32;
const _: () = assert!(BAND_GAP >= 8.0, "as três faixas não cabem no grande");

const fn grid_h(step: f32) -> f32 {
    7.0 * step - GAP
}

/// os cinco níveis do mapa: o dia vazio, quase apagado, e o verde cada vez mais claro. Verdes
/// sólidos, com o brilho subindo em passos que se distinguem sobre o vidro escuro (o mesmo verde
/// com mais ou menos transparência ficava parecido de um nível para o outro; veja o teste
/// `levels_are_told_apart`)
const LEVELS: [Rgba; 5] = [Rgba(255, 255, 255, 18), Rgba::rgb(24, 92, 52), Rgba::rgb(30, 146, 68), Rgba::rgb(48, 198, 88), Rgba::rgb(126, 236, 152)];
/// O contorno do quadrado de hoje: o verde mais claro dos níveis.
const TODAY: Rgba = LEVELS[4];

const FETCH_EVERY: Duration = Duration::from_secs(10 * 60);
const RETRY: Duration = Duration::from_secs(2 * 60);
/// buscando: de quanto em quanto tempo ver se a resposta chegou
const POLL: Duration = Duration::from_millis(200);
/// quantos pedidos de revisão já avisados ficam guardados (os mais novos)
const SEEN_KEEP: usize = 200;

// ações
const OPEN_PROFILE: u32 = 1;
pub const UPDATE: u32 = 2;
pub const ASK_USER: u32 = 3;
/// na ilha: abrir o pedido mais novo, abrir todos, deixar para depois
const OPEN_REVIEW: u32 = 4;
const OPEN_REVIEWS: u32 = 5;
const DISMISS_REVIEWS: u32 = 6;
const REVIEWS_URL: &str = "https://github.com/pulls/review-requested";


/// Um bloco de quadrados: colunas são semanas (domingo em cima), a partir do dia `start`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grid {
    start: i64,
    weeks: i64,
    x: f32,
    y: f32,
    step: f32,
}

impl Grid {
    /// O lado de cada quadrado (o passo sem o espaço entre eles).
    fn cell(&self) -> f32 {
        self.step - GAP
    }

    /// Canto de cima à esquerda do quadrado do dia, se ele está neste bloco.
    fn spot(&self, day: i64) -> Option<(f32, f32)> {
        let offset = day - self.start;
        (0..self.weeks * 7).contains(&offset).then(|| (self.x + (offset / 7) as f32 * self.step, self.y + (offset % 7) as f32 * self.step))
    }

    /// O dia do quadrado no ponto (x, y), se há um quadrado ali.
    fn day_at(&self, x: f32, y: f32) -> Option<i64> {
        let (col, row) = (((x - self.x) / self.step).floor(), ((y - self.y) / self.step).floor());
        let inside = x - self.x - col * self.step <= self.cell() && y - self.y - row * self.step <= self.cell();
        ((0.0..self.weeks as f32).contains(&col) && (0.0..7.0).contains(&row) && inside).then(|| self.start + col as i64 * 7 + row as i64)
    }
}

/// Dia de hoje (dias desde 1970, no horário local).
fn today() -> i64 {
    let local = win::local_time();
    util::days_from_civil(local.year as i64, local.month as u32, local.day as u32)
}

/// Primeiro dia (um domingo) de um mapa de `weeks` semanas cuja última coluna é a semana de
/// `today`.
fn grid_start(today: i64, weeks: i64) -> i64 {
    let weekday = (today + 3).rem_euclid(7); // 0 = segunda (1970-01-01 foi quinta)
    today - ((weekday + 1) % 7 + 7 * (weeks - 1))
}

/// Os blocos de quadrados do tamanho dado (no grande, as três faixas, da mais antiga à de agora).
fn grids(mode: Mode, today: i64) -> Vec<Grid> {
    match mode {
        Mode::P => vec![Grid { start: grid_start(today, P_WEEKS), weeks: P_WEEKS, x: PAD, y: P_GRID_Y, step: P_STEP }],
        Mode::M => vec![Grid { start: grid_start(today, M_WEEKS), weeks: M_WEEKS, x: M_GRID_X, y: M_GRID_Y, step: M_STEP }],
        Mode::G => {
            let first = grid_start(today, BANDS * BAND_WEEKS);
            (0..BANDS).map(|band| Grid { start: first + band * BAND_WEEKS * 7, weeks: BAND_WEEKS, x: PAD, y: band_top(band) + BAND_HEAD, step: G_STEP }).collect()
        }
    }
}

/// Topo da faixa `band` do grande (a linha do período).
fn band_top(band: i64) -> f32 {
    G_BANDS_Y + band as f32 * (BAND_HEAD + grid_h(G_STEP) + BAND_GAP)
}

/// "812" com o ponto dos milhares ("1.234").
fn grouped(number: i64) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

/// O período de uma faixa, sempre do mesmo jeito: os dois meses e o ano do fim ("nov – mar
/// 2026", "mar – jul 2026"). Antes o ano aparecia ora no começo, ora nos dois lados.
fn band_range(first: i64, last: i64) -> String {
    let (first, last) = (util::civil_from_days(first), util::civil_from_days(last));
    format!("{} – {} {}", MONTHS[first.month as usize - 1], MONTHS[last.month as usize - 1], last.year)
}

/// "Nenhuma contribuição", "1 contribuição", "1.500 contribuições".
fn amount(count: i64) -> String {
    match count {
        0 => "Nenhuma contribuição".to_string(),
        1 => "1 contribuição".to_string(),
        n => format!("{} contribuições", grouped(n)),
    }
}

pub struct GitHub {
    data: Option<Data>,
    msg: String,
    token: Option<String>,
    pending: Option<Receiver<FetchResult>>,
    poll_at: Instant,
    next_fetch: Option<Instant>,
    status: Status,
    /// os pedidos de revisão abertos (da última busca)
    reviews: Vec<Review>,
    /// os que já foram vistos na ilha (não pedem mais atenção)
    island_seen: Vec<i64>,
}

impl GitHub {
    pub fn new(cfg: &Value, cache: &Value) -> GitHub {
        let user = cfg["github"].str_of("usuario");
        // abre com os últimos dados guardados, enquanto busca os atuais
        let data = cache.get("github").and_then(Data::from_json).filter(|saved| Some(saved.user.as_str()) == user);
        let updated = if data.is_some() { cache.f64_of("github_time") } else { None };
        let island_seen = cache["reviews_ilha"].as_array().map(|ids| ids.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
        GitHub {
            data,
            msg: "Carregando…".into(),
            token: None,
            pending: None,
            poll_at: Instant::now(),
            next_fetch: None,
            status: Status { updated, ..Default::default() },
            reviews: Vec::new(),
            island_seen,
        }
    }

    /// Buscando as contribuições pela primeira vez (o esqueleto no lugar do resto).
    #[cfg(test)]
    pub fn sample_loading(cfg: &Value) -> GitHub {
        let mut github = GitHub::new(cfg, &json!({}));
        github.data = None;
        github.status.loading = true;
        github
    }

    /// Exemplo para a galeria: os dados guardados, ou inventados se não houver.
    pub fn sample(cfg: &Value, cache: &Value) -> GitHub {
        let mut github = GitHub::new(cfg, cache);
        if github.data.is_none() {
            github.data = cache.get("github").and_then(Data::from_json);
        }
        if github.data.is_none() {
            let today = today();
            // um ano e pouco (53 semanas) de dias inventados, com os fins de semana mais calmos
            let days = (0..371)
                .rev()
                .map(|ago| {
                    let weekend = (today - ago + 3).rem_euclid(7) >= 5;
                    let level = if ago == 0 || (ago % 4 == 0) || (weekend && ago % 3 != 0) { 0 } else { (ago * 7 + ago / 3) % 5 };
                    DayCount { day: today - ago, level, count: [0, 2, 4, 7, 11][level as usize] + ago % 3 }
                })
                .collect::<Vec<_>>();
            let total = days.iter().map(|entry| entry.count).sum();
            github.data = Some(Data { user: "voce".into(), days, total });
        }
        github.msg = String::new();
        github
    }

    /// Com dois pedidos de revisão esperando (o aviso da ilha).
    #[cfg(test)]
    pub fn sample_reviews(cfg: &Value) -> GitHub {
        let mut github = GitHub::sample(cfg, &json!({}));
        let review = |id: i64, repo: &str, title: &str| Review { id, title: title.into(), url: format!("https://github.com/{repo}/pull/{id}"), repo: repo.into() };
        github.reviews = vec![review(34, "widgets", "Ilha na barra de tarefas"), review(12, "site", "Corrige o menu no celular")];
        github
    }

    /// Começa uma busca numa thread (se nenhuma está em andamento).
    pub fn update(&mut self, ctx: &mut Ctx) {
        if self.pending.is_some() {
            return;
        }
        let user = ctx.cfg["github"].str_of("usuario").map(String::from);
        let reviews = crate::data::config::notify_enabled(ctx.cfg, "github") || super::on_island(ctx.cfg, "github");
        let token = self.token.clone().or_else(|| ctx.cfg["github"].str_of("token").map(String::from));
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(fetch(user, reviews, token));
            crate::platform::events::wake();
        });
        self.pending = Some(rx);
        self.poll_at = Instant::now() + POLL;
        self.status.loading = true;
        self.next_fetch = None;
    }

    /// Troca o usuário (e esquece os pedidos de revisão já avisados, que eram do outro).
    pub fn set_user(&mut self, user: &str, ctx: &mut Ctx) {
        ctx.wcfg().insert("usuario".into(), json!(user));
        ctx.save();
        self.data = None;
        self.msg = "Atualizando…".into();
        if let Some(cache) = ctx.cache.as_object_mut() {
            cache.remove("reviews_seen");
        }
        self.pending = None;
        self.update(ctx);
    }

    /// Guarda o resultado de uma busca (no widget e no cache) e marca a próxima.
    fn receive(&mut self, result: FetchResult, ctx: &mut Ctx) {
        self.status.loading = false;
        let mut delay = FETCH_EVERY;
        match result {
            Err(Failure::NoUser) => {
                self.msg = "Clique direito → Mudar usuário".into();
            }
            Err(Failure::NotFound(msg)) => {
                self.data = None;
                self.msg = msg;
                self.status.error = None;
            }
            Err(Failure::Offline) => {
                self.msg = "Sem conexão. Clique para tentar de novo".into();
                self.status.error = Some("Sem conexão com o GitHub".into());
                delay = RETRY;
            }
            Ok(Fetched { data, reviews, token }) => {
                self.token = Some(token);
                self.status.error = None;
                self.status.updated = Some(win::now());
                if ctx.cfg["github"].str_of("usuario") != Some(data.user.as_str()) {
                    ctx.wcfg().insert("usuario".into(), json!(data.user));
                    ctx.save();
                }
                ctx.cache["github"] = data.to_json();
                ctx.cache["github_time"] = json!(self.status.updated);
                if let Some(reviews) = reviews {
                    if crate::data::config::notify_enabled(ctx.cfg, "github") {
                        notify_reviews(&reviews, ctx);
                    }
                    self.island_seen.retain(|id| reviews.iter().any(|review| review.id == *id));
                    self.reviews = reviews;
                }
                ctx.out.push(Request::SaveCache);
                self.data = Some(data);
            }
        }
        self.next_fetch = Some(Instant::now() + delay);
    }

    /// O ponto está na linha do rótulo (que abre o perfil)?
    fn on_header(x: f32, y: f32, mode: Mode) -> bool {
        mode != Mode::P && (6.0..=28.0).contains(&y) && (PAD - 4.0..=W - PAD + 4.0).contains(&x)
    }

    /// O ponto está na mensagem de erro (que tenta de novo com um clique)?
    fn on_message(&self, x: f32) -> bool {
        self.data.is_none() && self.status.error.is_some() && (8.0..=W - 8.0).contains(&x)
    }

    /// Nível e quantidade de cada dia com dados.
    fn by_day(&self) -> HashMap<i64, (i64, i64)> {
        self.data.as_ref().map(|data| data.days.iter().map(|entry| (entry.day, (entry.level, entry.count))).collect()).unwrap_or_default()
    }

    /// O dia (e quantas contribuições teve) do quadrado no ponto, se há um ali.
    fn day_at(&self, x: f32, y: f32, mode: Mode) -> Option<(i64, i64)> {
        self.data.as_ref()?;
        let today = today();
        let day = grids(mode, today).iter().find_map(|grid| grid.day_at(x, y)).filter(|&day| day <= today)?;
        Some((day, self.by_day().get(&day).map_or(0, |&(_, count)| count)))
    }

    /// Os quadrados de um bloco: os dias que já passaram (sem dados: o nível vazio) e hoje com
    /// um contorno verde (branco, parecia uma caixa de marcar).
    fn draw_grid(c: &mut Canvas, grid: &Grid, days: &HashMap<i64, (i64, i64)>, today: i64) {
        let cell = grid.cell();
        let radius = cell * 0.3;
        for day in grid.start..(grid.start + grid.weeks * 7).min(today + 1) {
            let Some((x, y)) = grid.spot(day) else { continue };
            let level = days.get(&day).map_or(0, |&(level, _)| level).clamp(0, 4) as usize;
            c.fill_round_rect(x, y, x + cell, y + cell, radius, LEVELS[level]);
            if day == today {
                c.stroke_round_rect(x + 0.75, y + 0.75, x + cell - 0.75, y + cell - 0.75, radius - 0.5, TODAY, 1.5);
            }
        }
    }

    /// "812 contribuições" (pequeno) ou "812 no último ano" (médio e grande, à direita do rótulo):
    /// o número forte, o resto apagado, na mesma linha de base.
    fn draw_total(c: &mut Canvas, total: i64, mode: Mode) {
        let number = grouped(total);
        if mode == Mode::P {
            let nw = c.text(PAD, TOP_BASE, &number, Face::Semibold, 12.5, FG, Anchor::LS, None);
            c.text(PAD + nw + 4.0, TOP_BASE, "contribuições", Face::Regular, 11.0, MUTED, Anchor::LS, Some(SMALL - 2.0 * PAD - nw - 4.0));
        } else {
            let rw = c.text(W - PAD, TOP_BASE, "no último ano", Face::Regular, 11.0, MUTED, Anchor::RS, None);
            c.text(W - PAD - rw - 4.0, TOP_BASE, &number, Face::Semibold, 11.5, FG, Anchor::RS, None);
        }
    }
}

/// Avisa dos pedidos de revisão novos (os que já existiam na primeira vez só são guardados).
fn notify_reviews(reviews: &[Review], ctx: &mut Ctx) {
    let seen: Option<Vec<i64>> = ctx.cache["reviews_seen"].as_array().map(|ids| ids.iter().filter_map(Value::as_i64).collect());
    if let Some(seen) = &seen {
        for review in reviews.iter().filter(|review| !seen.contains(&review.id)) {
            ctx.notify("github", "Revisão pedida no GitHub", format!("{} ({})", review.title, review.repo), Some(NotifyAction::Open(review.url.clone())));
        }
    }
    let mut all: Vec<i64> = seen.unwrap_or_default();
    all.extend(reviews.iter().map(|review| review.id));
    all.sort();
    all.dedup();
    let start = all.len().saturating_sub(SEEN_KEEP);
    ctx.cache["reviews_seen"] = json!(all[start..]);
}

impl Kind for GitHub {
    /// Na ilha com um pedido de revisão que ainda não foi visto (um aviso).
    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        let unseen = self.unseen();
        let newest = unseen.first()?;
        Some(IslandItem::alert(GITHUB_GREEN, alert_hash(&newest.id.to_string())))
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, _env: &Env) {
        self.draw_island(c, form, r, hot);
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        island_buttons(form, w, h).into_iter().find(|(_, rect)| iu::inside(*rect, x, y)).map(|(action, _)| action)
    }

    fn key(&self) -> &'static str {
        "github"
    }
    fn title(&self) -> &'static str {
        "GitHub"
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, _env: &Env) {
        let Some(data) = &self.data else {
            let h = Layout::of(mode).h;
            if self.status.loading {
                // buscando pela primeira vez: o rótulo e o esqueleto pulsando no lugar do resto
                // (na mesma altura de quando os dados chegam: o rótulo não pula; o pequeno não
                // tem rótulo)
                if mode != Mode::P {
                    super::draw_label(c, PAD, HEAD_Y, "GitHub");
                }
            } else if mode == Mode::G {
                super::draw_label(c, PAD, HEAD_Y, "GitHub");
                c.text(PAD, 36.0, &self.msg, Face::Semibold, 14.0, FG, Anchor::LA, Some(W - 2.0 * PAD));
            } else if mode == Mode::P {
                super::draw_label(c, PAD, HEAD_Y, "GitHub");
                for (i, line) in c.wrap(&self.msg, Face::Semibold, 12.0, SMALL - 2.0 * PAD, 4).iter().enumerate() {
                    c.text(PAD, 36.0 + 17.0 * i as f32, line, Face::Semibold, 12.0, FG, Anchor::LA, Some(SMALL - 2.0 * PAD));
                }
            } else {
                c.text(W / 2.0, h / 2.0, &self.msg, Face::Semibold, 12.0, FG, Anchor::MM, Some(W - 2.0 * PAD));
            }
            return;
        };
        if mode != Mode::P {
            super::draw_label(c, PAD, HEAD_Y, "GitHub");
        }
        Self::draw_total(c, data.total, mode);
        let days = self.by_day();
        let today = today();
        for (band, grid) in grids(mode, today).iter().enumerate() {
            Self::draw_grid(c, grid, &days, today);
            if mode == Mode::G {
                // a linha da faixa: o período e quantas contribuições teve
                let last = (grid.start + grid.weeks * 7 - 1).min(today);
                let sum: i64 = (grid.start..=last).filter_map(|day| days.get(&day)).map(|&(_, count)| count).sum();
                let y = band_top(band as i64) + 11.0;
                c.text(PAD, y, &band_range(grid.start, last), Face::Semibold, 11.0, FG.alpha(0.85), Anchor::LS, None);
                c.text(W - PAD, y, &amount(sum).to_lowercase(), Face::Regular, 11.0, MUTED, Anchor::RS, None);
            }
        }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        if self.on_message(x) {
            return Some(UPDATE);
        }
        (Self::on_header(x, y, mode) && env.shortcuts()).then_some(OPEN_PROFILE)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            UPDATE => self.update(ctx),
            ASK_USER => ctx.out.push(Request::Ask("user")),
            OPEN_REVIEW => {
                if let Some(review) = self.unseen().first() {
                    ctx.open(review.url.clone());
                }
                self.mark_seen(ctx);
            }
            OPEN_REVIEWS => {
                ctx.open(REVIEWS_URL);
                self.mark_seen(ctx);
            }
            DISMISS_REVIEWS => self.mark_seen(ctx),
            OPEN_PROFILE => {
                let user = self.data.as_ref().map(|data| data.user.clone()).or_else(|| ctx.cfg["github"].str_of("usuario").map(String::from));
                ctx.open(match user {
                    Some(name) if !name.is_empty() => format!("https://github.com/{name}"),
                    _ => "https://github.com".into(),
                });
            }
            _ => {}
        }
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<Hot> {
        if Self::on_header(x, y, mode) {
            return Some((PAD - 6.0, 6.0, W - PAD + 6.0, 28.0, 8.0));
        }
        if self.on_message(x) {
            let layout = Layout::of(mode);
            return Some((8.0, 8.0, layout.w - 8.0, layout.h - 8.0, 14.0));
        }
        None
    }

    fn hover_style(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> super::HoverStyle {
        if Self::on_header(x, y, mode) || self.on_message(x) { super::HoverStyle::Quiet } else { super::HoverStyle::Surface }
    }

    fn tooltip(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<String> {
        let (day, count) = self.day_at(x, y, mode)?;
        let date = util::civil_from_days(day);
        let weekday = (day + 3).rem_euclid(7) as usize;
        Some(format!("{}\n{}, {} de {}", amount(count), WEEKDAYS[weekday], date.day, MONTHS[date.month as usize - 1]))
    }

    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        vec![MenuItem::Cmd("Atualizar".into(), UPDATE), MenuItem::Cmd("Mudar usuário…".into(), ASK_USER)]
    }

    fn next_tick(&self) -> Option<Instant> {
        match (self.next_fetch, self.pending.is_some().then_some(self.poll_at)) {
            (Some(fetch), Some(poll)) => Some(fetch.min(poll)),
            (fetch, poll) => fetch.or(poll),
        }
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
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    // o fio da busca caiu sem responder
                    self.receive(Err(Failure::Offline), ctx);
                    changed = true;
                }
                Err(TryRecvError::Empty) => self.poll_at = now + POLL,
            }
        }
        if self.next_fetch.is_some_and(|due| now >= due) {
            self.update(ctx);
            changed = true;
        }
        changed
    }

    fn status(&self) -> Status {
        self.status.clone()
    }

    /// O esqueleto da primeira busca: o total em cima e os blocos de quadrados.
    fn skeleton(&self, mode: Mode) -> Vec<Hot> {
        if self.data.is_some() || !self.status.loading {
            return Vec::new();
        }
        let mut blocks = match mode {
            Mode::P => vec![(PAD, TOP_BASE - 9.0, PAD + 100.0, TOP_BASE + 1.0, 4.0)],
            _ => vec![(W - PAD - 100.0, TOP_BASE - 9.0, W - PAD, TOP_BASE + 1.0, 4.0)],
        };
        for (band, grid) in grids(mode, today()).iter().enumerate() {
            if mode == Mode::G {
                let y = band_top(band as i64) + 2.0;
                blocks.push((PAD, y, PAD + 96.0, y + 9.0, 4.0));
            }
            blocks.push((grid.x, grid.y, grid.x + grid.weeks as f32 * grid.step - GAP, grid.y + grid_h(grid.step), 8.0));
        }
        blocks
    }

    fn retry(&mut self, ctx: &mut Ctx) {
        self.update(ctx);
    }

    fn input(&mut self, field: &str, value: &str, _extra: Option<&Value>, ctx: &mut Ctx) {
        let user = value.trim().trim_start_matches('@');
        if field == "user" && !user.is_empty() {
            self.set_user(user, ctx);
        }
    }

    fn start(&mut self, ctx: &mut Ctx) {
        self.update(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada tamanho mostra mais semanas em quadrados maiores; os blocos ficam dentro do cartão,
    /// e no grande as três faixas cobrem 54 semanas seguidas, sem buraco nem repetição.
    #[test]
    fn grids_fill_each_size() {
        let today = util::days_from_civil(2026, 11, 17); // uma terça
        let (p, m, g) = (grids(Mode::P, today), grids(Mode::M, today), grids(Mode::G, today));
        assert!(p[0].cell() >= 12.5 && m[0].cell() >= 13.0 && m[0].cell() > p[0].cell() && g[0].cell() > m[0].cell(), "{} {} {}", p[0].cell(), m[0].cell(), g[0].cell());
        for (mode, list) in [(Mode::P, &p), (Mode::M, &m), (Mode::G, &g)] {
            let l = Layout::of(mode);
            for b in list.iter() {
                let (x1, y1) = (b.x + b.weeks as f32 * b.step - GAP, b.y + grid_h(b.step));
                assert!(b.x >= PAD - 0.01 && x1 <= l.w - PAD + 0.01 && y1 <= l.h - PAD + 0.01, "{mode:?}: {b:?} passa do cartão");
                assert!(b.y >= GRID_TOP - 0.01 || mode == Mode::P, "{mode:?}: encosta no rótulo");
            }
        }
        // hoje é o último quadrado preenchido (terça: a terceira linha da última coluna)
        for list in [&p, &m, &g] {
            let last = list.last().unwrap();
            assert_eq!(last.start + (last.weeks - 1) * 7 + 2, today);
            assert_eq!((last.start + 3).rem_euclid(7), 6, "a coluna começa no domingo");
        }
        assert_eq!(g.len(), 3);
        for w in g.windows(2) {
            assert_eq!(w[1].start, w[0].start + BAND_WEEKS * 7);
            assert!(w[1].y > w[0].y + grid_h(G_STEP) + BAND_HEAD, "faixas encostadas");
        }
        // ida e volta: o ponto no meio de cada quadrado dá o mesmo dia
        for b in g.iter().chain(m.iter()) {
            for day in [b.start, b.start + 10, b.start + b.weeks * 7 - 1] {
                let (x, y) = b.spot(day).unwrap();
                assert_eq!(b.day_at(x + b.cell() / 2.0, y + b.cell() / 2.0), Some(day));
                assert_eq!(b.day_at(x + b.cell() + GAP / 2.0, y + b.cell() / 2.0), None, "no espaço entre os quadrados");
            }
        }
    }

    /// Cada nível se distingue do anterior sobre o vidro escuro: o brilho (a luminância relativa
    /// da WCAG) sobe pelo menos 40% de um para o outro; e o dia vazio fica quase apagado.
    #[test]
    fn levels_are_told_apart() {
        let glass = Rgba::rgb(42, 40, 60);
        for pair in LEVELS.windows(2) {
            let (dimmer, brighter) = (pair[0].over(glass), pair[1].over(glass));
            assert!(brighter.luminance() > dimmer.luminance(), "{pair:?}: não clareia");
            assert!(contrast(brighter, dimmer) >= 1.4, "{pair:?}: {:.2}", contrast(brighter, dimmer));
        }
        assert!(LEVELS[0].3 < 30, "o dia vazio chama atenção");
    }

    #[test]
    fn words() {
        assert_eq!(grouped(812), "812");
        assert_eq!(grouped(1234), "1.234");
        assert_eq!(grouped(1234567), "1.234.567");
        assert_eq!(amount(0), "Nenhuma contribuição");
        assert_eq!(amount(1), "1 contribuição");
        assert_eq!(amount(1500), "1.500 contribuições");
        let d = |y, m, d| util::days_from_civil(y, m, d);
        assert_eq!(band_range(d(2026, 3, 1), d(2026, 7, 4)), "mar – jul 2026");
        assert_eq!(band_range(d(2025, 11, 9), d(2026, 3, 14)), "nov – mar 2026");
    }

    /// O cache de uma versão antiga (com a lista de atividades) ainda abre, e o novo não a guarda.
    #[test]
    fn old_cache_still_opens() {
        let old = json!({"user": "voce", "total": 3, "days": [["2026-11-16", 2, 3]], "rows": [{"title": "x", "when": "2026-11-16T10:00:00+00:00"}]});
        let data = Data::from_json(&old).expect("cache antigo");
        assert_eq!((data.total, data.days.len()), (3, 1));
        assert!(data.to_json().get("rows").is_none());
    }

    /// A dica de um quadrado diz o dia e quantas contribuições; fora dos quadrados, nada.
    #[test]
    fn tooltip_names_the_day() {
        let g = GitHub::sample(&json!({}), &json!({}));
        let today = today();
        let grid = grids(Mode::M, today)[0];
        let (x, y) = grid.spot(today).unwrap();
        let tip = g.tooltip(x + 2.0, y + 2.0, Mode::M, &Env { cfg: &json!({}), key: "github" }).unwrap();
        assert!(tip.starts_with("Nenhuma contribuição\n"), "{tip}");
        assert_eq!(g.tooltip(W - 2.0, 4.0, Mode::M, &Env { cfg: &json!({}), key: "github" }), None);
    }
}

// ---------- na ilha ----------

/// Os botões da ilha: (ação, retângulo a partir do canto da pílula).
fn island_buttons(form: IslandForm, w: f32, h: f32) -> Vec<(u32, (f32, f32, f32, f32))> {
    match form {
        IslandForm::Medium => vec![(OPEN_REVIEW, (w - 86.0, h / 2.0 - 16.0, w - 18.0, h / 2.0 + 16.0))],
        IslandForm::Large => vec![(OPEN_REVIEWS, (22.0, h - 46.0, 150.0, h - 14.0)), (DISMISS_REVIEWS, (w - 110.0, h - 44.0, w - 18.0, h - 16.0))],
        _ => Vec::new(),
    }
}

/// "1 revisão", "2 revisões".
fn reviews_text(count: usize) -> String {
    if count == 1 { "1 revisão".into() } else { format!("{count} revisões") }
}

impl GitHub {
    /// Os pedidos que a ilha ainda não mostrou a ninguém (o mais novo primeiro).
    fn unseen(&self) -> Vec<&Review> {
        let mut list: Vec<&Review> = self.reviews.iter().filter(|review| !self.island_seen.contains(&review.id)).collect();
        list.sort_by_key(|review| std::cmp::Reverse(review.id));
        list
    }

    /// Todos os pedidos abertos viram "vistos" (a ilha para de avisar deles), guardado no cache.
    fn mark_seen(&mut self, ctx: &mut Ctx) {
        self.island_seen.extend(self.reviews.iter().map(|review| review.id));
        self.island_seen.sort();
        self.island_seen.dedup();
        ctx.cache["reviews_ilha"] = json!(self.island_seen);
        ctx.out.push(Request::SaveCache);
    }

    /// Quadradinhos dos dias de `start` a hoje, em colunas de semanas (o mesmo mapa do cartão).
    fn island_map(&self, c: &mut Canvas, x: f32, y: f32, weeks: i64, cell: f32, gap: f32) {
        let today = today();
        let days = self.by_day();
        let start = grid_start(today, weeks);
        for day in start..=today {
            let (col, row) = ((day - start) / 7, (day - start) % 7);
            let (cx, cy) = (x + col as f32 * (cell + gap), y + row as f32 * (cell + gap));
            let level = days.get(&day).map_or(0, |&(level, _)| level).clamp(0, 4) as usize;
            c.fill_round_rect(cx, cy, cx + cell, cy + cell, cell * 0.3, LEVELS[level]);
        }
    }

    /// O pedido de revisão em cada forma da ilha: o ícone, a última semana do mapa, o pedido
    /// mais novo e, aberta, o mapa, a contagem, os dois primeiros pedidos e os botões.
    fn draw_island(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>) {
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let unseen = self.unseen();
        let count = unseen.len().max(1);
        let newest = unseen.first().map(|review| format!("{} · {}", review.repo, review.title)).unwrap_or_default();
        match form {
            IslandForm::Bubble => iu::badge_icon(c, x + w / 2.0, cy, 15.0, "github", GITHUB_GREEN),
            IslandForm::Compact | IslandForm::Banner => {
                // a última semana, um dia em cada quadradinho (hoje contornado)
                let today = today();
                let days = self.by_day();
                for weekday in 0..7 {
                    let day = today - 6 + weekday;
                    let bx = x + 16.0 + weekday as f32 * 12.0;
                    let level = days.get(&day).map_or(0, |&(level, _)| level).clamp(0, 4) as usize;
                    c.fill_round_rect(bx, cy - 4.5, bx + 9.0, cy + 4.5, 2.7, LEVELS[level]);
                    if weekday == 6 {
                        c.stroke_round_rect(bx + 0.5, cy - 4.0, bx + 8.5, cy + 4.0, 2.4, WHITE.alpha(0.85), 1.0);
                    }
                }
                let label = if form == IslandForm::Banner { format!("Revisão pedida · {}", unseen.first().map_or("", |review| review.repo.as_str())) } else { reviews_text(count) };
                c.text(x + 108.0, cy, &label, Face::Regular, 12.0, FG, Anchor::LM, Some(w - 140.0));
                c.fill_circle(x + w - 20.0, cy, 4.0, ACCENT_HOVER);
            }
            IslandForm::Medium => {
                iu::badge_icon(c, x + 39.0, cy, 22.0, "github", GITHUB_GREEN);
                let sub = if count > 1 { format!("Revisão pedida · mais {}", count - 1) } else { "Revisão pedida".into() };
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 98.0, &newest, &sub, GITHUB_GREEN.mix(WHITE, 0.2));
                for (action, button) in island_buttons(form, w, h) {
                    iu::pill_button(c, (x + button.0, y + button.1, x + button.2, y + button.3), "Abrir", true, hot == Some(action));
                }
            }
            IslandForm::Large => {
                let total = self.data.as_ref().map_or(0, |data| data.total);
                iu::header(c, r, "GITHUB", GITHUB_GREEN, &format!("{} no último ano", grouped(total)), SOFT);
                self.island_map(c, x + 22.0, y + 42.0, 18, 8.5, 2.5);
                c.text(x + w - 22.0, y + 64.0, &count.to_string(), Face::Semibold, 30.0, FG, Anchor::RM, None);
                c.text(x + w - 22.0, y + 90.0, if count == 1 { "revisão pedida" } else { "revisões pedidas" }, Face::Regular, 11.5, SOFT, Anchor::RM, None);
                for (index, review) in unseen.iter().take(2).enumerate() {
                    let ry = y + 136.0 + index as f32 * 20.0;
                    c.fill_circle(x + 26.0, ry, 3.0, if index == 0 { ACCENT_HOVER } else { Rgba(255, 255, 255, 70) });
                    c.text(x + 36.0, ry, &format!("{} · {}", review.repo, review.title), Face::Regular, 12.0, if index == 0 { FG } else { SOFT }, Anchor::LM, Some(w - 58.0));
                }
                for (action, button) in island_buttons(form, w, h) {
                    let rect = (x + button.0, y + button.1, x + button.2, y + button.3);
                    if action == OPEN_REVIEWS {
                        iu::pill_button(c, rect, "Abrir os pedidos", true, hot == Some(action));
                    } else {
                        iu::link(c, x + button.2, (rect.1 + rect.3) / 2.0, "Mais tarde", true, hot == Some(action));
                    }
                }
            }
        }
    }
}
