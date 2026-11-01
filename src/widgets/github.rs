//! Widget do GitHub: mapa de contribuições, sequência de dias e atividades recentes.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{Ctx, Env, Hot, Kind, Layout, MenuItem, Mode, NotifyAction, Request, Status};
use crate::config::Get;
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::*;
use crate::util::{self, MONTHS, WEEKDAYS};
use crate::win;

const GITHUB_GREEN: Rgba = Rgba::rgb(63, 185, 80);
// níveis do mapa de calor, nas cores do tema escuro do GitHub
const HEAT: [Rgba; 5] = [Rgba(255, 255, 255, 26), Rgba::rgb(20, 88, 50), Rgba::rgb(0, 109, 50), Rgba::rgb(38, 166, 65), Rgba::rgb(57, 211, 83)];

fn kind_color(icon: &str) -> Rgba {
    match icon {
        "commit" | "tag" => Rgba::rgb(88, 166, 255),
        "pr_open" | "issue" => Rgba::rgb(63, 185, 80),
        "pr_merged" => Rgba::rgb(163, 113, 247),
        "pr_closed" | "delete" => Rgba::rgb(248, 81, 73),
        "star" => Rgba::rgb(227, 179, 65),
        _ => Rgba::rgb(176, 186, 200),
    }
}

fn pr_state(action: &str) -> &str {
    match action {
        "opened" => "aberto",
        "closed" => "fechado",
        "merged" => "mesclado",
        "reopened" => "reaberto",
        a => a,
    }
}

#[derive(Clone)]
pub struct Row {
    title: String,
    info: String,
    icon: String,
    /// segundos desde 1970
    when: f64,
    url: String,
}

#[derive(Clone)]
pub struct Data {
    user: String,
    /// (dia desde 1970, nível 0–4, quantidade), em ordem
    days: Vec<(i64, i64, i64)>,
    total: i64,
    rows: Vec<Row>,
}

impl Data {
    fn to_json(&self) -> Value {
        let days: Vec<Value> = self
            .days
            .iter()
            .map(|&(d, level, count)| {
                let (y, m, day) = util::civil_from_days(d);
                json!([format!("{y:04}-{m:02}-{day:02}"), level, count])
            })
            .collect();
        let rows: Vec<Value> = self
            .rows
            .iter()
            .map(|r| {
                let secs = r.when as i64;
                let (y, m, d) = util::civil_from_days(secs.div_euclid(86400));
                let t = secs.rem_euclid(86400);
                let when = format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00", t / 3600, t / 60 % 60, t % 60);
                json!({"title": r.title, "info": r.info, "icon": r.icon, "when": when, "url": r.url})
            })
            .collect();
        json!({"user": self.user, "days": days, "total": self.total, "rows": rows})
    }

    fn from_json(v: &Value) -> Option<Data> {
        let days = v["days"]
            .as_array()?
            .iter()
            .map(|d| Some((util::parse_date(d[0].as_str()?)?, d[1].as_i64()?, d[2].as_i64()?)))
            .collect::<Option<Vec<_>>>()?;
        let rows = v["rows"]
            .as_array()?
            .iter()
            .map(|r| {
                Some(Row {
                    title: r.str_of("title")?.into(),
                    info: r.str_of("info").unwrap_or("").into(),
                    icon: r.str_of("icon").unwrap_or("generic").into(),
                    when: util::parse_iso_utc(r.str_of("when")?)?,
                    url: r.str_of("url").unwrap_or("").into(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Data { user: v.str_of("user")?.into(), days, total: v.i64_of("total").unwrap_or(0), rows })
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

enum Failure {
    NotFound(String),
    Offline(String),
    NoUser,
}

/// Dias do mapa de contribuições do último ano e o total.
fn contributions(user: &str) -> Result<(Vec<(i64, i64, i64)>, i64), Failure> {
    let url = format!("https://github.com/users/{}/contributions", util::urlencode(user));
    let (status, html) = util::fetch_text(&url, &[]).map_err(Failure::Offline)?;
    if status == 404 {
        return Err(Failure::NotFound(format!("Usuário não encontrado: {user}")));
    }
    if status >= 400 {
        return Err(Failure::Offline(format!("HTTP {status}")));
    }
    // a quantidade de cada dia está no texto do tooltip ligado à célula ("3 contributions on…")
    let mut tips: HashMap<&str, i64> = HashMap::new();
    for chunk in html.split("<tool-tip").skip(1) {
        let Some(end) = chunk.find('>') else { continue };
        let (tag, rest) = (&chunk[..end], &chunk[end + 1..]);
        if let Some(id) = attr(tag, "for") {
            let text = rest.split("</tool-tip>").next().unwrap_or("");
            let n: String = text.trim().chars().take_while(char::is_ascii_digit).collect();
            tips.insert(id, n.parse().unwrap_or(0));
        }
    }
    let mut days = Vec::new();
    for chunk in html.split("<td").skip(1) {
        let Some(end) = chunk.find('>') else { continue };
        let tag = &chunk[..end];
        let Some(date) = attr(tag, "data-date").and_then(util::parse_date) else { continue };
        let level = attr(tag, "data-level").and_then(|l| l.parse().ok()).unwrap_or(0);
        let count = attr(tag, "id").and_then(|id| tips.get(id)).copied().unwrap_or(0);
        days.push((date, level, count));
    }
    if days.is_empty() {
        return Err(Failure::Offline("A página do GitHub mudou de formato".into()));
    }
    days.sort();
    let total = html
        .find(" in the last year")
        .and_then(|i| {
            let before = &html[..i];
            let before = before.trim_end().strip_suffix("contributions").or(before.trim_end().strip_suffix("contribution"))?;
            let digits: String = before.trim_end().chars().rev().take_while(|c| c.is_ascii_digit() || *c == ',' || *c == '.').collect();
            digits.chars().rev().filter(char::is_ascii_digit).collect::<String>().parse().ok()
        })
        .unwrap_or_else(|| days.iter().map(|d| d.2).sum());
    Ok((days, total))
}

/// Texto, ícone e link de um evento da API do GitHub.
fn describe(e: &Value, user: &str, lookup: &mut dyn FnMut(&str) -> Option<Value>) -> Row {
    let kind = e.str_of("type").unwrap_or("");
    let p = &e["payload"];
    let repo = e["repo"].str_of("name").unwrap_or("").to_string();
    let short = if repo.to_lowercase().starts_with(&(user.to_lowercase() + "/")) { repo.split_once('/').unwrap().1.to_string() } else { repo.clone() };
    let mut url = format!("https://github.com/{repo}");
    let mut info = short.clone();
    let (title, icon): (String, &str);
    match kind {
        "PushEvent" => {
            let branch = p.str_of("ref").unwrap_or("").trim_start_matches("refs/heads/").to_string();
            let head = p.str_of("head").map(String::from);
            // a API deixou de mandar os commits no evento; o texto vem do commit de cima
            let mut message = p["commits"].as_array().and_then(|c| c.last()).and_then(|c| c.str_of("message")).map(String::from);
            if message.is_none() {
                if let Some(h) = &head {
                    message = lookup(&format!("/repos/{repo}/commits/{h}")).and_then(|c| c["commit"].str_of("message").map(String::from));
                }
            }
            title = message.and_then(|m| m.lines().next().map(String::from)).unwrap_or_else(|| format!("Push em {branch}"));
            icon = "commit";
            info = format!("{short}  ·  {branch}");
            if let Some(h) = head {
                url += &format!("/commit/{h}");
            }
        }
        "PullRequestEvent" => {
            let number = p.i64_of("number").unwrap_or(0);
            let pr = &p["pull_request"];
            let action = if p.str_of("action") == Some("closed") && pr.bool_of("merged").unwrap_or(false) {
                "merged".to_string()
            } else {
                p.str_of("action").unwrap_or("").to_string()
            };
            title = pr
                .str_of("title")
                .map(String::from)
                .or_else(|| lookup(&format!("/repos/{repo}/pulls/{number}")).and_then(|v| v.str_of("title").map(String::from)))
                .unwrap_or_else(|| format!("Pull request #{number}"));
            icon = match action.as_str() {
                "merged" => "pr_merged",
                "closed" => "pr_closed",
                _ => "pr_open",
            };
            info = format!("{short}  ·  #{number} {}", pr_state(&action));
            url += &format!("/pull/{number}");
        }
        "CreateEvent" | "DeleteEvent" => {
            let r = p.str_of("ref").unwrap_or("");
            let verb = if kind == "CreateEvent" { "Criou" } else { "Apagou" };
            match p.str_of("ref_type") {
                Some("repository") => {
                    title = format!("{verb} o repositório");
                    icon = "generic";
                }
                Some("tag") => {
                    title = format!("{verb} a tag {r}");
                    icon = "tag";
                }
                _ => {
                    title = format!("{verb} o branch {r}");
                    icon = if kind == "CreateEvent" { "branch" } else { "delete" };
                    if kind == "CreateEvent" {
                        url += &format!("/tree/{r}");
                    }
                }
            }
        }
        "WatchEvent" => {
            title = format!("Deu estrela em {repo}");
            icon = "star";
            info = String::new();
        }
        "ForkEvent" => {
            title = format!("Fez fork de {repo}");
            icon = "branch";
            info = String::new();
        }
        "IssuesEvent" | "IssueCommentEvent" => {
            let issue = &p["issue"];
            let number = issue.i64_of("number").or(p.i64_of("number")).unwrap_or(0);
            let text = issue.str_of("title").map(String::from).unwrap_or_else(|| format!("#{number}"));
            if kind == "IssuesEvent" {
                title = text;
                icon = "issue";
                info = format!("{short}  ·  issue {}", if p.str_of("action") == Some("closed") { "fechada" } else { "aberta" });
            } else {
                title = format!("Comentou em {text}");
                icon = "comment";
            }
            url += &format!("/issues/{number}");
        }
        "PullRequestReviewEvent" | "PullRequestReviewCommentEvent" => {
            let number = p["pull_request"].i64_of("number").or(p.i64_of("number")).unwrap_or(0);
            title = format!("Revisou o PR #{number}");
            icon = "comment";
            url += &format!("/pull/{number}");
        }
        "ReleaseEvent" => {
            let r = &p["release"];
            title = format!("Publicou {}", r.str_of("name").filter(|n| !n.is_empty()).or(r.str_of("tag_name")).unwrap_or("uma versão"));
            icon = "tag";
        }
        "PublicEvent" => {
            title = "Tornou o repositório público".into();
            icon = "generic";
        }
        other => {
            title = other.trim_end_matches("Event").to_string();
            icon = "generic";
        }
    }
    let when = e.str_of("created_at").and_then(util::parse_iso_utc).unwrap_or_else(win::now);
    Row { title, info, icon: icon.into(), when, url }
}

struct Review {
    id: i64,
    title: String,
    url: String,
    repo: String,
}

fn fetch(user: Option<String>, want_reviews: bool, token: Option<String>, lookups: &mut HashMap<String, Option<Value>>, max_rows: usize) -> Result<(Data, Option<Vec<Review>>, String), Failure> {
    let token = token.or_else(gh_token).unwrap_or_default();
    let user = match user.filter(|u| !u.is_empty()) {
        Some(u) => u,
        None => {
            if token.is_empty() {
                return Err(Failure::NoUser);
            }
            api("/user", &token).map_err(Failure::Offline)?.str_of("login").ok_or(Failure::NoUser)?.to_string()
        }
    };
    let (days, total) = contributions(&user)?;
    // com o token, a lista inclui os eventos dos repositórios privados do próprio usuário
    let path = format!("/users/{user}/events{}?per_page=60", if token.is_empty() { "/public" } else { "" });
    let mut events = api(&path, &token).map_err(Failure::Offline)?.as_array().cloned().unwrap_or_default();
    events.sort_by(|a, b| b.str_of("created_at").cmp(&a.str_of("created_at")));
    let mut lookup = |p: &str| -> Option<Value> {
        if !lookups.contains_key(p) {
            match api(p, &token) {
                Ok(v) => {
                    lookups.insert(p.to_string(), Some(v));
                }
                Err(_) => return None, // tenta de novo na próxima atualização
            }
        }
        lookups.get(p).cloned().flatten()
    };
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    // sem token o limite da API é baixo (60 pedidos por hora): carrega menos
    let limit = if token.is_empty() { 10 } else { max_rows };
    for e in &events {
        let p = &e["payload"];
        let kind = e.str_of("type").unwrap_or("");
        // de cada PR ou issue, só o evento mais recente (ex.: "mesclado" no lugar de "aberto")
        let grouped = matches!(kind, "PullRequestEvent" | "IssuesEvent");
        let key = format!(
            "{kind}|{}|{}|{}|{}|{}",
            e["repo"].str_of("name").unwrap_or(""),
            if grouped { "" } else { p.str_of("action").unwrap_or("") },
            p.i64_of("number").or(p["issue"].i64_of("number")).unwrap_or(-1),
            p.str_of("ref").unwrap_or(""),
            p.str_of("head").unwrap_or("")
        );
        if !seen.insert(key) {
            continue;
        }
        rows.push(describe(e, &user, &mut lookup));
        if rows.len() == limit {
            break;
        }
    }
    let mut reviews = None;
    if want_reviews && !token.is_empty() {
        let q = util::urlencode(&format!("is:pr is:open review-requested:{user}"));
        if let Ok(found) = api(&format!("/search/issues?q={q}&per_page=20"), &token) {
            reviews = Some(
                found["items"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|i| Review {
                        id: i.i64_of("id").unwrap_or(0),
                        title: i.str_of("title").unwrap_or("").into(),
                        url: i.str_of("html_url").unwrap_or("").into(),
                        repo: i.str_of("repository_url").unwrap_or("").rsplit('/').next().unwrap_or("").into(),
                    })
                    .collect(),
            );
        }
    }
    Ok((Data { user, days, total, rows }, reviews, token))
}

// ---------- o widget ----------

const W: f32 = 320.0;
const PAD: f32 = 18.0;
const RADIUS: f32 = 26.0;
const LIST_Y: f32 = 92.0;
const ROW_H: f32 = 40.0;
const ROWS: usize = 6;
const H: f32 = LIST_Y + ROW_H * ROWS as f32 + 8.0;
const MAX_ROWS: usize = 20; // carregadas; a roda do mouse rola a lista
const ICON_D: f32 = 26.0;
// pequeno, com a mesma largura dos outros: os últimos 6 meses, e a altura que der
const COMPACT_RADIUS: f32 = 22.0;
const WEEKS: i64 = 26;
const GAP: f32 = 2.0;
const C_PAD: f32 = 14.0;
const STEP: f32 = (W - 2.0 * C_PAD + GAP) / WEEKS as f32;
const CELL: f32 = STEP - GAP;
const GRID_X: f32 = C_PAD;
// médio: o mapa e as 3 últimas atividades
const M_ROWS: usize = 3;

fn compact_h() -> f32 {
    (2.0 * C_PAD + 7.0 * STEP - GAP).round()
}
fn grid_y() -> f32 {
    (compact_h() - (7.0 * STEP - GAP)) / 2.0
}
fn m_list_y() -> f32 {
    compact_h() + 6.0
}
fn medium_h() -> f32 {
    m_list_y() + ROW_H * M_ROWS as f32 + 2.0
}

const FETCH_EVERY: Duration = Duration::from_secs(10 * 60);
const RETRY: Duration = Duration::from_secs(2 * 60);
const CLOCK: Duration = Duration::from_secs(60); // atualiza o "há 5 min"

// ações
const OPEN_ROW: u32 = 100; // + índice da linha visível
const OPEN_PROFILE: u32 = 1;
pub const UPDATE: u32 = 2;
pub const ASK_USER: u32 = 3;

type Result3 = Result<(Data, Option<Vec<Review>>, String, HashMap<String, Option<Value>>), Failure>;

pub struct GitHub {
    data: Option<Data>,
    msg: String,
    scroll: usize,
    token: Option<String>,
    lookups: HashMap<String, Option<Value>>,
    pending: Option<Receiver<Result3>>,
    poll_at: Instant,
    next_fetch: Option<Instant>,
    next_clock: Instant,
    status: Status,
}

impl GitHub {
    pub fn new(cfg: &Value, cache: &Value) -> GitHub {
        let user = cfg["github"].str_of("usuario");
        // abre com os últimos dados guardados, enquanto busca os atuais
        let data = cache.get("github").and_then(Data::from_json).filter(|d| Some(d.user.as_str()) == user);
        let updated = if data.is_some() { cache.f64_of("github_time") } else { None };
        GitHub {
            data,
            msg: "Carregando…".into(),
            scroll: 0,
            token: None,
            lookups: HashMap::new(),
            pending: None,
            poll_at: Instant::now(),
            next_fetch: None,
            next_clock: Instant::now() + CLOCK,
            status: Status { updated, ..Default::default() },
        }
    }

    /// Exemplo para a galeria: os dados guardados, ou inventados se não houver.
    pub fn sample(cfg: &Value, cache: &Value) -> GitHub {
        let mut g = GitHub::new(cfg, cache);
        if g.data.is_none() {
            g.data = cache.get("github").and_then(Data::from_json);
        }
        if g.data.is_none() {
            let t = win::local_time();
            let today = util::days_from_civil(t.year as i64, t.month as u32, t.day as u32);
            let days = (0..371).rev().map(|i| (today - i, if i % 4 == 0 { 0 } else { (i * 7 + i / 3) % 5 }, i % 6)).collect();
            let now = win::now();
            let rows = [("commit", "Ajusta o layout do cartão", "meu-projeto", 1.0), ("pr_merged", "Adiciona modo escuro", "meu-projeto  ·  #42", 5.0)]
                .iter()
                .map(|(icon, title, info, h)| Row { title: title.to_string(), info: info.to_string(), icon: icon.to_string(), when: now - h * 3600.0, url: String::new() })
                .collect();
            g.data = Some(Data { user: "voce".into(), days, total: 812, rows });
        }
        g.msg = String::new();
        g
    }

    pub fn update(&mut self, ctx: &mut Ctx) {
        if self.pending.is_some() {
            return;
        }
        let user = ctx.cfg["github"].str_of("usuario").map(String::from);
        let reviews = crate::config::notify_enabled(ctx.cfg, "github");
        let token = self.token.clone().or_else(|| ctx.cfg["github"].str_of("token").map(String::from));
        let mut lookups = std::mem::take(&mut self.lookups);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let result = fetch(user, reviews, token, &mut lookups, MAX_ROWS).map(|(d, r, t)| (d, r, t, lookups));
            let _ = tx.send(result);
            crate::events::wake();
        });
        self.pending = Some(rx);
        self.poll_at = Instant::now() + Duration::from_millis(200);
        self.status.loading = true;
        self.next_fetch = None;
    }

    pub fn set_user(&mut self, user: &str, ctx: &mut Ctx) {
        ctx.wcfg().insert("usuario".into(), json!(user));
        ctx.save();
        self.data = None;
        self.msg = "Atualizando…".into();
        if let Some(c) = ctx.cache.as_object_mut() {
            c.remove("reviews_seen");
        }
        self.pending = None;
        self.update(ctx);
    }

    fn receive(&mut self, result: Result3, ctx: &mut Ctx) {
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
            Err(Failure::Offline(_)) => {
                self.msg = "Sem conexão. Clique para tentar de novo".into();
                self.status.error = Some("Sem conexão com o GitHub".into());
                delay = RETRY;
            }
            Ok((data, reviews, token, lookups)) => {
                self.token = Some(token);
                self.lookups = lookups;
                self.status.error = None;
                self.status.updated = Some(win::now());
                self.scroll = self.scroll.min(data.rows.len().saturating_sub(ROWS));
                if ctx.cfg["github"].str_of("usuario") != Some(data.user.as_str()) {
                    ctx.wcfg().insert("usuario".into(), json!(data.user));
                    ctx.save();
                }
                ctx.cache["github"] = data.to_json();
                ctx.cache["github_time"] = json!(self.status.updated);
                if let Some(reviews) = reviews {
                    notify_reviews(&reviews, ctx);
                }
                ctx.out.push(Request::SaveCache);
                self.data = Some(data);
            }
        }
        self.next_fetch = Some(Instant::now() + delay);
    }

    fn list_geom(mode: Mode) -> Option<(f32, usize)> {
        match mode {
            Mode::P => None,
            Mode::M => Some((m_list_y(), M_ROWS)),
            Mode::G => Some((LIST_Y, ROWS)),
        }
    }

    fn visible_rows(&self, mode: Mode) -> &[Row] {
        match (Self::list_geom(mode), &self.data) {
            (Some((_, n)), Some(d)) => {
                let start = self.scroll.min(d.rows.len());
                &d.rows[start..(start + n).min(d.rows.len())]
            }
            _ => &[],
        }
    }

    fn row_index(&self, x: f32, y: f32, mode: Mode) -> Option<usize> {
        let (top, _) = Self::list_geom(mode)?;
        self.data.as_ref()?;
        let i = ((y - top) / ROW_H).floor();
        (y >= top && i >= 0.0 && (i as usize) < self.visible_rows(mode).len() && (PAD - 6.0..=W - PAD + 6.0).contains(&x)).then_some(i as usize)
    }

    fn on_header(x: f32, y: f32, mode: Mode) -> bool {
        mode == Mode::G && (10.0..=30.0).contains(&y) && (PAD - 4.0..=W - PAD + 4.0).contains(&x)
    }

    fn on_message(&self, x: f32) -> bool {
        self.data.is_none() && self.status.error.is_some() && (8.0..=W - 8.0).contains(&x)
    }

    /// Primeiro dia do mapa: colunas são semanas de domingo a sábado; a última é a atual.
    fn grid_start(&self) -> i64 {
        let last = self.data.as_ref().and_then(|d| d.days.last()).map(|d| d.0).unwrap_or(0);
        let weekday = (last + 3).rem_euclid(7); // 0 = segunda (1970-01-01 foi quinta)
        last - ((weekday + 1) % 7 + 7 * (WEEKS - 1))
    }

    fn day_at(&self, x: f32, y: f32, mode: Mode) -> Option<(i64, i64)> {
        if mode == Mode::G {
            return None;
        }
        let d = self.data.as_ref()?;
        let col = ((x - GRID_X) / STEP).floor();
        let row = ((y - grid_y()) / STEP).floor();
        let inside = x - GRID_X - col * STEP <= CELL && y - grid_y() - row * STEP <= CELL;
        if !(0.0..WEEKS as f32).contains(&col) || !(0.0..7.0).contains(&row) || !inside {
            return None;
        }
        let day = self.grid_start() + col as i64 * 7 + row as i64;
        d.days.iter().find(|x| x.0 == day).map(|x| (day, x.2))
    }

    fn streak(&self) -> (i64, i64) {
        let Some(d) = &self.data else { return (0, 0) };
        let counts: HashMap<i64, i64> = d.days.iter().map(|x| (x.0, x.2)).collect();
        let t = win::local_time();
        let today = util::days_from_civil(t.year as i64, t.month as u32, t.day as u32);
        let mut day = today;
        if counts.get(&day).copied().unwrap_or(0) == 0 {
            day -= 1; // o dia de hoje ainda pode ganhar contribuições
        }
        let mut streak = 0;
        while counts.get(&day).copied().unwrap_or(0) > 0 {
            streak += 1;
            day -= 1;
        }
        (streak, counts.get(&today).copied().unwrap_or(0))
    }

    fn heatmap(&self, c: &mut Canvas) {
        let Some(d) = &self.data else { return };
        let start = self.grid_start();
        let t = win::local_time();
        let today = util::days_from_civil(t.year as i64, t.month as u32, t.day as u32);
        for &(day, level, _) in &d.days {
            let offset = day - start;
            if offset < 0 {
                continue;
            }
            let x = GRID_X + (offset / 7) as f32 * STEP;
            let y = grid_y() + (offset % 7) as f32 * STEP;
            c.fill_round_rect(x, y, x + CELL, y + CELL, CELL * 0.25, HEAT[level.clamp(0, 4) as usize]);
            if day == today {
                c.stroke_round_rect(x, y, x + CELL, y + CELL, CELL * 0.25, Rgba(255, 255, 255, 220), 1.2);
            }
        }
    }

    fn rows(&self, c: &mut Canvas, mode: Mode) {
        let Some((list_y, visible)) = Self::list_geom(mode) else { return };
        let Some(d) = &self.data else { return };
        if d.rows.len() > visible {
            // barra de rolagem fininha
            let x = W - 7.0;
            let track = visible as f32 * ROW_H - 10.0;
            c.fill_round_rect(x, list_y, x + 3.0, list_y + track, 1.5, WHITE.alpha(0.08));
            let thumb = track * visible as f32 / d.rows.len() as f32;
            let scroll = self.scroll.min(d.rows.len() - visible);
            let ty = list_y + (track - thumb) * scroll as f32 / (d.rows.len() - visible) as f32;
            c.fill_round_rect(x, ty, x + 3.0, ty + thumb, 1.5, WHITE.alpha(0.4));
        }
        if d.rows.is_empty() {
            c.text(PAD, list_y + 6.0, "Nenhuma atividade recente", Face::Regular, 12.0, MUTED, Anchor::LA, None);
        }
        let x = PAD + ICON_D + 10.0;
        let w = W - x - PAD;
        for (i, row) in self.visible_rows(mode).iter().enumerate() {
            let top = list_y + i as f32 * ROW_H;
            let color = kind_color(&row.icon);
            let (cx, cy) = (PAD + ICON_D / 2.0, top + 17.0);
            let bg = color.alpha(0.18);
            c.fill_circle(cx, cy, ICON_D / 2.0, bg);
            glyph(c, &row.icon, cx, cy, 13.0, color, Rgba(30, 30, 36, 255).mix(color, 0.18));
            c.text(x, top + 1.0, &row.title, Face::Semibold, 12.0, FG, Anchor::LA, Some(w));
            let ago = util::time_ago(row.when);
            let info = if row.info.is_empty() { ago } else { format!("{}  ·  {ago}", row.info) };
            c.text(x, top + 19.0, &info, Face::Regular, 10.5, MUTED, Anchor::LA, Some(w));
        }
    }
}

/// Avisa dos pedidos de revisão novos (os que já existiam na primeira vez só são guardados).
fn notify_reviews(reviews: &[Review], ctx: &mut Ctx) {
    let seen: Option<Vec<i64>> = ctx.cache["reviews_seen"].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect());
    if let Some(seen) = &seen {
        for r in reviews.iter().filter(|r| !seen.contains(&r.id)) {
            ctx.notify("github", "Revisão pedida no GitHub", format!("{} ({})", r.title, r.repo), Some(NotifyAction::Open(r.url.clone())));
        }
    }
    let mut all: Vec<i64> = seen.unwrap_or_default();
    all.extend(reviews.iter().map(|r| r.id));
    all.sort();
    all.dedup();
    let start = all.len().saturating_sub(200);
    ctx.cache["reviews_seen"] = json!(all[start..]);
}

/// Ícone de atividade no estilo dos Octicons, num quadro de lado `size` centrado em (cx, cy).
fn glyph(c: &mut Canvas, kind: &str, cx: f32, cy: f32, size: f32, color: Rgba, bg: Rgba) {
    let u = size / 2.0;
    let w = (size * 0.12).max(1.0 / c.s);
    let pt = |x: f32, y: f32| (cx + x * u, cy + y * u);
    let line = |c: &mut Canvas, pts: &[(f32, f32)]| {
        let pts: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| pt(x, y)).collect();
        c.line(&pts, color, w);
    };
    let ring = |c: &mut Canvas, x: f32, y: f32, r: f32| {
        let (x, y) = pt(x, y);
        c.fill_circle(x, y, r * u, bg);
        c.stroke_circle(x, y, r * u - w / 2.0, color, w);
    };
    match kind {
        "commit" => {
            line(c, &[(-1.0, 0.0), (1.0, 0.0)]);
            ring(c, 0.0, 0.0, 0.45);
        }
        "pr_open" | "pr_closed" => {
            line(c, &[(-0.5, -0.62), (-0.5, 0.62)]);
            ring(c, -0.5, -0.62, 0.27);
            ring(c, -0.5, 0.62, 0.27);
            line(c, &[(0.5, 0.6), (0.5, -0.45), (0.05, -0.45)]);
            line(c, &[(0.05, -0.45), (0.27, -0.67)]);
            line(c, &[(0.05, -0.45), (0.27, -0.23)]);
            ring(c, 0.5, 0.62, 0.27);
        }
        "pr_merged" => {
            line(c, &[(-0.5, -0.62), (-0.5, 0.62)]);
            line(c, &[(-0.5, -0.4), (-0.2, 0.2), (0.5, 0.2)]);
            ring(c, -0.5, -0.62, 0.27);
            ring(c, -0.5, 0.62, 0.27);
            ring(c, 0.5, 0.2, 0.27);
        }
        "branch" | "delete" => {
            line(c, &[(-0.5, -0.62), (-0.5, 0.62)]);
            line(c, &[(0.5, -0.4), (0.5, -0.1), (-0.5, 0.35)]);
            ring(c, -0.5, -0.62, 0.27);
            ring(c, -0.5, 0.62, 0.27);
            ring(c, 0.5, -0.55, 0.27);
        }
        "star" => {
            let pts: Vec<(f32, f32)> = (0..10)
                .map(|i| {
                    let a = (-90.0 + i as f32 * 36.0f32).to_radians();
                    let r = if i % 2 == 0 { 0.95 } else { 0.42 };
                    pt(a.cos() * r, a.sin() * r + 0.06)
                })
                .collect();
            c.polygon(&pts, color);
        }
        "issue" => {
            ring(c, 0.0, 0.0, 0.8);
            c.fill_circle(cx, cy, 0.2 * u, color);
        }
        "comment" => {
            let ((x0, y0), (x1, y1)) = (pt(-0.85, -0.7), pt(0.85, 0.45));
            c.stroke_round_rect(x0, y0, x1, y1, 0.3 * u, color, w);
            line(c, &[(-0.35, 0.45), (-0.55, 0.85), (0.05, 0.45)]);
        }
        "tag" => {
            let pts = [pt(-0.8, -0.8), pt(0.1, -0.8), pt(0.85, 0.0), pt(0.1, 0.8), pt(-0.8, 0.8)];
            c.line_with(&pts, crate::gfx::Fill::Solid(color), w, true);
            ring(c, -0.38, -0.38, 0.16);
        }
        _ => ring(c, 0.0, 0.0, 0.45),
    }
}

impl Kind for GitHub {
    fn key(&self) -> &'static str {
        "github"
    }
    fn title(&self) -> &'static str {
        "GitHub"
    }

    fn layout(&self, mode: Mode) -> Layout {
        match mode {
            Mode::P => Layout { w: W, h: compact_h(), radius: COMPACT_RADIUS },
            Mode::M => Layout { w: W, h: medium_h(), radius: RADIUS },
            Mode::G => Layout { w: W, h: H, radius: RADIUS },
        }
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, env: &Env) {
        let Some(d) = &self.data else {
            let h = self.layout(mode).h;
            if mode == Mode::G {
                c.text(PAD, 19.0, "GITHUB", Face::Semibold, 10.0, GITHUB_GREEN, Anchor::LA, None);
                c.text(PAD, 36.0, &self.msg, Face::Semibold, 14.0, FG, Anchor::LA, Some(W - 2.0 * PAD));
            } else {
                c.text(W / 2.0, h / 2.0, &self.msg, Face::Semibold, 12.0, FG, Anchor::MM, Some(W - 2.0 * C_PAD));
            }
            return;
        };
        if mode != Mode::G {
            self.heatmap(c);
            if mode == Mode::M {
                c.hline_px(PAD, W - PAD, m_list_y() - 6.0, DIVIDER);
                self.rows(c, mode);
            }
            return;
        }
        c.text(PAD, 19.0, "GITHUB", Face::Semibold, 10.0, GITHUB_GREEN, Anchor::LA, None);
        let user = if d.user.is_empty() { env.wcfg().str_of("usuario").unwrap_or("").to_string() } else { d.user.clone() };
        if !user.is_empty() {
            c.text(W - PAD, 19.0, &format!("@{user}"), Face::Regular, 10.5, MUTED, Anchor::RA, Some(180.0));
        }
        let total = d.total.to_string();
        let mut grouped = String::new();
        for (i, ch) in total.chars().enumerate() {
            if i > 0 && (total.len() - i) % 3 == 0 {
                grouped.push('.');
            }
            grouped.push(ch);
        }
        c.text(PAD, 35.0, &format!("{grouped} contribuições no último ano"), Face::Semibold, 14.0, FG, Anchor::LA, Some(W - 2.0 * PAD));
        let (streak, today) = self.streak();
        let days = if streak == 1 { "dia" } else { "dias" };
        c.text(PAD, 56.0, &format!("Sequência de {streak} {days}  ·  Hoje: {today}"), Face::Regular, 11.0, MUTED, Anchor::LA, None);
        c.hline_px(PAD, W - PAD, LIST_Y - 10.0, DIVIDER);
        self.rows(c, mode);
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        let _ = y;
        if self.on_message(x) {
            return Some(UPDATE);
        }
        let shortcuts = env.shortcuts();
        if let Some(i) = self.row_index(x, y, mode) {
            return shortcuts.then_some(OPEN_ROW + i as u32);
        }
        if Self::on_header(x, y, mode) && shortcuts {
            return Some(OPEN_PROFILE);
        }
        None
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            UPDATE => self.update(ctx),
            ASK_USER => ctx.out.push(Request::Ask("user")),
            OPEN_PROFILE => {
                let user = self.data.as_ref().map(|d| d.user.clone()).or_else(|| ctx.cfg["github"].str_of("usuario").map(String::from));
                ctx.open(match user {
                    Some(u) if !u.is_empty() => format!("https://github.com/{u}"),
                    _ => "https://github.com".into(),
                });
            }
            a if a >= OPEN_ROW => {
                let i = (a - OPEN_ROW) as usize + self.scroll;
                if let Some(row) = self.data.as_ref().and_then(|d| d.rows.get(i)) {
                    ctx.open(row.url.clone());
                }
            }
            _ => {}
        }
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<Hot> {
        if let Some(i) = self.row_index(x, y, mode) {
            let top = Self::list_geom(mode).unwrap().0 + i as f32 * ROW_H;
            return Some((PAD - 8.0, top - 4.0, W - PAD + 6.0, top + ROW_H - 6.0, 10.0));
        }
        if Self::on_header(x, y, mode) {
            return Some((PAD - 6.0, 10.0, W - PAD + 6.0, 30.0, 8.0));
        }
        if self.on_message(x) {
            return Some((8.0, 8.0, W - 8.0, self.layout(mode).h - 8.0, 14.0));
        }
        None
    }

    fn tooltip(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<String> {
        if let Some((day, count)) = self.day_at(x, y, mode) {
            let amount = match count {
                0 => "Nenhuma contribuição".to_string(),
                1 => "1 contribuição".to_string(),
                n => format!("{n} contribuições"),
            };
            let (_, m, d) = util::civil_from_days(day);
            let weekday = (day + 3).rem_euclid(7) as usize;
            return Some(format!("{amount}\n{}, {d} de {}", WEEKDAYS[weekday], MONTHS[m as usize - 1]));
        }
        if let Some(i) = self.row_index(x, y, mode) {
            let row = &self.visible_rows(mode)[i];
            let probe = Canvas::new(1, 1, 1.0);
            let title = probe.wrap(&row.title, Face::Semibold, 11.5, 320.0, 4).join("\n");
            let local = row.when - util::tz_offset(); // horário local
            let (_, m, d) = util::civil_from_days((local / 86400.0).floor() as i64);
            let secs = local.rem_euclid(86400.0) as i64;
            let mut text = format!("{title}\n\n{d} de {} às {:02}:{:02}", MONTHS[m as usize - 1], secs / 3600, secs / 60 % 60);
            if env.shortcuts() {
                text += "\nClique para abrir no GitHub";
            }
            return Some(text);
        }
        if Self::on_header(x, y, mode) {
            let mut lines = vec!["Abrir o perfil no GitHub".to_string()];
            if let Some(t) = self.status.updated {
                lines.push(format!("Atualizado {}", util::time_ago(t)));
            }
            return Some(lines.join("\n"));
        }
        None
    }

    fn wheel(&mut self, _x: f32, _y: f32, steps: f32, mode: Mode, _ctx: &mut Ctx) {
        let (Some((_, visible)), Some(d)) = (Self::list_geom(mode), &self.data) else { return };
        let top = d.rows.len().saturating_sub(visible) as i64;
        self.scroll = (self.scroll as i64 - steps.round() as i64).clamp(0, top) as usize;
    }

    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        vec![MenuItem::Cmd("Atualizar".into(), UPDATE), MenuItem::Cmd("Mudar usuário…".into(), ASK_USER)]
    }

    fn next_tick(&self) -> Option<Instant> {
        let mut next = self.next_clock;
        if let Some(t) = self.next_fetch {
            next = next.min(t);
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
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.receive(Err(Failure::Offline("falha inesperada".into())), ctx);
                    changed = true;
                }
                Err(TryRecvError::Empty) => self.poll_at = now + Duration::from_millis(200),
            }
        }
        if self.next_fetch.is_some_and(|t| now >= t) {
            self.update(ctx);
            changed = true;
        }
        if now >= self.next_clock {
            self.next_clock = now + CLOCK;
            changed |= self.data.is_some();
        }
        changed
    }

    fn status(&self) -> Status {
        self.status.clone()
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
