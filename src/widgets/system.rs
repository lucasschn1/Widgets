//! Widget Sistema: três anéis (CPU, memória e disco) no estilo dos anéis de atividade. Clicar num
//! anel mostra os apps que mais usam aquele recurso (no disco, as unidades): no pequeno e no
//! médio o cartão "vira" e mostra os 3 primeiros; no grande, a lista fica embaixo dos anéis.

use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::odometer::draw_counting;
use super::island_ui::{self as iu, SOFT};
use super::{alert_hash, Ctx, Env, Hot, IslandForm, IslandItem, Kind, MenuItem, Mode, Rect4, Status, LARGE_H, SMALL, WIDE};
use crate::gfx::drawing;
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::platform::sysinfo::{self, AppUse, CpuSpeed, Drive, Sampler};
use crate::ui::theme::*;
use crate::platform::win;

const W: f32 = WIDE;
const PAD: f32 = super::CARD_PAD;
const ROW_H: f32 = 38.0;
const LIST_N: usize = 5;
/// médio virado: linhas mais baixas, para caber 3 embaixo do cabeçalho
const BACK_ROW_H: f32 = 34.0;
const BACK_LIST_Y: f32 = 44.0;
const BACK_N: usize = 3;
/// grande: o cabeçalho "SISTEMA" em cima empurra o resto para baixo
const G_TOP: f32 = 24.0;
/// médio: as linhas descem um pouco para o rótulo "SISTEMA" caber em cima delas (a última barra
/// termina junto com o anel de fora)
const M_TOP: f32 = 12.0;
const SEG_Y: f32 = 150.0 + G_TOP;
const G_LIST_Y: f32 = 194.0 + G_TOP;
/// rodapé (rede e tempo ligado) no pé do cartão, logo depois da lista
const G_FOOT_Y: f32 = LARGE_H - 34.0;
const _: () = assert!(G_LIST_Y + ROW_H * LIST_N as f32 + 6.0 <= G_FOOT_Y, "a lista não cabe no grande");

const EVERY: Duration = Duration::from_secs(2);
const RING_MS: f32 = 550.0;
/// mudança a partir da qual os anéis animam (3 pontos percentuais); menor, vão direto
const RING_STEP: f32 = 0.03;
const BARS_MS: f32 = 650.0;
/// as barras da lista enchem uma depois da outra, com este atraso entre elas
const BAR_STAGGER_MS: f32 = 55.0;
/// o pequeno virado: as linhas sobem e aparecem uma depois da outra (atraso e duração de cada uma)
const BACK_P_STAGGER_MS: f32 = 60.0;
const BACK_P_RISE_MS: f32 = 380.0;
/// as barras param de pedir quadros um pouco depois de cheias (a última começa mais tarde)
const BARS_TAIL_MS: f32 = 300.0;
/// animando: um quadro a cada 33 ms (30 por segundo)
const FRAME: Duration = Duration::from_millis(33);
/// esperando a primeira leitura dos apps: olhar de novo a cada 200 ms
const APPS_POLL: Duration = Duration::from_millis(200);
/// a lista acabou de aparecer: a próxima medida vem em no máximo 900 ms
const SOON: Duration = Duration::from_millis(900);
/// as unidades de disco são lidas de novo com a lista aberta a cada 30 s
const DRIVES_EVERY: Duration = Duration::from_secs(30);

// ações
const SELECT: u32 = 10; // + recurso (0 CPU, 1 memória, 2 disco)
const OPEN_EXPLORER: u32 = 2;
const CLOSE: u32 = 3;
/// na ilha: abrir o Gerenciador de Tarefas e não avisar por uma hora
const OPEN_TASKMGR: u32 = 4;
const DISMISS: u32 = 5;

/// CPU ou memória acima disto por `HIGH_FOR` entram na ilha (os mesmos limites da notificação);
/// saem abaixo de `CALM`.
const HIGH: f32 = crate::data::config::HIGH_USE;
const CALM: f32 = HIGH - 0.1;
const HIGH_FOR: Duration = Duration::from_secs(crate::data::config::HIGH_USE_SECS);
const QUIET_FOR: Duration = Duration::from_secs(3600);

const NAMES: [&str; 3] = ["CPU", "Memória", "Disco"];
const TITLES: [&str; 3] = ["Processador", "Memória", "Armazenamento"];

/// Cor do anel de cada recurso (0 CPU, 1 memória, 2 disco); o ícone do app usa as mesmas.
pub(crate) fn color(i: usize) -> Rgba {
    match i {
        0 => PINK,
        1 => GREEN,
        _ => CYAN,
    }
}

/// Um "ícone" de teste: um círculo verde cheio, com borda suave, num fundo transparente.
#[cfg(test)]
fn test_icon(px: u32) -> Image {
    let mut c = Canvas::new(px, px, 1.0);
    let r = px as f32 / 2.0;
    c.fill_circle(r, r, r - 2.0, Rgba::rgb(30, 215, 96));
    c.finish()
}

/// Cores dos ícones de letra (apps sem ícone): cada app fica sempre com a mesma.
const TILE_COLORS: [Rgba; 8] = [
    Rgba::rgb(94, 129, 244),
    Rgba::rgb(52, 168, 140),
    Rgba::rgb(236, 112, 99),
    Rgba::rgb(171, 112, 230),
    Rgba::rgb(242, 166, 56),
    Rgba::rgb(64, 156, 220),
    Rgba::rgb(220, 96, 160),
    Rgba::rgb(96, 170, 90),
];

/// A cor do ícone de letra de um app (pelo nome, para não mudar de uma leitura para outra).
fn tile_color(name: &str) -> Rgba {
    let hash = name.to_lowercase().bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    TILE_COLORS[(hash % TILE_COLORS.len() as u32) as usize]
}

/// O ícone de um app (ou de uma unidade) no quadrado de lado `size`, com o canto em (x, y):
/// o do Windows, redimensionado com qualidade para o tamanho exato (nítido em qualquer escala);
/// sem ele, a inicial do nome num quadrado arredondado com a cor do app.
fn app_icon(c: &mut Canvas, icon: Option<&Image>, name: &str, x: f32, y: f32, size: f32, alpha: f32) {
    match icon {
        Some(img) => c.image_sharp(img, x, y, size, size, alpha),
        None => {
            let radius = size * 0.27;
            let col = tile_color(name);
            c.gradient_round_rect(x, y, x + size, y + size, radius, &[(0.0, col.mix(WHITE, 0.18).alpha(alpha)), (1.0, col.alpha(alpha))], y, y + size);
            let letter: String = name.chars().find(|ch| ch.is_alphanumeric()).map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
            c.text(x + size / 2.0, y + size / 2.0, &letter, Face::Semibold, size * 0.46, WHITE.alpha(alpha), Anchor::MM, None);
        }
    }
}

/// Sombra suave logo à frente da ponta de um anel (o fim arredondado em `end` graus), como nos
/// anéis de atividade: desenhada antes do arco, só aparece a parte que sai da frente dele e dá
/// a impressão de a ponta estar um pouco acima do trilho.
fn ring_tip_shadow(c: &mut Canvas, cx: f32, cy: f32, r: f32, end: f32, thickness: f32) {
    let angle = end.to_radians();
    // a direção em que o anel anda (sentido horário na tela)
    let (dx, dy) = (-angle.sin(), angle.cos());
    let (tx, ty) = (cx + r * angle.cos() + dx * thickness * 0.14, cy + r * angle.sin() + dy * thickness * 0.14);
    // quatro círculos cada vez maiores e mais claros: a sombra esfumada
    for layer in 0..4 {
        c.fill_circle(tx, ty, thickness / 2.0 + 0.6 * layer as f32, Rgba(0, 0, 0, 22 - 4 * layer as u8));
    }
}

/// espaço entre o nome e a porcentagem no título do pequeno virado
const BACK_P_GAP: f32 = 5.0;

/// Letra do título do pequeno virado ("Memória 68%"): 12,5, ou menor até caber em `room`.
fn back_p_title_size(c: &Canvas, name: &str, pct: &str, room: f32) -> f32 {
    let mut size = 12.5;
    while size > 10.5 && c.text_width(name, Face::Semibold, size) + BACK_P_GAP + c.text_width(pct, Face::Semibold, size) > room {
        size -= 0.5;
    }
    size
}

/// Começa rápido e chega devagar (0 a 1).
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Um número com `digits` casas e vírgula, como se escreve no Brasil ("2,84").
fn br(value: f64, digits: usize) -> String {
    format!("{value:.digits$}").replace('.', ",")
}

/// A última leitura dos bytes da rede: recebidos, enviados e quando.
#[derive(Clone, Copy)]
struct NetReading {
    received: u64,
    sent: u64,
    at: Instant,
}

/// Onde ficam os três anéis em cada tamanho.
#[derive(Clone, Copy)]
struct RingsGeom {
    cx: f32,
    cy: f32,
    /// o diâmetro do anel de fora
    diameter: f32,
    /// a espessura de cada anel
    thickness: f32,
    /// quanto o diâmetro diminui de um anel para o de dentro
    step: f32,
}

/// Uma linha da lista: nome, valor, fração da barra, ícone.
struct Row {
    name: String,
    value: String,
    frac: f32,
    icon: Option<Arc<Image>>,
}

pub struct System {
    /// valores de agora (0 a 1): CPU, memória, disco
    target: [f32; 3],
    /// valores desenhados (andam até `target` com animação)
    from: [f32; 3],
    ring_since: Instant,
    details: [String; 3],
    /// quanto está em uso agora, curto (médio e grande): "2,84 GHz", "10,9/16 GB", "412/477 GB"
    amounts: [String; 3],
    speed: Option<CpuSpeed>,
    last_cpu: (u64, u64),
    last_net: NetReading,
    rates: (f64, f64),
    next: Instant,
    sampler: Sampler,
    apps: Vec<AppUse>,
    drives: Vec<Drive>,
    drives_at: Option<Instant>,
    /// recurso aberto na lista do grande (sem escolha: memória)
    selected: Option<usize>,
    /// pequeno ou médio "virado" mostrando os apps deste recurso (desvira ao mudar de tamanho)
    flipped: Cell<Option<usize>>,
    bars_since: Instant,
    /// o último tamanho desenhado (para saber se a lista está na tela)
    mode_seen: Cell<Mode>,
    status: Status,
    /// quando chamar `tick` de novo (um prazo fixo: "agora + x" nunca venceria)
    due: Instant,
    /// desde quando a CPU e a memória estão acima de `HIGH` (até baixarem de `CALM`)
    high_since: [Option<Instant>; 2],
    /// "Dispensar por 1 hora" na ilha
    quiet_until: Option<Instant>,
}

impl System {
    pub fn new() -> System {
        let mut system = System::with(Sampler::new());
        system.speed = CpuSpeed::new();
        system
    }

    /// O widget com os anéis zerados e uma primeira medida, lendo os apps com `sampler`.
    fn with(sampler: Sampler) -> System {
        let (received, sent) = sysinfo::net_bytes();
        let mut system = System {
            target: [0.0; 3],
            from: [0.0; 3],
            ring_since: Instant::now(),
            details: Default::default(),
            amounts: Default::default(),
            speed: None,
            last_cpu: win::cpu_times(),
            last_net: NetReading { received, sent, at: Instant::now() },
            rates: (0.0, 0.0),
            next: Instant::now(),
            sampler,
            apps: Vec::new(),
            drives: Vec::new(),
            drives_at: None,
            selected: None,
            flipped: Cell::new(None),
            bars_since: Instant::now(),
            mode_seen: Cell::new(Mode::M),
            status: Status::default(),
            due: Instant::now(),
            high_since: [None; 2],
            quiet_until: None,
        };
        system.measure();
        system
    }

    /// Exemplo para a galeria (sem ler o sistema de verdade).
    pub fn sample() -> System {
        let mut system = System::with(Sampler::idle());
        system.target = [0.24, 0.68, 0.54];
        system.from = system.target;
        system.amounts = ["2,84 GHz".into(), "10,9/16 GB".into(), "258/477 GB".into()];
        system.ring_since = Instant::now() - Duration::from_secs(5);
        system
    }

    /// Exemplo com CPU, memória e disco escolhidos (0 a 1), já sem a animação dos anéis (a capa
    /// do README).
    #[cfg(test)]
    pub fn sample_values(values: [f32; 3]) -> System {
        let mut system = System::sample();
        system.target = values;
        system.from = values;
        system
    }

    /// Os anéis indo de outros números aos do exemplo há `ms` milissegundos: as porcentagens
    /// contando, com os dígitos rolando.
    #[cfg(test)]
    pub fn sample_counting(ms: u64) -> System {
        let mut system = System::sample();
        system.from = [0.17, 0.75, 0.47];
        system.ring_since = Instant::now() - Duration::from_millis(ms);
        system
    }

    /// Exemplo virado (pequeno ou médio) mostrando os apps de um recurso (imagens do README).
    #[cfg(test)]
    pub fn sample_flipped(which: Option<usize>, mode: Mode) -> System {
        let mut system = System::sample();
        system.mode_seen.set(mode);
        system.flipped.set(which);
        // cinco, como no uso de verdade (a lista do grande mostra até `LIST_N`): nome, % da CPU e
        // GB de memória
        let apps = [("Google Chrome", 6.2, 2.1), ("Visual Studio Code", 3.4, 1.2), ("Spotify", 1.1, 0.4), ("Discord", 0.6, 0.6), ("Explorador de Arquivos", 0.4, 0.2)];
        system.apps = apps.iter().map(|&(name, cpu, gb)| AppUse { name: name.into(), cpu, mem: (gb * 1024f64.powi(3)) as u64, icon: None }).collect();
        system.bars_since = Instant::now() - Duration::from_secs(5);
        system.rates = (1.8e6, 2.4e5);
        system
    }

    /// A CPU alta há dois minutos, com os apps lidos: o aviso da ilha.
    #[cfg(test)]
    pub fn sample_alarm() -> System {
        let mut system = System::sample_flipped(None, Mode::M);
        system.target = [0.92, 0.68, 0.41];
        system.from = system.target;
        system.apps[0].cpu = 41.0;
        system.high_since = [Some(Instant::now() - HIGH_FOR - Duration::from_secs(60)), None];
        system
    }

    /// O mesmo, com ícones de 64 px nos apps (como os que o Windows dá), para desenhar o caminho
    /// dos ícones de verdade.
    #[cfg(test)]
    pub fn sample_with_icons(which: Option<usize>, mode: Mode) -> System {
        let mut system = System::sample_flipped(which, mode);
        let icon = Arc::new(test_icon(64));
        for app in system.apps.iter_mut().take(2) {
            app.icon = Some(icon.clone());
        }
        system
    }

    /// Lê a CPU, a memória, o disco e a rede de agora e põe os anéis e os textos em dia.
    fn measure(&mut self) {
        let (idle, total) = win::cpu_times();
        let (last_idle, last_total) = self.last_cpu;
        self.last_cpu = (idle, total);
        let d_total = total.saturating_sub(last_total);
        let cpu = if d_total > 0 { 1.0 - idle.saturating_sub(last_idle) as f64 / d_total as f64 } else { 0.0 };
        let cpu = cpu.clamp(0.0, 1.0);
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let (used, total_mem) = win::memory();
        let disk = win::disk_usage("C:\\");
        let (dused, dtotal) = (disk.used, disk.total);
        let values = [cpu as f32, (used as f64 / total_mem as f64) as f32, (dused as f64 / dtotal.max(1) as f64) as f32];
        let ghz = self.speed.as_ref().and_then(CpuSpeed::ghz);
        let speed = ghz.map(|ghz| format!(" · {} GHz", br(ghz, 2))).unwrap_or_default();
        self.details = [
            format!("{}% em uso · {cores} núcleos{speed}", (cpu * 100.0).round()),
            format!("{} de {:.0} GB em uso", br(win::gb(used), 1), win::gb(total_mem)),
            format!("C: {:.0} de {:.0} GB em uso", win::gb(dused), win::gb(dtotal)),
        ];
        self.amounts = [
            ghz.map(|ghz| format!("{} GHz", br(ghz, 2))).unwrap_or_else(|| format!("{cores} núcleos")),
            format!("{}/{:.0} GB", br(win::gb(used), 1), win::gb(total_mem)),
            format!("{:.0}/{:.0} GB", win::gb(dused), win::gb(dtotal)),
        ];
        self.set_rings(values);
        self.watch_high(values);
        let (received, sent) = sysinfo::net_bytes();
        let secs = self.last_net.at.elapsed().as_secs_f64().max(0.1);
        self.rates = (received.saturating_sub(self.last_net.received) as f64 / secs, sent.saturating_sub(self.last_net.sent) as f64 / secs);
        self.last_net = NetReading { received, sent, at: Instant::now() };
        self.next = Instant::now() + EVERY;
    }

    /// Leva os anéis aos valores novos (0 a 1): partindo de onde estão agora, animados só se a
    /// mudança aparece. Cada quadro da animação redesenha o cartão inteiro, e a CPU oscila um
    /// pouco a cada medida: animar esses tremores gastava mais CPU do que o que eles mostram.
    fn set_rings(&mut self, values: [f32; 3]) {
        let shown = self.shown();
        let moved = (0..3).any(|i| (values[i] - shown[i]).abs() > RING_STEP);
        self.from = if moved { shown } else { values };
        self.target = values;
        if moved {
            self.ring_since = Instant::now();
        }
    }

    /// Onde os anéis estão agora (0 a 1), no meio da animação até `target`.
    fn shown(&self) -> [f32; 3] {
        let progress = ease_out(self.ring_since.elapsed().as_secs_f32() * 1000.0 / RING_MS);
        let mut out = [0.0; 3];
        for i in 0..3 {
            out[i] = self.from[i] + (self.target[i] - self.from[i]) * progress;
        }
        out
    }

    /// A porcentagem de `i` contando junto com o anel (de um inteiro ao outro, no mesmo ritmo):
    /// 43,6 é o 3 rolando para o 4; parada, o número de agora.
    fn counted(&self, i: usize) -> f64 {
        let progress = ease_out(self.ring_since.elapsed().as_secs_f32() * 1000.0 / RING_MS) as f64;
        let (from, to) = ((self.from[i] as f64 * 100.0).round(), (self.target[i] as f64 * 100.0).round());
        from + (to - from) * progress
    }

    /// A lista de apps está na tela, mas a primeira leitura ainda não chegou.
    fn waiting_apps(&self) -> bool {
        self.list_visible() && self.apps.is_empty() && self.showing(self.mode_seen.get()) != Some(2)
    }

    /// Os anéis ou as barras da lista ainda estão andando (pedem um quadro atrás do outro)?
    fn animating(&self) -> bool {
        self.ring_since.elapsed().as_secs_f32() * 1000.0 < RING_MS || self.bars_since.elapsed().as_secs_f32() * 1000.0 < BARS_MS + BARS_TAIL_MS
    }

    /// A lista está na tela?
    fn list_visible(&self) -> bool {
        self.showing(self.mode_seen.get()).is_some()
    }

    /// Recurso destacado nos anéis do grande.
    fn current(&self, mode: Mode) -> Option<usize> {
        (mode == Mode::G).then(|| self.selected.unwrap_or(1))
    }

    /// De qual recurso a lista de apps está na tela (no grande, sempre uma; virado, a dele).
    fn showing(&self, mode: Mode) -> Option<usize> {
        if mode == Mode::G { self.current(mode) } else { self.flipped.get() }
    }

    /// O ✕ do lado virado, que volta aos anéis: afastado do canto, onde o botão de tamanho do
    /// cartão aparece com o mouse em cima (encostado nele, um clique no canto do ✕ trocava o
    /// tamanho).
    fn close_rect(mode: Mode) -> (f32, f32, f32, f32) {
        let right = if mode == Mode::P { SMALL } else { W };
        (right - 46.0, 10.0, right - 20.0, 36.0)
    }

    /// Onde ficam os anéis neste tamanho.
    fn rings_geom(mode: Mode) -> RingsGeom {
        let (cx, cy, diameter, thickness, step) = match mode {
            Mode::P => (SMALL / 2.0, 62.0, 100.0, 10.5, 24.0),
            Mode::M => (PAD + 60.0, 75.0, 120.0, 13.0, 30.0),
            Mode::G => (PAD + 60.0, 75.0 + G_TOP, 120.0, 13.0, 30.0),
        };
        RingsGeom { cx, cy, diameter, thickness, step }
    }

    /// O anel embaixo do ponto (x, y): 0 o de fora (CPU), 2 o de dentro (disco).
    fn ring_at(&self, x: f32, y: f32, mode: Mode) -> Option<usize> {
        let rings = Self::rings_geom(mode);
        let distance = ((x - rings.cx).powi(2) + (y - rings.cy).powi(2)).sqrt();
        if distance > rings.diameter / 2.0 + 3.0 {
            return None;
        }
        // anel 0 por fora; cada anel tem "step/2" de raio
        let i = ((rings.diameter / 2.0 - distance + (rings.step / 2.0 - rings.thickness) / 2.0) / (rings.step / 2.0)).floor().max(0.0) as usize;
        Some(i.min(2))
    }

    /// Retângulo da linha (médio/grande) ou coluna (pequeno) de cada recurso.
    fn metric_rect(mode: Mode, i: usize) -> (f32, f32, f32, f32) {
        if mode == Mode::P {
            let col = (SMALL - 16.0) / 3.0;
            (8.0 + col * i as f32, 118.0, 8.0 + col * (i + 1) as f32, SMALL - 6.0)
        } else {
            let top = if mode == Mode::G { G_TOP } else { M_TOP };
            let y = 22.0 + top + 36.0 * i as f32;
            (148.0, y - 6.0, W - 10.0, y + 30.0)
        }
    }

    /// A linha (ou coluna) de recurso embaixo do ponto (x, y).
    fn metric_at(x: f32, y: f32, mode: Mode) -> Option<usize> {
        (0..3).find(|&i| {
            let (x0, y0, x1, y1) = Self::metric_rect(mode, i);
            (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
        })
    }

    /// O botão `i` da barra de escolha do grande (CPU, Memória, Disco).
    fn seg_rect(i: usize) -> (f32, f32, f32, f32) {
        let w = (W - 2.0 * PAD - 6.0) / 3.0;
        let x = PAD + 3.0 + w * i as f32;
        (x, SEG_Y + 3.0, x + w, SEG_Y + 31.0)
    }

    /// As linhas da lista de um recurso: as unidades (disco) ou os apps que mais usam a CPU ou
    /// a memória, com a barra relativa ao primeiro.
    fn rows(&self, which: usize) -> Vec<Row> {
        match which {
            2 => self
                .drives
                .iter()
                .take(LIST_N)
                .map(|drive| Row {
                    name: drive.label.clone(),
                    value: format!("{:.0} de {:.0} GB", win::gb(drive.used), win::gb(drive.total)),
                    frac: drive.used as f32 / drive.total.max(1) as f32,
                    icon: drive.icon.clone(),
                })
                .collect(),
            _ => {
                let use_of = |app: &AppUse| if which == 0 { app.cpu as f64 } else { app.mem as f64 };
                let mut apps: Vec<&AppUse> = self.apps.iter().collect();
                if which == 0 {
                    apps.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
                } else {
                    apps.sort_by(|a, b| b.mem.cmp(&a.mem));
                }
                let apps: Vec<&AppUse> = apps.into_iter().take(LIST_N).collect();
                let max = apps.first().map(|app| use_of(app)).unwrap_or(1.0).max(1e-6);
                apps.into_iter()
                    .map(|app| Row {
                        name: app.name.clone(),
                        value: if which == 0 { format!("{}%", br(app.cpu as f64, 1)) } else { sysinfo::fmt_bytes(app.mem) },
                        frac: (use_of(app) / max) as f32,
                        icon: app.icon.clone(),
                    })
                    .collect()
            }
        }
    }

    /// Vira (ou desvira, com None) o pequeno ou o médio para mostrar os apps de um recurso.
    fn flip(&mut self, which: Option<usize>) {
        self.flipped.set(which);
        self.refresh_list(which);
    }

    /// Escolhe o recurso da lista do grande.
    fn select(&mut self, which: Option<usize>) {
        self.selected = which;
        self.refresh_list(which);
    }

    /// A lista vai aparecer: as barras enchem de novo e a leitura dos apps liga (ou desliga).
    fn refresh_list(&mut self, which: Option<usize>) {
        self.bars_since = Instant::now();
        if which == Some(2) || self.drives_at.is_none() {
            self.drives = sysinfo::drives();
            self.drives_at = Some(Instant::now());
        }
        self.sampler.set_active(self.wants_apps());
        if let Some(apps) = self.sampler.latest() {
            self.apps = apps;
        }
        // a leitura guardada pode ser antiga: a próxima vem logo
        self.next = self.next.min(Instant::now() + SOON);
        self.due = Instant::now();
    }

    /// Marca quando o próximo `tick` precisa vir: já (animando), logo (esperando os apps) ou na
    /// próxima medida.
    fn plan(&mut self) {
        let now = Instant::now();
        self.due = if self.animating() {
            now + FRAME
        } else if self.waiting_apps() {
            now + APPS_POLL
        } else {
            self.next
        };
    }

    // ---------- desenho ----------

    /// Os três anéis, cada um no trilho dele, cheios até o valor de agora (no grande, os que não
    /// estão escolhidos ficam apagados).
    fn draw_rings(&self, c: &mut Canvas, mode: Mode) {
        let RingsGeom { cx, cy, diameter, thickness, step } = Self::rings_geom(mode);
        let shown = self.shown();
        let selected = self.current(mode);
        for (i, level) in shown.iter().enumerate() {
            let dim = selected.is_some_and(|chosen| chosen != i);
            let col = if dim { color(i).alpha(0.32) } else { color(i) };
            let ring_diameter = diameter - step * i as f32;
            let radius = ring_diameter / 2.0 - thickness / 2.0;
            c.stroke_circle(cx, cy, radius, color(i).alpha(if dim { 0.1 } else { 0.22 }), thickness);
            if *level > 0.003 {
                let end = -90.0 + 360.0 * level.min(0.999);
                if !dim {
                    ring_tip_shadow(c, cx, cy, radius, end, thickness);
                }
                c.arc(cx, cy, radius, -90.0, end, col, thickness);
            }
        }
    }

    /// Cabeçalho: "SISTEMA" na cor da CPU; no grande, em cima de tudo e com os núcleos à
    /// direita; no médio, no alto da coluna das medidas (os anéis ocupam a altura toda).
    fn draw_header(&self, c: &mut Canvas, mode: Mode) {
        if mode == Mode::M {
            super::draw_label(c, Self::metric_rect(mode, 0).0, super::LABEL_Y, "SISTEMA", PINK.mix(WHITE, 0.12));
            return;
        }
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        super::draw_label(c, PAD, super::LABEL_Y, "SISTEMA", PINK.mix(WHITE, 0.12));
        c.text(W - PAD, super::LABEL_Y, &format!("{cores} núcleos"), Face::Regular, 11.5, MUTED, Anchor::RA, None);
    }

    /// As medidas ao lado dos anéis: no pequeno, a porcentagem e o nome embaixo de cada um; no
    /// médio e no grande, uma linha por recurso com nome, quanto está em uso, porcentagem e barra.
    fn draw_metrics(&self, c: &mut Canvas, mode: Mode) {
        let shown = self.shown();
        let selected = self.current(mode);
        for i in 0..3 {
            let (x0, y0, x1, _) = Self::metric_rect(mode, i);
            let pct = self.counted(i);
            let dim = selected.is_some_and(|chosen| chosen != i) && mode != Mode::G;
            let text = if dim { MUTED } else { FG };
            if mode == Mode::P {
                let cx = (x0 + x1) / 2.0;
                draw_counting(c, cx, y0 + 8.0, pct, "%", Face::Semibold, 13.5, color(i), Anchor::MM);
                c.text(cx, y0 + 23.0, NAMES[i], Face::Regular, 10.0, MUTED, Anchor::MM, Some(x1 - x0 - 2.0));
            } else {
                let y = y0 + 6.0;
                c.fill_circle(x0 + 4.0, y + 7.0, 3.5, color(i));
                let pw = draw_counting(c, x1 - 6.0, y + 7.0, pct, "%", Face::Semibold, 13.5, text, Anchor::RM);
                // quanto está em uso agora, antes da porcentagem
                let aw = c.text(x1 - 6.0 - pw - 7.0, y + 7.5, &self.amounts[i], Face::Regular, 11.0, MUTED, Anchor::RM, None);
                let name_w = (x1 - 6.0 - pw - 7.0 - aw - 6.0) - (x0 + 13.0);
                c.text(x0 + 13.0, y + 7.0, NAMES[i], Face::Regular, 12.0, MUTED, Anchor::LM, Some(name_w.max(10.0)));
                let (bx0, bx1, by) = (x0, x1 - 6.0, y + 19.0);
                c.fill_round_rect(bx0, by, bx1, by + 4.0, 2.0, WHITE.alpha(0.12));
                let w = (bx1 - bx0) * shown[i].clamp(0.0, 1.0);
                if w > 1.0 {
                    c.fill_round_rect(bx0, by, bx0 + w.max(4.0), by + 4.0, 2.0, color(i));
                }
            }
        }
    }

    /// A lista dos apps (ou das unidades) de `which` a partir de `y0`: até `n_max` linhas de
    /// altura `row_h`, cada uma com ícone, nome, valor e uma barra que enche.
    fn draw_list(&self, c: &mut Canvas, which: usize, y0: f32, n_max: usize, row_h: f32) {
        let rows = self.rows(which);
        if rows.is_empty() {
            let msg = if which == 2 { "Nenhuma unidade encontrada" } else { "Lendo os apps…" };
            c.text(W / 2.0, y0 + 40.0, msg, Face::Regular, 12.0, MUTED, Anchor::MM, None);
            return;
        }
        let since = self.bars_since.elapsed().as_secs_f32() * 1000.0;
        for (index, row) in rows.iter().take(n_max).enumerate() {
            let y = y0 + row_h * index as f32;
            let icon = 26.0;
            let ix = PAD;
            app_icon(c, row.icon.as_deref(), &row.name, ix, y + 5.0, icon, 1.0);
            let tx = ix + icon + 10.0;
            let vw = c.text(W - PAD, y + 12.0, &row.value, Face::Regular, 12.0, MUTED, Anchor::RM, None);
            c.text(tx, y + 12.0, &row.name, Face::Regular, 12.5, FG, Anchor::LM, Some(W - PAD - tx - vw - 10.0));
            let (bx0, bx1, by) = (tx, W - PAD, y + 24.0);
            c.fill_round_rect(bx0, by, bx1, by + 4.0, 2.0, WHITE.alpha(0.1));
            // as barras enchem uma depois da outra
            let filling = ease_out((since - index as f32 * BAR_STAGGER_MS) / BARS_MS);
            let w = (bx1 - bx0) * row.frac.clamp(0.0, 1.0) * filling;
            if w > 1.0 {
                c.fill_round_rect(bx0, by, bx0 + w.max(4.0), by + 4.0, 2.0, color(which));
            }
        }
    }

    /// O ✕ do lado virado.
    fn draw_close(&self, c: &mut Canvas, mode: Mode) {
        let (x0, y0, x1, y1) = Self::close_rect(mode);
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        c.fill_circle(cx, cy, 12.0, WHITE.alpha(0.14));
        c.line(&[(cx - 4.0, cy - 4.0), (cx + 4.0, cy + 4.0)], FG, 1.6);
        c.line(&[(cx + 4.0, cy - 4.0), (cx - 4.0, cy + 4.0)], FG, 1.6);
    }

    /// Médio virado: o recurso, quanto está em uso e os 3 apps que mais usam (ou as unidades).
    fn draw_back_m(&self, c: &mut Canvas, which: usize) {
        let y = 22.0;
        c.fill_circle(PAD + 4.0, y, 4.0, color(which));
        let nw = c.text(PAD + 14.0, y, TITLES[which], Face::Semibold, 13.5, FG, Anchor::LM, None);
        let pct = format!("{}%", (self.target[which] * 100.0).round());
        let pw = c.text(PAD + 14.0 + nw + 7.0, y, &pct, Face::Semibold, 13.5, color(which), Anchor::LM, None);
        let x = PAD + 14.0 + nw + 7.0 + pw + 8.0;
        let room = Self::close_rect(Mode::M).0 - 8.0 - x;
        c.text(x, y + 0.5, &self.amounts[which], Face::Regular, 11.5, MUTED, Anchor::LM, Some(room.max(10.0)));
        self.draw_close(c, Mode::M);
        self.draw_list(c, which, BACK_LIST_Y, BACK_N, BACK_ROW_H);
    }

    /// Pequeno virado: o recurso escolhido e os 3 apps que mais usam (ou as unidades).
    fn draw_back_p(&self, c: &mut Canvas, which: usize) {
        let pct = format!("{}%", (self.target[which] * 100.0).round());
        c.fill_circle(17.0, 22.0, 4.0, color(which));
        // o nome inteiro ("Memória", como no médio e no grande) e a porcentagem antes do ✕ (a letra
        // diminui um pouco se não couber)
        let room = Self::close_rect(Mode::P).0 - 4.0 - 26.0;
        let size = back_p_title_size(c, NAMES[which], &pct, room);
        let nw = c.text(26.0, 22.0, NAMES[which], Face::Semibold, size, FG, Anchor::LM, None);
        c.text(26.0 + nw + BACK_P_GAP, 22.0, &pct, Face::Semibold, size, color(which), Anchor::LM, None);
        self.draw_close(c, Mode::P);
        let rows = self.rows(which);
        if rows.is_empty() {
            let msg = if which == 2 { "Nenhuma unidade" } else { "Lendo os apps…" };
            c.text(SMALL / 2.0, 90.0, msg, Face::Regular, 11.5, MUTED, Anchor::MM, None);
            return;
        }
        let since = self.bars_since.elapsed().as_secs_f32() * 1000.0;
        for (index, row) in rows.iter().take(BACK_N).enumerate() {
            // as linhas sobem e aparecem uma depois da outra
            let appear = ease_out((since - index as f32 * BACK_P_STAGGER_MS) / BACK_P_RISE_MS);
            let y = 44.0 + 36.0 * index as f32 + (1.0 - appear) * 8.0;
            let icon = 22.0;
            app_icon(c, row.icon.as_deref(), &row.name, 12.0, y + 3.0, icon, appear);
            let tx = 12.0 + icon + 8.0;
            let w = SMALL - tx - 10.0;
            c.text(tx, y + 2.0, &row.name, Face::Regular, 11.5, FG.alpha(appear), Anchor::LA, Some(w));
            c.text(tx, y + 17.0, &row.value, Face::Regular, 10.5, MUTED.alpha(0.66 * appear), Anchor::LA, Some(w));
        }
    }

    /// A barra de escolha do grande (CPU, Memória, Disco), com `which` aceso.
    fn draw_seg(&self, c: &mut Canvas, which: usize) {
        c.fill_round_rect(PAD, SEG_Y, W - PAD, SEG_Y + 34.0, 10.0, WHITE.alpha(0.08));
        for i in 0..3 {
            let (x0, y0, x1, y1) = Self::seg_rect(i);
            if i == which {
                c.fill_round_rect(x0, y0, x1, y1, 8.0, WHITE.alpha(0.18));
            }
            c.text((x0 + x1) / 2.0, (y0 + y1) / 2.0, NAMES[i], if i == which { Face::Semibold } else { Face::Regular }, 12.0, if i == which { FG } else { MUTED }, Anchor::MM, None);
        }
    }

    /// O rodapé do grande: a rede (descendo e subindo) e há quanto tempo o PC está ligado.
    fn draw_footer(&self, c: &mut Canvas) {
        let y = G_FOOT_Y;
        c.hline_px(PAD, W - PAD, y, DIVIDER);
        let net = format!("↓ {}   ↑ {}", sysinfo::fmt_rate(self.rates.0), sysinfo::fmt_rate(self.rates.1));
        c.text(PAD, y + 17.0, &net, Face::Regular, 11.5, MUTED, Anchor::LM, None);
        c.text(W - PAD, y + 17.0, &format!("Ligado há {}", sysinfo::fmt_uptime(sysinfo::uptime())), Face::Regular, 11.5, MUTED, Anchor::RM, None);
    }
}

impl Kind for System {
    /// Na ilha só com a CPU ou a memória alta por um minuto (um aviso: não toma a pílula).
    fn island_item(&self, _env: &Env) -> Option<IslandItem> {
        let (which, since) = self.alarm()?;
        // um aviso novo a cada vez que passa do limite
        let id = alert_hash(&format!("{which}{:?}", since));
        Some(IslandItem::alert(color(which), id))
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, _env: &Env) {
        self.draw_island(c, form, r, hot);
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        island_buttons(form, w, h).into_iter().find(|(_, rect)| iu::inside(*rect, x, y)).map(|(action, _)| action)
    }

    fn key(&self) -> &'static str {
        "sistema"
    }
    fn title(&self) -> &'static str {
        "Sistema"
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, _env: &Env) {
        if self.mode_seen.get() != mode {
            // mudou de tamanho: o cartão desvira
            self.mode_seen.set(mode);
            self.flipped.set(None);
            self.sampler.set_active(self.wants_apps());
        }
        if let Some(which) = self.flipped.get() {
            match mode {
                Mode::P => return self.draw_back_p(c, which),
                Mode::M => return self.draw_back_m(c, which),
                Mode::G => {}
            }
        }
        self.draw_rings(c, mode);
        self.draw_metrics(c, mode);
        if mode == Mode::M {
            self.draw_header(c, mode);
        }
        if mode == Mode::G {
            self.draw_header(c, mode);
            let which = self.current(mode).unwrap_or(1);
            self.draw_seg(c, which);
            self.draw_list(c, which, G_LIST_Y, LIST_N, ROW_H);
            self.draw_footer(c);
        }
    }

    fn hit(&self, x: f32, y: f32, mode: Mode, env: &Env) -> Option<u32> {
        let inside = |(x0, y0, x1, y1): (f32, f32, f32, f32)| (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
        if mode != Mode::G && self.flipped.get().is_some() {
            // virado: um clique em qualquer lugar volta aos anéis
            return Some(CLOSE);
        }
        if mode == Mode::G {
            if let Some(i) = (0..3).find(|&i| inside(Self::seg_rect(i))) {
                return Some(SELECT + i as u32);
            }
            if y >= G_LIST_Y && y < G_FOOT_Y {
                // as unidades abrem o Explorador; os apps não abrem nada
                return (self.current(mode) == Some(2) && env.shortcuts()).then_some(OPEN_EXPLORER);
            }
        }
        let which = self.ring_at(x, y, mode).or_else(|| Self::metric_at(x, y, mode))?;
        Some(SELECT + which as u32)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            OPEN_EXPLORER => ctx.open("shell:MyComputerFolder"),
            OPEN_TASKMGR => ctx.open("taskmgr"),
            DISMISS => self.quiet_until = Some(Instant::now() + QUIET_FOR),
            CLOSE => self.flip(None),
            select if (SELECT..SELECT + 3).contains(&select) => {
                let which = (select - SELECT) as usize;
                if self.mode_seen.get() == Mode::G {
                    self.select(Some(which));
                } else {
                    self.flip(Some(which));
                }
            }
            _ => {}
        }
    }

    fn hot_rect(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<Hot> {
        if mode != Mode::G && self.flipped.get().is_some() {
            let (x0, y0, x1, y1) = Self::close_rect(mode);
            return Some((x0 - 2.0, y0 - 2.0, x1 + 2.0, y1 + 2.0, 16.0));
        }
        if mode == Mode::G && (y >= SEG_Y || y >= G_LIST_Y) {
            return None;
        }
        if self.ring_at(x, y, mode).is_some() {
            let RingsGeom { cx, cy, diameter, .. } = Self::rings_geom(mode);
            let r = diameter / 2.0 + 4.0;
            return Some((cx - r, cy - r, cx + r, cy + r, r));
        }
        if let Some(i) = Self::metric_at(x, y, mode) {
            let (x0, y0, x1, y1) = Self::metric_rect(mode, i);
            return Some((x0 - 4.0, y0, x1, y1, 10.0));
        }
        None
    }

    fn hover_style(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> super::HoverStyle {
        if self.ring_at(x, y, mode).is_some() { super::HoverStyle::Quiet } else { super::HoverStyle::Surface }
    }

    fn tooltip(&self, _x: f32, _y: f32, _mode: Mode, _env: &Env) -> Option<String> {
        None
    }

    fn menu(&self, _env: &Env) -> Vec<MenuItem> {
        Vec::new()
    }

    fn next_tick(&self) -> Option<Instant> {
        Some(self.due)
    }

    fn tick(&mut self, ctx: &mut Ctx) -> bool {
        let changed = self.step(ctx);
        self.plan();
        changed
    }

    fn status(&self) -> Status {
        self.status.clone()
    }
}

impl System {
    /// Um passo do relógio: guarda a primeira leitura dos apps quando ela chega e mede de novo
    /// na hora. Diz se o desenho mudou.
    fn step(&mut self, _ctx: &mut Ctx) -> bool {
        let now = Instant::now();
        // a primeira leitura dos apps chegou: as barras enchem a partir de agora
        if self.waiting_apps() {
            if let Some(apps) = self.sampler.latest() {
                self.apps = apps;
                self.bars_since = now;
                return true;
            }
        }
        if now >= self.next {
            self.measure();
            self.sampler.set_active(self.wants_apps());
            if self.wants_apps() {
                if let Some(apps) = self.sampler.latest() {
                    self.apps = apps;
                }
                if self.showing(self.mode_seen.get()) == Some(2) && self.drives_at.is_none_or(|read_at| read_at.elapsed() > DRIVES_EVERY) {
                    self.drives = sysinfo::drives();
                    self.drives_at = Some(now);
                }
            }
            return true;
        }
        self.animating()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O ✕ do lado virado não encosta no botão de tamanho do cartão (canto de cima à direita,
    /// centro a 5 pontos das bordas, que recebe cliques até `BADGE_D / 2 + 2`).
    #[test]
    fn close_stays_clear_of_the_size_button() {
        for mode in [Mode::P, Mode::M, Mode::G] {
            let w = if mode == Mode::P { SMALL } else { W };
            let (x0, y0, x1, y1) = System::close_rect(mode);
            let (bx, by): (f32, f32) = (w - 5.0, 5.0);
            // o ponto do ✕ mais perto do centro do botão
            let (nx, ny) = (bx.clamp(x0, x1), by.clamp(y0, y1));
            let d = ((nx - bx).powi(2) + (ny - by).powi(2)).sqrt();
            assert!(d > BADGE_D / 2.0 + 2.0 + 2.0, "{mode:?}: o ✕ fica a {d} do botão de tamanho");
        }
    }

    /// "Memória" em todos os tamanhos (o pequeno dizia "RAM"): o nome inteiro cabe na coluna do
    /// pequeno e, virado, o título com 100% cabe antes do ✕; no médio, o rótulo e as três linhas
    /// cabem na altura dos anéis.
    #[test]
    fn memory_has_one_name_and_fits() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let probe = Canvas::new(1, 1, 1.0);
        assert!(!NAMES.contains(&"RAM"));
        for i in 0..3 {
            let (x0, _, x1, _) = System::metric_rect(Mode::P, i);
            assert!(probe.text_width(NAMES[i], Face::Regular, 10.0) <= x1 - x0 - 2.0, "{} não cabe no pequeno", NAMES[i]);
            let room = System::close_rect(Mode::P).0 - 4.0 - 26.0;
            let size = back_p_title_size(&probe, NAMES[i], "100%", room);
            assert!(size >= 11.0, "{}: letra {size} no pequeno virado", NAMES[i]);
            assert!(probe.text_width(NAMES[i], Face::Semibold, size) + BACK_P_GAP + probe.text_width("100%", Face::Semibold, size) <= room);
        }
        // médio: a primeira linha começa embaixo do rótulo e a última barra termina com os anéis
        let RingsGeom { cy, diameter: d, .. } = System::rings_geom(Mode::M);
        let (_, first_top, _, _) = System::metric_rect(Mode::M, 0);
        let (_, _, _, last_bottom) = System::metric_rect(Mode::M, 2);
        assert!(first_top >= super::super::LABEL_Y + super::super::LABEL_SIZE + 2.0, "a CPU encosta no rótulo");
        let last_bar = last_bottom - 30.0 + 6.0 + 19.0 + 4.0;
        assert!(last_bar <= cy + d / 2.0 + 0.5, "a última barra passa do anel: {last_bar}");
    }

    /// As porcentagens contam junto com os anéis, de um inteiro ao outro, e param no número.
    #[test]
    fn percentages_count_with_the_rings() {
        let system = System::sample_counting(0);
        assert_eq!(system.counted(0).round(), 17.0, "começa no número de antes");
        let mid = System::sample_counting((RING_MS / 3.0) as u64).counted(0);
        assert!(mid > 17.0 && mid < 24.0, "no meio do caminho: {mid}");
        assert_eq!(System::sample_counting(5000).counted(0), 24.0, "para no número");
        assert_eq!(System::sample().counted(1), 68.0);
    }

    #[test]
    fn app_icons_are_resized_sharply() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        // um ícone de 64 px num quadrado de 26 pontos a 125% (33 px): sai igual ao ícone reduzido
        // antes com o filtro de qualidade, e não ao esticado na hora (bilinear, que serrilha)
        let icon = test_icon(64);
        let mut c = Canvas::new(40, 40, 1.25);
        app_icon(&mut c, Some(&icon), "Spotify", 0.0, 0.0, 26.0, 1.0);
        let drawn = c.finish();
        let sharp = crate::gfx::resize_photo(&icon, 33, 33);
        for y in 0..33 {
            for x in 0..33 {
                let (i, j) = (((y * drawn.w + x) * 4) as usize, ((y * 33 + x) * 4) as usize);
                assert_eq!(drawn.px[i..i + 4], sharp.px[j..j + 4], "pixel ({x}, {y})");
            }
        }
    }

    #[test]
    fn letter_tiles_keep_their_color() {
        assert_eq!(tile_color("Discord"), tile_color("discord"), "mesma cor em qualquer leitura");
        let names = ["Google Chrome", "Visual Studio Code", "Discord", "Spotify", "Steam", "Notepad"];
        let distinct: std::collections::HashSet<_> = names.iter().map(|n| { let c = tile_color(n); (c.0, c.1, c.2) }).collect();
        assert!(distinct.len() >= 3, "as cores variam entre os apps");
    }

    #[test]
    fn rings_animate_only_visible_changes() {
        let mut system = System::sample();
        system.ring_since = Instant::now() - Duration::from_secs(5);
        system.bars_since = Instant::now() - Duration::from_secs(5);
        let before = system.shown();
        // a CPU oscilou 2 pontos: vai direto, sem quadros de animação
        system.set_rings([before[0] + 0.02, before[1], before[2]]);
        assert!(!system.animating(), "tremor pequeno não anima");
        assert!((system.shown()[0] - (before[0] + 0.02)).abs() < 1e-6, "já mostra o valor novo");
        // subiu 10 pontos: anima, partindo de onde estava
        let now = system.shown();
        system.set_rings([now[0] + 0.10, now[1], now[2]]);
        assert!(system.animating(), "mudança que aparece anima");
        assert!((system.from[0] - now[0]).abs() < 1e-6);
    }

    #[test]
    #[ignore] // desenha num PNG: cargo test -- --ignored
    fn draw_sizes() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let cfg = serde_json::json!({"sistema": {}, "app": {}});
        let apps = [("Google Chrome", 6.2, 2.1), ("Visual Studio Code", 3.4, 1.2), ("Spotify", 1.1, 0.4), ("Discord", 0.6, 0.6)];
        let flips = [(Mode::M, "m", None), (Mode::G, "g", None), (Mode::P, "p", None), (Mode::P, "p_virado", Some(1)), (Mode::M, "m_virado", Some(0))];
        for (mode, name, flip) in flips {
            let mut system = System::sample();
            system.mode_seen.set(mode);
            system.flipped.set(flip);
            system.apps = apps.iter().map(|&(app, cpu, gb)| AppUse { name: app.into(), cpu, mem: (gb * 1024f64.powi(3)) as u64, icon: None }).collect();
            system.bars_since = Instant::now() - Duration::from_secs(5);
            let l = crate::widgets::Layout::of(mode);
            let mut c = Canvas::new((l.w * 1.25) as u32, (l.h * 1.25) as u32, 1.25);
            c.fill_round_rect(0.0, 0.0, l.w, l.h, l.radius, Rgba::rgb(40, 40, 50));
            system.draw(&mut c, mode, &Env { cfg: &cfg, key: "sistema" });
            let path = std::env::temp_dir().join(format!("sistema_{name}.png"));
            crate::gfx::save_png(&c.finish(), &path).unwrap();
            println!("{}", path.display());
        }
    }
}

// ---------- na ilha ----------

/// Os botões da ilha aberta: (ação, retângulo a partir do canto da pílula).
fn island_buttons(form: IslandForm, w: f32, h: f32) -> Vec<(u32, (f32, f32, f32, f32))> {
    match form {
        IslandForm::Large => vec![(OPEN_TASKMGR, (22.0, h - 46.0, 182.0, h - 14.0)), (DISMISS, (w - 172.0, h - 44.0, w - 18.0, h - 16.0))],
        _ => Vec::new(),
    }
}

/// "há 2 min" (desde quando está alto).
fn ago(since: Instant) -> String {
    let minutes = (since.elapsed().as_secs() / 60).max(1);
    if minutes < 60 { format!("há {minutes} min") } else { format!("há {} h", minutes / 60) }
}

impl System {
    /// Os apps aparecem na lista do cartão ou na ilha (com um aviso)?
    fn wants_apps(&self) -> bool {
        self.list_visible() || self.alarm().is_some()
    }

    /// Acompanha quanto tempo a CPU e a memória ficam altas.
    fn watch_high(&mut self, values: [f32; 3]) {
        for i in 0..2 {
            match self.high_since[i] {
                None if values[i] >= HIGH => self.high_since[i] = Some(Instant::now()),
                Some(_) if values[i] < CALM => self.high_since[i] = None,
                _ => {}
            }
        }
        if self.quiet_until.is_some_and(|until| Instant::now() >= until) {
            self.quiet_until = None;
        }
    }

    /// O recurso que está alto há mais de um minuto (CPU antes da memória) e desde quando.
    fn alarm(&self) -> Option<(usize, Instant)> {
        if self.quiet_until.is_some() {
            return None;
        }
        (0..2).find_map(|i| self.high_since[i].filter(|since| since.elapsed() >= HIGH_FOR).map(|since| (i, since)))
    }

    /// Os apps que mais gastam do recurso `which` (os três primeiros): nome e fração.
    fn top_apps(&self, which: usize) -> Vec<(String, f32)> {
        let total_mem = win::memory().1.max(1) as f32;
        let mut list: Vec<(String, f32)> = self.apps.iter().map(|app| (app.name.clone(), if which == 0 { app.cpu / 100.0 } else { app.mem as f32 / total_mem })).collect();
        list.sort_by(|a, b| b.1.total_cmp(&a.1));
        list.truncate(3);
        list
    }

    /// O aviso de CPU ou memória alta em cada forma da ilha: o anel, as três barrinhas, quem
    /// mais gasta e, aberta, os três anéis, a lista e os botões.
    fn draw_island(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>) {
        let (x, y, w, h) = r;
        let (i, since) = self.alarm().unwrap_or((0, Instant::now()));
        let col = color(i);
        let value = format!("{:.0}%", self.target[i] * 100.0);
        let top = self.top_apps(i);
        let who = top.first().map(|(app, _)| app.clone());
        let cy = y + h / 2.0;
        match form {
            IslandForm::Bubble => {
                drawing::ring(c, x + w / 2.0, cy, 30.0, 3.5, self.target[i], col);
                c.text(x + w / 2.0, cy, &format!("{:.0}", self.target[i] * 100.0), Face::Semibold, 10.0, FG, Anchor::MM, None);
            }
            IslandForm::Compact | IslandForm::Banner => {
                // as três barrinhas, de baixo para cima
                for bar in 0..3 {
                    let bx = x + 16.0 + bar as f32 * 7.0;
                    c.fill_round_rect(bx, cy - 10.0, bx + 4.0, cy + 10.0, 2.0, Rgba(255, 255, 255, 30));
                    let fill_h = (20.0 * self.target[bar]).max(3.0);
                    c.fill_round_rect(bx, cy + 10.0 - fill_h, bx + 4.0, cy + 10.0, 2.0, color(bar));
                }
                let label = match (&who, form) {
                    (Some(app), IslandForm::Banner) => format!("{} alta · {app}", NAMES[i]),
                    (Some(app), _) => format!("{} · {app}", NAMES[i]),
                    (None, _) => format!("{} alta", NAMES[i]),
                };
                iu::compact_text(c, x + 44.0, r, &label, &value, AMBER);
            }
            IslandForm::Medium => {
                drawing::ring(c, x + 39.0, cy, 50.0, 4.5, self.target[i], col);
                c.text(x + 39.0, cy, &value, Face::Semibold, 12.0, FG, Anchor::MM, None);
                let title = match &top.first() {
                    Some((app, share)) => format!("{app} usa {:.0}% {}", share * 100.0, if i == 0 { "da CPU" } else { "da memória" }),
                    None => format!("{} em {value}", TITLES[i]),
                };
                let lines = c.wrap(&title, Face::Semibold, 13.5, w - 96.0, 2);
                let sub = format!("Acima de {:.0}% {}", HIGH * 100.0, ago(since));
                if lines.len() > 1 {
                    c.text(x + 78.0, cy - 17.0, &lines[0], Face::Semibold, 13.5, FG, Anchor::LM, None);
                    c.text(x + 78.0, cy + 1.0, &lines[1], Face::Semibold, 13.5, FG, Anchor::LM, Some(w - 96.0));
                    c.text(x + 78.0, cy + 19.0, &sub, Face::Regular, 11.5, AMBER, Anchor::LM, None);
                } else {
                    iu::two_lines(c, x + 78.0, cy, w - 96.0, &title, &sub, AMBER);
                }
            }
            IslandForm::Large => {
                iu::header(c, r, "SISTEMA", PINK.mix(WHITE, 0.12), &format!("{} alta {}", NAMES[i], ago(since)), AMBER);
                // os três anéis
                for which in 0..3 {
                    let (rx, ry) = (x + 22.0 + which as f32 * 108.0, y + 62.0);
                    drawing::ring(c, rx + 20.0, ry, 40.0, 4.0, self.target[which], color(which));
                    c.text(rx + 20.0, ry, &format!("{:.0}", self.target[which] * 100.0), Face::Semibold, 11.0, FG, Anchor::MM, None);
                    c.text(rx + 48.0, ry, NAMES[which], Face::Regular, 12.0, if which == i { FG } else { SOFT }, Anchor::LM, None);
                }
                // quem mais gasta
                for (index, (app, share)) in top.iter().enumerate() {
                    let ry = y + 104.0 + index as f32 * 22.0;
                    c.text(x + 22.0, ry, app, Face::Regular, 12.0, FG, Anchor::LM, Some(96.0));
                    iu::bar(c, x + 126.0, x + w - 64.0, ry, *share, col);
                    c.text(x + w - 22.0, ry, &format!("{:.0}%", share * 100.0), Face::Regular, 12.0, SOFT, Anchor::RM, None);
                }
                if top.is_empty() {
                    c.text(x + 22.0, y + 104.0, "Lendo os apps…", Face::Regular, 12.0, SOFT, Anchor::LM, None);
                }
                for (action, button) in island_buttons(form, w, h) {
                    let rect = (x + button.0, y + button.1, x + button.2, y + button.3);
                    if action == OPEN_TASKMGR {
                        iu::pill_button(c, rect, "Gerenciador de Tarefas", false, hot == Some(action));
                    } else {
                        iu::link(c, x + button.2, (rect.1 + rect.3) / 2.0, "Dispensar por 1 hora", true, hot == Some(action));
                    }
                }
            }
        }
    }
}
