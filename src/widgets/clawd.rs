//! O Clawd, o caranguejo do Claude Code, em pixel art, com animações curtas.
//!
//! O desenho é o do logo que o Claude Code mostra no terminal (blocos ▐▛███▜▌), passado para
//! uma grade de 18 × 10 quadradinhos: o corpo, dois olhos, uma garra de cada lado e quatro
//! patinhas. Cada quadradinho cai em pixels inteiros da tela, então ele fica nítido em qualquer
//! escala, sem borrar.
//!
//! As animações tocam em episódios de um ou dois segundos (piscar, olhar para os lados, andar de
//! lado, pular, acenar as garras...), com intervalos parados entre eles: parado, o widget não
//! redesenha nada. O humor muda o repertório: dormindo sem números, suando perto do limite.

use std::time::{Duration, Instant};

use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::*;

/// A grade do desenho, em quadradinhos.
pub const COLS: f32 = 18.0;
pub const ROWS: f32 = 10.0;

/// A cor do Clawd no Claude Code e a dos olhos.
const BODY: Rgba = Rgba::rgb(215, 119, 87);
const EYE: Rgba = Rgba::rgb(28, 20, 18);
const SWEAT: Rgba = Rgba::rgb(120, 196, 255);

/// Quadros por segundo enquanto anima (pixel art não precisa de mais).
const FPS: u64 = 12;

/// Como o Clawd está: muda as animações que ele faz.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mood {
    Normal,
    /// sem números para mostrar (não conectado ou esperando o Claude Code): dorme
    Sleepy,
    /// o diário perto do limite: sua e treme
    Worried,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Anim {
    Blink,
    Look,
    Scuttle,
    Hop,
    Wave,
    Snooze,
    Sweat,
    Shake,
}

#[cfg(test)]
pub const ANIMS: [Anim; 8] = [Anim::Blink, Anim::Look, Anim::Scuttle, Anim::Hop, Anim::Wave, Anim::Snooze, Anim::Sweat, Anim::Shake];

impl Anim {
    /// Duração, em segundos.
    fn secs(self) -> f32 {
        match self {
            Anim::Blink => 0.2,
            Anim::Look => 1.8,
            Anim::Scuttle => 2.0,
            Anim::Hop => 0.8,
            Anim::Wave => 1.4,
            Anim::Snooze => 2.8,
            Anim::Sweat => 1.6,
            Anim::Shake => 0.8,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Eyes {
    #[default]
    Open,
    Closed,
    /// olhando para um lado: (-1, 0, 1) na horizontal e na vertical
    Look(i32, i32),
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Legs {
    #[default]
    Stand,
    /// andando: um par de patas levantado
    StepA,
    StepB,
    /// no ar, encolhidas
    Tucked,
}

/// Um quadro do Clawd.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Pose {
    /// deslocamento, em quadradinhos (inteiros: o desenho não sai da grade)
    pub dx: i32,
    pub dy: i32,
    pub eyes: Eyes,
    pub legs: Legs,
    pub left_up: bool,
    pub right_up: bool,
    /// gota de suor descendo (0 a 1)
    pub sweat: Option<f32>,
    /// "z" subindo (0 a 1)
    pub zzz: Option<f32>,
}

/// Para onde olhar, vendo algo em (x, y) a partir do Clawd em (cx, cy) (em pontos): de lado
/// só se estiver longe o bastante, para os olhos não ficarem trocando à toa.
pub fn gaze(cx: f32, cy: f32, x: f32, y: f32) -> (i32, i32) {
    let step = |d: f32, dead: f32| if d < -dead { -1 } else if d > dead { 1 } else { 0 };
    (step(x - cx, 14.0), step(y - cy, 12.0))
}

/// O quadro de uma animação `t` (0 a 1) depois de começar, com o humor de agora e, se o mouse
/// está por perto, olhando para ele (`gaze`).
pub fn pose(mood: Mood, anim: Option<(Anim, f32)>, gaze: Option<(i32, i32)>) -> Pose {
    let eyes = match (mood, gaze) {
        (_, Some((x, y))) => Eyes::Look(x, y),
        (Mood::Sleepy, None) => Eyes::Closed,
        _ => Eyes::Open,
    };
    let mut p = Pose { eyes, ..Pose::default() };
    if mood == Mood::Worried {
        p.sweat = Some(0.0); // a gota fica na testa
    }
    let Some((anim, t)) = anim else { return p };
    let t = t.clamp(0.0, 1.0);
    match anim {
        Anim::Blink => p.eyes = Eyes::Closed,
        Anim::Look => {
            p.eyes = match t {
                t if t < 0.4 => Eyes::Look(-1, 0),
                t if t < 0.5 => Eyes::Open,
                t if t < 0.9 => Eyes::Look(1, 0),
                _ => Eyes::Open,
            }
        }
        Anim::Scuttle => {
            // de lado, como caranguejo: vai para a direita, volta e passa um pouco para a esquerda
            let step = (t * 10.0) as i32;
            p.dx = [0, 1, 2, 3, 3, 2, 1, 0, -1, -1, 0][step.clamp(0, 10) as usize];
            p.legs = if step % 2 == 0 { Legs::StepA } else { Legs::StepB };
        }
        Anim::Hop => {
            // dois pulinhos
            let h = (t * 2.0).fract();
            p.dy = -((h * std::f32::consts::PI).sin() * 3.0).round() as i32;
            p.legs = if p.dy < 0 { Legs::Tucked } else { Legs::Stand };
            p.left_up = p.dy <= -2;
            p.right_up = p.dy <= -2;
        }
        Anim::Wave => {
            let k = (t * 6.0) as i32;
            p.left_up = k % 2 == 0;
            p.right_up = k % 2 == 1;
        }
        Anim::Snooze => {
            p.eyes = Eyes::Closed;
            p.zzz = Some(t);
        }
        Anim::Sweat => p.sweat = Some(t),
        Anim::Shake => {
            p.dx = if (t * 12.0) as i32 % 2 == 0 { 1 } else { -1 };
            p.eyes = Eyes::Closed;
        }
    }
    p
}

/// Os retângulos do quadro, em quadradinhos: (x, y, largura, altura, cor).
pub fn cells(p: &Pose) -> Vec<(i32, i32, i32, i32, Rgba)> {
    let mut v = Vec::new();
    let mut put = |x: i32, y: i32, w: i32, h: i32, c: Rgba| v.push((x + p.dx, y + p.dy, w, h, c));
    // corpo
    put(3, 0, 12, 8, BODY);
    // garras: dos lados do meio do corpo; levantadas, sobem até a altura dos olhos
    put(1, if p.left_up { 2 } else { 4 }, 2, 2, BODY);
    put(15, if p.right_up { 2 } else { 4 }, 2, 2, BODY);
    // olhos: abertos, um quadradinho de largura e dois de altura; fechados, um tracinho deitado
    match p.eyes {
        Eyes::Closed => {
            put(4, 3, 2, 1, EYE);
            put(12, 3, 2, 1, EYE);
        }
        eyes => {
            let (sx, sy) = match eyes {
                Eyes::Look(x, y) => (x.clamp(-1, 1), y.clamp(-1, 1)),
                _ => (0, 0),
            };
            put(5 + sx, 2 + sy, 1, 2, EYE);
            put(12 + sx, 2 + sy, 1, 2, EYE);
        }
    }
    // patas
    for (i, x) in [4, 6, 11, 13].into_iter().enumerate() {
        let h = match p.legs {
            Legs::Stand => 2,
            Legs::Tucked => 1,
            Legs::StepA => 1 + (i % 2 == 0) as i32,
            Legs::StepB => 1 + (i % 2 == 1) as i32,
        };
        put(x, 8, 1, h, BODY);
    }
    // gota de suor, do lado da cabeça
    if let Some(t) = p.sweat {
        put(16, (t * 2.0).round() as i32 - 1, 1, 2, SWEAT);
    }
    v
}

/// Tamanho de um quadradinho, em pontos, para algo de `unit` pontos: um número inteiro de
/// pixels da tela (o desenho fica nítido).
pub fn unit_for(c: &Canvas, unit: f32) -> f32 {
    (unit * c.s).round().max(1.0) / c.s
}

/// Desenha o Clawd centrado em (cx, cy), com quadradinhos de mais ou menos `unit` pontos.
/// Devolve a largura usada.
pub fn draw(c: &mut Canvas, cx: f32, cy: f32, unit: f32, p: &Pose) -> f32 {
    let s = c.s;
    let u = unit_for(c, unit);
    let (w, h) = (COLS * u, ROWS * u);
    let snap = |v: f32| (v * s).round() / s;
    let (x0, y0) = (snap(cx - w / 2.0), snap(cy - h / 2.0));
    for (x, y, cw, ch, col) in cells(p) {
        let (ax, ay) = (x0 + x as f32 * u, y0 + y as f32 * u);
        c.fill_round_rect(ax, ay, ax + cw as f32 * u, ay + ch as f32 * u, 0.0, col);
    }
    if let Some(t) = p.zzz {
        // três "z" subindo e sumindo, um depois do outro, do lado direito da cabeça
        for k in 0..3 {
            let local = (t * 1.6 - k as f32 * 0.3).clamp(0.0, 1.0);
            if local <= 0.0 || local >= 1.0 {
                continue;
            }
            // saem do lado da cabeça e sobem pouco (sem passar por cima do que há acima dele)
            let size = u * (2.6 + 0.8 * k as f32);
            let x = x0 + w + u * (0.5 + 2.2 * k as f32 + 1.5 * local);
            let y = y0 + u * (2.0 - 4.0 * local) - k as f32 * u * 1.2;
            let alpha = (1.0 - local) * 0.9;
            c.text(x, y, "z", Face::Semibold, size, FG.alpha(alpha), Anchor::MM, None);
        }
    }
    w
}

/// Quando e o que o Clawd anima.
pub struct Crab {
    anim: Option<(Anim, Instant)>,
    next: Instant,
    seed: u64,
}

impl Default for Crab {
    fn default() -> Self {
        Crab::new()
    }
}

impl Crab {
    pub fn new() -> Crab {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7) | 1;
        Crab { anim: None, next: Instant::now() + Duration::from_secs(3), seed }
    }

    /// O mesmo, já no meio de uma animação (para as imagens e os testes).
    #[cfg(test)]
    pub fn playing(anim: Anim, t: f32) -> Crab {
        let mut c = Crab::new();
        c.anim = Some((anim, Instant::now() - Duration::from_secs_f32(anim.secs() * t)));
        c
    }

    fn random(&mut self) -> u64 {
        // xorshift: basta para sortear a próxima animação
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// A animação tocando agora e quanto dela já foi (0 a 1).
    fn current(&self) -> Option<(Anim, f32)> {
        let (anim, start) = self.anim?;
        let t = start.elapsed().as_secs_f32() / anim.secs();
        (t < 1.0).then_some((anim, t))
    }

    pub fn animating(&self) -> bool {
        self.current().is_some()
    }

    pub fn pose(&self, mood: Mood, gaze: Option<(i32, i32)>) -> Pose {
        pose(mood, self.current(), gaze)
    }

    /// Quando acordar: o próximo quadro, se está animando; senão, a próxima animação.
    pub fn next_tick(&self) -> Instant {
        if self.animating() {
            Instant::now() + Duration::from_millis(1000 / FPS)
        } else {
            self.next
        }
    }

    /// Avança; devolve true se precisa redesenhar.
    pub fn tick(&mut self, mood: Mood) -> bool {
        let now = Instant::now();
        if self.anim.is_some() {
            if !self.animating() {
                // acabou: volta à pose parada e espera um pouco até a próxima
                self.anim = None;
                let (lo, hi) = if mood == Mood::Sleepy { (5, 9) } else { (7, 16) };
                self.next = now + Duration::from_secs(lo + self.random() % (hi - lo));
            }
            return true;
        }
        if now >= self.next {
            let anim = self.pick(mood);
            self.anim = Some((anim, now));
            return true;
        }
        false
    }

    /// Toca `anim` agora (um clique nele, o mouse chegando perto), mesmo no meio de outra.
    pub fn play(&mut self, anim: Anim) {
        self.anim = Some((anim, Instant::now()));
    }

    /// O mouse entrou no widget: uma animação na hora (se já não está animando).
    pub fn poke(&mut self, mood: Mood) {
        if self.animating() {
            return;
        }
        let anim = match mood {
            Mood::Sleepy => Anim::Hop, // acorda num pulo
            Mood::Worried => Anim::Shake,
            Mood::Normal => [Anim::Wave, Anim::Hop][(self.random() % 2) as usize],
        };
        self.anim = Some((anim, Instant::now()));
    }

    fn pick(&mut self, mood: Mood) -> Anim {
        match mood {
            Mood::Sleepy => Anim::Snooze,
            Mood::Worried => [Anim::Sweat, Anim::Shake, Anim::Sweat, Anim::Blink][(self.random() % 4) as usize],
            // piscar é o mais comum; pular e acenar, os mais raros
            Mood::Normal => [Anim::Blink, Anim::Blink, Anim::Blink, Anim::Look, Anim::Look, Anim::Scuttle, Anim::Scuttle, Anim::Hop, Anim::Wave][(self.random() % 9) as usize],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os quadradinhos ocupados (sem cor), para comparar desenhos.
    fn grid(p: &Pose) -> std::collections::BTreeSet<(i32, i32)> {
        let mut set = std::collections::BTreeSet::new();
        for (x, y, w, h, _) in cells(p) {
            for i in x..x + w {
                for j in y..y + h {
                    set.insert((i, j));
                }
            }
        }
        set
    }

    #[test]
    fn standing_clawd_is_the_terminal_logo() {
        // ▐▛███▜▌ / ▝▜█████▛▘ / ▘▘ ▝▝ em quadradinhos (cada bloco do terminal tem 1 × 2)
        let p = pose(Mood::Normal, None, None);
        let cells = cells(&p);
        let body: Vec<_> = cells.iter().filter(|c| c.4 == BODY).collect();
        let g: std::collections::BTreeSet<_> = body.iter().flat_map(|&&(x, y, w, h, _)| (x..x + w).flat_map(move |i| (y..y + h).map(move |j| (i, j)))).collect();
        assert!(g.contains(&(3, 0)) && g.contains(&(14, 0)) && !g.contains(&(2, 0)) && !g.contains(&(15, 0)), "cabeça de 3 a 14");
        assert!(g.contains(&(1, 4)) && g.contains(&(16, 5)) && !g.contains(&(0, 4)) && !g.contains(&(17, 4)), "garras de 1 a 16");
        for x in [4, 6, 11, 13] {
            assert!(g.contains(&(x, 8)) && g.contains(&(x, 9)), "pata em {x}");
        }
        assert!(!g.contains(&(5, 9)) && !g.contains(&(12, 9)));
        // os olhos, escuros, no lugar dos buracos do logo
        let eyes: Vec<_> = cells.iter().filter(|c| c.4 == EYE).map(|c| (c.0, c.1, c.3)).collect();
        assert_eq!(eyes, vec![(5, 2, 2), (12, 2, 2)]);
        // e tudo dentro da grade
        assert!(g.iter().all(|&(x, y)| (0..COLS as i32).contains(&x) && (0..ROWS as i32).contains(&y)));
    }

    #[test]
    fn every_animation_changes_the_drawing_and_ends() {
        // as de cada humor: as que ele sorteia e a do mouse entrando
        let uses = [
            (Mood::Normal, vec![Anim::Blink, Anim::Look, Anim::Scuttle, Anim::Hop, Anim::Wave]),
            (Mood::Sleepy, vec![Anim::Snooze, Anim::Look]),
            (Mood::Worried, vec![Anim::Sweat, Anim::Shake, Anim::Blink]),
        ];
        for (mood, anims) in uses {
            let still = pose(mood, None, None);
            for anim in anims {
                // em algum momento da animação o desenho é outro
                let moves = (1..20).any(|k| {
                    let p = pose(mood, Some((anim, k as f32 / 20.0)), None);
                    p != still || grid(&p) != grid(&still)
                });
                assert!(moves, "{anim:?} com humor {mood:?} não mexe em nada");
                // e nada sai muito da grade (cabe no espaço reservado em volta do desenho)
                for k in 0..=20 {
                    let p = pose(mood, Some((anim, k as f32 / 20.0)), None);
                    assert!(grid(&p).iter().all(|&(x, y)| (-2..COLS as i32 + 2).contains(&x) && (-4..ROWS as i32 + 1).contains(&y)), "{anim:?} sai da grade");
                }
            }
        }
    }

    #[test]
    fn rests_between_animations() {
        let mut crab = Crab::new();
        assert!(!crab.animating(), "começa parado");
        assert!(!crab.tick(Mood::Normal), "parado não redesenha");
        assert!(crab.next_tick() > Instant::now() + Duration::from_secs(1), "a primeira animação espera um pouco");
        // o mouse entrou: anima na hora, num ritmo de quadros
        crab.poke(Mood::Normal);
        assert!(crab.animating());
        assert!(crab.next_tick() <= Instant::now() + Duration::from_millis(1000 / FPS));
        assert!(crab.tick(Mood::Normal));
        // acabou: um último desenho, parado, e a próxima fica para daqui a alguns segundos
        crab.anim = Some((Anim::Blink, Instant::now() - Duration::from_secs(5)));
        assert!(crab.tick(Mood::Normal));
        assert!(!crab.animating());
        let wait = crab.next_tick().saturating_duration_since(Instant::now());
        assert!(wait >= Duration::from_secs(6), "descansa entre animações: {wait:?}");
    }

    #[test]
    fn moods_pick_their_own_animations() {
        let mut crab = Crab::new();
        for _ in 0..50 {
            assert_eq!(crab.pick(Mood::Sleepy), Anim::Snooze);
            assert!([Anim::Sweat, Anim::Shake, Anim::Blink].contains(&crab.pick(Mood::Worried)));
            assert!(![Anim::Snooze, Anim::Sweat, Anim::Shake].contains(&crab.pick(Mood::Normal)));
        }
    }

    /// Folha com os quadros de cada animação, ampliada (%TEMP%\clawd.png):
    ///   cargo test clawd_sheet -- --ignored --nocapture
    #[test]
    #[ignore]
    fn clawd_sheet() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let rows: Vec<(Mood, Option<Anim>)> = std::iter::once((Mood::Normal, None))
            .chain([Anim::Blink, Anim::Look, Anim::Scuttle, Anim::Hop, Anim::Wave].map(|a| (Mood::Normal, Some(a))))
            .chain([(Mood::Sleepy, None), (Mood::Sleepy, Some(Anim::Snooze)), (Mood::Worried, Some(Anim::Sweat)), (Mood::Worried, Some(Anim::Shake))])
            .collect();
        let (cell_w, cell_h, frames) = (90.0, 70.0, 8);
        let mut c = Canvas::new((cell_w * frames as f32) as u32, (cell_h * rows.len() as f32) as u32, 1.0);
        c.fill_round_rect(0.0, 0.0, c.w as f32, c.h as f32, 0.0, Rgba::rgb(60, 50, 90));
        for (r, (mood, anim)) in rows.iter().enumerate() {
            for f in 0..frames {
                let t = f as f32 / (frames - 1) as f32;
                let p = pose(*mood, anim.map(|a| (a, t)), None);
                draw(&mut c, cell_w * (f as f32 + 0.5), cell_h * (r as f32 + 0.6), 4.0, &p);
            }
        }
        let path = std::env::temp_dir().join("clawd.png");
        crate::gfx::save_png(&c.finish(), &path).unwrap();
        println!("{}", path.display());
    }

    #[test]
    fn drawn_on_whole_pixels() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        // a 125%, um quadradinho de "1,2 pontos" vira 2 pixels inteiros: as bordas não borram,
        // então cada pixel do desenho é ou laranja puro ou fundo puro
        let mut c = Canvas::new(60, 40, 1.25);
        draw(&mut c, 24.0, 16.0, 1.2, &pose(Mood::Normal, None, None));
        let img = c.finish();
        let mut orange = 0;
        for p in img.px.chunks(4) {
            if p[3] == 0 {
                continue;
            }
            assert_eq!(p[3], 255, "pixel meio transparente na borda: {p:?}");
            orange += 1;
        }
        assert!(orange > 200);
    }
}
