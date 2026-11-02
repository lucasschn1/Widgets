//! Widget Sistema: três anéis (CPU, memória e disco) no estilo dos anéis de atividade. Clicar num
//! anel mostra os apps que mais usam aquele recurso (no disco, as unidades): no pequeno e no
//! médio o cartão "vira" e mostra os 3 primeiros; no grande, a lista fica embaixo dos anéis.

use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Ctx, Env, Hot, Kind, MenuItem, Mode, Status, LARGE_H, SMALL, WIDE};
use crate::gfx::{Anchor, Canvas, Face, Image};
use crate::sysinfo::{self, AppUse, CpuSpeed, Drive, Sampler};
use crate::theme::*;
use crate::win;

const W: f32 = WIDE;
const PAD: f32 = 16.0;
const ROW_H: f32 = 38.0;
const LIST_N: usize = 5;
/// médio virado: linhas mais baixas, para caber 3 embaixo do cabeçalho
const BACK_ROW_H: f32 = 34.0;
const BACK_LIST_Y: f32 = 44.0;
const BACK_N: usize = 3;
/// grande: o cabeçalho "SISTEMA" em cima empurra o resto para baixo
const G_TOP: f32 = 24.0;
const SEG_Y: f32 = 150.0 + G_TOP;
const G_LIST_Y: f32 = 194.0 + G_TOP;
/// rodapé (rede e tempo ligado) no pé do cartão, logo depois da lista
const G_FOOT_Y: f32 = LARGE_H - 34.0;
const _: () = assert!(G_LIST_Y + ROW_H * LIST_N as f32 + 6.0 <= G_FOOT_Y, "a lista não cabe no grande");

const EVERY: Duration = Duration::from_secs(2);
const RING_MS: f32 = 550.0;
const BARS_MS: f32 = 650.0;

// ações
const SELECT: u32 = 10; // + recurso (0 CPU, 1 memória, 2 disco)
const OPEN_EXPLORER: u32 = 2;
const CLOSE: u32 = 3;

const NAMES: [&str; 3] = ["CPU", "Memória", "Disco"];
const TITLES: [&str; 3] = ["Processador", "Memória", "Armazenamento"];

fn color(i: usize) -> Rgba {
    match i {
        0 => Rgba::rgb(255, 55, 95),
        1 => Rgba::rgb(48, 209, 88),
        _ => Rgba::rgb(100, 210, 255),
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn br(v: f64, digits: usize) -> String {
    format!("{v:.digits$}").replace('.', ",")
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
    last_net: (u64, u64, Instant),
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
}

impl System {
    pub fn new() -> System {
        let mut s = System::with(Sampler::new());
        s.speed = CpuSpeed::new();
        s
    }

    fn with(sampler: Sampler) -> System {
        let mut s = System {
            target: [0.0; 3],
            from: [0.0; 3],
            ring_since: Instant::now(),
            details: Default::default(),
            amounts: Default::default(),
            speed: None,
            last_cpu: win::cpu_times(),
            last_net: (0, 0, Instant::now()),
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
        };
        let (rx, tx) = sysinfo::net_bytes();
        s.last_net = (rx, tx, Instant::now());
        s.measure();
        s
    }

    /// Exemplo para a galeria (sem ler o sistema de verdade).
    pub fn sample() -> System {
        let mut s = System::with(Sampler::idle());
        s.target = [0.24, 0.68, 0.54];
        s.from = s.target;
        s.amounts = ["2,84 GHz".into(), "10,9/16 GB".into(), "258/477 GB".into()];
        s.ring_since = Instant::now() - Duration::from_secs(5);
        s
    }

    /// Exemplo virado (pequeno ou médio) mostrando os apps de um recurso (imagens do README).
    #[cfg(test)]
    pub fn sample_flipped(which: Option<usize>, mode: Mode) -> System {
        let s = System::sample();
        s.mode_seen.set(mode);
        s.flipped.set(which);
        let mut s = s;
        let apps = [("Google Chrome", 6.2, 2.1), ("Visual Studio Code", 3.4, 1.2), ("Spotify", 1.1, 0.4), ("Discord", 0.6, 0.6)];
        s.apps = apps.iter().map(|&(n, cpu, gb)| AppUse { name: n.into(), cpu, mem: (gb * 1024f64.powi(3)) as u64, icon: None }).collect();
        s.bars_since = Instant::now() - Duration::from_secs(5);
        s.rates = (1.8e6, 2.4e5);
        s
    }

    fn measure(&mut self) {
        let (idle, total) = win::cpu_times();
        let (last_idle, last_total) = self.last_cpu;
        self.last_cpu = (idle, total);
        let d_total = total.saturating_sub(last_total);
        let cpu = if d_total > 0 { 1.0 - idle.saturating_sub(last_idle) as f64 / d_total as f64 } else { 0.0 };
        let cpu = cpu.clamp(0.0, 1.0);
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let (used, total_mem) = win::memory();
        let (dused, dtotal, _) = win::disk_usage("C:\\");
        let values = [cpu as f32, (used as f64 / total_mem as f64) as f32, (dused as f64 / dtotal.max(1) as f64) as f32];
        let ghz = self.speed.as_ref().and_then(CpuSpeed::ghz);
        let speed = ghz.map(|g| format!(" · {} GHz", br(g, 2))).unwrap_or_default();
        self.details = [
            format!("{}% em uso · {cores} núcleos{speed}", (cpu * 100.0).round()),
            format!("{} de {:.0} GB em uso", br(win::gb(used), 1), win::gb(total_mem)),
            format!("C: {:.0} de {:.0} GB em uso", win::gb(dused), win::gb(dtotal)),
        ];
        self.amounts = [
            ghz.map(|g| format!("{} GHz", br(g, 2))).unwrap_or_else(|| format!("{cores} núcleos")),
            format!("{}/{:.0} GB", br(win::gb(used), 1), win::gb(total_mem)),
            format!("{:.0}/{:.0} GB", win::gb(dused), win::gb(dtotal)),
        ];
        // os anéis partem de onde estão agora (mudança pequena: sem animação, para poupar a CPU)
        let shown = self.shown();
        let moved = (0..3).any(|i| (values[i] - shown[i]).abs() > 0.008);
        self.from = if moved { shown } else { values };
        self.target = values;
        if moved {
            self.ring_since = Instant::now();
        }
        let (rx, tx) = sysinfo::net_bytes();
        let secs = self.last_net.2.elapsed().as_secs_f64().max(0.1);
        self.rates = (rx.saturating_sub(self.last_net.0) as f64 / secs, tx.saturating_sub(self.last_net.1) as f64 / secs);
        self.last_net = (rx, tx, Instant::now());
        self.next = Instant::now() + EVERY;
    }

    fn shown(&self) -> [f32; 3] {
        let t = ease_out(self.ring_since.elapsed().as_secs_f32() * 1000.0 / RING_MS);
        let mut out = [0.0; 3];
        for i in 0..3 {
            out[i] = self.from[i] + (self.target[i] - self.from[i]) * t;
        }
        out
    }

    /// A lista de apps está na tela, mas a primeira leitura ainda não chegou.
    fn waiting_apps(&self) -> bool {
        self.list_visible() && self.apps.is_empty() && self.showing(self.mode_seen.get()) != Some(2)
    }

    fn animating(&self) -> bool {
        self.ring_since.elapsed().as_secs_f32() * 1000.0 < RING_MS || self.bars_since.elapsed().as_secs_f32() * 1000.0 < BARS_MS + 300.0
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

    /// Botão de voltar aos anéis, no cartão virado.
    fn close_rect(mode: Mode) -> (f32, f32, f32, f32) {
        let r = if mode == Mode::P { SMALL } else { W };
        (r - 38.0, 8.0, r - 10.0, 36.0)
    }

    /// (centro x, centro y, diâmetro de fora, espessura, passo entre anéis)
    fn rings_geom(mode: Mode) -> (f32, f32, f32, f32, f32) {
        match mode {
            Mode::P => (SMALL / 2.0, 62.0, 100.0, 10.5, 24.0),
            Mode::M => (PAD + 60.0, 75.0, 120.0, 13.0, 30.0),
            Mode::G => (PAD + 60.0, 75.0 + G_TOP, 120.0, 13.0, 30.0),
        }
    }

    fn ring_at(&self, x: f32, y: f32, mode: Mode) -> Option<usize> {
        let (cx, cy, d, wd, step) = Self::rings_geom(mode);
        let r = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        if r > d / 2.0 + 3.0 {
            return None;
        }
        // anel 0 por fora; cada anel tem "step/2" de raio
        let i = ((d / 2.0 - r + (step / 2.0 - wd) / 2.0) / (step / 2.0)).floor().max(0.0) as usize;
        Some(i.min(2))
    }

    /// Retângulo da linha (médio/grande) ou coluna (pequeno) de cada recurso.
    fn metric_rect(mode: Mode, i: usize) -> (f32, f32, f32, f32) {
        if mode == Mode::P {
            let col = (SMALL - 16.0) / 3.0;
            (8.0 + col * i as f32, 118.0, 8.0 + col * (i + 1) as f32, SMALL - 6.0)
        } else {
            let top = if mode == Mode::G { G_TOP } else { 0.0 };
            let y = 22.0 + top + 36.0 * i as f32;
            (148.0, y - 6.0, W - 10.0, y + 30.0)
        }
    }

    fn metric_at(x: f32, y: f32, mode: Mode) -> Option<usize> {
        (0..3).find(|&i| {
            let (x0, y0, x1, y1) = Self::metric_rect(mode, i);
            (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
        })
    }

    fn seg_rect(i: usize) -> (f32, f32, f32, f32) {
        let w = (W - 2.0 * PAD - 6.0) / 3.0;
        let x = PAD + 3.0 + w * i as f32;
        (x, SEG_Y + 3.0, x + w, SEG_Y + 31.0)
    }

    fn rows(&self, which: usize) -> Vec<Row> {
        match which {
            2 => self
                .drives
                .iter()
                .take(LIST_N)
                .map(|d| Row {
                    name: d.label.clone(),
                    value: format!("{:.0} de {:.0} GB", win::gb(d.used), win::gb(d.total)),
                    frac: d.used as f32 / d.total.max(1) as f32,
                    icon: d.icon.clone(),
                })
                .collect(),
            _ => {
                let mut apps: Vec<&AppUse> = self.apps.iter().collect();
                if which == 0 {
                    apps.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
                } else {
                    apps.sort_by(|a, b| b.mem.cmp(&a.mem));
                }
                let apps: Vec<&AppUse> = apps.into_iter().take(LIST_N).collect();
                let max = apps.first().map(|a| if which == 0 { a.cpu as f64 } else { a.mem as f64 }).unwrap_or(1.0).max(1e-6);
                apps.into_iter()
                    .map(|a| {
                        let v = if which == 0 { a.cpu as f64 } else { a.mem as f64 };
                        Row {
                            name: a.name.clone(),
                            value: if which == 0 { format!("{}%", br(a.cpu as f64, 1)) } else { sysinfo::fmt_bytes(a.mem) },
                            frac: (v / max) as f32,
                            icon: a.icon.clone(),
                        }
                    })
                    .collect()
            }
        }
    }

    /// Vira (ou desvira, com None) o pequeno ou o médio para mostrar os apps de um recurso.
    fn flip(&mut self, i: Option<usize>) {
        self.flipped.set(i);
        self.refresh_list(i);
    }

    fn select(&mut self, i: Option<usize>) {
        self.selected = i;
        self.refresh_list(i);
    }

    /// A lista vai aparecer: as barras enchem de novo e a leitura dos apps liga (ou desliga).
    fn refresh_list(&mut self, i: Option<usize>) {
        self.bars_since = Instant::now();
        if i == Some(2) || self.drives_at.is_none() {
            self.drives = sysinfo::drives();
            self.drives_at = Some(Instant::now());
        }
        self.sampler.set_active(self.list_visible());
        if let Some(apps) = self.sampler.latest() {
            self.apps = apps;
        }
        // a leitura guardada pode ser antiga: a próxima vem logo
        self.next = self.next.min(Instant::now() + Duration::from_millis(900));
        self.due = Instant::now();
    }

    fn plan(&mut self) {
        let now = Instant::now();
        self.due = if self.animating() {
            now + Duration::from_millis(33)
        } else if self.waiting_apps() {
            now + Duration::from_millis(200)
        } else {
            self.next
        };
    }

    // ---------- desenho ----------

    fn draw_rings(&self, c: &mut Canvas, mode: Mode) {
        let (cx, cy, d, wd, step) = Self::rings_geom(mode);
        let shown = self.shown();
        let sel = self.current(mode);
        for (i, v) in shown.iter().enumerate() {
            let dim = sel.is_some_and(|s| s != i);
            let col = if dim { color(i).alpha(0.32) } else { color(i) };
            let di = d - step * i as f32;
            let rc = di / 2.0 - wd / 2.0;
            c.stroke_circle(cx, cy, rc, color(i).alpha(if dim { 0.1 } else { 0.22 }), wd);
            if *v > 0.003 {
                let end = -90.0 + 360.0 * v.min(0.999);
                c.arc(cx, cy, rc, -90.0, end, col, wd);
                if !dim {
                    // a ponta brilha um pouco, como nos anéis de atividade
                    let a = end.to_radians();
                    let (tx, ty) = (cx + rc * a.cos(), cy + rc * a.sin());
                    c.fill_circle(tx, ty, wd * 0.78, color(i).alpha(0.2));
                    c.fill_circle(tx, ty, wd * 0.48, color(i).mix(WHITE, 0.35));
                }
            }
        }
    }

    /// Cabeçalho do grande: "SISTEMA" na cor da CPU e os núcleos à direita.
    fn draw_header(&self, c: &mut Canvas) {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        c.text(PAD, 12.0, "SISTEMA", Face::Semibold, 11.0, Rgba::rgb(255, 107, 139), Anchor::LA, None);
        c.text(W - PAD, 12.0, &format!("{cores} núcleos"), Face::Regular, 11.5, MUTED, Anchor::RA, None);
    }

    fn draw_metrics(&self, c: &mut Canvas, mode: Mode) {
        let shown = self.shown();
        let sel = self.current(mode);
        for i in 0..3 {
            let (x0, y0, x1, _) = Self::metric_rect(mode, i);
            let pct = format!("{}%", (self.target[i] * 100.0).round());
            let dim = sel.is_some_and(|s| s != i) && mode != Mode::G;
            let text = if dim { MUTED } else { FG };
            if mode == Mode::P {
                let cx = (x0 + x1) / 2.0;
                let _ = text;
                c.text(cx, y0 + 8.0, &pct, Face::Semibold, 13.5, color(i), Anchor::MM, None);
                let label = if i == 1 { "RAM" } else { NAMES[i] };
                c.text(cx, y0 + 23.0, label, Face::Regular, 10.0, MUTED, Anchor::MM, None);
            } else {
                let y = y0 + 6.0;
                c.fill_circle(x0 + 4.0, y + 7.0, 3.5, color(i));
                let pw = c.text(x1 - 6.0, y + 7.0, &pct, Face::Semibold, 13.5, text, Anchor::RM, None);
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

    fn draw_list(&self, c: &mut Canvas, which: usize, y0: f32, n_max: usize, row_h: f32) {
        let rows = self.rows(which);
        if rows.is_empty() {
            let msg = if which == 2 { "Nenhuma unidade encontrada" } else { "Lendo os apps…" };
            c.text(W / 2.0, y0 + 40.0, msg, Face::Regular, 12.0, MUTED, Anchor::MM, None);
            return;
        }
        let since = self.bars_since.elapsed().as_secs_f32() * 1000.0;
        for (n, row) in rows.iter().take(n_max).enumerate() {
            let y = y0 + row_h * n as f32;
            let icon = 26.0;
            let ix = PAD;
            match &row.icon {
                Some(img) => {
                    let s = c.s;
                    c.image_px(img, (ix * s).round(), ((y + 5.0) * s).round(), (icon * s).round(), (icon * s).round(), 1.0);
                }
                None => {
                    c.fill_round_rect(ix, y + 5.0, ix + icon, y + 5.0 + icon, 7.0, WHITE.alpha(0.16));
                    let letter: String = row.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                    c.text(ix + icon / 2.0, y + 5.0 + icon / 2.0, &letter, Face::Semibold, 12.0, FG, Anchor::MM, None);
                }
            }
            let tx = ix + icon + 10.0;
            let vw = c.text(W - PAD, y + 12.0, &row.value, Face::Regular, 12.0, MUTED, Anchor::RM, None);
            c.text(tx, y + 12.0, &row.name, Face::Regular, 12.5, FG, Anchor::LM, Some(W - PAD - tx - vw - 10.0));
            let (bx0, bx1, by) = (tx, W - PAD, y + 24.0);
            c.fill_round_rect(bx0, by, bx1, by + 4.0, 2.0, WHITE.alpha(0.1));
            // as barras enchem uma depois da outra
            let t = ease_out((since - n as f32 * 55.0) / BARS_MS);
            let w = (bx1 - bx0) * row.frac.clamp(0.0, 1.0) * t;
            if w > 1.0 {
                c.fill_round_rect(bx0, by, bx0 + w.max(4.0), by + 4.0, 2.0, color(which));
            }
        }
    }

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
        let name = if which == 1 { "RAM" } else { NAMES[which] };
        let nw = c.text(26.0, 22.0, name, Face::Semibold, 12.5, FG, Anchor::LM, None);
        c.text(26.0 + nw + 6.0, 22.0, &pct, Face::Semibold, 12.5, color(which), Anchor::LM, None);
        self.draw_close(c, Mode::P);
        let rows = self.rows(which);
        if rows.is_empty() {
            let msg = if which == 2 { "Nenhuma unidade" } else { "Lendo os apps…" };
            c.text(SMALL / 2.0, 90.0, msg, Face::Regular, 11.5, MUTED, Anchor::MM, None);
            return;
        }
        let since = self.bars_since.elapsed().as_secs_f32() * 1000.0;
        for (n, row) in rows.iter().take(BACK_N).enumerate() {
            // as linhas sobem e aparecem uma depois da outra
            let t = ease_out((since - n as f32 * 60.0) / 380.0);
            let y = 44.0 + 36.0 * n as f32 + (1.0 - t) * 8.0;
            let icon = 22.0;
            match &row.icon {
                Some(img) => {
                    let s = c.s;
                    c.image_px(img, (12.0 * s).round(), ((y + 3.0) * s).round(), (icon * s).round(), (icon * s).round(), t);
                }
                None => {
                    c.fill_round_rect(12.0, y + 3.0, 12.0 + icon, y + 3.0 + icon, 6.0, WHITE.alpha(0.16 * t));
                    let letter: String = row.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                    c.text(12.0 + icon / 2.0, y + 3.0 + icon / 2.0, &letter, Face::Semibold, 11.0, FG.alpha(t), Anchor::MM, None);
                }
            }
            let tx = 12.0 + icon + 8.0;
            let w = SMALL - tx - 10.0;
            c.text(tx, y + 2.0, &row.name, Face::Regular, 11.5, FG.alpha(t), Anchor::LA, Some(w));
            c.text(tx, y + 17.0, &row.value, Face::Regular, 10.5, MUTED.alpha(0.66 * t), Anchor::LA, Some(w));
        }
    }

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

    fn draw_footer(&self, c: &mut Canvas) {
        let y = G_FOOT_Y;
        c.hline_px(PAD, W - PAD, y, DIVIDER);
        let net = format!("↓ {}   ↑ {}", sysinfo::fmt_rate(self.rates.0), sysinfo::fmt_rate(self.rates.1));
        c.text(PAD, y + 17.0, &net, Face::Regular, 11.5, MUTED, Anchor::LM, None);
        c.text(W - PAD, y + 17.0, &format!("Ligado há {}", sysinfo::fmt_uptime(sysinfo::uptime())), Face::Regular, 11.5, MUTED, Anchor::RM, None);
    }
}

impl Kind for System {
    fn key(&self) -> &'static str {
        "sistema"
    }
    fn title(&self) -> &'static str {
        "Sistema"
    }
    fn default_mode(&self) -> Mode {
        Mode::M
    }

    fn draw(&self, c: &mut Canvas, mode: Mode, _env: &Env) {
        if self.mode_seen.get() != mode {
            // mudou de tamanho: o cartão desvira
            self.mode_seen.set(mode);
            self.flipped.set(None);
            self.sampler.set_active(self.list_visible());
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
        if mode == Mode::G {
            self.draw_header(c);
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
        let i = self.ring_at(x, y, mode).or_else(|| Self::metric_at(x, y, mode))?;
        Some(SELECT + i as u32)
    }

    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        match action {
            OPEN_EXPLORER => ctx.open("shell:MyComputerFolder"),
            CLOSE => self.flip(None),
            a if (SELECT..SELECT + 3).contains(&a) => {
                let i = (a - SELECT) as usize;
                if self.mode_seen.get() == Mode::G {
                    self.select(Some(i));
                } else {
                    self.flip(Some(i));
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
            let (cx, cy, d, ..) = Self::rings_geom(mode);
            let r = d / 2.0 + 4.0;
            return Some((cx - r, cy - r, cx + r, cy + r, r));
        }
        if let Some(i) = Self::metric_at(x, y, mode) {
            let (x0, y0, x1, y1) = Self::metric_rect(mode, i);
            return Some((x0 - 4.0, y0, x1, y1, 10.0));
        }
        None
    }

    fn tooltip(&self, x: f32, y: f32, mode: Mode, _env: &Env) -> Option<String> {
        if mode == Mode::G && y >= SEG_Y {
            return None;
        }
        if mode != Mode::G && self.flipped.get().is_some() {
            return Some("Clique para voltar aos anéis".into());
        }
        let i = self.ring_at(x, y, mode).or_else(|| Self::metric_at(x, y, mode))?;
        let action = if i == 2 { "Clique para ver as unidades" } else { "Clique para ver os apps que mais usam" };
        Some(format!("{}  ·  {}%\n{}\n{action}", TITLES[i], (self.target[i] * 100.0).round(), self.details[i]))
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
            if self.list_visible() {
                if let Some(apps) = self.sampler.latest() {
                    self.apps = apps;
                }
                if self.showing(self.mode_seen.get()) == Some(2) && self.drives_at.is_none_or(|t| t.elapsed() > Duration::from_secs(30)) {
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
            let s = System::sample();
            s.mode_seen.set(mode);
            s.flipped.set(flip);
            let mut s = s;
            s.apps = apps.iter().map(|&(n, cpu, gb)| AppUse { name: n.into(), cpu, mem: (gb * 1024f64.powi(3)) as u64, icon: None }).collect();
            s.bars_since = Instant::now() - Duration::from_secs(5);
            let l = crate::widgets::Layout::of(mode);
            let mut c = Canvas::new((l.w * 1.25) as u32, (l.h * 1.25) as u32, 1.25);
            c.fill_round_rect(0.0, 0.0, l.w, l.h, l.radius, Rgba::rgb(40, 40, 50));
            s.draw(&mut c, mode, &Env { cfg: &cfg, key: "sistema" });
            let path = std::env::temp_dir().join(format!("sistema_{name}.png"));
            crate::gfx::save_png(&c.finish(), &path).unwrap();
            println!("{}", path.display());
        }
    }
}
