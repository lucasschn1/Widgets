//! A ilha em repouso: quando nenhum widget tem o que mostrar, a ilha continua na barra de tarefas
//! com o que a pessoa escolheu: o relógio, a linha do dia, o próximo acontecimento, o tempo, as
//! cotações, o foco do dia ou a última música. Com mais de um escolhido, eles se alternam a cada
//! 7 s (a troca sobe e esmaece, como nas saudações).
//!
//! Qualquer coisa de verdade passa na frente: o repouso só entra quando a ilha está vazia, e sai
//! quando chega alguém. Com o PC parado há 5 min, ela vira uma gota apagada; com a opção "só uma
//! gota", fica sempre assim, e o resto aparece com o mouse em cima. Em tela cheia, some (como a
//! ilha sempre fez). Parada, só é redesenhada quando muda algo (o minuto, a troca, os dados).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::quote::{bitcoin, brl, pct, wallet};
use super::{location, workday, Clock};
use crate::data::astro;
use crate::data::config::Get;
use crate::gfx::drawing::{self, weather_art};
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::ui::theme::{ease_in_out, Rgba, FAINT, FG, MUTED, WHITE};
use crate::widgets::island_ui as iu;
use crate::widgets::{alert_hash, island_act, on_island, pomodoro, Ctx, Env, IslandForm, IslandItem, Kind, Mode, Rect4, Request};

pub const KEY: &str = "repouso";
/// com mais de um escolhido, cada um fica este tanto (em segundos) antes de dar a vez
pub const ROTATE_EVERY: f64 = 7.0;
/// quanto dura a troca de um para o outro (o de saída sobe e apaga, o novo sobe e acende)
const TURN_SECS: f64 = 0.4;
/// a régua da "Linha do dia" vai das 4 h às 23 h (a madrugada fica de fora)
const DAY_TRACK_FROM_HOUR: f64 = 4.0;
const DAY_TRACK_TO_HOUR: f64 = 23.0;
/// "PC parado": sem mouse nem teclado há este tanto
pub const IDLE_AFTER: f64 = 5.0 * 60.0;
/// as cotações do cache valem por este tanto
const QUOTES_FRESH: f64 = 2.0 * 3600.0;
/// o tempo do cache vale por este tanto
const WEATHER_FRESH: f64 = 3.0 * 3600.0;

/// botões da ilha: começar um foco, tocar a última música
pub const START: u32 = 1;
pub const PLAY: u32 = 2;
/// sair do modo foco
pub const END_FOCUS: u32 = 3;
/// o calendário do relógio aberto: o mês anterior, o seguinte e de volta ao de hoje (no nome do
/// mês)
pub const PREV_MONTH: u32 = 4;
pub const NEXT_MONTH: u32 = 5;
pub const THIS_MONTH: u32 = 6;
/// abrir o player da última música (com ele fechado)
pub const OPEN_APP: u32 = 7;
/// procurar no player a música `n` das tocadas antes (RECENT + n)
pub const RECENT: u32 = 100;

/// quantas músicas tocadas antes da última ficam guardadas (e aparecem na ilha aberta)
pub const RECENT_MAX: usize = 3;
/// a pasta (dentro da pasta de dados) com as capas das músicas guardadas, e o lado delas
const COVERS_DIR: &str = "capas";
const COVER_PX: u32 = 96;

// ---------- a última música aberta (medidas a partir do canto da ilha de 364 × 220) ----------

/// a capa grande e onde começa o texto ao lado dela
const MEDIA_COVER: f32 = 56.0;
const MEDIA_TEXT_X: f32 = 88.0;
/// a barra de onde a música parou e a linha dos tempos embaixo dela
const MEDIA_BAR_Y: f32 = 94.0;
const MEDIA_TIMES_Y: f32 = 104.0;
/// o título das tocadas antes e a primeira delas; cada uma numa linha de `RECENT_ROW`
const RECENT_HEAD_Y: f32 = 130.0;
const RECENT_FIRST_Y: f32 = 140.0;
const RECENT_ROW: f32 = 24.0;
const RECENT_COVER: f32 = 18.0;
/// o botão redondo de tocar ou de abrir o player, no alto à direita
const MEDIA_BUTTON: f32 = 36.0;
/// o campo de `input` que liga e desliga o modo foco: "off", "on" ou o horário do fim ("15:30")
pub const FOCUS_FIELD: &str = "modo_foco";
/// a cor do modo foco (a mesma do aviso dele)
const FOCUS: Rgba = Rgba::rgb(191, 90, 242);

const UP: Rgba = Rgba::rgb(48, 209, 88);
const DOWN: Rgba = Rgba::rgb(255, 92, 80);
const GOLD: Rgba = Rgba::rgb(255, 199, 64);
const WORK: Rgba = Rgba::rgb(122, 209, 168);
const TOMATO: Rgba = Rgba::rgb(255, 107, 91);
const ISS: Rgba = Rgba::rgb(126, 188, 255);
const MOON: Rgba = Rgba::rgb(223, 230, 255);
const SUNSET: Rgba = Rgba::rgb(255, 154, 107);
const SUNRISE: Rgba = Rgba::rgb(255, 201, 77);
const DAWN: Rgba = Rgba::rgb(255, 179, 107);
const DAY: Rgba = Rgba::rgb(255, 215, 107);
const DUSK: Rgba = Rgba::rgb(255, 143, 163);
const NIGHT: Rgba = Rgba::rgb(143, 177, 255);
/// o vermelho do Calendário da Apple: o dia da semana, o nome do mês, o dia de hoje e o ano
const CAL_RED: Rgba = Rgba::rgb(255, 69, 58);

// ---------- o relógio aberto: a hora e o ano à esquerda, o calendário do mês à direita ----------
// (medidas a partir do canto da ilha aberta, de 364 × 220)

/// onde começa a coluna do calendário (a da hora vai da margem até 20 pt antes dela)
const CAL_X0: f32 = 170.0;
/// a margem da direita do calendário
const CAL_RIGHT: f32 = 18.0;
/// o meio da linha do nome do mês (e das setas), a das iniciais dos dias e a primeira semana
const CAL_HEAD_Y: f32 = 26.0;
const CAL_INITIALS_Y: f32 = 50.0;
const CAL_FIRST_ROW_Y: f32 = 70.0;
/// a altura de cada semana (são até 6) e o raio do círculo do dia de hoje
const CAL_ROW: f32 = 24.0;
const CAL_TODAY_R: f32 = 10.5;
/// o lado do botão de cada seta
const CAL_ARROW: f32 = 22.0;
/// as iniciais dos dias, de domingo a sábado (como no Calendário em português)
const CAL_INITIALS: [&str; 7] = ["D", "S", "T", "Q", "Q", "S", "S"];

/// Um mês do calendário: o ano, o mês (1 a 12), o dia do dia 1 (em dias desde 1970), quantos
/// dias tem e quantos lugares ficam vazios antes do dia 1 na primeira semana (que começa no
/// domingo).
#[derive(Clone, Copy, Debug, PartialEq)]
struct MonthView {
    year: i64,
    month: u32,
    first: i64,
    days: i64,
    lead: i64,
}

impl MonthView {
    /// O mês `shift` meses depois (ou antes, negativo) do de `day`.
    fn around(day: i64, shift: i32) -> MonthView {
        use crate::data::util::{civil_from_days, days_from_civil};
        let today = civil_from_days(day);
        let index = today.year * 12 + today.month as i64 - 1 + shift as i64;
        let (year, month) = (index.div_euclid(12), index.rem_euclid(12) as u32 + 1);
        let first = days_from_civil(year, month, 1);
        let next = if month == 12 { days_from_civil(year + 1, 1, 1) } else { days_from_civil(year, month + 1, 1) };
        // 1970-01-01 foi uma quinta: com a semana começando no domingo, o lugar 4
        MonthView { year, month, first, days: next - first, lead: (first + 4).rem_euclid(7) }
    }
}

/// Quanto do ano já passou: o ano, a fração (0 a 1) e quantos dias faltam depois de hoje.
#[derive(Clone, Copy, Debug, PartialEq)]
struct YearSoFar {
    year: i64,
    done: f64,
    days_left: i64,
}

/// O que a ilha em repouso pode mostrar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Clock,
    Day,
    Next,
    Weather,
    Quotes,
    Focus,
    Media,
}

impl Look {
    pub const ALL: [Look; 7] = [Look::Clock, Look::Day, Look::Next, Look::Weather, Look::Quotes, Look::Focus, Look::Media];

    pub fn code(self) -> &'static str {
        match self {
            Look::Clock => "relogio",
            Look::Day => "dia",
            Look::Next => "proximo",
            Look::Weather => "tempo",
            Look::Quotes => "cotacoes",
            Look::Focus => "foco",
            Look::Media => "midia",
        }
    }

    /// O interruptor dele nas Configurações (e no menu da ilha).
    pub fn setting_id(self) -> &'static str {
        match self {
            Look::Clock => "repouso:relogio",
            Look::Day => "repouso:dia",
            Look::Next => "repouso:proximo",
            Look::Weather => "repouso:tempo",
            Look::Quotes => "repouso:cotacoes",
            Look::Focus => "repouso:foco",
            Look::Media => "repouso:midia",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Look::Clock => "Relógio",
            Look::Day => "Linha do dia",
            Look::Next => "Próximo acontecimento",
            Look::Weather => "Tempo agora",
            Look::Quotes => "Cotações",
            Look::Focus => "Foco do dia",
            Look::Media => "Última música",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Look::Clock => "A hora e a data; aberta, o calendário do mês e quanto do ano já passou",
            Look::Day => "O dia numa régua: o sol andando até o pôr, com o fim do expediente marcado (na cidade do Tempo)",
            Look::Next => "Quanto falta para o próximo: fim do expediente, pôr do sol, lua cheia, a ISS passando",
            Look::Weather => "A temperatura e a próxima chuva, do widget Tempo",
            Look::Quotes => "O dólar e o bitcoin, com a variação do dia",
            Look::Focus => "Quantos Pomodoros você fez hoje, com um botão para começar o próximo",
            Look::Media => "A última música do Tocando agora, pausada, pronta para voltar",
        }
    }
}

/// Os escolhidos de fábrica (alternando).
pub const DEFAULT: [Look; 4] = [Look::Clock, Look::Weather, Look::Next, Look::Quotes];
/// O interruptor do "só uma gota".
pub const DOT_ID: &str = "repouso:gota";

/// Os escolhidos nas opções, na ordem de `Look::ALL`.
pub fn chosen(cfg: &Value) -> Vec<Look> {
    match cfg[KEY]["mostrar"].as_array() {
        Some(list) => Look::ALL.into_iter().filter(|l| list.iter().any(|v| v.as_str() == Some(l.code()))).collect(),
        None => DEFAULT.to_vec(),
    }
}

/// "Só uma gota": fechada, a ilha em repouso é só um ponto.
pub fn dot_on(cfg: &Value) -> bool {
    cfg[KEY].bool_of("gota").unwrap_or(false)
}

/// A ilha em repouso está ligada e mostra `look`?
pub fn wants(cfg: &Value, look: Look) -> bool {
    on_island(cfg, KEY) && chosen(cfg).contains(&look)
}

/// Liga ou desliga um escolhido (ou o "só uma gota") pelo id do interruptor. Devolve false se o id
/// não é da ilha em repouso.
pub fn toggle(cfg: &mut Value, id: &str) -> bool {
    let Some(code) = id.strip_prefix("repouso:") else { return false };
    if code == "gota" {
        let on = dot_on(cfg);
        cfg.obj_mut(KEY).insert("gota".into(), json!(!on));
        return true;
    }
    let Some(look) = Look::ALL.into_iter().find(|l| l.code() == code) else { return false };
    let mut list = chosen(cfg);
    match list.iter().position(|l| *l == look) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(look),
    }
    let codes: Vec<&str> = Look::ALL.into_iter().filter(|l| list.contains(l)).map(Look::code).collect();
    cfg.obj_mut(KEY).insert("mostrar".into(), json!(codes));
    true
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Weather {
    temp: i64,
    icon: String,
    desc: String,
    /// a condição em poucas letras ("Quase limpo"), para quando a inteira não cabe
    desc_short: String,
    max: Option<i64>,
    min: Option<i64>,
    /// "Chuva a partir das 17h"
    line: Option<String>,
    /// "Chuva às 17h"
    line_short: Option<String>,
    /// as próximas horas: (hora, ícone, temperatura)
    hours: Vec<(i64, String, i64)>,
}

#[derive(Clone, Debug, PartialEq)]
struct Quotes {
    at: f64,
    /// o dólar e o bitcoin: (preço, variação no dia, os preços de hoje)
    list: [(f64, f64, Vec<f64>); 2],
}

#[derive(Clone, Debug, PartialEq)]
struct Track {
    title: String,
    artist: String,
    app: String,
    playing: bool,
    /// o player ainda está aberto (dá para tocar daqui)
    live: bool,
    colors: [Rgba; 2],
    /// a última vez que ela estava tocando (segundos desde 1970; None: guardada por uma versão
    /// antiga, sem a hora)
    when: Option<f64>,
    /// onde ela parou e quanto ela tem (segundos; 0: o player não diz)
    position: f64,
    duration: f64,
    /// o nome da capa guardada (na pasta das capas), se tem
    cover: Option<String>,
}

/// O sol de um dia numa cidade (os horários em segundos desde 1970; None: o sol não nasce ou
/// não se põe nesse dia, perto dos polos). Calculado uma vez por dia e cidade.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sun {
    day: i64,
    lat: f64,
    lon: f64,
    rise: Option<f64>,
    set: Option<f64>,
    /// o nascer de amanhã (de noite, o próximo nascer)
    rise_next: Option<f64>,
}

/// Os Pomodoros de hoje, do que o widget Pomodoro guarda nas opções.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FocusToday {
    done: i64,
    minutes: i64,
    /// a meta do dia: os focos até a pausa longa ("longa_a_cada", 4 de fábrica)
    goal: i64,
}

/// A próxima lua cheia (procurar custa algumas contas, então é uma vez por dia).
#[derive(Clone, Copy, Debug, PartialEq)]
struct FullMoonSearch {
    /// o dia em que procurou
    searched_on: i64,
    /// a próxima lua cheia nos próximos 31 dias
    next: Option<f64>,
}

/// O que a pílula mostra num instante: o escolhido da vez e, logo depois de uma troca, o que
/// está saindo.
struct Showing {
    look: Look,
    leaving: Option<Leaving>,
}

/// O escolhido que está saindo, e quanto da troca já passou (0 a 1).
struct Leaving {
    look: Look,
    progress: f32,
    /// a troca vai para trás (a roda do mouse para cima): o que sai desce e o novo vem do alto
    back: bool,
}

/// Uma troca feita pela roda do mouse: o turno em que ela caiu, quem estava na pílula e se foi
/// para trás. Na conta do relógio, o que sai seria o do turno anterior; aqui é quem estava.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Wheeled {
    turn: i64,
    from: Look,
    back: bool,
}

pub struct Rest {
    clock: Clock,
    check_at: Instant,
    /// o PC parado há 5 min
    idle: bool,
    /// nos testes: segundos parado
    fake_idle: Option<f64>,
    weather: Option<Weather>,
    quotes: Option<Quotes>,
    track: Option<Track>,
    focus_today: FocusToday,
    /// o fim do expediente hoje (só de segunda a sexta, com a fonte ligada)
    workday_end: Option<f64>,
    /// a cidade do widget Tempo: (latitude, longitude)
    city: Option<(f64, f64)>,
    sun: Option<Sun>,
    full_moon: Option<FullMoonSearch>,
    /// a próxima passagem visível da ISS: (início, de onde vem)
    iss: Option<(f64, f64)>,
    /// os escolhidos que dá para mostrar agora (nunca vazio)
    looks: Vec<Look>,
    /// "Só uma gota" ligado nas opções
    dot_only: bool,
    /// um resumo de tudo o que se mostra (fora o relógio): mudou, a ilha precisa redesenhar
    data_version: u64,
    /// aberta, a ilha para de alternar e fica no que estava quando abriu
    held_while_open: Cell<Option<Look>>,
    /// o modo foco ligado (só a hora, o dia e o ícone), com o horário do fim ("" = até desligar)
    focus_mode: Option<String>,
    /// segundos somados ao relógio na conta da vez de cada escolhido: a roda do mouse adianta
    /// (ou atrasa) a vez, e a alternância continua dali, 7 s depois
    shift: f64,
    /// a última troca pela roda (para animar a saída de quem estava, e no sentido certo)
    wheeled: Option<Wheeled>,
    /// o mês no calendário do relógio aberto, em meses a partir do de hoje (volta a 0 quando a
    /// ilha fecha)
    month_shift: Cell<i32>,
    /// as músicas tocadas antes da última (a mais nova primeiro)
    recent: Vec<Track>,
    /// a pasta das capas guardadas (None nos testes: sem ler a pasta de dados de verdade) e as
    /// capas já lidas dela, pelo nome
    covers_dir: Option<PathBuf>,
    covers: RefCell<HashMap<String, Arc<Image>>>,
}

impl Default for Rest {
    fn default() -> Rest {
        Rest::new()
    }
}

/// Espaço entre a descrição do tempo e a linha das próximas horas, na mesma altura.
const ROW_GAP: f32 = 14.0;

/// Dois textos na mesma linha, um à esquerda e outro à direita, com `room` de espaço para os
/// dois (fora o vão entre eles): devolve a largura máxima de cada um. Cabendo, cada um fica com o
/// que precisa; não cabendo, nenhum fica com menos da metade (o outro cede o que sobra), sem
/// nunca um passar por cima do outro. É só a última garantia: antes, `weather_row` escolhe as
/// versões curtas, que cabem.
fn share_row(left: f32, right: f32, room: f32) -> (f32, f32) {
    if left + right <= room {
        return (left, right);
    }
    let half = room / 2.0;
    match (left <= half, right <= half) {
        (true, _) => (left, room - left),
        (_, true) => (room - right, right),
        _ => (half, half),
    }
}

/// A descrição e a linha das próximas horas que dividem uma linha da ilha aberta (`room` de
/// espaço, fora o vão): as inteiras se couberem; senão, encurta primeiro a descrição, depois a
/// linha, depois as duas ("Granizo" e "Trovoada até 17h"). Nunca cortadas com "…".
fn weather_row<'a>(c: &Canvas, desc: [&'a str; 2], line: [&'a str; 2], room: f32) -> (&'a str, &'a str) {
    let width = |desc: &str, line: &str| c.text_width(desc, Face::Regular, 12.5) + if line.is_empty() { 0.0 } else { ROW_GAP + c.text_width(line, Face::Regular, 12.0) };
    [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().map(|(d, l)| (desc[d], line[l])).find(|(d, l)| width(d, l) <= room).unwrap_or((desc[1], line[1]))
}

/// "2h28", "40 min", "3 dias"
fn countdown(secs: f64) -> String {
    let m = (secs / 60.0).ceil().max(0.0) as i64;
    if m < 1 {
        "agora".into()
    } else if m < 60 {
        format!("{m} min")
    } else if m < 24 * 60 {
        let (h, r) = (m / 60, m % 60);
        if r == 0 { format!("{h} h") } else { format!("{h}h{r:02}") }
    } else {
        let d = (secs / 86400.0).round().max(1.0) as i64;
        if d == 1 { "1 dia".into() } else { format!("{d} dias") }
    }
}

/// "5,94", "369 mil"
fn short_price(v: f64) -> String {
    if v >= 10000.0 {
        format!("{:.0} mil", v / 1000.0)
    } else {
        format!("{v:.2}").replace('.', ",")
    }
}

const WEEKDAYS: [&str; 7] = ["segunda", "terça", "quarta", "quinta", "sexta", "sábado", "domingo"];
const WEEKDAYS_SHORT: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
const MONTHS_SHORT: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];

impl Rest {
    pub fn new() -> Rest {
        Rest {
            clock: Clock::default(),
            check_at: Instant::now(),
            idle: false,
            fake_idle: None,
            weather: None,
            quotes: None,
            track: None,
            focus_today: FocusToday { done: 0, minutes: 0, goal: 4 },
            workday_end: None,
            city: None,
            sun: None,
            full_moon: None,
            iss: None,
            looks: vec![Look::Clock],
            dot_only: false,
            data_version: 0,
            held_while_open: Cell::new(None),
            focus_mode: None,
            shift: 0.0,
            wheeled: None,
            month_shift: Cell::new(0),
            recent: Vec::new(),
            covers_dir: if cfg!(test) { None } else { Some(covers_dir()) },
            covers: RefCell::new(HashMap::new()),
        }
    }

    /// Mostrando só `look` (ou a gota, ou parado), numa sexta-feira 14:32, com dados de exemplo.
    #[cfg(test)]
    pub fn sample(look: Option<Look>, idle: bool) -> Rest {
        let friday = crate::data::util::days_from_civil(2026, 11, 20) as f64 * 86400.0;
        let now = friday + (14.0 + 32.0 / 60.0 + 3.0) * 3600.0;
        let mut r = Rest::new();
        r.clock = Clock::at(now, -3.0 * 3600.0);
        r.fake_idle = Some(if idle { IDLE_AFTER + 1.0 } else { 1.0 });
        let mut cfg = json!({"tempo": {"lat": -23.55, "lon": -46.63, "place": "São Paulo"}, "app": {},
            "pomodoro": {"hoje": {"data": "2026-11-20", "pomodoros": 3, "minutos": 75}}});
        match look {
            Some(l) => cfg[KEY] = json!({"mostrar": [l.code()]}),
            None => cfg[KEY] = json!({"mostrar": ["relogio"], "gota": true}),
        }
        let mut cache = sample_cache(now);
        let mut out = Vec::new();
        r.tick(&mut Ctx { cfg: &mut cfg, cache: &mut cache, key: KEY, out: &mut out });
        r
    }

    /// O tempo com textos compridos (a descrição e a linha das próximas horas disputando a mesma
    /// altura na ilha aberta).
    #[cfg(test)]
    pub fn sample_long_weather() -> Rest {
        let mut r = Rest::sample(Some(Look::Weather), false);
        if let Some(wx) = r.weather.as_mut() {
            wx.desc = "Trovoada com granizo".into();
            wx.desc_short = "Granizo".into();
            wx.line = Some("Trovoadas até 17h, depois predominantemente limpo".into());
            wx.line_short = Some("Trovoada até 17h".into());
        }
        r
    }

    /// No modo foco até 15:30.
    #[cfg(test)]
    pub fn sample_focus() -> Rest {
        Rest { focus_mode: Some("15:30".into()), ..Rest::sample(Some(Look::Clock), false) }
    }

    /// O relógio parado em `unix` (nos testes).
    #[cfg(test)]
    fn at(&mut self, unix: f64) {
        self.clock = Clock::at(unix, self.clock.offset());
    }

    /// O que a pílula mostra em `now`. A vez de cada escolhido sai do relógio (o turno `n` é o
    /// n-ésimo bloco de 7 s desde 1970), então não precisa guardar estado: a mesma hora dá sempre
    /// o mesmo escolhido. Parado ou com um só, não alterna.
    fn showing(&self, now: f64) -> Showing {
        let count = self.looks.len() as i64;
        if count <= 1 || self.idle {
            return Showing { look: self.looks.first().copied().unwrap_or(Look::Clock), leaving: None };
        }
        let turn = self.turn(now);
        let into_turn = now + self.shift - turn as f64 * ROTATE_EVERY;
        let look_of = |turn: i64| self.looks[turn.rem_euclid(count) as usize];
        let leaving = (into_turn < TURN_SECS).then(|| {
            let progress = (into_turn / TURN_SECS) as f32;
            match self.wheeled.filter(|w| w.turn == turn) {
                Some(w) => Leaving { look: w.from, progress, back: w.back },
                None => Leaving { look: look_of(turn - 1), progress, back: false },
            }
        });
        Showing { look: look_of(turn), leaving }
    }

    /// O turno de `now` (o bloco de 7 s), contando o que a roda do mouse adiantou ou atrasou.
    fn turn(&self, now: f64) -> i64 {
        ((now + self.shift) / ROTATE_EVERY).floor() as i64
    }

    /// Alternando e perto de uma troca (ou no meio dela)? Aí a ilha pede quadros, começando um
    /// pouco antes: ela só confere a cada 250 ms e perderia o começo da animação.
    fn near_a_turn(&self, now: f64) -> bool {
        if self.looks.len() <= 1 || self.idle {
            return false;
        }
        let into_turn = (now + self.shift).rem_euclid(ROTATE_EVERY);
        into_turn < TURN_SECS + 0.05 || into_turn > ROTATE_EVERY - 0.3
    }

    /// A cor da hora do dia: amanhecer, dia, entardecer, noite (pelo sol, se souber a cidade).
    fn period_color(&self, now: f64) -> Rgba {
        let hour = (now + self.clock.offset()).rem_euclid(86400.0) / 3600.0;
        match self.city.map(|(lat, lon)| astro::sun_alt_az(now, lat, lon).0) {
            Some(a) if a < -6.0 => NIGHT,
            Some(a) if a < 8.0 => {
                if hour < 12.0 {
                    DAWN
                } else {
                    DUSK
                }
            }
            Some(_) => DAY,
            None => match hour {
                h if (6.0..8.0).contains(&h) => DAWN,
                h if (8.0..17.0).contains(&h) => DAY,
                h if (17.0..19.0).contains(&h) => DUSK,
                _ => NIGHT,
            },
        }
    }

    fn secs_of_day(&self, t: f64) -> f64 {
        (t + self.clock.offset()).rem_euclid(86400.0)
    }

    /// "sex, 20 nov"
    fn date_short(&self, t: f64) -> String {
        let date = crate::data::util::civil_from_days(self.clock.day(t));
        format!("{}, {} {}", WEEKDAYS_SHORT[self.clock.weekday(t) as usize], date.day, MONTHS_SHORT[(date.month as usize).clamp(1, 12) - 1])
    }

    /// "sexta, 20 de novembro"
    fn date_long(&self, t: f64) -> String {
        format!("{}, {}", WEEKDAYS[self.clock.weekday(t) as usize], self.clock.date_text(t))
    }

    /// "17:00" hoje, "ter, 24" em outro dia
    fn time_or_weekday(&self, t: f64, now: f64) -> String {
        if self.clock.day(t) == self.clock.day(now) {
            self.clock.hhmm(t)
        } else {
            let date = crate::data::util::civil_from_days(self.clock.day(t));
            format!("{}, {}", WEEKDAYS_SHORT[self.clock.weekday(t) as usize], date.day)
        }
    }

    /// Os próximos acontecimentos, do mais perto: (nome, horário, cor).
    fn upcoming(&self, now: f64) -> Vec<(&'static str, f64, Rgba)> {
        let mut out = Vec::new();
        if let Some(end) = self.workday_end.filter(|e| *e > now) {
            out.push(("Fim do expediente", end, WORK));
        }
        if let Some(sun) = self.sun {
            let set = sun.set.map(|s| if s > now { s } else { s + 86400.0 });
            let rise = match (sun.rise, sun.rise_next) {
                (Some(r), _) if r > now => Some(r),
                (_, next) => next.filter(|r| *r > now),
            };
            if let Some(s) = set {
                out.push(("Pôr do sol", s, SUNSET));
            }
            if let Some(r) = rise {
                out.push(("Nascer do sol", r, SUNRISE));
            }
        }
        if let Some(t) = self.full_moon.and_then(|f| f.next).filter(|t| *t > now) {
            out.push(("Lua cheia", t, MOON));
        }
        if let Some((t, _)) = self.iss.filter(|p| p.0 > now) {
            out.push(("A ISS passa", t, ISS));
        }
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        out
    }

    /// A frase do expediente: "faltam 2h28 para o fim do expediente".
    fn work_line(&self, now: f64) -> String {
        match self.workday_end {
            Some(end) if end > now => format!("faltam {} para o fim do expediente", countdown(end - now)),
            Some(_) => "expediente encerrado".into(),
            None if self.clock.weekday(now) >= 5 => "fim de semana".into(),
            None => match self.upcoming(now).first() {
                Some((name, t, _)) => format!("{} às {}", name.to_lowercase(), self.time_or_weekday(*t, now)),
                None => String::new(),
            },
        }
    }

    /// Dá para mostrar `look` agora (tem os dados)?
    fn available(&self, look: Look) -> bool {
        match look {
            Look::Clock | Look::Next | Look::Focus => true,
            Look::Day => self.sun.is_some_and(|s| s.rise.is_some() && s.set.is_some()),
            Look::Weather => self.weather.is_some(),
            Look::Quotes => self.quotes.is_some(),
            Look::Media => self.track.is_some(),
        }
    }

    /// O botão do que está aberto (a forma média ou a aberta), se tem: (ação, rótulo).
    fn button(&self, look: Look) -> Option<(u32, &'static str)> {
        match look {
            Look::Focus => Some((START, "Começar um foco")),
            Look::Media if self.track.as_ref().is_some_and(|t| t.live && !t.playing) => Some((PLAY, "Tocar")),
            _ => None,
        }
    }

    /// O que está na pílula nesta forma (aberta, parado no que estava quando abriu).
    fn look_for(&self, form: IslandForm, now: f64) -> Look {
        if form == IslandForm::Large {
            if let Some(l) = self.held_while_open.get().filter(|l| self.looks.contains(l)) {
                return l;
            }
        }
        self.showing(now).look
    }

    // ---------- desenho ----------

    /// O modo foco: só a hora, o dia e o ícone do foco (e, aberta, o botão de sair).
    fn draw_focus_mode(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, now: f64) {
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let glyph = |c: &mut Canvas, cx: f32, cy: f32, size: f32| {
            c.text(cx, cy, crate::ui::theme::icon("foco"), Face::Icons, size, FOCUS, Anchor::MM, None);
        };
        let time = self.clock.hhmm(now);
        let until = match self.focus_mode.as_deref() {
            Some("") | None => "Modo foco".to_string(),
            Some(t) => format!("Modo foco até {t}"),
        };
        match form {
            IslandForm::Bubble => glyph(c, x + w / 2.0, cy, (w * 0.42).min(14.0)),
            IslandForm::Compact | IslandForm::Banner => {
                glyph(c, x + 24.0, cy, 13.0);
                c.text(x + 42.0, cy, &time, Face::Semibold, 15.0, FG, Anchor::LM, None);
                c.text(x + w - 20.0, cy, &self.date_short(now), Face::Regular, 12.0, MUTED, Anchor::RM, None);
            }
            IslandForm::Medium => {
                iu::badge_icon(c, x + 39.0, cy, 20.0, "foco", FOCUS);
                let tw = c.text(x + 72.0, cy, &time, Face::Semibold, 28.0, FG, Anchor::LM, None);
                let tx = x + 72.0 + tw + 14.0;
                iu::two_lines(c, tx, cy, x + w - 20.0 - tx, &self.date_long(now), &until, FOCUS);
            }
            IslandForm::Large => {
                iu::header(c, r, "MODO FOCO", FOCUS, &self.date_short(now), MUTED);
                glyph(c, x + 36.0, y + 76.0, 22.0);
                c.text(x + 58.0, y + 76.0, &time, Face::Semibold, 46.0, FG, Anchor::LM, None);
                c.text(x + 24.0, y + 116.0, &self.date_long(now), Face::Regular, 13.0, MUTED, Anchor::LM, None);
                c.text(x + 24.0, y + 138.0, &until, Face::Regular, 12.0, FOCUS, Anchor::LM, None);
                let b = button_rect(IslandForm::Large, w, h);
                iu::pill_button(c, (x + b.0, y + b.1, x + b.2, y + b.3), "Sair do modo foco", true, hot == Some(END_FOCUS));
            }
        }
    }

    /// A gota: um ponto na cor da hora do dia, respirando (parado, só o ponto apagado).
    fn draw_dot(&self, c: &mut Canvas, r: Rect4, now: f64) {
        let (x, y, w, h) = r;
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let col = self.period_color(now);
        if self.idle {
            c.fill_circle(cx, cy, 4.5, col.alpha(0.65));
            return;
        }
        // respira devagar: um ciclo a cada 3,2 s, de 72% a 100% do tamanho, com um halo em volta
        const BREATH_SECS: f64 = 3.2;
        let phase = (now.rem_euclid(BREATH_SECS) / BREATH_SECS) as f32;
        let breath = 0.72 + 0.28 * (0.5 - 0.5 * (phase * std::f32::consts::TAU).cos());
        c.fill_circle(cx, cy, 10.0 * breath, col.alpha(0.16 * breath));
        c.fill_circle(cx, cy, 6.0 * breath, col.alpha(0.55 + 0.45 * breath));
    }

    fn draw_look(&self, c: &mut Canvas, look: Look, form: IslandForm, r: Rect4, hot: Option<u32>, now: f64) {
        let form = if form == IslandForm::Banner { IslandForm::Compact } else { form };
        match look {
            Look::Clock => self.draw_clock(c, form, r, hot, now),
            Look::Day => self.draw_day(c, form, r, now),
            Look::Next => self.draw_next(c, form, r, now),
            Look::Weather => self.draw_weather(c, form, r, now),
            Look::Quotes => self.draw_quotes(c, form, r, now),
            Look::Focus => self.draw_focus(c, form, r, hot),
            Look::Media => self.draw_media(c, form, r, hot),
        }
        // o botão do que está aberto (a música aberta tem os dela)
        let own_buttons = look == Look::Media && form == IslandForm::Large;
        if let (IslandForm::Medium | IslandForm::Large, Some((act, label)), false) = (form, self.button(look), own_buttons) {
            let (x, y, w, h) = r;
            let b = button_rect(form, w, h);
            let rect = (x + b.0, y + b.1, x + b.2, y + b.3);
            if form == IslandForm::Medium {
                let (cx, cy) = ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0);
                c.fill_circle(cx, cy, 18.0, if hot == Some(act) { WHITE } else { FG });
                drawing::play(c, cx + 1.0, cy, 14.0, Rgba(18, 18, 20, 255));
            } else {
                iu::pill_button(c, rect, label, true, hot == Some(act));
            }
        }
    }

    fn draw_clock(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, now: f64) {
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let time = self.clock.hhmm(now);
        let col = self.period_color(now);
        match form {
            IslandForm::Compact => {
                c.text(x + 20.0, cy, &time, Face::Semibold, 15.0, FG, Anchor::LM, None);
                c.text(x + w - 20.0, cy, &self.date_short(now), Face::Regular, 12.0, MUTED, Anchor::RM, None);
            }
            IslandForm::Medium => {
                let tw = c.text(x + 22.0, cy, &time, Face::Semibold, 30.0, FG, Anchor::LM, None);
                let tx = x + 22.0 + tw + 16.0;
                iu::two_lines(c, tx, cy, x + w - 20.0 - tx, &self.date_long(now), &self.work_line(now), col);
            }
            _ => {
                self.draw_today_column(c, r, now);
                self.draw_calendar(c, r, hot, now);
            }
        }
    }

    /// A coluna da esquerda do relógio aberto: o dia da semana, a hora, a data e, embaixo, quanto
    /// do ano já passou (a porcentagem, a barra e quantos dias faltam).
    fn draw_today_column(&self, c: &mut Canvas, r: Rect4, now: f64) {
        let (x, y, _, h) = r;
        let (x0, x1) = (x + 22.0, x + CAL_X0 - 20.0);
        let weekday = crate::data::util::WEEKDAYS_LONG[self.clock.weekday(now) as usize].to_uppercase();
        c.text(x0, y + CAL_HEAD_Y, &weekday, Face::Semibold, 10.5, CAL_RED, Anchor::LM, Some(x1 - x0));
        c.text(x0 - 1.0, y + 64.0, &self.clock.hhmm(now), Face::Semibold, 40.0, FG, Anchor::LM, None);
        c.text(x0, y + 98.0, &self.clock.date_text(now), Face::Regular, 13.0, MUTED, Anchor::LM, Some(x1 - x0));
        let year = self.year_so_far(now);
        let base = y + h - 22.0;
        c.text(x0, base - 34.0, &year.year.to_string(), Face::Semibold, 12.0, FG, Anchor::LM, None);
        c.text(x1, base - 34.0, &format!("{}%", (year.done * 100.0).floor() as i64), crate::widgets::VALUE, 12.0, CAL_RED, Anchor::RM, None);
        iu::bar(c, x0, x1, base - 17.0, year.done as f32, CAL_RED);
        let left = match year.days_left {
            0 => "último dia do ano".to_string(),
            1 => "falta 1 dia".to_string(),
            n => format!("faltam {n} dias"),
        };
        c.text(x0, base, &left, Face::Regular, 11.0, MUTED, Anchor::LM, Some(x1 - x0));
    }

    /// O calendário do mês, como no Calendário da Apple: o nome do mês em vermelho com as setas
    /// para os outros meses, as iniciais dos dias e as semanas de domingo a sábado, com o dia de
    /// hoje num círculo vermelho e o fim de semana mais apagado.
    fn draw_calendar(&self, c: &mut Canvas, r: Rect4, hot: Option<u32>, now: f64) {
        let (x, y, w, _) = r;
        let today = self.clock.day(now);
        let view = MonthView::around(today, self.month_shift.get());
        let rects = calendar_rects(w);
        // o nome do mês (com o ano, se não for o de hoje)
        let this_year = crate::data::util::civil_from_days(today).year;
        let mut title = crate::data::util::MONTHS_LONG[view.month as usize - 1].to_uppercase();
        if view.year != this_year {
            title += &format!(" {}", view.year);
        }
        let (tx0, _, tx1, _) = rects.title;
        c.text(x + tx0, y + CAL_HEAD_Y, &title, Face::Semibold, 11.0, CAL_RED, Anchor::LM, Some(tx1 - tx0));
        // as setas, num círculo claro com o mouse em cima
        for (act, (ax0, ay0, ax1, ay1), back) in [(PREV_MONTH, rects.prev, true), (NEXT_MONTH, rects.next, false)] {
            let (cx, cy) = (x + (ax0 + ax1) / 2.0, y + (ay0 + ay1) / 2.0);
            if hot == Some(act) {
                c.fill_circle(cx, cy, CAL_ARROW / 2.0, WHITE.alpha(0.14));
            }
            let dx = if back { 2.5 } else { -2.5 };
            c.line(&[(cx + dx, cy - 5.0), (cx - dx, cy), (cx + dx, cy + 5.0)], FG.alpha(0.85), 1.8);
        }
        // as iniciais e os dias
        let col_w = (w - CAL_RIGHT - CAL_X0) / 7.0;
        let col_x = |col: i64| x + CAL_X0 + col_w * (col as f32 + 0.5);
        for (col, initial) in CAL_INITIALS.iter().enumerate() {
            c.text(col_x(col as i64), y + CAL_INITIALS_Y, initial, Face::Semibold, 9.5, MUTED, Anchor::MM, None);
        }
        for date in 0..view.days {
            let place = view.lead + date;
            let (col, row) = (place % 7, place / 7);
            let (cx, cy) = (col_x(col), y + CAL_FIRST_ROW_Y + CAL_ROW * row as f32);
            let number = (date + 1).to_string();
            if view.first + date == today {
                c.fill_circle(cx, cy, CAL_TODAY_R, CAL_RED);
                c.text(cx, cy, &number, Face::NumbersMedium, 11.5, WHITE, Anchor::MM, None);
            } else {
                let weekend = col == 0 || col == 6;
                c.text(cx, cy, &number, Face::Numbers, 11.5, if weekend { MUTED } else { FG }, Anchor::MM, None);
            }
        }
    }

    /// Quanto do ano já passou em `now` (de meia-noite de 1º de janeiro até a do ano seguinte).
    fn year_so_far(&self, now: f64) -> YearSoFar {
        use crate::data::util::{civil_from_days, days_from_civil};
        let today = self.clock.day(now);
        let year = civil_from_days(today).year;
        let (first, next) = (days_from_civil(year, 1, 1), days_from_civil(year + 1, 1, 1));
        let (start, end) = (self.clock.midnight(first), self.clock.midnight(next));
        YearSoFar { year, done: ((now - start) / (end - start)).clamp(0.0, 1.0), days_left: next - today - 1 }
    }

    /// A régua do dia (das 4 h às 23 h) entre x0 e x1, na altura cy.
    fn draw_track(&self, c: &mut Canvas, x0: f32, x1: f32, cy: f32, thick: f32, now: f64) {
        let pos = |t: f64| self.x_on_day_track(x0, x1, t);
        c.fill_round_rect(x0, cy - thick / 2.0, x1, cy + thick / 2.0, thick / 2.0, Rgba(255, 255, 255, 34));
        if let Some(Sun { rise: Some(rise), set: Some(set), .. }) = self.sun {
            c.fill_round_rect(pos(rise), cy - thick / 2.0, pos(set), cy + thick / 2.0, thick / 2.0, Rgba(255, 210, 110, 215));
        }
        if let Some(end) = self.workday_end {
            let ex = pos(end);
            c.fill_round_rect(ex - 1.0, cy - thick / 2.0 - 3.0, ex + 1.0, cy + thick / 2.0 + 3.0, 1.0, WORK);
        }
        let night = self.period_color(now) == NIGHT;
        let nx = pos(now);
        let rr = thick / 2.0 + 3.5;
        c.fill_circle(nx, cy, rr + 1.6, Rgba(0, 0, 0, 255));
        c.fill_circle(nx, cy, rr, if night { MOON } else { DAY });
    }

    /// Onde o horário `t` cai na régua do dia que vai de x0 a x1 (antes das 4 h, no começo;
    /// depois das 23 h, no fim).
    fn x_on_day_track(&self, x0: f32, x1: f32, t: f64) -> f32 {
        let hour = self.secs_of_day(t) / 3600.0;
        let frac = (hour - DAY_TRACK_FROM_HOUR) / (DAY_TRACK_TO_HOUR - DAY_TRACK_FROM_HOUR);
        x0 + (x1 - x0) * frac.clamp(0.0, 1.0) as f32
    }

    fn draw_day(&self, c: &mut Canvas, form: IslandForm, r: Rect4, now: f64) {
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let time = self.clock.hhmm(now);
        let sun = self.sun.unwrap_or(Sun { day: 0, lat: 0.0, lon: 0.0, rise: None, set: None, rise_next: None });
        match form {
            IslandForm::Compact => {
                self.draw_track(c, x + 20.0, x + w - 70.0, cy, 5.0, now);
                c.text(x + w - 18.0, cy, &time, Face::Semibold, 14.0, FG, Anchor::RM, None);
            }
            IslandForm::Medium => {
                let (x0, x1) = (x + 26.0, x + w - 26.0);
                self.draw_track(c, x0, x1, cy, 7.0, now);
                if let Some(end) = self.workday_end {
                    c.text(self.x_on_day_track(x0, x1, end), cy - 15.0, &format!("fim {}", self.clock.hhmm(end)), Face::Semibold, 10.5, WORK, Anchor::MS, None);
                }
                for t in [sun.rise, sun.set].into_iter().flatten() {
                    c.text(self.x_on_day_track(x0, x1, t), cy + 14.0, &self.clock.hhmm(t), Face::Regular, 10.5, MUTED, Anchor::MA, None);
                }
            }
            _ => {
                let col = self.period_color(now);
                let light = match (sun.rise, sun.set) {
                    (Some(rise), _) if now < rise => format!("o sol nasce em {}", countdown(rise - now)),
                    (_, Some(set)) if now < set => format!("luz do dia: falta {}", countdown(set - now)),
                    _ => "o sol já se pôs".into(),
                };
                iu::header(c, r, "O DIA", col, &light, MUTED);
                c.text(x + 22.0, y + 62.0, &time, Face::Semibold, 30.0, FG, Anchor::LM, None);
                let (x0, x1) = (x + 22.0, x + w - 22.0);
                self.draw_track(c, x0, x1, y + 96.0, 7.0, now);
                c.text(x0, y + 112.0, "4h", Face::Regular, 10.0, FAINT, Anchor::LA, None);
                c.text((x0 + x1) / 2.0, y + 112.0, "12h", Face::Regular, 10.0, FAINT, Anchor::MA, None);
                c.text(x1, y + 112.0, "23h", Face::Regular, 10.0, FAINT, Anchor::RA, None);
                let mut rows: Vec<(&str, f64, Rgba)> = Vec::new();
                if let Some(t) = sun.rise {
                    rows.push(("Nascer do sol", t, SUNRISE));
                }
                if let Some(t) = self.workday_end {
                    rows.push(("Fim do expediente", t, WORK));
                }
                if let Some(t) = sun.set {
                    rows.push(("Pôr do sol", t, SUNSET));
                }
                for (i, (name, t, dot)) in rows.iter().enumerate() {
                    let ry = y + 144.0 + i as f32 * 22.0;
                    c.fill_circle(x + 28.0, ry, 3.5, *dot);
                    c.text(x + 40.0, ry, name, Face::Regular, 12.5, FG, Anchor::LM, None);
                    let ago = if *t > now { format!("em {}", countdown(t - now)) } else { format!("há {}", countdown(now - t)) };
                    c.text(x + w - 22.0, ry, &format!("{} · {ago}", self.clock.hhmm(*t)), Face::Regular, 12.0, MUTED, Anchor::RM, None);
                }
            }
        }
    }

    fn draw_next(&self, c: &mut Canvas, form: IslandForm, r: Rect4, now: f64) {
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let list = self.upcoming(now);
        let Some(&(name, t, col)) = list.first() else {
            iu::compact_text(c, x + 20.0, r, "Nada marcado", "", MUTED);
            return;
        };
        match form {
            IslandForm::Compact => {
                c.fill_circle(x + 24.0, cy, 4.5, col);
                iu::compact_text(c, x + 38.0, r, name, &countdown(t - now), col);
            }
            IslandForm::Medium => {
                iu::badge_icon(c, x + 39.0, cy, 20.0, next_glyph(name), col);
                let sub = match list.get(1) {
                    Some((n2, t2, _)) => format!("às {} · {} {}", self.time_or_weekday(t, now), n2.to_lowercase(), self.time_or_weekday(*t2, now)),
                    None => format!("às {}", self.time_or_weekday(t, now)),
                };
                let vw = c.text(x + w - 22.0, cy, &format!("em {}", countdown(t - now)), Face::Semibold, 14.5, col, Anchor::RM, None);
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 22.0 - vw - 12.0, name, &sub, MUTED);
            }
            _ => {
                iu::header(c, r, "A SEGUIR", col, &self.date_short(now), MUTED);
                for (i, (n, at, dot)) in list.iter().take(5).enumerate() {
                    let ry = y + 58.0 + i as f32 * 28.0;
                    if i > 0 {
                        c.fill_round_rect(x + 22.0, ry - 14.5, x + w - 22.0, ry - 14.0, 0.0, Rgba(255, 255, 255, 22));
                    }
                    c.fill_circle(x + 28.0, ry, 4.0, *dot);
                    c.text(x + 42.0, ry, n, if i == 0 { Face::Semibold } else { Face::Regular }, 13.0, FG, Anchor::LM, Some(w - 200.0));
                    c.text(x + w - 92.0, ry, &self.time_or_weekday(*at, now), Face::Regular, 12.0, MUTED, Anchor::RM, None);
                    c.text(x + w - 22.0, ry, &countdown(at - now), Face::Semibold, 12.5, if i == 0 { *dot } else { FG }, Anchor::RM, None);
                }
            }
        }
    }

    fn draw_weather(&self, c: &mut Canvas, form: IslandForm, r: Rect4, now: f64) {
        let Some(wx) = &self.weather else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let art = |c: &mut Canvas, name: &str, cx: f32, cy: f32, size: f32| {
            let img = weather_art(name, (size * c.s).round() as u32);
            c.image(&img, cx - size / 2.0, cy - size / 2.0);
        };
        let minmax = match (wx.max, wx.min) {
            (Some(a), Some(b)) => format!("Máx. {a}° · Mín. {b}°"),
            _ => String::new(),
        };
        match form {
            IslandForm::Compact => {
                art(c, &wx.icon, x + 26.0, cy, 26.0);
                let temp = format!("{}°", wx.temp);
                let options = match (&wx.line, &wx.line_short) {
                    (Some(line), Some(short)) => [line.as_str(), short.as_str()],
                    _ => [wx.desc.as_str(), wx.desc_short.as_str()],
                };
                let label = iu::fitting(c, &options, Face::Regular, 12.0, iu::compact_room(c, x + 46.0, r, &temp));
                iu::compact_text(c, x + 46.0, r, label, &temp, FG);
            }
            IslandForm::Medium => {
                art(c, &wx.icon, x + 39.0, cy, 46.0);
                let room = w - 74.0 - 22.0;
                let (long, short) = (format!("{}° · {}", wx.temp, wx.desc), format!("{}° · {}", wx.temp, wx.desc_short));
                let title = iu::fitting(c, &[&long, &short], Face::Semibold, 13.5, room);
                let sub = match (&wx.line, &wx.line_short) {
                    (Some(line), Some(short)) => iu::fitting(c, &[line, short], Face::Regular, 11.5, room).to_string(),
                    _ => minmax,
                };
                iu::two_lines(c, x + 74.0, cy, room, title, &sub, MUTED);
            }
            _ => {
                iu::header(c, r, "TEMPO", self.period_color(now), &self.date_short(now), MUTED);
                art(c, &wx.icon, x + 46.0, y + 74.0, 50.0);
                c.text(x + 82.0, y + 66.0, &format!("{}°", wx.temp), Face::Semibold, 30.0, FG, Anchor::LM, None);
                c.text(x + w - 22.0, y + 66.0, &minmax, Face::Regular, 12.0, MUTED, Anchor::RM, None);
                // a descrição (embaixo da temperatura) e a linha das próximas horas (à direita)
                // ficam na mesma altura: dividem o espaço entre a temperatura e a borda
                let (left, right) = (x + 82.0, x + w - 22.0);
                let lines = [wx.line.as_deref().unwrap_or(""), wx.line_short.as_deref().unwrap_or("")];
                let (desc, line) = weather_row(c, [&wx.desc, &wx.desc_short], lines, right - left);
                let line_w = if line.is_empty() { 0.0 } else { c.text_width(line, Face::Regular, 12.0) };
                let gap = if line.is_empty() { 0.0 } else { ROW_GAP };
                let (desc_max, line_max) = share_row(c.text_width(desc, Face::Regular, 12.5), line_w, right - left - gap);
                c.text(left, y + 92.0, desc, Face::Regular, 12.5, MUTED, Anchor::LM, Some(desc_max));
                if !line.is_empty() {
                    c.text(right, y + 88.0, line, Face::Regular, 12.0, MUTED, Anchor::RM, Some(line_max));
                }
                // as próximas horas
                let n = wx.hours.len().min(6);
                if n > 0 {
                    let (x0, x1) = (x + 18.0, x + w - 18.0);
                    let cw = (x1 - x0) / 6.0;
                    for (i, (hour, icon, temp)) in wx.hours.iter().take(6).enumerate() {
                        let cx = x0 + cw * (i as f32 + 0.5);
                        c.fill_round_rect(cx - cw / 2.0 + 3.0, y + 118.0, cx + cw / 2.0 - 3.0, y + h - 16.0, 12.0, Rgba(255, 255, 255, 14));
                        c.text(cx, y + 134.0, &format!("{hour}h"), Face::Regular, 11.0, MUTED, Anchor::MM, None);
                        art(c, icon, cx, y + 160.0, 24.0);
                        c.text(cx, y + 186.0, &format!("{temp}°"), Face::Semibold, 12.5, FG, Anchor::MM, None);
                    }
                }
            }
        }
    }

    fn draw_quotes(&self, c: &mut Canvas, form: IslandForm, r: Rect4, now: f64) {
        let Some(q) = &self.quotes else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let col = |p: f64| if p < 0.0 { DOWN } else { UP };
        let symbol = |c: &mut Canvas, k: usize, cx: f32, cy: f32, r: f32| if k == 0 { wallet(c, cx, cy, r * 2.0) } else { bitcoin(c, cx, cy, r) };
        let long = |k: usize, v: f64| brl(v, if k == 0 { 2 } else { 0 });
        const NAMES: [&str; 2] = ["Dólar", "Bitcoin"];
        match form {
            IslandForm::Compact => {
                // um depois do outro, do tamanho que precisam, centralizados
                let parts: Vec<(usize, String, String, f64)> = q.list.iter().enumerate().map(|(k, (price, p, _))| (k, short_price(*price), pct(*p), *p)).collect();
                let width = |c: &Canvas, text: &str, change: &str| 25.0 + c.text_width(text, Face::Semibold, 12.5) + 4.0 + c.text_width(change, Face::Semibold, 11.5);
                let total: f32 = parts.iter().map(|(_, t, ch, _)| width(c, t, ch)).sum::<f32>() + 12.0;
                let mut x0 = x + ((w - total) / 2.0).max(14.0);
                for (k, text, change, p) in &parts {
                    symbol(c, *k, x0 + 10.0, cy, 9.5);
                    let tw = c.text(x0 + 25.0, cy, text, Face::Semibold, 12.5, FG, Anchor::LM, None);
                    c.text(x0 + 25.0 + tw + 4.0, cy, change, Face::Semibold, 11.5, col(*p), Anchor::LM, None);
                    x0 += width(c, text, change) + 12.0;
                }
            }
            IslandForm::Medium => {
                for (k, (price, p, _)) in q.list.iter().enumerate() {
                    let ry = cy + if k == 0 { -12.0 } else { 12.0 };
                    symbol(c, k, x + 34.0, ry, 9.5);
                    c.text(x + 52.0, ry, NAMES[k], Face::Regular, 12.0, MUTED, Anchor::LM, None);
                    c.text(x + 112.0, ry, &long(k, *price), Face::Semibold, 13.0, FG, Anchor::LM, None);
                    c.text(x + w - 26.0, ry, &pct(*p), Face::Semibold, 12.0, col(*p), Anchor::RM, None);
                }
            }
            _ => {
                iu::header(c, r, "COTAÇÕES", GOLD, &format!("atualizado às {}", self.clock.hhmm(q.at)), MUTED);
                for (k, (price, p, samples)) in q.list.iter().enumerate() {
                    let top = y + 46.0 + k as f32 * 76.0;
                    symbol(c, k, x + 36.0, top + 24.0, 13.0);
                    c.text(x + 60.0, top + 10.0, NAMES[k], Face::Regular, 11.5, MUTED, Anchor::LM, None);
                    c.text(x + 60.0, top + 30.0, &long(k, *price), Face::Semibold, 17.0, FG, Anchor::LM, None);
                    c.text(x + 60.0, top + 50.0, &format!("{} hoje", pct(*p)), Face::Semibold, 12.0, col(*p), Anchor::LM, None);
                    // os preços de hoje, num gráfico pequeno
                    if samples.len() >= 2 {
                        let (gx0, gx1, gy0, gy1) = (x + w - 22.0 - 120.0, x + w - 22.0, top + 8.0, top + 44.0);
                        let (lo, hi) = samples.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
                        let span = (hi - lo).max(hi.abs() * 1e-6).max(1e-9);
                        let pts: Vec<(f32, f32)> = samples
                            .iter()
                            .enumerate()
                            .map(|(i, v)| (gx0 + (gx1 - gx0) * i as f32 / (samples.len() - 1) as f32, gy1 - (gy1 - gy0) * ((v - lo) / span) as f32))
                            .collect();
                        c.line(&pts, col(*p), 1.6);
                    }
                }
                let _ = now;
            }
        }
    }

    fn draw_focus(&self, c: &mut Canvas, form: IslandForm, r: Rect4, _hot: Option<u32>) {
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let FocusToday { done, minutes, goal } = self.focus_today;
        let goal = goal.clamp(1, 8);
        let dots = |c: &mut Canvas, x0: f32, cy: f32, rr: f32, gap: f32| {
            for i in 0..goal {
                let cx = x0 + rr + i as f32 * (2.0 * rr + gap);
                if i < done {
                    c.fill_circle(cx, cy, rr, TOMATO);
                } else {
                    c.stroke_circle(cx, cy, rr - 0.75, TOMATO.alpha(0.8), 1.5);
                }
            }
            x0 + goal as f32 * (2.0 * rr + gap) - gap
        };
        let count = match done {
            0 => "Nenhum foco hoje".to_string(),
            1 => "1 foco hoje".to_string(),
            n => format!("{n} focos hoje"),
        };
        let time = countdown(minutes as f64 * 60.0);
        match form {
            IslandForm::Compact => {
                let end = dots(c, x + 20.0, cy, 4.5, 4.5);
                iu::compact_text(c, end + 10.0, r, &count, if done > 0 { &time } else { "" }, FG);
            }
            IslandForm::Medium => {
                let end = dots(c, x + 22.0, cy, 5.5, 5.0);
                let title = if done > 0 { format!("{done} de {goal} focos · {time}") } else { "Nenhum foco hoje".into() };
                let sub = if done >= goal { "Meta do dia batida".to_string() } else { format!("Começar o {}º?", done + 1) };
                iu::two_lines(c, end + 14.0, cy, x + w - 70.0 - end - 14.0, &title, &sub, TOMATO);
            }
            _ => {
                iu::header(c, r, "FOCO DO DIA", TOMATO, &if done > 0 { format!("{time} focado") } else { String::new() }, MUTED);
                c.text(x + 22.0, y + 72.0, &format!("{done} de {goal}"), Face::Semibold, 34.0, FG, Anchor::LM, None);
                let goal_line = match goal - done {
                    n if n <= 0 => "Meta do dia batida".to_string(),
                    1 => "Falta 1 para a meta do dia".to_string(),
                    n => format!("Faltam {n} para a meta do dia"),
                };
                c.text(x + 24.0, y + 102.0, &goal_line, Face::Regular, 12.5, MUTED, Anchor::LM, None);
                dots(c, x + 22.0, y + 136.0, 8.0, 8.0);
                let _ = h;
            }
        }
    }

    fn draw_media(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>) {
        let Some(t) = &self.track else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let now = self.clock.now();
        match form {
            IslandForm::Compact => {
                self.draw_cover(c, t, x + 8.0, cy - 13.0, 26.0, 7.0);
                iu::compact_text(c, x + 44.0, r, &t.title, "", MUTED);
            }
            IslandForm::Medium => {
                self.draw_cover(c, t, x + 14.0, cy - 25.0, 50.0, 12.0);
                let right = if self.button(Look::Media).is_some() { 70.0 } else { 22.0 };
                // o artista e como ela está; sem espaço, só como ela está (nunca cortado)
                let (state, room) = (self.track_state(t, now), w - 76.0 - right);
                let full = format!("{} · {state}", t.artist);
                let sub = iu::fitting(c, &[&full, &state], Face::Regular, 11.5, room);
                iu::two_lines(c, x + 76.0, cy, room, &t.title, sub, MUTED);
            }
            _ => self.draw_media_open(c, t, r, hot, now),
        }
    }

    /// A última música na ilha aberta: a capa, o título, o artista e o player (com o ponto na cor
    /// dele) e quando tocou; o botão de tocar (ou de abrir o player, fechado); a barra de onde ela
    /// parou; e embaixo as tocadas antes, cada uma numa linha que procura a música no player.
    fn draw_media_open(&self, c: &mut Canvas, t: &Track, r: Rect4, hot: Option<u32>, now: f64) {
        let (x, y, w, _) = r;
        self.draw_cover(c, t, x + 20.0, y + 18.0, MEDIA_COVER, 13.0);
        let spots = self.media_spots(w);
        let button = spots.iter().find(|(act, _)| *act == PLAY || *act == OPEN_APP).copied();
        let text_right = button.map_or(w - 22.0, |(_, b)| b.0 - 12.0);
        let tw = text_right - MEDIA_TEXT_X;
        c.text(x + MEDIA_TEXT_X, y + 20.0, &t.title, Face::Semibold, 15.0, FG, Anchor::LA, Some(tw));
        c.text(x + MEDIA_TEXT_X, y + 40.0, &t.artist, Face::Regular, 12.5, MUTED, Anchor::LA, Some(tw));
        c.fill_circle(x + MEDIA_TEXT_X + 3.5, y + 64.5, 3.5, app_color(t));
        // o player e como ela está; sem espaço, só como ela está (nunca cortado)
        let state = self.track_state(t, now);
        let full = if t.app.is_empty() { state.clone() } else { format!("{} · {state}", t.app) };
        let line = iu::fitting(c, &[&full, &state], Face::Regular, 11.5, tw - 12.0);
        c.text(x + MEDIA_TEXT_X + 12.0, y + 64.5, line, Face::Regular, 11.5, FAINT, Anchor::LM, Some(tw - 12.0));
        // tocar (o player aberto e a música parada) ou abrir o player (fechado), num círculo
        if let Some((act, b)) = button {
            let (cx, cy) = (x + (b.0 + b.2) / 2.0, y + (b.1 + b.3) / 2.0);
            c.fill_circle(cx, cy, MEDIA_BUTTON / 2.0, if hot == Some(act) { WHITE } else { FG });
            if act == PLAY {
                drawing::play(c, cx + 1.0, cy, 14.0, Rgba(18, 18, 20, 255));
            } else {
                c.text(cx, cy, crate::ui::theme::icon("abrir"), Face::Icons, 14.0, Rgba(18, 18, 20, 255), Anchor::MM, None);
            }
        }
        // onde ela parou
        if t.duration > 0.0 {
            let (x0, x1) = (x + 20.0, x + w - 20.0);
            iu::bar(c, x0, x1, y + MEDIA_BAR_Y, (t.position / t.duration) as f32, app_color(t));
            let at = clock_text(t.position);
            let left = if t.playing { at } else { format!("parou em {at}") };
            c.text(x0, y + MEDIA_TIMES_Y, &left, Face::Numbers, 10.5, MUTED, Anchor::LA, None);
            c.text(x1, y + MEDIA_TIMES_Y, &clock_text(t.duration), Face::Numbers, 10.5, MUTED, Anchor::RA, None);
        }
        // as tocadas antes
        if self.recent.is_empty() {
            return;
        }
        c.text(x + 20.0, y + RECENT_HEAD_Y, "TOCADAS ANTES", Face::Semibold, 9.5, FAINT, Anchor::LM, None);
        for (i, old) in self.recent.iter().enumerate() {
            let top = y + RECENT_FIRST_Y + RECENT_ROW * i as f32;
            let mid = top + RECENT_ROW / 2.0;
            if hot == Some(RECENT + i as u32) {
                c.fill_round_rect(x + 12.0, top, x + w - 12.0, top + RECENT_ROW, 8.0, WHITE.alpha(0.08));
            }
            self.draw_cover(c, old, x + 20.0, mid - RECENT_COVER / 2.0, RECENT_COVER, 5.0);
            let when = old.when.map(|t| self.ago_short(t, now)).unwrap_or_default();
            let ww = c.text(x + w - 20.0, mid, &when, Face::Regular, 11.0, FAINT, Anchor::RM, None);
            let (tx, room) = (x + 20.0 + RECENT_COVER + 10.0, w - 20.0 - ww - 12.0 - (20.0 + RECENT_COVER + 10.0));
            let tw = c.text(tx, mid, &old.title, Face::Regular, 12.0, FG, Anchor::LM, Some(room));
            if !old.artist.is_empty() && room - tw > 30.0 {
                c.text(tx + tw, mid, &format!(" · {}", old.artist), Face::Regular, 12.0, MUTED, Anchor::LM, Some(room - tw));
            }
        }
    }

    /// A capa da música: a guardada, se tem; senão, o degradê com as cores dela e o "play".
    fn draw_cover(&self, c: &mut Canvas, t: &Track, x0: f32, y0: f32, size: f32, rr: f32) {
        match self.cover_of(t) {
            Some(img) => {
                c.push_round_clip(x0, y0, x0 + size, y0 + size, rr);
                c.image_sharp(&img, x0, y0, size, size, 1.0);
                c.pop_clip();
            }
            None => {
                c.gradient_round_rect(x0, y0, x0 + size, y0 + size, rr, &[(0.0, t.colors[0]), (1.0, t.colors[1])], y0, y0 + size);
                drawing::play(c, x0 + size / 2.0 + size * 0.03, y0 + size / 2.0, size * 0.36, Rgba(255, 255, 255, 150));
            }
        }
    }

    /// A capa guardada da música (lida da pasta uma vez só).
    fn cover_of(&self, t: &Track) -> Option<Arc<Image>> {
        let key = t.cover.as_ref()?;
        if let Some(img) = self.covers.borrow().get(key) {
            return Some(img.clone());
        }
        let bytes = std::fs::read(self.covers_dir.as_ref()?.join(format!("{key}.png"))).ok()?;
        let img = Arc::new(crate::gfx::decode_image(&bytes)?);
        self.covers.borrow_mut().insert(key.clone(), img.clone());
        Some(img)
    }

    /// Como está a música: "tocando agora", "pausada há 3 min" ou "tocou ontem às 22:10" (sem a
    /// hora, de uma versão antiga: "a última").
    fn track_state(&self, t: &Track, now: f64) -> String {
        match (t.playing, t.live, t.when) {
            (true, _, _) => "tocando agora".into(),
            (false, true, Some(when)) => format!("pausada {}", self.ago(when, now)),
            (false, true, None) => "pausada".into(),
            (false, false, Some(when)) => format!("tocou {}", self.ago(when, now)),
            (false, false, None) => "a última".into(),
        }
    }

    /// Há quanto tempo foi `t`: "há pouco", "há 25 min", "hoje às 10:12", "ontem às 22:10" ou "em
    /// 12 nov".
    fn ago(&self, t: f64, now: f64) -> String {
        let minutes = ((now - t) / 60.0).floor().max(0.0) as i64;
        let days = self.clock.day(now) - self.clock.day(t);
        match (minutes, days) {
            (0, _) => "há pouco".into(),
            (1..=59, _) => format!("há {minutes} min"),
            (_, 0) => format!("hoje às {}", self.clock.hhmm(t)),
            (_, 1) => format!("ontem às {}", self.clock.hhmm(t)),
            _ => format!("em {}", self.day_month(t)),
        }
    }

    /// O mesmo, curto (na lista das tocadas antes): "agora", "25 min", "10:12", "ontem" ou "12 nov".
    fn ago_short(&self, t: f64, now: f64) -> String {
        let minutes = ((now - t) / 60.0).floor().max(0.0) as i64;
        let days = self.clock.day(now) - self.clock.day(t);
        match (minutes, days) {
            (0, _) => "agora".into(),
            (1..=59, _) => format!("{minutes} min"),
            (_, 0) => self.clock.hhmm(t),
            (_, 1) => "ontem".into(),
            _ => self.day_month(t),
        }
    }

    /// "12 nov"
    fn day_month(&self, t: f64) -> String {
        let date = crate::data::util::civil_from_days(self.clock.day(t));
        format!("{} {}", date.day, MONTHS_SHORT[(date.month as usize).clamp(1, 12) - 1])
    }

    /// Os botões da música aberta (na ilha de largura `w`): tocar (o player aberto e a música
    /// parada) ou abrir o player (fechado), e cada uma das tocadas antes que dá para procurar no
    /// player.
    fn media_spots(&self, w: f32) -> Vec<(u32, Rect4)> {
        let mut out = Vec::new();
        let Some(t) = &self.track else { return out };
        let (cy, half) = (18.0 + MEDIA_COVER / 2.0, MEDIA_BUTTON / 2.0);
        let round = (w - 22.0 - MEDIA_BUTTON, cy - half, w - 22.0, cy + half);
        if t.live && !t.playing {
            out.push((PLAY, round));
        } else if !t.live && player_url(&t.app).is_some() {
            out.push((OPEN_APP, round));
        }
        for (i, old) in self.recent.iter().enumerate() {
            if search_url(old).is_some() {
                let top = RECENT_FIRST_Y + RECENT_ROW * i as f32;
                out.push((RECENT + i as u32, (12.0, top, w - 12.0, top + RECENT_ROW)));
            }
        }
        out
    }
}

/// A cor do ponto do player: o verde do Spotify, o vermelho do YouTube (como no título do
/// widget); de outro player, a cor da capa.
fn app_color(t: &Track) -> Rgba {
    match t.app.to_lowercase().as_str() {
        "spotify" => crate::widgets::media::source_color("spotify"),
        "youtube" => crate::widgets::media::source_color("youtube"),
        _ => t.colors[0],
    }
}

/// O endereço que abre o player (None: um player que o app não sabe abrir).
fn player_url(app: &str) -> Option<&'static str> {
    match app.to_lowercase().as_str() {
        "spotify" => Some("spotify:"),
        "youtube" => Some("https://music.youtube.com"),
        _ => None,
    }
}

/// O endereço que procura a música no player dela (o Windows só deixa controlar a que está
/// tocando: as antigas, dá para procurar).
fn search_url(t: &Track) -> Option<String> {
    let query = crate::data::util::urlencode(format!("{} {}", t.title, t.artist).trim());
    match t.app.to_lowercase().as_str() {
        "spotify" => Some(format!("spotify:search:{query}")),
        "youtube" => Some(format!("https://music.youtube.com/search?q={query}")),
        _ => None,
    }
}

/// "3:33" (ou "1:02:07", passando de uma hora)
fn clock_text(secs: f64) -> String {
    let s = secs.max(0.0) as i64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// O botão da forma média (redondo, à direita) e da aberta (embaixo, à direita).
fn button_rect(form: IslandForm, w: f32, h: f32) -> Rect4 {
    if form == IslandForm::Medium {
        (w - 62.0, h / 2.0 - 18.0, w - 26.0, h / 2.0 + 18.0)
    } else {
        (w - 22.0 - 150.0, h - 46.0, w - 22.0, h - 14.0)
    }
}

/// Onde ficam, na ilha aberta de largura `w`, o nome do mês (que volta ao mês de hoje) e as
/// setas do calendário.
struct CalendarRects {
    title: Rect4,
    prev: Rect4,
    next: Rect4,
}

fn calendar_rects(w: f32) -> CalendarRects {
    let (top, bottom) = (CAL_HEAD_Y - CAL_ARROW / 2.0, CAL_HEAD_Y + CAL_ARROW / 2.0);
    let next = (w - CAL_RIGHT - CAL_ARROW, top, w - CAL_RIGHT, bottom);
    let prev = (next.0 - 4.0 - CAL_ARROW, top, next.0 - 4.0, bottom);
    CalendarRects { title: (CAL_X0, top, prev.0 - 4.0, bottom), prev, next }
}

/// O ícone de cada acontecimento (das fontes de ícones do Windows, ver theme::icon).
fn next_glyph(name: &str) -> &'static str {
    match name {
        "Fim do expediente" => "pomodoro",
        "A ISS passa" => "mover",
        "Lua cheia" => "clima",
        _ => "clima",
    }
}

/// O tempo guardado pelo Tempo, se é de agora (das últimas 3 h).
fn weather_of(cache: &Value, clock: &Clock, now: f64) -> Option<Weather> {
    let w = &cache["weather"];
    let temp = w.i64_of("temp")?;
    // quando foi buscado: "2026-11-20T14:30" (hora local)
    let stamp = w["now"].str_of("time").unwrap_or("");
    let fetched = (|| {
        let (y, m, d) = (stamp.get(0..4)?.parse().ok()?, stamp.get(5..7)?.parse().ok()?, stamp.get(8..10)?.parse().ok()?);
        let (hh, mm) = (stamp.get(11..13)?.parse::<f64>().ok()?, stamp.get(14..16)?.parse::<f64>().ok()?);
        Some((crate::data::util::days_from_civil(y, m, d), crate::data::util::days_from_civil(y, m, d) as f64 * 86400.0 + hh * 3600.0 + mm * 60.0 - clock.offset()))
    })();
    let (fetch_day, fetched_at) = fetched?;
    if now - fetched_at > WEATHER_FRESH {
        return None;
    }
    // as horas depois desta, a partir do dia em que foi buscado
    let since = clock.day(now) - fetch_day;
    let hour_now = ((now + clock.offset()).rem_euclid(86400.0) / 3600.0).floor() as i64;
    let mark = since * 24 + hour_now;
    let hours = w["hours"]
        .as_array()
        .map(|l| {
            l.iter()
                .filter(|h| h[2].is_i64())
                .filter_map(|h| Some((h[5].as_i64()?, h[6].as_i64()?, h[1].as_str()?.to_string(), h[2].as_i64()?)))
                .filter(|(d, hr, ..)| d * 24 + hr > mark)
                .map(|(_, hr, icon, t)| (hr, icon, t))
                .take(6)
                .collect()
        })
        .unwrap_or_default();
    Some(Weather {
        temp,
        icon: w.str_of("icon").unwrap_or("sun").to_string(),
        desc: w.str_of("desc").unwrap_or("").to_string(),
        // guardado por uma versão antiga do Tempo, sem as curtas: as inteiras
        desc_short: w.str_of("desc_short").or(w.str_of("desc")).unwrap_or("").to_string(),
        max: w.i64_of("max"),
        min: w.i64_of("min"),
        line: w.str_of("rain_line").map(String::from),
        line_short: w.str_of("rain_line_short").or(w.str_of("rain_line")).map(String::from),
        hours,
    })
}

/// As cotações guardadas (pela fonte da cotação), se recentes.
fn quotes_of(cache: &Value, now: f64) -> Option<Quotes> {
    let c = &cache["cotacao"];
    let at = c.f64_of("em")?;
    if now - at > QUOTES_FRESH {
        return None;
    }
    let one = |code: &str| -> Option<(f64, f64, Vec<f64>)> {
        let a = &c[code];
        let samples = a["dia"].as_array().map(|l| l.iter().filter_map(|p| p[1].as_f64()).collect()).unwrap_or_default();
        Some((a.f64_of("preco").filter(|p| *p > 0.0)?, a.f64_of("variacao").unwrap_or(0.0), samples))
    };
    Some(Quotes { at, list: [one("USD")?, one("BTC")?] })
}

/// Uma música guardada pelo App (a última, em "midia_ultima", ou uma das tocadas antes, em
/// "midia_recentes").
fn track_of(m: &Value) -> Option<Track> {
    let title = m.str_of("titulo").filter(|t| !t.is_empty())?.to_string();
    let color = |i: usize| {
        let c = &m["cores"][i];
        match (c[0].as_u64(), c[1].as_u64(), c[2].as_u64()) {
            (Some(r), Some(g), Some(b)) => Rgba::rgb(r as u8, g as u8, b as u8),
            _ => [Rgba::rgb(244, 184, 96), Rgba::rgb(91, 58, 140)][i],
        }
    };
    Some(Track {
        title,
        artist: m.str_of("artista").unwrap_or("").to_string(),
        app: m.str_of("app").unwrap_or("").to_string(),
        playing: m.bool_of("tocando").unwrap_or(false),
        live: m.bool_of("ativo").unwrap_or(false),
        colors: [color(0), color(1)],
        when: m.f64_of("quando"),
        position: m.f64_of("posicao").unwrap_or(0.0),
        duration: m.f64_of("duracao").unwrap_or(0.0),
        cover: m.str_of("capa").map(str::to_string),
    })
}

/// O que `note_track` mudou: se vale gravar o cache no disco agora, e a capa nova a guardar na
/// pasta das capas (o nome e a imagem).
pub struct Noted {
    pub save: bool,
    pub cover: Option<(String, Arc<Image>)>,
}

/// O nome da capa de uma música (o mesmo para o mesmo título e artista).
fn cover_key(title: &str, artist: &str) -> String {
    format!("{:016x}", alert_hash(&format!("{title}\u{1}{artist}")))
}

/// Guarda a música do Tocando agora no cache, em `now` (o App chama a cada mudança dele): a
/// posição e a hora vão sendo atualizadas; uma música nova empurra a anterior para as tocadas
/// antes (até `RECENT_MAX`, sem repetir); o player fechando deixa a última parada onde estava.
pub fn note_track(cache: &mut Value, info: Option<&crate::widgets::IslandInfo>, now: f64) -> Noted {
    let last = cache["midia_ultima"].clone();
    let Some(i) = info else {
        // o player fechou: a última fica, parada
        let was_live = last.bool_of("ativo") == Some(true);
        if was_live {
            cache["midia_ultima"]["ativo"] = json!(false);
            cache["midia_ultima"]["tocando"] = json!(false);
        }
        return Noted { save: was_live, cover: None };
    };
    let same = last.str_of("titulo") == Some(i.title.as_str()) && last.str_of("artista") == Some(i.artist.as_str());
    let key = cover_key(&i.title, &i.artist);
    // a capa costuma chegar depois do título: guarda quando ela aparece
    let cover = i.art.as_ref().filter(|_| !same || last.str_of("capa").is_none()).map(|art| (key.clone(), art.clone()));
    if !same {
        // a anterior vai para as tocadas antes; a nova sai de lá, se estava
        let mut list: Vec<Value> = cache["midia_recentes"].as_array().cloned().unwrap_or_default();
        list.retain(|m| !(m.str_of("titulo") == Some(i.title.as_str()) && m.str_of("artista") == Some(i.artist.as_str())));
        if last.str_of("titulo").is_some_and(|t| !t.is_empty()) {
            let mut old = last.clone();
            old["ativo"] = json!(false);
            old["tocando"] = json!(false);
            list.retain(|m| !(m.str_of("titulo") == old.str_of("titulo") && m.str_of("artista") == old.str_of("artista")));
            list.insert(0, old);
        }
        list.truncate(RECENT_MAX);
        cache["midia_recentes"] = json!(list);
        let [a, b] = i.colors;
        cache["midia_ultima"] = json!({"titulo": i.title, "artista": i.artist, "app": i.app, "tocando": i.playing, "ativo": true,
            "cores": [[a.0, a.1, a.2], [b.0, b.1, b.2]], "quando": now, "posicao": i.position, "duracao": i.duration});
        if cover.is_some() {
            cache["midia_ultima"]["capa"] = json!(key);
        }
        return Noted { save: true, cover };
    }
    // a mesma: a posição, a hora (tocando) e o estado
    let changed = last.bool_of("tocando") != Some(i.playing) || last.bool_of("ativo") != Some(true);
    let m = &mut cache["midia_ultima"];
    m["tocando"] = json!(i.playing);
    m["ativo"] = json!(true);
    m["posicao"] = json!(i.position);
    m["duracao"] = json!(i.duration);
    // (tocando, ou acabou de pausar: a hora em que parou de tocar)
    if i.playing || last.bool_of("tocando") == Some(true) || m.get("quando").is_none() {
        m["quando"] = json!(now);
    }
    if cover.is_some() {
        m["capa"] = json!(key);
    }
    Noted { save: changed || cover.is_some(), cover }
}

/// A pasta das capas guardadas (na pasta de dados do app).
pub fn covers_dir() -> PathBuf {
    crate::data::config::data_dir().join(COVERS_DIR)
}

/// Guarda a capa `art` (pequena, quadrada) como `key`.png em `dir`.
pub fn save_cover(dir: &Path, key: &str, art: &Image) {
    let _ = std::fs::create_dir_all(dir);
    let _ = crate::gfx::save_png(&crate::gfx::fit_square(art, COVER_PX), &dir.join(format!("{key}.png")));
}

/// Apaga de `dir` as capas que nenhuma música guardada no cache usa mais.
pub fn prune_covers(dir: &Path, cache: &Value) {
    let mut used: Vec<String> = cache["midia_recentes"].as_array().map(|l| l.iter().filter_map(|m| m.str_of("capa").map(str::to_string)).collect()).unwrap_or_default();
    used.extend(cache["midia_ultima"].str_of("capa").map(str::to_string));
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if path.extension().is_some_and(|e| e == "png") && !used.contains(&stem) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Um cache com o tempo, as cotações, a última música e uma passagem da ISS, para os testes.
#[cfg(test)]
fn sample_cache(now: f64) -> Value {
    let hours: Vec<Value> = (14..=22).map(|h| json!([format!("{h}h"), if h >= 17 { "rain" } else { "partly_day" }, 27 - (h - 14), 61, 70, 0, h])).collect();
    let samples = |base: f64, p: f64| -> Vec<Value> { (0..=40).map(|k| json!([now - (40 - k) as f64 * 600.0, base * (1.0 + p / 100.0 * k as f64 / 40.0 + 0.002 * ((k as f64) * 0.7).sin())])).collect() };
    json!({
        "weather": {"temp": 24, "icon": "partly_day", "desc": "Parcialmente nublado", "max": 27, "min": 18, "rain_line": "Chuva a partir das 17h",
            "hours": hours, "now": {"time": "2026-11-20T14:15"}},
        "cotacao": {"em": now - 120.0, "USD": {"preco": 5.61, "variacao": 0.3, "dia": samples(5.59, 0.3)}, "BTC": {"preco": 369262.0, "variacao": -1.2, "dia": samples(373000.0, -1.2)}},
        "midia_ultima": {"titulo": "Clube da Esquina nº 2", "artista": "Milton Nascimento", "app": "Spotify", "tocando": false, "ativo": true, "cores": [[244, 184, 96], [91, 58, 140]],
            "quando": now - 25.0 * 60.0, "posicao": 74.0, "duracao": 213.0},
        "midia_recentes": [
            {"titulo": "Cais", "artista": "Milton Nascimento", "app": "Spotify", "cores": [[96, 160, 220], [30, 50, 90]], "quando": now - 40.0 * 60.0},
            {"titulo": "Trem Azul", "artista": "Lô Borges", "app": "Spotify", "cores": [[90, 200, 160], [20, 60, 70]], "quando": now - 26.0 * 3600.0},
        ],
        "iss_visiveis": [[now + 2.0 * 86400.0, 300.0, 40.0]],
    })
}

impl Kind for Rest {
    fn key(&self) -> &'static str {
        KEY
    }
    fn title(&self) -> &'static str {
        "Ilha em repouso"
    }
    fn draw(&self, _c: &mut Canvas, _mode: Mode, _env: &Env) {}

    fn next_tick(&self) -> Option<Instant> {
        Some(self.check_at)
    }

    fn input(&mut self, field: &str, value: &str, _extra: Option<&Value>, _ctx: &mut Ctx) {
        if field == FOCUS_FIELD {
            self.focus_mode = match value {
                "off" => None,
                "on" => Some(String::new()),
                t => Some(t.to_string()),
            };
        }
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        self.check_at = Instant::now() + Duration::from_secs(1);
        let now = self.clock.now();
        let cfg = &*ctx.cfg;
        let cache = &*ctx.cache;
        self.idle = self.fake_idle.unwrap_or_else(crate::platform::win::idle_secs) >= IDLE_AFTER;
        self.city = location(cfg).map(|place| (place.lat, place.lon));
        let day = self.clock.day(now);
        // o sol do dia (uma vez por dia e cidade)
        match self.city {
            Some((lat, lon)) if !self.sun.is_some_and(|s| s.day == day && s.lat == lat && s.lon == lon) => {
                let midnight = self.clock.midnight(day);
                let alt = |t: f64| astro::sun_alt_az(t, lat, lon).0 - astro::SUN_SET;
                let rise = astro::crossing(alt, midnight, midnight + 86400.0, 600.0, true);
                let set = astro::crossing(alt, midnight, midnight + 86400.0, 600.0, false);
                let rise_next = astro::crossing(alt, midnight + 86400.0, midnight + 2.0 * 86400.0, 600.0, true);
                self.sun = Some(Sun { day, lat, lon, rise, set, rise_next });
            }
            None => self.sun = None,
            _ => {}
        }
        // a próxima lua cheia (uma vez por dia)
        if self.full_moon.is_none_or(|f| f.searched_on != day) {
            let next = astro::full_moons(now, now + 31.0 * 86400.0).first().copied();
            self.full_moon = Some(FullMoonSearch { searched_on: day, next });
        }
        self.weather = weather_of(cache, &self.clock, now);
        self.quotes = quotes_of(cache, now);
        self.track = track_of(&cache["midia_ultima"]);
        self.recent = cache["midia_recentes"].as_array().map(|list| list.iter().filter_map(track_of).take(RECENT_MAX).collect()).unwrap_or_default();
        // os focos de hoje (o Pomodoro zera quando muda o dia)
        let date = crate::data::util::civil_from_days(day);
        let today = format!("{:04}-{:02}-{:02}", date.year, date.month, date.day);
        let stats = &cfg["pomodoro"]["hoje"];
        let goal = cfg["pomodoro"].i64_of("longa_a_cada").unwrap_or(4);
        self.focus_today = if stats.str_of("data") == Some(today.as_str()) {
            FocusToday { done: stats.i64_of("pomodoros").unwrap_or(0), minutes: stats.i64_of("minutos").unwrap_or(0), goal }
        } else {
            FocusToday { done: 0, minutes: 0, goal }
        };
        self.workday_end = (self.clock.weekday(now) < 5 && on_island(cfg, "expediente")).then(|| self.clock.midnight(day) + workday::end_of(cfg));
        self.iss = cache["iss_visiveis"].as_array().and_then(|l| l.iter().filter_map(|p| Some((p[0].as_f64()?, p[1].as_f64().unwrap_or(0.0)))).find(|p| p.0 > now));
        let mut looks: Vec<Look> = chosen(cfg).into_iter().filter(|l| self.available(*l)).collect();
        if looks.is_empty() {
            looks.push(Look::Clock);
        }
        self.looks = looks;
        self.dot_only = dot_on(cfg);
        let version = alert_hash(&format!("{:?}", (&self.weather, &self.quotes, &self.track, &self.recent, self.focus_today, self.workday_end, self.iss, &self.looks, self.idle, self.dot_only, self.full_moon, &self.focus_mode)));
        let changed = version != self.data_version;
        self.data_version = version;
        changed
    }

    fn island_item(&self, env: &Env) -> Option<IslandItem> {
        // no modo foco, sempre (só a hora e o dia), mesmo com a ilha em repouso desligada
        if self.focus_mode.is_some() {
            return Some(IslandItem { idle: true, dim: self.idle, ..IslandItem::live(FOCUS) });
        }
        if !on_island(env.cfg, KEY) {
            return None;
        }
        let item = IslandItem::live(self.period_color(self.clock.now()));
        Some(IslandItem { idle: true, dot: self.dot_only || self.idle, dim: self.idle, ..item })
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, _env: &Env) {
        let now = self.clock.now();
        if self.focus_mode.is_some() {
            self.draw_focus_mode(c, form, r, hot, now);
            return;
        }
        match form {
            IslandForm::Bubble => self.draw_dot(c, r, now),
            IslandForm::Large => {
                // aberta, para de alternar
                let look = self.look_for(form, now);
                self.held_while_open.set(Some(look));
                self.draw_look(c, look, form, r, hot, now);
            }
            _ => {
                self.held_while_open.set(None);
                // fechada, o calendário volta ao mês de hoje
                self.month_shift.set(0);
                let (x, y, w, h) = r;
                let showing = self.showing(now);
                match showing.leaving {
                    Some(leaving) => {
                        // o de saída sobe e apaga; o novo vem de baixo e acende (para trás, ao
                        // contrário)
                        let eased = ease_in_out(leaving.progress);
                        let lift = if leaving.back { -10.0 } else { 10.0 };
                        c.push_opacity(1.0 - eased);
                        self.draw_look(c, leaving.look, form, (x, y - lift * eased, w, h), None, now);
                        c.pop_clip();
                        c.push_opacity(eased);
                        self.draw_look(c, showing.look, form, (x, y + lift * (1.0 - eased), w, h), hot, now);
                        c.pop_clip();
                    }
                    None => self.draw_look(c, showing.look, form, r, hot, now),
                }
            }
        }
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        if self.focus_mode.is_some() {
            return (form == IslandForm::Large && iu::inside(button_rect(form, w, h), x, y)).then_some(END_FOCUS);
        }
        if !matches!(form, IslandForm::Medium | IslandForm::Large) {
            return None;
        }
        let look = self.look_for(form, self.clock.now());
        if form == IslandForm::Large && look == Look::Clock {
            let rects = calendar_rects(w);
            // o nome do mês só volta ao de hoje quando está em outro
            let spots = [(PREV_MONTH, rects.prev), (NEXT_MONTH, rects.next), (THIS_MONTH, rects.title)];
            return spots.into_iter().find(|(act, rect)| iu::inside(*rect, x, y) && (*act != THIS_MONTH || self.month_shift.get() != 0)).map(|(act, _)| act);
        }
        if form == IslandForm::Large && look == Look::Media {
            return self.media_spots(w).into_iter().find(|(_, rect)| iu::inside(*rect, x, y)).map(|(act, _)| act);
        }
        let (act, _) = self.button(look)?;
        iu::inside(button_rect(form, w, h), x, y).then_some(act)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            START => ctx.out.push(Request::Act("pomodoro", pomodoro::START_IF_IDLE)),
            PLAY => ctx.out.push(Request::Act("midia", island_act::TOGGLE)),
            END_FOCUS => ctx.out.push(Request::Focus(false)),
            PREV_MONTH => self.month_shift.set(self.month_shift.get() - 1),
            NEXT_MONTH => self.month_shift.set(self.month_shift.get() + 1),
            THIS_MONTH => self.month_shift.set(0),
            OPEN_APP => {
                if let Some(url) = self.track.as_ref().and_then(|t| player_url(&t.app)) {
                    ctx.open(url);
                }
            }
            n if n >= RECENT => {
                if let Some(url) = self.recent.get((n - RECENT) as usize).and_then(search_url) {
                    ctx.open(url);
                }
            }
            _ => {}
        }
    }

    /// A roda do mouse na pílula: o próximo escolhido (passo negativo, rolando para baixo) ou o
    /// anterior, já, com a mesma animação da troca; a alternância segue dali. Aberta, troca o
    /// que ela está segurando.
    fn wheel(&mut self, _x: f32, _y: f32, steps: f32, _mode: Mode, _ctx: &mut Ctx) {
        let count = self.looks.len() as i64;
        if self.focus_mode.is_some() || count <= 1 || steps == 0.0 {
            return;
        }
        let now = self.clock.now();
        let forward = steps < 0.0;
        let current = self.look_for(IslandForm::Large, now);
        let at = self.looks.iter().position(|l| *l == current).unwrap_or(0) as i64;
        let want = (at + if forward { 1 } else { -1 }).rem_euclid(count);
        // o turno mais perto, no sentido da roda, em que é a vez de `want`
        let turn = self.turn(now);
        let ahead = if forward { (want - turn).rem_euclid(count) } else { -(turn - want).rem_euclid(count) };
        let target = turn + ahead;
        // o relógio passa a cair bem no começo desse turno (a troca anima dali)
        self.shift = target as f64 * ROTATE_EVERY - now;
        self.wheeled = Some(Wheeled { turn: target, from: current, back: !forward });
        if self.held_while_open.get().is_some() {
            self.held_while_open.set(Some(self.looks[want as usize]));
        }
    }

    fn uses_wheel(&self, _x: f32, _y: f32, _mode: Mode) -> bool {
        self.focus_mode.is_none() && self.looks.len() > 1
    }

    fn island_fps(&self, form: IslandForm) -> u32 {
        let now = self.clock.now();
        if self.focus_mode.is_some() {
            0
        } else if form == IslandForm::Bubble && !self.idle {
            10
        } else if form != IslandForm::Large && self.near_a_turn(now) {
            40
        } else {
            0
        }
    }

    fn island_stamp(&self, _env: &Env) -> Option<u64> {
        // muda com o minuto (o relógio), o turno (a vez de outro escolhido) e os dados: parada, a
        // ilha só redesenha quando um deles muda
        let now = self.clock.now();
        let minute = (now / 60.0).floor() as i64;
        let rotating = self.looks.len() > 1 && !self.idle && self.focus_mode.is_none();
        let turn = if rotating { self.turn(now) } else { 0 };
        Some(alert_hash(&format!("{}|{minute}|{turn}|{}|{}", self.data_version, self.shift.to_bits(), self.month_shift.get())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(cfg: &Value) -> Env<'_> {
        Env { cfg, key: KEY }
    }

    /// Os meses do calendário: onde cai o dia 1 (com a semana começando no domingo) e quantos
    /// dias tem, inclusive passando de um ano para o outro com as setas.
    #[test]
    fn calendar_months_start_on_the_right_weekday() {
        let day = crate::data::util::days_from_civil(2026, 11, 20);
        let nov = MonthView::around(day, 0);
        assert_eq!((nov.year, nov.month, nov.days, nov.lead), (2026, 11, 30, 0), "1º/11/2026 é um domingo");
        let jan = MonthView::around(day, 2);
        assert_eq!((jan.year, jan.month, jan.days, jan.lead), (2027, 1, 31, 5), "1º/1/2027 é uma sexta");
        let dec = MonthView::around(day, -11);
        assert_eq!((dec.year, dec.month, dec.days, dec.lead), (2025, 12, 31, 1), "1º/12/2025 é uma segunda");
        let feb = MonthView::around(day, 15);
        assert_eq!((feb.year, feb.month, feb.days), (2028, 2, 29), "2028 é bissexto");
        // nenhum mês passa de 6 semanas (o que cabe na ilha aberta)
        for shift in -40..40 {
            let m = MonthView::around(day, shift);
            assert!(m.lead + m.days <= 42, "{m:?}");
        }
        assert!(CAL_FIRST_ROW_Y + 5.0 * CAL_ROW + CAL_TODAY_R <= iu::size(IslandForm::Large).h - 12.0, "a sexta semana passa do pé da ilha");
    }

    /// Quanto do ano já passou: zero na virada, quase tudo no último dia, e os dias que faltam.
    #[test]
    fn year_so_far_counts_the_local_year() {
        let mut r = Rest::sample(Some(Look::Clock), false);
        let offset = r.clock.offset();
        let year = r.year_so_far(r.clock.now());
        // sexta, 20/11/2026, 14:32: o dia 324 de 365
        assert_eq!((year.year, year.days_left), (2026, 41));
        assert_eq!((year.done * 100.0).floor(), 88.0, "{}", year.done);
        let new_year = crate::data::util::days_from_civil(2026, 1, 1) as f64 * 86400.0 - offset;
        r.at(new_year);
        let start = r.year_so_far(r.clock.now());
        assert_eq!((start.year, start.done, start.days_left), (2026, 0.0, 364));
        r.at(new_year - 60.0);
        let last = r.year_so_far(r.clock.now());
        assert_eq!((last.year, last.days_left), (2025, 0), "31/12 às 23:59");
        assert!(last.done > 0.999);
    }

    /// As setas do calendário mudam o mês, o nome do mês volta ao de hoje, e fechar a ilha também.
    #[test]
    fn calendar_arrows_move_the_month() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let mut r = Rest::sample(Some(Look::Clock), false);
        let mut cfg = json!({"app": {}});
        let (w, h) = (iu::size(IslandForm::Large).w, iu::size(IslandForm::Large).h);
        let rects = calendar_rects(w);
        let mid = |rect: Rect4| ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0);
        let hit = |r: &Rest, at: (f32, f32), cfg: &Value| r.island_hit(IslandForm::Large, at.0, at.1, w, h, &env(cfg));
        assert_eq!(hit(&r, mid(rects.prev), &cfg), Some(PREV_MONTH));
        assert_eq!(hit(&r, mid(rects.next), &cfg), Some(NEXT_MONTH));
        assert_eq!(hit(&r, mid(rects.title), &cfg), None, "no mês de hoje, o nome não é botão");
        let stamp = r.island_stamp(&env(&cfg));
        let (mut cache, mut out) = (json!({}), Vec::new());
        for action in [NEXT_MONTH, NEXT_MONTH, PREV_MONTH] {
            r.act(action, &mut Ctx { cfg: &mut cfg, cache: &mut cache, key: KEY, out: &mut out });
        }
        assert_eq!(r.month_shift.get(), 1);
        assert_ne!(r.island_stamp(&env(&cfg)), stamp, "a ilha redesenha com o mês novo");
        assert_eq!(hit(&r, mid(rects.title), &cfg), Some(THIS_MONTH));
        r.act(THIS_MONTH, &mut Ctx { cfg: &mut cfg, cache: &mut cache, key: KEY, out: &mut out });
        assert_eq!(r.month_shift.get(), 0);
        // aberta num mês qualquer, fechar volta ao de hoje
        r.month_shift.set(-3);
        let mut c = Canvas::new(400, 240, 1.0);
        r.island_draw(&mut c, IslandForm::Large, (0.0, 0.0, w, h), None, &env(&cfg));
        assert_eq!(r.month_shift.get(), -3);
        r.island_draw(&mut c, IslandForm::Compact, (0.0, 0.0, 236.0, 42.0), None, &env(&cfg));
        assert_eq!(r.month_shift.get(), 0);
        assert!(out.is_empty(), "as setas não pedem nada ao app");
    }

    #[test]
    fn choices_toggle_and_default() {
        let mut cfg = json!({"app": {}});
        assert_eq!(chosen(&cfg), DEFAULT.to_vec());
        assert!(toggle(&mut cfg, "repouso:tempo"));
        assert_eq!(chosen(&cfg), vec![Look::Clock, Look::Next, Look::Quotes]);
        assert!(toggle(&mut cfg, "repouso:foco"));
        assert_eq!(chosen(&cfg), vec![Look::Clock, Look::Next, Look::Quotes, Look::Focus], "na ordem de sempre");
        assert!(!dot_on(&cfg));
        assert!(toggle(&mut cfg, DOT_ID));
        assert!(dot_on(&cfg));
        assert!(!toggle(&mut cfg, "na_ilha:sol"), "não é da ilha em repouso");
        assert!(!toggle(&mut cfg, "repouso:nada"));
        for l in Look::ALL {
            assert_eq!(l.setting_id(), format!("repouso:{}", l.code()));
        }
    }

    #[test]
    fn rotates_every_7_seconds_and_holds_when_open() {
        // desenha (a ilha aberta): o Direct2D precisa do COM nesta thread
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let mut r = Rest::sample(Some(Look::Clock), false);
        r.looks = vec![Look::Clock, Look::Weather, Look::Quotes];
        let base = (r.clock.now() / ROTATE_EVERY).floor() * ROTATE_EVERY;
        r.at(base + 3.0);
        let first = r.showing(r.clock.now());
        let (a, turn) = (first.look, first.leaving);
        assert!(turn.is_none());
        r.at(base + ROTATE_EVERY + 0.1);
        let second = r.showing(r.clock.now());
        let (b, turn) = (second.look, second.leaving);
        assert_ne!(a, b, "trocou depois de 7 s");
        assert_eq!(turn.map(|t| t.look), Some(a), "a anterior sai subindo");
        assert!(r.island_fps(IslandForm::Compact) > 0, "a troca anima");
        r.at(base + 3.0);
        assert_eq!(r.island_fps(IslandForm::Compact), 0, "parada, só quando muda");
        // aberta: fica no que estava
        let cfg = json!({"app": {}});
        let mut c = Canvas::new(400, 240, 1.0);
        r.island_draw(&mut c, IslandForm::Large, (0.0, 0.0, 364.0, 220.0), None, &env(&cfg));
        let held = r.held_while_open.get();
        r.at(base + 2.0 * ROTATE_EVERY + 1.0);
        assert_eq!(r.look_for(IslandForm::Large, r.clock.now()), held.unwrap());
        // parado, não alterna
        r.idle = true;
        assert!(r.showing(r.clock.now()).leaving.is_none());
        assert_eq!(r.showing(r.clock.now()).look, Look::Clock);
    }

    /// Na ilha, o tempo nunca fica cortado com "…": em cada forma cabe ao menos a versão curta
    /// de cada condição e de cada aviso das próximas horas, com a temperatura mais larga.
    #[test]
    fn island_weather_texts_are_never_cut() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let c = Canvas::new(1, 1, 1.0);
        let (descs, lines) = crate::widgets::tempo::all_island_texts();
        let width = |text: &str, face: Face, size: f32| c.text_width(text, face, size);
        // aberta: a descrição e a linha dividem a mesma altura
        let large = iu::size(IslandForm::Large);
        let room = large.w - 82.0 - 22.0;
        for (desc, desc_short) in &descs {
            for (line, line_short) in lines.iter().map(|(l, s)| (l.as_str(), s.as_str())).chain([("", "")]) {
                let (d, l) = weather_row(&c, [desc, desc_short], [line, line_short], room);
                let gap = if l.is_empty() { 0.0 } else { ROW_GAP };
                assert!(width(d, Face::Regular, 12.5) + gap + width(l, Face::Regular, 12.0) <= room, "{d:?} + {l:?}");
            }
        }
        // fechada: o aviso (ou a condição) ao lado da temperatura
        let compact = iu::size(IslandForm::Compact);
        let room = iu::compact_room(&c, 46.0, (0.0, 0.0, compact.w, compact.h), "-18°");
        for (long, short) in lines.iter().chain(descs.iter()) {
            let label = iu::fitting(&c, &[long, short], Face::Regular, 12.0, room);
            assert!(width(label, Face::Regular, 12.0) <= room, "{label:?}");
        }
        // média: "-18° · condição" em cima, o aviso embaixo
        let medium = iu::size(IslandForm::Medium);
        let room = medium.w - 74.0 - 22.0;
        for (_, short) in &descs {
            assert!(width(&format!("-18° · {short}"), Face::Semibold, 13.5) <= room, "{short:?}");
        }
        for (_, short) in &lines {
            assert!(width(short, Face::Regular, 11.5) <= room, "{short:?}");
        }
    }

    #[test]
    fn weather_texts_share_the_row_without_overlapping() {
        // cabem: cada um com o que precisa
        assert_eq!(share_row(100.0, 120.0, 260.0), (100.0, 120.0));
        // o curto fica inteiro e o comprido fica com o resto
        assert_eq!(share_row(60.0, 300.0, 260.0), (60.0, 200.0));
        assert_eq!(share_row(300.0, 60.0, 260.0), (200.0, 60.0));
        // os dois compridos: metade para cada
        assert_eq!(share_row(200.0, 200.0, 260.0), (130.0, 130.0));
        for (left, right) in [(10.0, 500.0), (150.0, 150.0), (129.0, 400.0), (400.0, 131.0)] {
            let (a, b) = share_row(left, right, 260.0);
            assert!(a + b <= 260.0 && a <= left && b <= right, "{left} {right}: {a} {b}");
        }
    }

    #[test]
    fn wheel_switches_now_both_ways() {
        let mut r = Rest::sample(Some(Look::Clock), false);
        r.looks = vec![Look::Clock, Look::Weather, Look::Quotes];
        let base = (r.clock.now() / ROTATE_EVERY).floor() * ROTATE_EVERY;
        r.at(base + 3.0);
        let first = r.showing(r.clock.now()).look;
        let at = r.looks.iter().position(|l| *l == first).unwrap();
        let (mut cfg, mut cache, mut out) = (json!({}), json!({}), Vec::new());
        let mut ctx = Ctx { cfg: &mut cfg, cache: &mut cache, key: KEY, out: &mut out };
        // para baixo: o próximo, já, com o de antes saindo para cima
        r.wheel(0.0, 0.0, -1.0, Mode::M, &mut ctx);
        let next = r.showing(r.clock.now());
        assert_eq!(next.look, r.looks[(at + 1) % 3]);
        let leaving = next.leaving.expect("a troca anima");
        assert_eq!((leaving.look, leaving.back), (first, false));
        assert!(r.island_fps(IslandForm::Compact) > 0);
        // a alternância segue dali: o mesmo por mais 7 s, depois o seguinte
        r.at(base + 3.0 + ROTATE_EVERY - 0.5);
        assert_eq!(r.showing(r.clock.now()).look, r.looks[(at + 1) % 3]);
        r.at(base + 3.0 + ROTATE_EVERY + 0.1);
        assert_eq!(r.showing(r.clock.now()).look, r.looks[(at + 2) % 3]);
        // para cima: o anterior, com o de agora descendo
        r.wheel(0.0, 0.0, 1.0, Mode::M, &mut ctx);
        let back = r.showing(r.clock.now());
        assert_eq!(back.look, r.looks[(at + 1) % 3]);
        let leaving = back.leaving.expect("a troca anima");
        assert_eq!((leaving.look, leaving.back), (r.looks[(at + 2) % 3], true));
        // aberta, troca o que ela segura
        r.held_while_open.set(Some(Look::Clock));
        r.wheel(0.0, 0.0, -1.0, Mode::M, &mut ctx);
        assert_eq!(r.look_for(IslandForm::Large, r.clock.now()), Look::Weather);
        // no modo foco, ou com um só, a roda não faz nada
        let shift = r.shift;
        r.focus_mode = Some(String::new());
        r.wheel(0.0, 0.0, -1.0, Mode::M, &mut ctx);
        assert_eq!(r.shift, shift);
        r.focus_mode = None;
        r.looks = vec![Look::Clock];
        r.wheel(0.0, 0.0, -1.0, Mode::M, &mut ctx);
        assert_eq!(r.shift, shift);
        assert!(!r.uses_wheel(0.0, 0.0, Mode::M));
    }

    #[test]
    fn stamp_changes_only_with_the_minute_or_the_turn() {
        let mut r = Rest::sample(Some(Look::Clock), false);
        let cfg = json!({"app": {}});
        let now = r.clock.now();
        let minute = (now / 60.0).floor() * 60.0;
        r.at(minute + 5.0);
        let a = r.island_stamp(&env(&cfg));
        r.at(minute + 50.0);
        assert_eq!(r.island_stamp(&env(&cfg)), a, "no mesmo minuto, nada a redesenhar");
        r.at(minute + 65.0);
        assert_ne!(r.island_stamp(&env(&cfg)), a);
    }

    #[test]
    fn idle_pc_turns_it_into_a_dim_dot() {
        let cfg = json!({"app": {}});
        let r = Rest::sample(Some(Look::Clock), false);
        let item = r.island_item(&env(&cfg)).unwrap();
        assert!(!item.dot && !item.dim && !item.alert && item.idle);
        let r = Rest::sample(Some(Look::Clock), true);
        let item = r.island_item(&env(&cfg)).unwrap();
        assert!(item.dot && item.dim, "parado há 5 min: gota apagada");
        assert_eq!(r.island_fps(IslandForm::Bubble), 0, "parada, a gota não respira");
        let r = Rest::sample(None, false);
        let item = r.island_item(&env(&cfg)).unwrap();
        assert!(item.dot && !item.dim, "só uma gota");
        assert!(r.island_fps(IslandForm::Bubble) > 0, "a gota respira");
        let off = json!({"app": {}, "repouso": {"na_ilha": false}});
        assert!(r.island_item(&env(&off)).is_none());
    }

    #[test]
    fn data_comes_from_the_cache() {
        let r = Rest::sample(Some(Look::Weather), false);
        let w = r.weather.as_ref().unwrap();
        assert_eq!((w.temp, w.max, w.min), (24, Some(27), Some(18)));
        assert_eq!(w.hours.first().map(|h| h.0), Some(15), "as horas depois de agora (14:32)");
        assert_eq!(w.hours.len(), 6);
        let q = r.quotes.as_ref().unwrap();
        assert_eq!((q.list[0].0, q.list[1].1), (5.61, -1.2));
        assert!(q.list[0].2.len() > 10);
        assert_eq!(r.focus_today, FocusToday { done: 3, minutes: 75, goal: 4 });
        let t = r.track.as_ref().unwrap();
        assert!(t.live && !t.playing);
        assert_eq!(r.button(Look::Media), Some((PLAY, "Tocar")));
        // sexta 14:32: o expediente acaba às 17:00, o sol se põe perto das 18:40
        let now = r.clock.now();
        let next = r.upcoming(now);
        assert_eq!(next[0].0, "Fim do expediente");
        assert_eq!(r.clock.hhmm(next[0].1), "17:00");
        assert_eq!(next[1].0, "Pôr do sol");
        assert!(r.clock.hhmm(next[1].1).starts_with("18:"), "{}", r.clock.hhmm(next[1].1));
        assert!(next.iter().any(|e| e.0 == "Lua cheia"), "a lua cheia de 24/11");
        assert!(next.iter().any(|e| e.0 == "A ISS passa"));
        assert_eq!(r.work_line(now), "faltam 2h28 para o fim do expediente");
        // velhos demais: some
        let mut cache = sample_cache(now);
        cache["cotacao"]["em"] = json!(now - 3.0 * 3600.0);
        assert!(quotes_of(&cache, now).is_none());
        assert!(weather_of(&cache, &r.clock, now + 5.0 * 3600.0).is_none());
    }

    #[test]
    fn looks_without_data_are_skipped() {
        let mut r = Rest::new();
        let friday = crate::data::util::days_from_civil(2026, 11, 20) as f64 * 86400.0;
        r.clock = Clock::at(friday + 17.5 * 3600.0, -3.0 * 3600.0);
        r.fake_idle = Some(0.0);
        let mut cfg = json!({"app": {}, "repouso": {"mostrar": ["dia", "tempo", "cotacoes", "midia"]}});
        let mut cache = json!({});
        let mut out = Vec::new();
        r.tick(&mut Ctx { cfg: &mut cfg, cache: &mut cache, key: KEY, out: &mut out });
        assert_eq!(r.looks, vec![Look::Clock], "sem cidade, tempo, cotação nem música: o relógio");
    }

    #[test]
    fn media_note_keeps_the_last_track() {
        let mut cache = json!({});
        let info = crate::widgets::media::Media::sample().island().unwrap();
        assert!(note_track(&mut cache, Some(&info), 1000.0).save, "música nova");
        assert!(!note_track(&mut cache, Some(&info), 1001.0).save, "a mesma");
        assert_eq!(cache["midia_ultima"]["ativo"], true);
        assert!(note_track(&mut cache, None, 1002.0).save, "o player fechou: vale guardar onde parou");
        assert_eq!(cache["midia_ultima"]["ativo"], false, "o player fechou");
        assert_eq!(cache["midia_ultima"]["titulo"], info.title.as_str(), "a música fica");
        assert!(!note_track(&mut cache, None, 1003.0).save, "já estava fechado");
        let t = track_of(&cache["midia_ultima"]).unwrap();
        assert!(!t.live);
        let r = Rest { track: Some(t), ..Rest::new() };
        assert_eq!(r.button(Look::Media), None, "sem o player, sem o botão de tocar");
    }

    /// Uma música do Tocando agora: `title`, tocando ou não, em `position` de 200 s.
    fn song(title: &str, playing: bool, position: f64) -> crate::widgets::IslandInfo {
        let mut info = crate::widgets::media::Media::sample().island().unwrap();
        (info.title, info.artist, info.app, info.art) = (title.into(), "Artista".into(), "Spotify".into(), None);
        (info.playing, info.position, info.duration) = (playing, position, 200.0);
        info
    }

    /// As tocadas antes: cada música nova empurra a anterior para a lista (a mais nova primeiro,
    /// até `RECENT_MAX`, sem repetir), e uma que volta a tocar sai de lá.
    #[test]
    fn recent_tracks_keep_the_order_without_repeats() {
        let mut cache = json!({});
        for (n, title) in ["A", "B", "C", "D", "E"].iter().enumerate() {
            note_track(&mut cache, Some(&song(title, true, 10.0)), 1000.0 + n as f64 * 300.0);
        }
        let titles = |cache: &Value| cache["midia_recentes"].as_array().unwrap().iter().map(|m| m["titulo"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(cache["midia_ultima"]["titulo"], "E");
        assert_eq!(titles(&cache), ["D", "C", "B"], "até {RECENT_MAX}, a mais nova primeiro");
        // C volta a tocar: sai das tocadas antes, e E entra
        note_track(&mut cache, Some(&song("C", true, 0.0)), 3000.0);
        assert_eq!(titles(&cache), ["E", "D", "B"]);
        let old = &cache["midia_recentes"][0];
        assert_eq!((old["tocando"].as_bool(), old["ativo"].as_bool()), (Some(false), Some(false)), "as antigas ficam paradas");
        assert_eq!(old["quando"].as_f64(), Some(1000.0 + 4.0 * 300.0), "com a hora em que tocaram");
    }

    /// A posição e a hora acompanham a música; pausada, a hora para (é quando ela parou de tocar),
    /// e a capa é guardada quando chega (depois do título, às vezes).
    #[test]
    fn note_follows_the_position_and_the_cover() {
        let mut cache = json!({});
        note_track(&mut cache, Some(&song("A", true, 10.0)), 1000.0);
        note_track(&mut cache, Some(&song("A", true, 70.0)), 1060.0);
        let paused = note_track(&mut cache, Some(&song("A", false, 74.0)), 1064.0);
        assert!(paused.save, "pausou: vale guardar onde parou");
        note_track(&mut cache, Some(&song("A", false, 74.0)), 1500.0);
        let t = track_of(&cache["midia_ultima"]).unwrap();
        assert_eq!((t.position, t.duration, t.when), (74.0, 200.0, Some(1064.0)));
        assert_eq!(t.cover, None, "ainda sem capa");
        // a capa chega: guarda uma vez só
        let mut with_art = song("A", false, 74.0);
        with_art.art = Some(Arc::new(Image::new(4, 4)));
        let noted = note_track(&mut cache, Some(&with_art), 1501.0);
        let key = noted.cover.as_ref().map(|c| c.0.clone()).expect("a capa nova");
        assert!(noted.save);
        assert_eq!(cache["midia_ultima"]["capa"], key.as_str());
        assert!(note_track(&mut cache, Some(&with_art), 1502.0).cover.is_none(), "já guardada");
        assert_eq!(key, cover_key("A", "Artista"), "o nome é o mesmo para a mesma música");
    }

    /// As capas vão para a pasta como PNG e são lidas de volta; as que nenhuma música usa mais
    /// saem da pasta.
    #[test]
    fn covers_are_saved_read_and_pruned() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let dir = std::env::temp_dir().join(format!("widgets-capas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut art = Image::new(300, 300);
        for p in art.px.chunks_mut(4) {
            p.copy_from_slice(&[40, 200, 90, 255]);
        }
        save_cover(&dir, "usada", &art);
        save_cover(&dir, "velha", &art);
        let cache = json!({"midia_ultima": {"titulo": "A", "capa": "usada"}, "midia_recentes": []});
        prune_covers(&dir, &cache);
        assert!(dir.join("usada.png").exists() && !dir.join("velha.png").exists());
        let r = Rest { covers_dir: Some(dir.clone()), ..Rest::new() };
        let t = track_of(&cache["midia_ultima"]).unwrap();
        let img = r.cover_of(&t).expect("a capa lida da pasta");
        assert_eq!((img.w, img.h), (COVER_PX, COVER_PX), "guardada pequena");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Como a música está cabe inteiro ao lado da capa, mesmo com o botão redondo e no texto mais
    /// comprido ("pausada ontem às 22:10"): o texto do player só cede o nome do app, nunca
    /// aparece cortado.
    #[test]
    fn open_media_state_is_never_cut() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let c = Canvas::new(1, 1, 1.0);
        let w = iu::size(IslandForm::Large).w;
        let room = w - 22.0 - MEDIA_BUTTON - 12.0 - MEDIA_TEXT_X - 12.0;
        for state in ["pausada ontem às 22:10", "tocou ontem às 22:10", "tocou em 28 dez", "tocando agora"] {
            assert!(c.text_width(state, Face::Regular, 11.5) <= room, "{state:?} não cabe em {room}");
        }
        let medium = iu::size(IslandForm::Medium).w - 76.0 - 70.0;
        assert!(c.text_width("pausada ontem às 22:10", Face::Regular, 11.5) <= medium, "no médio, com o botão");
    }

    /// Quando tocou, em palavras: há pouco, em minutos, hoje, ontem e em outro dia.
    #[test]
    fn track_times_read_naturally() {
        let r = Rest::sample(Some(Look::Media), false);
        let now = r.clock.now();
        assert_eq!(r.ago(now - 20.0, now), "há pouco");
        assert_eq!(r.ago(now - 25.0 * 60.0, now), "há 25 min");
        assert_eq!(r.ago(now - 3.0 * 3600.0, now), "hoje às 11:32");
        assert_eq!(r.ago(now - 17.0 * 3600.0, now), "ontem às 21:32");
        assert_eq!(r.ago(now - 9.0 * 86400.0, now), "em 11 nov");
        assert_eq!((r.ago_short(now - 25.0 * 60.0, now), r.ago_short(now - 26.0 * 3600.0, now)), ("25 min".into(), "ontem".into()));
        let t = r.track.clone().unwrap();
        assert_eq!(r.track_state(&t, now), "pausada há 25 min");
        assert_eq!(r.track_state(&Track { live: false, ..t.clone() }, now), "tocou há 25 min");
        assert_eq!(r.track_state(&Track { playing: true, ..t }, now), "tocando agora");
        assert_eq!(clock_text(213.0), "3:33");
        assert_eq!(clock_text(3727.0), "1:02:07");
    }

    /// A música aberta: tocar (pausada), abrir o player (fechado) e procurar as tocadas antes no
    /// player delas; o ponto do player na cor do logo.
    #[test]
    fn open_media_buttons_play_open_and_search() {
        let cfg = json!({"app": {}});
        let iu::FormSize { w, h, .. } = iu::size(IslandForm::Large);
        let mut r = Rest::sample(Some(Look::Media), false);
        assert_eq!(r.recent.len(), 2, "as duas tocadas antes do exemplo");
        let spots = r.media_spots(w);
        let mid = |act: u32| spots.iter().find(|(a, _)| *a == act).map(|(_, b)| ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0)).unwrap();
        assert_eq!(r.island_hit(IslandForm::Large, mid(PLAY).0, mid(PLAY).1, w, h, &env(&cfg)), Some(PLAY), "pausada, com o player aberto: tocar");
        assert_eq!(r.island_hit(IslandForm::Large, mid(RECENT + 1).0, mid(RECENT + 1).1, w, h, &env(&cfg)), Some(RECENT + 1));
        let (mut c, mut cache, mut out) = (cfg.clone(), json!({}), Vec::new());
        r.act(RECENT + 1, &mut Ctx { cfg: &mut c, cache: &mut cache, key: KEY, out: &mut out });
        assert!(matches!(out.as_slice(), [Request::Open(url)] if url == "spotify:search:Trem+Azul+L%C3%B4+Borges"));
        // com o player fechado: o botão de abrir
        r.track.as_mut().unwrap().live = false;
        let spots = r.media_spots(w);
        assert!(spots.iter().any(|(a, _)| *a == OPEN_APP) && !spots.iter().any(|(a, _)| *a == PLAY));
        out.clear();
        r.act(OPEN_APP, &mut Ctx { cfg: &mut c, cache: &mut cache, key: KEY, out: &mut out });
        assert!(matches!(out.as_slice(), [Request::Open(url)] if url == "spotify:"));
        // a última linha das tocadas antes cabe na ilha
        assert!(RECENT_FIRST_Y + RECENT_ROW * RECENT_MAX as f32 <= h - 8.0);
        assert_eq!(app_color(r.track.as_ref().unwrap()), crate::widgets::media::source_color("spotify"));
    }

    #[test]
    fn buttons_start_a_focus_and_play() {
        let cfg = json!({"app": {}});
        let r = Rest::sample(Some(Look::Focus), false);
        let iu::FormSize { w, h, .. } = iu::size(IslandForm::Medium);
        let b = button_rect(IslandForm::Medium, w, h);
        assert_eq!(r.island_hit(IslandForm::Medium, (b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0, w, h, &env(&cfg)), Some(START));
        assert_eq!(r.island_hit(IslandForm::Compact, 10.0, 10.0, 236.0, 42.0, &env(&cfg)), None);
        let mut r = r;
        let mut c = cfg.clone();
        let mut cache = json!({});
        let mut out = Vec::new();
        r.act(START, &mut Ctx { cfg: &mut c, cache: &mut cache, key: KEY, out: &mut out });
        assert!(matches!(out.as_slice(), [Request::Act("pomodoro", a)] if *a == pomodoro::START_IF_IDLE));
    }

    /// No modo foco: só a hora, o dia e o ícone, sem alternar, mesmo com a ilha em repouso
    /// desligada; aberta, o botão de sair.
    #[test]
    fn focus_mode_shows_only_the_clock() {
        let off = json!({"app": {}, "repouso": {"na_ilha": false}});
        let mut r = Rest::sample(Some(Look::Clock), false);
        r.looks = vec![Look::Clock, Look::Weather];
        let mut c = off.clone();
        let mut cache = json!({});
        let mut out = Vec::new();
        r.input(FOCUS_FIELD, "15:30", None, &mut Ctx { cfg: &mut c, cache: &mut cache, key: KEY, out: &mut out });
        assert_eq!(r.focus_mode.as_deref(), Some("15:30"));
        let item = r.island_item(&env(&off)).expect("no modo foco, sempre na ilha");
        assert!(!item.dot && !item.alert);
        let base = (r.clock.now() / ROTATE_EVERY).floor() * ROTATE_EVERY;
        r.at(base + ROTATE_EVERY + 0.1);
        assert_eq!(r.island_fps(IslandForm::Compact), 0, "sem alternar");
        let iu::FormSize { w, h, .. } = iu::size(IslandForm::Large);
        let b = button_rect(IslandForm::Large, w, h);
        assert_eq!(r.island_hit(IslandForm::Large, (b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0, w, h, &env(&off)), Some(END_FOCUS));
        assert_eq!(r.island_hit(IslandForm::Medium, 300.0, 39.0, 340.0, 78.0, &env(&off)), None);
        r.act(END_FOCUS, &mut Ctx { cfg: &mut c, cache: &mut cache, key: KEY, out: &mut out });
        assert!(matches!(out.as_slice(), [Request::Focus(false)]));
        r.input(FOCUS_FIELD, "off", None, &mut Ctx { cfg: &mut c, cache: &mut cache, key: KEY, out: &mut out });
        assert!(r.island_item(&env(&off)).is_none(), "fora do foco, desligada");
    }

    #[test]
    fn countdown_reads_well() {
        assert_eq!(countdown(30.0), "1 min");
        assert_eq!(countdown(40.0 * 60.0), "40 min");
        assert_eq!(countdown(148.0 * 60.0), "2h28");
        assert_eq!(countdown(3.0 * 3600.0), "3 h");
        assert_eq!(countdown(3.6 * 86400.0), "4 dias");
        assert_eq!(countdown(0.0), "agora");
    }
}
