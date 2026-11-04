//! O Clawd, o caranguejo do Claude Code, em pixel art, com animações curtas.
//!
//! O desenho é o do logo que o Claude Code mostra no terminal (blocos ▐▛███▜▌), passado para
//! uma grade de 18 × 10 quadradinhos: o corpo, dois olhos, uma garra de cada lado e quatro
//! patinhas. Cada quadradinho cai em pixels inteiros da tela, então ele fica nítido em qualquer
//! escala, sem borrar.
//!
//! As animações tocam em episódios de um ou dois segundos (piscar, olhar para os lados, andar de
//! lado, pular, acenar as garras...), com intervalos parados entre eles: parado, o widget não
//! redesenha nada. O humor muda o repertório: dormindo sem números, suando perto do limite e,
//! enquanto o Claude Code trabalha, com um caderno: escreve (no ritmo do Claude Code), apaga,
//! amassa a folha, pensa, tem ideias, batuca o lápis e, depois de um bom tempo, toma um café.
//! Quando o Claude Code termina, ele fecha o caderno, se espreguiça e mostra um ✓.

use std::time::{Duration, Instant};

use crate::gfx::Canvas;
use crate::theme::*;

/// A grade do desenho, em quadradinhos.
pub const COLS: f32 = 18.0;
pub const ROWS: f32 = 10.0;

/// A cor do Clawd no Claude Code e a dos olhos.
const BODY: Rgba = Rgba::rgb(215, 119, 87);
const EYE: Rgba = Rgba::rgb(28, 20, 18);
const SWEAT: Rgba = Rgba::rgb(120, 196, 255);
/// O caderno: a folha, a capa (a faixa de cima), a escrita e o lápis (com a borracha na ponta).
const PAGE: Rgba = Rgba::rgb(246, 241, 228);
const COVER: Rgba = Rgba::rgb(92, 128, 214);
const INK: Rgba = Rgba::rgb(66, 70, 92);
const PENCIL: Rgba = Rgba::rgb(255, 206, 74);
const GRAPHITE: Rgba = Rgba::rgb(40, 36, 40);
const ERASER: Rgba = Rgba::rgb(242, 132, 164);
/// A lâmpada da ideia (acesa e apagada), a caneca de café e o ✓ de quando termina.
const BULB: Rgba = Rgba::rgb(255, 226, 92);
const BULB_OFF: Rgba = Rgba::rgb(150, 150, 162);
const MUG: Rgba = Rgba::rgb(236, 236, 240);
const COFFEE: Rgba = Rgba::rgb(122, 74, 44);
const CHECK: Rgba = Rgba::rgb(112, 214, 128);
/// O notebook (no pequeno, no lugar do caderno): a tampa de alumínio com o símbolo laranja, o
/// teclado (com a tecla apertada mais clara), a base e a luz da tela no rosto dele.
const ALU: Rgba = Rgba::rgb(201, 204, 214);
const ALU_DARK: Rgba = Rgba::rgb(139, 143, 156);
const KEYS: Rgba = Rgba::rgb(93, 96, 110);
const KEY_LIT: Rgba = Rgba::rgb(158, 162, 176);
const LOGO: Rgba = Rgba::rgb(242, 140, 40);
const SCREEN_LIGHT: Rgba = Rgba::rgb(235, 164, 138);
/// O notebook de lado (médio, grande e na barra de tarefas): a moldura escura e a tela, com o
/// código em cores.
const BEZEL: Rgba = Rgba::rgb(42, 44, 54);
const SCREEN: Rgba = Rgba::rgb(27, 35, 54);
const CODE: [Rgba; 5] = [Rgba::rgb(242, 140, 40), Rgba::rgb(120, 196, 255), Rgba::rgb(112, 214, 128), Rgba::rgb(242, 132, 164), Rgba::rgb(255, 226, 92)];
/// As linhas de código que aparecem na tela: (recuo, letras, cor).
const LINES: [(i32, i32, usize); 10] = [(0, 4, 0), (1, 3, 1), (1, 4, 2), (2, 2, 3), (1, 3, 1), (0, 2, 4), (1, 4, 0), (2, 3, 2), (1, 2, 1), (0, 3, 3)];
/// Quantas letras de código cada letra da página vira (a página inteira enche a tela).
const CODE_PER_LETTER: i32 = 3;
/// A largura do desenho com o notebook de lado, em quadradinhos (o Clawd e o notebook à direita).
pub const SIDE_COLS: f32 = 26.0;

/// Quantas vezes por segundo confere o quadro enquanto anima; só redesenha quando o desenho
/// muda, então conferir mais vezes deixa os movimentos no tempo certo sem custar desenhos.
const FPS: u64 = 30;
/// A parte do pulo em que ele agacha antes de subir.
const CROUCH: f32 = 0.1;
/// Segundos por página, sem saber o ritmo do Claude Code; e o mais rápido e o mais devagar.
pub const PACE: f32 = 4.2;
const PACE_FAST: f32 = 2.4;
const PACE_SLOW: f32 = 7.0;
/// Trabalhando há tanto tempo, ele pode parar para um café; e não toma outro antes de tanto.
pub const COFFEE_AFTER: Duration = Duration::from_secs(20 * 60);
const COFFEE_EVERY: Duration = Duration::from_secs(12 * 60);

/// Como o Clawd está: muda as animações que ele faz.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mood {
    Normal,
    /// sem números para mostrar (não conectado ou esperando o Claude Code): dorme
    Sleepy,
    /// o Claude Code está trabalhando agora: fica com o caderno
    Busy,
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
    /// escreve uma página e vira
    Write,
    /// escreve, apaga a última palavra com a borracha e escreve de novo
    Erase,
    /// escreve, arranca a folha, amassa e joga longe
    Crumple,
    /// para, olha para cima e aparecem uns pontinhos
    Think,
    /// acende uma lâmpada; a página seguinte sai mais rápido
    Idea,
    /// batuca o lápis no caderno, esperando
    Tap,
    /// toma um gole de café
    Coffee,
    /// o Claude Code terminou: fecha o caderno, se espreguiça e mostra um ✓
    Done,
}

#[cfg(test)]
pub const ANIMS: [Anim; 16] = [
    Anim::Blink,
    Anim::Look,
    Anim::Scuttle,
    Anim::Hop,
    Anim::Wave,
    Anim::Snooze,
    Anim::Sweat,
    Anim::Shake,
    Anim::Write,
    Anim::Erase,
    Anim::Crumple,
    Anim::Think,
    Anim::Idea,
    Anim::Tap,
    Anim::Coffee,
    Anim::Done,
];

impl Anim {
    /// Duração, em segundos, escrevendo uma página a cada `pace` segundos.
    fn secs(self, pace: f32) -> f32 {
        match self {
            Anim::Blink => 0.2,
            Anim::Look => 1.8,
            Anim::Scuttle => 2.0,
            Anim::Hop => 0.9,
            Anim::Wave => 1.4,
            Anim::Snooze => 3.6,
            Anim::Sweat => 1.6,
            Anim::Shake => 0.8,
            // o mesmo tempo por letra nas três: só a quantidade de passos muda
            Anim::Write => pace,
            Anim::Erase => pace * 1.6,
            Anim::Crumple => pace * 1.4,
            Anim::Think => 2.6,
            Anim::Idea => 1.8,
            Anim::Tap => 1.6,
            Anim::Coffee => 3.4,
            Anim::Done => 2.8,
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
    /// esticadas (se espreguiçando, com o corpo um quadradinho mais alto)
    Stretch,
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
    /// garras caídas, relaxadas (dormindo)
    pub limp: bool,
    /// gota de suor descendo (0 a 1)
    pub sweat: Option<f32>,
    /// "z" subindo (0 a 1)
    pub zzz: Option<f32>,
    /// o caderno na frente dele, com a página como está
    pub notebook: Option<Page>,
    /// o caderno fechado
    pub closed: bool,
    /// pontinhos de quem está pensando (0 a 3)
    pub dots: u8,
    /// a lâmpada da ideia: (acesa, com os raios)
    pub bulb: Option<(bool, bool)>,
    /// a caneca de café: (levantada até a boca, o vapor de um lado ou do outro)
    pub mug: Option<(bool, bool)>,
    /// o ✓ de quando o Claude Code termina
    pub check: bool,
    /// o notebook no colo, de frente (a tampa virada para nós), no lugar do caderno
    pub laptop: Option<Laptop>,
    /// as garras fora do lugar de sempre (no teclado): o canto de cima de cada uma
    pub claws: Option<[(i32, i32); 2]>,
    /// o notebook de lado, no chão, à direita, com a tela à mostra
    pub side: Option<Side>,
    /// a garra esquerda atrás das costas (indo pegar ou guardar a caneca): só a ponta aparece
    pub reach_back: bool,
    /// a caneca na frente da boca (tomando café, com o notebook de lado)
    pub mug_front: bool,
}

/// O notebook de lado.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Side {
    /// a tampa aberta (fechada: deitado no chão)
    pub open: bool,
    pub screen: Screen,
}

/// O que aparece na tela do notebook de lado.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Screen {
    /// o código, com tantas letras escritas (e o cursor no fim)
    Code(i32),
    /// o ✓ verde de quando o Claude Code termina
    Done,
}

/// O notebook no colo.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Laptop {
    /// aberto (a tampa em pé, a tela acesa no rosto dele) ou fechado, deitado
    pub open: bool,
    /// a tecla apertada agora (coluna), se ele está digitando
    pub key: Option<i32>,
}

/// As garras paradas em cima do teclado (por dentro do corpo, para aparecerem em laranja contra
/// o cinza do notebook) e a linha de uma delas apertando uma tecla.
const ON_KEYS: [(i32, i32); 2] = [(4, 6), (12, 6)];
const TAP_DOWN: i32 = 7;

/// As palavras que ele escreve na página: (linha, primeira coluna, letras), em quadradinhos.
const WORDS: [(i32, i32, i32); 4] = [(7, 6, 3), (7, 10, 2), (8, 6, 2), (8, 9, 3)];
/// A folha do caderno: colunas e linhas (o fim não entra).
const PAGE_X: (i32, i32) = (5, 13);
const PAGE_Y: (i32, i32) = (5, 10);

/// O lápis: onde está a ponta (coluna, linha), se encosta na folha e se está virado (com a
/// borracha para baixo).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pen {
    pub x: i32,
    pub y: i32,
    pub down: bool,
    pub eraser: bool,
}

/// O lápis parado, no começo da página.
const PEN_REST: Pen = Pen { x: WORDS[0].1, y: WORDS[0].0, down: false, eraser: false };

/// Uma página do caderno.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Page {
    /// as letras escritas: um bit por quadradinho das duas linhas
    pub ink: u16,
    /// o lápis (None: guardado, enquanto a folha vira ou é arrancada)
    pub pen: Option<Pen>,
    /// a folha subindo (virando ou sendo arrancada)
    pub lifted: bool,
    /// a bolinha de papel voando: (coluna, linha) do canto de cima
    pub ball: Option<(i32, i32)>,
}

/// A página em branco, com o lápis parado.
const PAGE_REST: Page = Page { ink: 0, pen: Some(PEN_REST), lifted: false, ball: None };

/// O bit de uma letra da página.
fn bit(x: i32, y: i32) -> u16 {
    1 << ((y - WORDS[0].0) * 8 + (x - PAGE_X.0))
}

/// Um passo do lápis.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Step {
    /// levanta e leva até aí
    Move(i32, i32),
    /// escreve uma letra
    Ink(i32, i32),
    /// apaga uma letra com a borracha
    Rub(i32, i32),
    /// vira o lápis (borracha para baixo, ou de volta)
    Turn,
}

/// Os passos de uma página: escreve as palavras; apagando, depois apaga a última, da direita
/// para a esquerda, e escreve de novo.
fn script(anim: Anim) -> Vec<Step> {
    let mut s = Vec::new();
    let write = |s: &mut Vec<Step>, (y, x, n): (i32, i32, i32)| {
        s.push(Step::Move(x, y));
        s.extend((x..x + n).map(|x| Step::Ink(x, y)));
    };
    for w in WORDS {
        write(&mut s, w);
    }
    if anim == Anim::Erase {
        let (y, x, n) = WORDS[WORDS.len() - 1];
        s.push(Step::Turn);
        s.extend((x..x + n).rev().map(|x| Step::Rub(x, y)));
        s.push(Step::Turn);
        write(&mut s, (y, x, n));
    }
    s
}

/// Até onde (0 a 1) da animação ele escreve; depois vira a página (ou amassa).
fn write_end(anim: Anim) -> f32 {
    match anim {
        Anim::Erase => 0.86,
        Anim::Crumple => 0.6,
        _ => 0.84,
    }
}

/// A página em `t` (0 a 1) de uma animação de escrever (`Write`, `Erase` ou `Crumple`).
pub fn page_at(anim: Anim, t: f32) -> Page {
    let end = write_end(anim);
    if t < end {
        let steps = script(anim);
        let k = ((t.max(0.0) / end * steps.len() as f32) as usize).min(steps.len() - 1);
        let (mut ink, mut pen) = (0u16, PEN_REST);
        for step in &steps[..=k] {
            match *step {
                Step::Move(x, y) => pen = Pen { x, y, down: false, ..pen },
                Step::Ink(x, y) => {
                    ink |= bit(x, y);
                    pen = Pen { x, y, down: true, ..pen };
                }
                Step::Rub(x, y) => {
                    ink &= !bit(x, y);
                    pen = Pen { x, y, down: true, ..pen };
                }
                Step::Turn => pen = Pen { down: false, eraser: !pen.eraser, ..pen },
            }
        }
        return Page { ink, pen: Some(pen), ..PAGE_REST };
    }
    let u = (t - end) / (1.0 - end);
    let lifted = Page { ink: 0, pen: None, lifted: true, ball: None };
    if anim != Anim::Crumple {
        // vira a página e volta ao começo
        return if u < 0.55 { lifted } else { PAGE_REST };
    }
    // arranca a folha, amassa numa bolinha e joga por cima do ombro, para a esquerda
    match u {
        u if u < 0.2 => lifted,
        u if u < 0.35 => Page { ball: Some((9, 3)), ..lifted },
        u if u < 0.9 => {
            let s = (u - 0.35) / 0.55;
            let x = 9 - (11.0 * s).round() as i32;
            let y = 3 - (4.0 * (s * std::f32::consts::PI).sin()).round() as i32;
            Page { ink: 0, pen: None, lifted: false, ball: Some((x, y)) }
        }
        _ => PAGE_REST,
    }
}

/// Os "z" de quem dorme, em pixel art: um pequeno e um grande ("z Z").
const Z_SMALL: [&str; 4] = ["###", "..#", ".#.", "###"];
const Z_BIG: [&str; 5] = ["####", "...#", "..#.", ".#..", "####"];
/// A lâmpada (o vidro e o pescoço; a base vai em cinza) e o ✓.
const BULB_GLASS: [&str; 3] = ["###", "###", ".#."];
const CHECK_MARK: [&str; 4] = ["....#", "...#.", "#.#..", ".#..."];
/// Onde ficam: a lâmpada e o ✓ do lado da cabeça, no alto; os pontinhos subindo em escada.
const BULB_AT: (i32, i32) = (16, -3);
const CHECK_AT: (i32, i32) = (17, -2);
const DOTS: [(i32, i32); 3] = [(16, 2), (18, 1), (20, 0)];
/// A caneca no chão, à direita, e levantada até a altura da garra.
const MUG_DOWN: (i32, i32) = (16, 7);
const MUG_UP: (i32, i32) = (16, 3);
/// Com o notebook de lado, a caneca vem na frente da boca, segura pela garra esquerda.
const MUG_FRONT: (i32, i32) = (3, 3);

/// Os "z" do cochilo: (desenho, canto de cima no começo, quando nasce e quando some, em `t`).
/// Um de cada vez, em escadinha: o pequeno do lado da cabeça, depois o grande, mais acima e à
/// direita (sem passar por cima do título, no grande).
const ZS: [(&[&str], (i32, i32), f32, f32); 2] = [(&Z_SMALL, (16, 0), 0.0, 0.48), (&Z_BIG, (18, -2), 0.5, 1.0)];

/// Os "z" em `t` do cochilo: (desenho, coluna e linha do canto de cima, opacidade). Cada um
/// aparece, sobe um quadradinho no meio da vida e some aos poucos, em degraus (pixel art; e o
/// widget só redesenha quando algo muda).
pub fn zs_at(t: f32) -> Vec<(&'static [&'static str], i32, i32, f32)> {
    let mut out = Vec::new();
    for &(glyph, (x, y), born, dies) in &ZS {
        let age = (t - born) / (dies - born);
        if !(0.0..1.0).contains(&age) {
            continue;
        }
        let fade = (age / 0.15).min(1.0).min((1.0 - age) / 0.4);
        let alpha = (fade * 4.0).ceil() / 4.0 * 0.95;
        if alpha > 0.0 {
            out.push((glyph, x, y - (age >= 0.5) as i32, alpha));
        }
    }
    out
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
        // olhando para o lápis, parado no começo da página
        (Mood::Busy, None) => Eyes::Look(-1, 1),
        _ => Eyes::Open,
    };
    let mut p = Pose { eyes, ..Pose::default() };
    match mood {
        Mood::Worried => p.sweat = Some(0.0), // a gota fica na testa
        Mood::Busy => p.notebook = Some(PAGE_REST), // o caderno aberto numa página em branco
        Mood::Sleepy => p.limp = gaze.is_none(),
        Mood::Normal => {}
    }
    let Some((anim, t)) = anim else { return p };
    let t = t.clamp(0.0, 1.0);
    // os olhos seguem o que acontece, a não ser que estejam no mouse
    let look = |p: &mut Pose, e: Eyes| {
        if gaze.is_none() {
            p.eyes = e;
        }
    };
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
            if t < CROUCH {
                // agacha antes de pular: já o primeiro quadro é outro (reage ao clique na hora)
                p.dy = 1;
                p.legs = Legs::Tucked;
            } else {
                // dois pulinhos
                let h = ((t - CROUCH) / (1.0 - CROUCH) * 2.0).fract();
                p.dy = -((h * std::f32::consts::PI).sin() * 3.0).round() as i32;
                p.legs = if p.dy < 0 { Legs::Tucked } else { Legs::Stand };
                p.left_up = p.dy <= -2;
                p.right_up = p.dy <= -2;
            }
        }
        Anim::Wave => {
            let k = (t * 6.0) as i32;
            p.left_up = k % 2 == 0;
            p.right_up = k % 2 == 1;
        }
        Anim::Snooze => {
            p.eyes = Eyes::Closed;
            p.limp = true;
            p.zzz = Some(t);
        }
        Anim::Write | Anim::Erase | Anim::Crumple => {
            let page = page_at(anim, t);
            p.notebook = Some(page);
            // os olhos acompanham o lápis na folha, ou a bolinha voando
            let side = |x: i32| if x < 8 { -1 } else if x > 9 { 1 } else { 0 };
            match (page.pen, page.ball) {
                (_, Some((x, y))) => look(&mut p, Eyes::Look(side(x), if y < 2 { -1 } else { 0 })),
                (Some(pen), _) => look(&mut p, Eyes::Look(side(pen.x), 1)),
                _ => look(&mut p, Eyes::Look(0, 1)),
            }
        }
        Anim::Think => {
            // olha para cima, para o lado dos pontinhos, que aparecem um a um
            look(&mut p, Eyes::Look(1, -1));
            p.dots = ((t - 0.15) / 0.18).floor().clamp(0.0, 3.0) as u8;
        }
        Anim::Idea => {
            // a lâmpada aparece apagada, pisca e acende de vez; ele levanta as garras
            let lit = (0.3..0.38).contains(&t) || t >= 0.44;
            if t >= 0.12 {
                p.bulb = Some((lit, lit && (t * 10.0) as i32 % 2 == 0));
            }
            if t >= 0.3 {
                look(&mut p, Eyes::Open);
            }
            p.left_up = (0.44..0.75).contains(&t);
            p.right_up = p.left_up;
        }
        Anim::Tap => {
            // o lápis batendo no caderno, olhando para a frente (esperando)
            look(&mut p, Eyes::Open);
            if let Some(page) = &mut p.notebook {
                page.pen = Some(Pen { x: 10, y: 7, down: (t * 8.0) as i32 % 2 == 0, eraser: false });
            }
        }
        Anim::Coffee => {
            // pega a caneca, toma um gole de olhos fechados e devolve
            let up = (0.3..0.7).contains(&t);
            if t < 0.95 {
                p.mug = Some((up, (t * 6.0) as i32 % 2 == 0));
            }
            if up {
                p.eyes = Eyes::Closed;
            }
        }
        Anim::Done => {
            if t < 0.2 {
                // fecha o caderno
                p.notebook = None;
                p.closed = true;
                look(&mut p, Eyes::Look(0, 1));
            } else if t < 0.5 {
                // se espreguiça: estica as patas e levanta as garras, de olhos fechados
                p.notebook = None;
                p.dy = -1;
                p.legs = Legs::Stretch;
                p.left_up = true;
                p.right_up = true;
                p.eyes = Eyes::Closed;
            } else {
                // e mostra o ✓ (piscando uma vez)
                p.notebook = None;
                p.check = !(0.7..0.76).contains(&t);
                look(&mut p, Eyes::Open);
            }
        }
        Anim::Sweat => p.sweat = Some(t),
        Anim::Shake => {
            p.dx = if (t * 12.0) as i32 % 2 == 0 { 1 } else { -1 };
            p.eyes = Eyes::Closed;
        }
    }
    p
}

/// O mesmo quadro com o notebook de frente, no colo, no lugar do caderno (no pequeno, em que o
/// caderno ficaria pequeno demais): onde o lápis escreveria uma letra, uma garra bate numa
/// tecla, uma de cada vez; com o lápis no ar, as duas ficam paradas no teclado. Os olhos ficam na
/// tela (a não ser que estejam no mouse ou pensando); fechar o caderno vira fechar a tampa.
pub fn on_laptop(mut p: Pose, gaze: Option<(i32, i32)>) -> Pose {
    if p.closed {
        p.closed = false;
        p.laptop = Some(Laptop { open: false, key: None });
        return p;
    }
    let Some(page) = p.notebook.take() else { return p };
    let [mut left, mut right] = ON_KEYS;
    let mut key = None;
    if page.pen.is_some_and(|pen| pen.down) {
        // cada letra nova troca de garra; a tecla acesa, ao lado dela, muda junto
        let n = page.ink.count_ones() as i32;
        if n % 2 == 0 {
            left.1 = TAP_DOWN;
            key = Some(6 + (n / 2) % 2);
        } else {
            right.1 = TAP_DOWN;
            key = Some(10 + (n / 2) % 2);
        }
    }
    // a folha e a bolinha não existem no notebook: os olhos ficam na tela
    if gaze.is_none() && (matches!(p.eyes, Eyes::Look(_, 1)) || page.ball.is_some()) {
        p.eyes = Eyes::Open;
    }
    // garras levantadas (acenando, com uma ideia) e a caneca na boca saem do teclado
    if p.left_up {
        left = (1, 2);
    }
    if p.right_up {
        right = (15, 2);
    }
    if p.mug.is_some_and(|(up, _)| up) {
        right = (15, 4);
    }
    p.claws = Some([left, right]);
    p.laptop = Some(Laptop { open: true, key });
    p
}

/// O mesmo quadro com o notebook de lado, no chão à direita, com a tela à mostra (no médio, no
/// grande e na barra de tarefas): onde o lápis escreveria, o código aparece na tela, letra a
/// letra, e a garra direita bate no teclado; a folha virando é a tela cheia de código. O café
/// não fica à mostra: a garra vai atrás das costas, volta com a caneca na frente da boca e a
/// guarda de novo. Quando o Claude Code termina, o ✓ aparece na tela e depois a tampa fecha.
pub fn on_side_laptop(mut p: Pose, gaze: Option<(i32, i32)>) -> Pose {
    let look_at_screen = |p: &mut Pose| {
        if gaze.is_none() {
            p.eyes = Eyes::Look(1, 0);
        }
    };
    if p.closed {
        // terminou: o ✓ na tela, com as garras para cima
        p.closed = false;
        p.side = Some(Side { open: true, screen: Screen::Done });
        p.left_up = true;
        p.right_up = true;
        look_at_screen(&mut p);
        return p;
    }
    if p.legs == Legs::Stretch || p.check {
        // e, já se espreguiçando, a tampa fechada no chão
        p.side = Some(Side { open: false, screen: Screen::Code(0) });
        return p;
    }
    let Some(page) = p.notebook.take() else { return p };
    let full = LINES.iter().map(|l| l.1).sum::<i32>();
    let n = page.ink.count_ones() as i32;
    let screen = match page.pen {
        Some(_) => Screen::Code((n * CODE_PER_LETTER).min(full)),
        // a página virando ou amassada: a tela cheia
        None => Screen::Code(full),
    };
    // a garra direita no teclado: batendo a cada letra, parada entre as palavras
    let typing = page.pen.is_some_and(|pen| pen.down);
    let right = if p.right_up {
        (15, 2)
    } else if typing {
        (15, 5 + (n % 2 == 0) as i32)
    } else {
        (15, 5)
    };
    let left = if p.left_up { (1, 2) } else { (1, 4) };
    // o café: a caneca vem de trás das costas
    match p.mug {
        Some((true, _)) => p.mug_front = true,
        Some((false, _)) => {
            p.mug = None;
            p.reach_back = true;
        }
        None => {}
    }
    p.claws = Some([left, right]);
    if gaze.is_none() && (matches!(p.eyes, Eyes::Look(_, 1)) || page.ball.is_some()) {
        p.eyes = Eyes::Look(1, 0);
    }
    p.side = Some(Side { open: true, screen });
    p
}

/// As letras de código na tela, linha a linha: (coluna, linha, letras, cor), as últimas 5
/// linhas (as de cima sobem quando a tela enche), e onde fica o cursor.
fn code_cells(chars: i32) -> (Vec<(i32, i32, i32, Rgba)>, (i32, i32)) {
    let mut shown = Vec::new();
    let mut left = chars;
    for &(indent, len, color) in &LINES {
        if left <= 0 {
            break;
        }
        shown.push((indent, len.min(left), CODE[color]));
        left -= len;
    }
    let skip = shown.len().saturating_sub(5);
    let view: Vec<_> = shown[skip..].iter().enumerate().map(|(i, &(ind, n, c))| (19 + ind, 2 + i as i32, n, c)).collect();
    let cursor = match view.last() {
        Some(&(x, y, n, _)) => ((x + n).min(24), y),
        None => (19, 2),
    };
    (view, cursor)
}

/// O Clawd pulando de um lugar para outro (quando o widget muda de tamanho): agachado antes de
/// subir e ao pousar, de olhos fechados; no ar, com as patas encolhidas e as garras para cima.
pub fn jump_pose(crouch: bool) -> Pose {
    if crouch {
        Pose { dy: 1, legs: Legs::Tucked, eyes: Eyes::Closed, limp: true, ..Pose::default() }
    } else {
        Pose { legs: Legs::Tucked, eyes: Eyes::Look(0, -1), left_up: true, right_up: true, ..Pose::default() }
    }
}

/// Um retângulo do desenho, em quadradinhos: (x, y, largura, altura, cor).
pub type Cell = (i32, i32, i32, i32, Rgba);

/// Os retângulos de um desenho em texto ("#": pintado) com o canto de cima em (gx, gy).
fn glyph(put: &mut impl FnMut(i32, i32, i32, i32, Rgba), rows: &[&str], gx: i32, gy: i32, color: Rgba) {
    for (j, row) in rows.iter().enumerate() {
        // cada trecho seguido de "#" vira um retângulo
        let bytes = row.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'#' {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && bytes[i] == b'#' {
                i += 1;
            }
            put(gx + start as i32, gy + j as i32, (i - start) as i32, 1, color);
        }
    }
}

/// Os retângulos do quadro, em quadradinhos.
pub fn cells(p: &Pose) -> Vec<Cell> {
    let mut v = Vec::new();
    let mut put = |x: i32, y: i32, w: i32, h: i32, c: Rgba| v.push((x + p.dx, y + p.dy, w, h, c));
    // a garra atrás das costas: antes do corpo, só a ponta aparece em cima
    if p.reach_back {
        put(3, -1, 2, 2, BODY);
    }
    // corpo
    put(3, 0, 12, 8, BODY);
    // a luz da tela do notebook aberto no rosto
    if p.laptop.is_some_and(|l| l.open) {
        put(4, 3, 10, 1, SCREEN_LIGHT.alpha(0.6));
    }
    // garras: dos lados do meio do corpo; levantadas, sobem até a altura dos olhos
    // (dormindo, caídas um quadradinho); no teclado, vão por cima do notebook, mais abaixo
    if p.claws.is_none() {
        let rest = if p.limp { 5 } else { 4 };
        put(1, if p.left_up { 2 } else { rest }, 2, 2, BODY);
        put(15, if p.right_up { 2 } else { rest }, 2, 2, BODY);
    }
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
            Legs::Stretch => 3,
        };
        put(x, 8, 1, h, BODY);
    }
    // gota de suor, do lado da cabeça
    if let Some(t) = p.sweat {
        put(16, (t * 2.0).round() as i32 - 1, 1, 2, SWEAT);
    }
    let ((x0, x1), (y0, y1)) = (PAGE_X, PAGE_Y);
    // o caderno no colo, a escrita e o lápis
    if let Some(Page { ink, pen, lifted, ball }) = p.notebook {
        put(x0, y0, x1 - x0, 1, COVER);
        put(x0, y0 + 1, x1 - x0, y1 - y0 - 1, PAGE);
        if lifted {
            // a folha escrita subindo, por cima do corpo
            put(x0 + 1, y0 - 1, x1 - x0 - 2, 1, PAGE);
        }
        for y in [WORDS[0].0, WORDS[0].0 + 1] {
            for x in x0..x1 {
                if ink & bit(x, y) != 0 {
                    put(x, y, 1, 1, INK);
                }
            }
        }
        if let Some(pen) = pen {
            // a ponta na letra (ou um pouco acima, levantada) e o lápis inclinado para a direita;
            // virado, com a borracha embaixo e a ponta em cima
            let y = pen.y - (!pen.down) as i32;
            let (tip, end) = if pen.eraser { (ERASER, GRAPHITE) } else { (GRAPHITE, ERASER) };
            put(pen.x, y, 1, 1, tip);
            put(pen.x + 1, y - 1, 1, 1, PENCIL);
            put(pen.x + 2, y - 2, 1, 1, PENCIL);
            if pen.eraser {
                put(pen.x + 3, y - 3, 1, 1, end);
            }
        }
        if let Some((bx, by)) = ball {
            // a bolinha de papel amassado, com uma dobra mais escura
            put(bx, by, 2, 2, PAGE);
            put(bx + 1, by + 1, 1, 1, INK.alpha(0.35));
        }
    }
    if p.closed {
        // o caderno fechado: a capa, com a borda das folhas embaixo
        put(x0 + 1, y0 + 1, x1 - x0 - 2, y1 - y0 - 2, COVER);
        put(x0 + 1, y1 - 1, x1 - x0 - 2, 1, PAGE);
    }
    // o notebook no colo: aberto, a tampa em pé (vista por trás, com o símbolo), o teclado e a
    // base; fechado, deitado
    if let Some(Laptop { open, key }) = p.laptop {
        if open {
            put(4, 4, 10, 3, ALU);
            put(8, 5, 2, 1, LOGO.alpha(0.85));
            put(3, 7, 12, 1, KEYS);
            if let Some(k) = key {
                put(k, 7, 1, 1, KEY_LIT);
            }
        } else {
            put(3, 7, 12, 1, ALU);
        }
        put(3, 8, 12, 1, ALU_DARK);
    }
    // o notebook de lado, no chão à direita: aberto, a moldura e a tela (com o código e o
    // cursor, ou o ✓), o teclado e a base; fechado, deitado
    if let Some(Side { open, screen }) = p.side {
        if open {
            put(18, 1, 8, 7, BEZEL);
            put(19, 2, 6, 5, SCREEN);
            match screen {
                Screen::Code(chars) => {
                    let (lines, (cx, cy)) = code_cells(chars);
                    for (x, y, n, c) in lines {
                        put(x, y, n, 1, c);
                    }
                    put(cx, cy, 1, 1, FG.alpha(0.85));
                }
                Screen::Done => glyph(&mut put, &CHECK_MARK, 19, 2, CHECK),
            }
            put(17, 8, 9, 1, KEYS);
        } else {
            put(17, 8, 9, 1, ALU);
        }
        put(17, 9, 9, 1, ALU_DARK);
    }
    if let Some([(lx, ly), (rx, ry)]) = p.claws {
        // (a esquerda atrás das costas já foi desenhada, antes do corpo)
        if !p.reach_back {
            put(lx, ly, 2, 2, BODY);
        }
        put(rx, ry, 2, 2, BODY);
    }
    // os pontinhos de quem está pensando
    for &(x, y) in DOTS.iter().take(p.dots as usize) {
        put(x, y, 1, 1, FG);
    }
    // a lâmpada da ideia, com a base e os raios
    if let Some((lit, rays)) = p.bulb {
        let (bx, by) = BULB_AT;
        glyph(&mut put, &BULB_GLASS, bx, by, if lit { BULB } else { BULB_OFF });
        put(bx + 1, by + 3, 1, 1, BULB_OFF);
        if rays {
            put(bx - 2, by + 1, 1, 1, BULB.alpha(0.8));
            put(bx + 4, by + 1, 1, 1, BULB.alpha(0.8));
        }
    }
    // a caneca: café em cima, a caneca e a asa; e o vapor subindo
    if let Some((up, steam)) = p.mug {
        let (mx, my) = if p.mug_front { MUG_FRONT } else if up { MUG_UP } else { MUG_DOWN };
        put(mx, my, 2, 1, COFFEE);
        put(mx, my + 1, 2, 2, MUG);
        put(mx + 2, my + 1, 1, 1, MUG);
        let s = steam as i32;
        put(mx + s, my - 1, 1, 1, FG.alpha(0.55));
        put(mx + 1 - s, my - 2, 1, 1, FG.alpha(0.55));
    }
    if p.check {
        glyph(&mut put, &CHECK_MARK, CHECK_AT.0, CHECK_AT.1, CHECK);
    }
    // os "z" de quem dorme
    if let Some(t) = p.zzz {
        for (rows, gx, gy, alpha) in zs_at(t) {
            glyph(&mut put, rows, gx, gy, FG.alpha(alpha));
        }
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
    w
}

/// Segundos por página para o Claude Code dando `per_min` passos por minuto (respostas e
/// resultados de ferramentas): quanto mais rápido ele trabalha, mais rápido o Clawd escreve.
pub fn pace_for(per_min: f32) -> f32 {
    (PACE_SLOW - per_min * 0.12).clamp(PACE_FAST, PACE_SLOW)
}

/// Quando e o que o Clawd anima.
pub struct Crab {
    /// a animação, quando começou e quanto dura (segundos)
    anim: Option<(Anim, Instant, f32)>,
    next: Instant,
    seed: u64,
    /// o humor e o desenho do último tick (só redesenha quando o desenho muda)
    mood: Mood,
    last: Option<Vec<Cell>>,
    /// segundos por página (o ritmo do Claude Code)
    pub pace: f32,
    /// teve uma ideia: a próxima página sai mais rápido
    inspired: bool,
    /// trabalhando há um bom tempo (pode parar para um café), e quando foi o último
    pub tired: bool,
    last_coffee: Option<Instant>,
}

impl Default for Crab {
    fn default() -> Self {
        Crab::new()
    }
}

impl Crab {
    pub fn new() -> Crab {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7) | 1;
        Crab {
            anim: None,
            next: Instant::now() + Duration::from_secs(3),
            seed,
            mood: Mood::Normal,
            last: None,
            pace: PACE,
            inspired: false,
            tired: false,
            last_coffee: None,
        }
    }

    /// O mesmo, já no meio de uma animação (para as imagens e os testes).
    #[cfg(test)]
    pub fn playing(anim: Anim, t: f32) -> Crab {
        let mut c = Crab::new();
        let secs = anim.secs(PACE);
        c.anim = Some((anim, Instant::now() - Duration::from_secs_f32(secs * t), secs));
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
        let (anim, start, secs) = self.anim?;
        let t = start.elapsed().as_secs_f32() / secs;
        (t < 1.0).then_some((anim, t))
    }

    pub fn animating(&self) -> bool {
        self.current().is_some()
    }

    /// Está tocando `anim` agora?
    pub fn is_playing(&self, anim: Anim) -> bool {
        self.current().is_some_and(|(a, _)| a == anim)
    }

    pub fn pose(&self, mood: Mood, gaze: Option<(i32, i32)>) -> Pose {
        pose(mood, self.current(), gaze)
    }

    /// O quadro com o notebook no colo, no lugar do caderno (o pequeno).
    pub fn laptop_pose(&self, mood: Mood, gaze: Option<(i32, i32)>) -> Pose {
        on_laptop(self.pose(mood, gaze), gaze)
    }

    /// O quadro com o notebook de lado, com a tela à mostra (o médio, o grande e a barra).
    pub fn side_pose(&self, mood: Mood, gaze: Option<(i32, i32)>) -> Pose {
        on_side_laptop(self.pose(mood, gaze), gaze)
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
        if mood != self.mood {
            // começou a trabalhar: pega o caderno logo, sem esperar o intervalo de antes
            if mood == Mood::Busy {
                self.next = self.next.min(now + Duration::from_millis(600));
            }
            self.mood = mood;
            self.last = None;
        }
        if self.anim.is_some() {
            if !self.animating() {
                // acabou: volta à pose parada e espera um pouco até a próxima
                self.anim = None;
                let (lo, hi) = match mood {
                    Mood::Sleepy => (5, 9),
                    Mood::Busy => (1, 4),
                    _ => (7, 16),
                };
                self.next = now + Duration::from_secs(lo + self.random() % (hi - lo));
                // com uma ideia na cabeça, volta a escrever logo
                if self.inspired && mood == Mood::Busy {
                    self.next = now + Duration::from_millis(300);
                }
                self.last = None;
                return true;
            }
        } else if now >= self.next {
            let anim = self.pick(mood);
            self.play(anim);
        }
        // parado não redesenha; animando, só se o quadro mudou (escrevendo, o lápis anda devagar)
        if self.anim.is_none() {
            return false;
        }
        let drawn = cells(&self.pose(mood, None));
        let changed = self.last.as_ref() != Some(&drawn);
        self.last = Some(drawn);
        changed
    }

    /// Toca `anim` agora (um clique nele, o mouse chegando perto, o Claude Code terminando),
    /// mesmo no meio de outra.
    pub fn play(&mut self, anim: Anim) {
        let mut secs = anim.secs(self.pace);
        match anim {
            // a página depois de uma ideia sai na metade do tempo
            Anim::Write | Anim::Erase | Anim::Crumple if self.inspired => {
                secs *= 0.5;
                self.inspired = false;
            }
            Anim::Idea => self.inspired = true,
            Anim::Coffee => self.last_coffee = Some(Instant::now()),
            _ => {}
        }
        self.anim = Some((anim, Instant::now(), secs));
        self.last = None;
    }

    /// O mouse entrou no widget: uma animação na hora (se já não está animando).
    pub fn poke(&mut self, mood: Mood) {
        if self.animating() {
            return;
        }
        let anim = match mood {
            Mood::Sleepy => Anim::Hop, // acorda num pulo
            Mood::Worried => Anim::Shake,
            Mood::Busy => Anim::Wave, // acena sem largar o caderno
            Mood::Normal => [Anim::Wave, Anim::Hop][(self.random() % 2) as usize],
        };
        self.play(anim);
    }

    fn pick(&mut self, mood: Mood) -> Anim {
        match mood {
            Mood::Sleepy => Anim::Snooze,
            Mood::Busy => {
                if self.inspired {
                    return Anim::Write;
                }
                // um café de vez em quando, depois de muito tempo trabalhando
                let coffee_due = self.tired && self.last_coffee.is_none_or(|t| t.elapsed() >= COFFEE_EVERY);
                if coffee_due && self.random() % 3 == 0 {
                    return Anim::Coffee;
                }
                // quase sempre escrevendo; às vezes apaga, amassa, pensa, tem uma ideia, batuca
                const BUSY: [Anim; 15] = [
                    Anim::Write,
                    Anim::Write,
                    Anim::Write,
                    Anim::Write,
                    Anim::Write,
                    Anim::Write,
                    Anim::Erase,
                    Anim::Erase,
                    Anim::Crumple,
                    Anim::Think,
                    Anim::Think,
                    Anim::Idea,
                    Anim::Tap,
                    Anim::Blink,
                    Anim::Look,
                ];
                BUSY[(self.random() % BUSY.len() as u64) as usize]
            }
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
            (Mood::Busy, vec![Anim::Write, Anim::Erase, Anim::Crumple, Anim::Think, Anim::Idea, Anim::Tap, Anim::Coffee, Anim::Blink, Anim::Look, Anim::Wave]),
            (Mood::Worried, vec![Anim::Sweat, Anim::Shake, Anim::Blink]),
            // o fim do trabalho toca já com o humor de depois
            (Mood::Normal, vec![Anim::Done]),
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
                // e nada sai muito da grade (cabe no espaço reservado em volta do desenho: os
                // "z" sobem até 3 quadradinhos acima e vão até 5 à direita)
                for k in 0..=40 {
                    let p = pose(mood, Some((anim, k as f32 / 40.0)), None);
                    assert!(grid(&p).iter().all(|&(x, y)| (-2..COLS as i32 + 5).contains(&x) && (-3..ROWS as i32 + 1).contains(&y)), "{anim:?} sai da grade");
                }
            }
        }
    }

    #[test]
    fn writes_the_page_letter_by_letter_and_turns_it() {
        let letters = |ink: u16| ink.count_ones() as i32;
        let all: i32 = WORDS.iter().map(|w| w.2).sum();
        for anim in [Anim::Write, Anim::Erase, Anim::Crumple] {
            // uma letra por vez (escrevendo ou apagando), com o lápis na letra que mudou
            let (mut before, mut lifted, mut rubbed) = (0u16, 0, 0);
            for k in 0..=400 {
                let t = k as f32 / 400.0 * write_end(anim) * 0.999;
                let page = page_at(anim, t);
                assert!(!page.lifted && page.ball.is_none());
                assert!((letters(page.ink) - letters(before)).abs() <= 1, "{anim:?}: pulou letras em {t}");
                let pen = page.pen.expect("o lápis some escrevendo");
                if page.ink != before {
                    assert!(pen.down && page.ink ^ before == bit(pen.x, pen.y), "{anim:?}: a letra mudou longe do lápis");
                    // escreve com a ponta e apaga com a borracha
                    assert_eq!(pen.eraser, page.ink < before, "{anim:?}: lápis do lado errado");
                    rubbed += (page.ink < before) as i32;
                }
                lifted += (!pen.down) as i32;
                before = page.ink;
            }
            assert_eq!(letters(before), all, "{anim:?}: não terminou a página");
            assert!(lifted > 0, "{anim:?}: não levanta o lápis entre as palavras");
            assert_eq!(rubbed, if anim == Anim::Erase { WORDS[3].2 } else { 0 }, "{anim:?}: apagou o que não devia");
            // depois vira a página (ou amassa) e volta ao começo: o fim emenda com o caderno
            // parado, sem pulo
            assert!(page_at(anim, write_end(anim) + 0.02).lifted);
            assert_eq!(pose(Mood::Busy, Some((anim, 0.995)), None).notebook, pose(Mood::Busy, None, None).notebook);
            assert_eq!(page_at(anim, 0.0).ink, 0);
        }
        // tudo dentro da folha, e a folha dentro do corpo e em cima do chão
        for &(y, x, n) in &WORDS {
            assert!(x > PAGE_X.0 && x + n < PAGE_X.1 && y > PAGE_Y.0 + 1 && y < PAGE_Y.1 - 1);
        }
        assert!(PAGE_X.0 > 3 && PAGE_X.1 < 15 && PAGE_Y.1 <= ROWS as i32);
        // e os olhos (olhando para baixo) não ficam cobertos pela folha
        let cells = cells(&pose(Mood::Busy, Some((Anim::Write, 0.3)), None));
        let eyes = cells.iter().position(|c| c.4 == EYE).unwrap();
        let page = cells.iter().position(|c| c.4 == COVER).unwrap();
        assert!(eyes < page && cells.iter().filter(|c| c.4 == EYE).all(|c| c.1 + c.3 <= PAGE_Y.0));
    }

    #[test]
    fn types_on_the_laptop_on_his_lap() {
        let at = |anim, t| on_laptop(pose(Mood::Busy, Some((anim, t)), None), None);
        // parado: o notebook aberto no colo, as garras no teclado, olhando a tela; sem o caderno
        let still = on_laptop(pose(Mood::Busy, None, None), None);
        assert_eq!(still.laptop, Some(Laptop { open: true, key: None }));
        assert!(still.notebook.is_none() && still.claws == Some(ON_KEYS) && still.eyes == Eyes::Open);
        // escrevendo uma página: uma garra de cada vez bate numa tecla, e elas se revezam
        let mut sides = Vec::new();
        for k in 0..=400 {
            let p = at(Anim::Write, k as f32 / 400.0 * write_end(Anim::Write) * 0.999);
            let [l, r] = p.claws.unwrap();
            assert!(!(l.1 == TAP_DOWN && r.1 == TAP_DOWN), "as duas garras batem juntas");
            if l.1 == TAP_DOWN || r.1 == TAP_DOWN {
                let key = p.laptop.unwrap().key.expect("bate sem tecla acesa");
                assert!((3..15).contains(&key));
                let side = l.1 == TAP_DOWN;
                if sides.last() != Some(&side) {
                    sides.push(side);
                }
            }
        }
        assert!(sides.len() >= 8, "não reveza as garras: {sides:?}");
        // a tampa fica embaixo dos olhos (ele enxerga a tela), e tudo dentro da grade, com as
        // garras por cima do notebook
        for anim in [Anim::Write, Anim::Erase, Anim::Crumple, Anim::Think, Anim::Idea, Anim::Tap, Anim::Coffee, Anim::Wave, Anim::Blink, Anim::Look] {
            for k in 0..=40 {
                let p = at(anim, k as f32 / 40.0);
                assert!(p.notebook.is_none() && p.laptop.is_some(), "{anim:?}: sem o notebook");
                let cells = cells(&p);
                assert!(cells.iter().filter(|c| c.4 == EYE).all(|c| c.1 + c.3 <= 4), "{anim:?}: a tampa cobre os olhos");
                let lid = cells.iter().position(|c| c.4 == ALU).unwrap();
                let claw = cells.iter().rposition(|c| c.4 == BODY).unwrap();
                assert!(claw > lid, "{anim:?}: a garra fica atrás do notebook");
                assert!(grid(&p).iter().all(|&(x, y)| (-2..COLS as i32 + 5).contains(&x) && (-3..ROWS as i32 + 1).contains(&y)), "{anim:?} sai da grade");
            }
        }
        // com uma ideia, levanta as garras do teclado; com a caneca na boca, a da direita a segura
        assert_eq!(at(Anim::Idea, 0.5).claws, Some([(1, 2), (15, 2)]));
        assert_eq!(at(Anim::Coffee, 0.5).claws.unwrap()[1], (15, 4));
        // pensando, continua olhando para cima, para os pontinhos
        assert_eq!(at(Anim::Think, 0.5).eyes, Eyes::Look(1, -1));
        // o mouse por perto: olha para ele, não para a tela
        assert_eq!(on_laptop(pose(Mood::Busy, Some((Anim::Write, 0.3)), Some((1, 1))), Some((1, 1))).eyes, Eyes::Look(1, 1));
        // terminou: fecha a tampa, depois se espreguiça sem o notebook
        let done = |t| on_laptop(pose(Mood::Normal, Some((Anim::Done, t)), None), None);
        assert_eq!(done(0.1).laptop, Some(Laptop { open: false, key: None }));
        assert!(!done(0.1).closed, "fechou o caderno em vez da tampa");
        assert!(done(0.35).laptop.is_none() && done(0.9).laptop.is_none());
        // sem trabalho, nada muda
        assert_eq!(on_laptop(pose(Mood::Normal, None, None), None), pose(Mood::Normal, None, None));
    }

    #[test]
    fn programs_on_the_side_laptop_with_the_screen_showing() {
        let at = |anim, t| on_side_laptop(pose(Mood::Busy, Some((anim, t)), None), None);
        let code = |p: &Pose| match p.side.unwrap().screen {
            Screen::Code(n) => n,
            Screen::Done => -1,
        };
        // parado: a tela aberta, ainda vazia, olhando para ela
        let still = on_side_laptop(pose(Mood::Busy, None, None), None);
        assert!(still.side == Some(Side { open: true, screen: Screen::Code(0) }) && still.notebook.is_none());
        assert_eq!(still.eyes, Eyes::Look(1, 0));
        // escrevendo uma página: o código cresce letra a letra, a garra direita bate no teclado
        let (mut before, mut taps, mut last_y) = (0, 0, 0);
        for k in 0..=400 {
            let p = at(Anim::Write, k as f32 / 400.0 * write_end(Anim::Write) * 0.999);
            let n = code(&p);
            assert!(n >= before && n - before <= CODE_PER_LETTER, "o código pula em {k}");
            before = n;
            let y = p.claws.unwrap()[1].1;
            taps += (y != last_y) as i32;
            last_y = y;
        }
        let full: i32 = LINES.iter().map(|l| l.1).sum();
        assert!(before >= full - CODE_PER_LETTER && taps >= 8, "{before} {taps}");
        // a tela mostra no máximo 5 linhas, dentro dela, com o cursor também dentro
        for n in 0..=full {
            let (lines, (cx, cy)) = code_cells(n);
            assert!(lines.len() <= 5);
            assert!(lines.iter().all(|&(x, y, w, _)| x >= 19 && x + w <= 25 && (2..7).contains(&y)), "{n}");
            assert!((19..25).contains(&cx) && (2..7).contains(&cy), "cursor fora da tela em {n}");
        }
        // tudo dentro do espaço reservado (até a coluna 26), em todas as animações
        for anim in [Anim::Write, Anim::Erase, Anim::Crumple, Anim::Think, Anim::Idea, Anim::Tap, Anim::Coffee, Anim::Wave, Anim::Blink, Anim::Look] {
            for k in 0..=40 {
                let p = at(anim, k as f32 / 40.0);
                assert!(grid(&p).iter().all(|&(x, y)| (-2..SIDE_COLS as i32).contains(&x) && (-3..ROWS as i32 + 1).contains(&y)), "{anim:?} sai da grade");
            }
        }
        // o café não fica à mostra: a garra vai atrás das costas, a caneca vem à boca
        assert!(at(Anim::Coffee, 0.1).reach_back && at(Anim::Coffee, 0.1).mug.is_none());
        let sip = at(Anim::Coffee, 0.5);
        assert!(sip.mug_front && !sip.reach_back && sip.eyes == Eyes::Closed);
        assert!(at(Anim::Coffee, 0.99).mug.is_none() && !at(Anim::Coffee, 0.99).reach_back);
        // terminou: o ✓ na tela, com as garras para cima; depois a tampa fecha no chão
        let done = |t| on_side_laptop(pose(Mood::Normal, Some((Anim::Done, t)), None), None);
        assert_eq!(done(0.1).side.unwrap().screen, Screen::Done);
        assert!(done(0.1).left_up && done(0.1).right_up && !done(0.1).closed);
        assert!(done(0.35).side.is_some_and(|s| !s.open) && done(0.9).side.is_some_and(|s| !s.open));
        // sem trabalho, nada muda
        assert_eq!(on_side_laptop(pose(Mood::Normal, None, None), None), pose(Mood::Normal, None, None));
    }

    #[test]
    fn jumps_crouched_and_with_claws_up() {
        let (crouch, air) = (jump_pose(true), jump_pose(false));
        assert!(crouch.dy > 0 && crouch.eyes == Eyes::Closed);
        assert!(air.left_up && air.right_up && air.legs == Legs::Tucked);
        assert!(crouch.notebook.is_none() && crouch.laptop.is_none() && air.laptop.is_none(), "pula com o notebook");
    }

    #[test]
    fn crumples_the_page_and_throws_it_over_the_shoulder() {
        // a bolinha nasce na frente dele e cai do lado esquerdo, passando por cima
        let balls: Vec<(i32, i32)> = (0..=200).filter_map(|k| page_at(Anim::Crumple, k as f32 / 200.0).ball).collect();
        assert!(balls.len() > 10);
        let (first, last) = (balls[0], balls[balls.len() - 1]);
        assert!(first.0 > 6 && last.0 < 1, "não voa para a esquerda: {balls:?}");
        assert!(balls.iter().any(|b| b.1 < first.1), "não sobe");
        assert!(balls.windows(2).all(|w| w[1].0 <= w[0].0), "volta para trás");
        // e os olhos acompanham
        assert_eq!(pose(Mood::Busy, Some((Anim::Crumple, 0.85)), None).eyes, Eyes::Look(-1, -1));
    }

    #[test]
    fn thinks_has_ideas_taps_and_drinks_coffee() {
        let at = |anim, t| pose(Mood::Busy, Some((anim, t)), None);
        // pensando: olha para cima e os pontinhos aparecem um a um
        let dots: Vec<u8> = (0..=20).map(|k| at(Anim::Think, k as f32 / 20.0).dots).collect();
        assert!(dots.windows(2).all(|w| w[1] >= w[0]) && dots[0] == 0 && dots[20] == 3, "{dots:?}");
        assert_eq!(at(Anim::Think, 0.5).eyes, Eyes::Look(1, -1));
        // a ideia: a lâmpada aparece apagada, pisca e acende; ele levanta as garras
        assert_eq!(at(Anim::Idea, 0.2).bulb, Some((false, false)));
        assert!(at(Anim::Idea, 0.32).bulb.unwrap().0 && !at(Anim::Idea, 0.4).bulb.unwrap().0, "não pisca");
        assert!(at(Anim::Idea, 0.9).bulb.unwrap().0 && at(Anim::Idea, 0.5).left_up);
        // batucando: o lápis sobe e desce no mesmo lugar
        let taps: Vec<bool> = (0..16).map(|k| at(Anim::Tap, k as f32 / 16.0).notebook.unwrap().pen.unwrap().down).collect();
        assert!(taps.windows(2).filter(|w| w[0] != w[1]).count() >= 6);
        // o café: no chão, levantado de olhos fechados, de volta e some no fim
        assert_eq!(at(Anim::Coffee, 0.1).mug.map(|m| m.0), Some(false));
        assert_eq!(at(Anim::Coffee, 0.5).mug.map(|m| m.0), Some(true));
        assert_eq!(at(Anim::Coffee, 0.5).eyes, Eyes::Closed);
        assert_eq!(at(Anim::Coffee, 0.99).mug, None);
        // a garra continua à vista segurando a caneca levantada
        let up = cells(&at(Anim::Coffee, 0.5));
        let claw = up.iter().position(|c| c.0 == 15 && c.1 == 4 && c.4 == BODY).unwrap();
        let mug = up.iter().position(|c| c.4 == MUG).unwrap();
        assert!(claw < mug && MUG_UP.0 > 15);
    }

    #[test]
    fn closes_the_notebook_stretches_and_checks_when_done() {
        let at = |t| pose(Mood::Normal, Some((Anim::Done, t)), None);
        assert!(at(0.1).closed && at(0.1).notebook.is_none());
        let stretch = at(0.35);
        assert!(stretch.legs == Legs::Stretch && stretch.left_up && stretch.right_up && stretch.dy == -1 && stretch.eyes == Eyes::Closed);
        // de pé no chão, esticado: as patas chegam até embaixo
        assert!(grid(&stretch).contains(&(4, ROWS as i32 - 1)));
        assert!(at(0.6).check && at(0.95).check && !at(0.72).check, "o ✓ não pisca");
        // e acaba como ele fica parado (sem o caderno)
        assert_eq!(cells(&at(1.0)).len(), cells(&pose(Mood::Normal, None, None)).len() + CHECK_MARK.iter().map(|r| r.split('.').filter(|s| !s.is_empty()).count()).sum::<usize>());
    }

    #[test]
    fn an_idea_speeds_up_the_next_page_and_coffee_waits_for_a_long_session() {
        let mut crab = Crab::new();
        crab.mood = Mood::Busy;
        crab.play(Anim::Idea);
        assert_eq!(crab.pick(Mood::Busy), Anim::Write, "depois da ideia, escreve");
        crab.play(Anim::Write);
        let fast = crab.anim.unwrap().2;
        crab.play(Anim::Write);
        assert!((fast - crab.pace * 0.5).abs() < 1e-4 && (crab.anim.unwrap().2 - crab.pace).abs() < 1e-4, "só a página seguinte é mais rápida");
        // o ritmo do Claude Code muda a velocidade, dentro de limites
        assert!(pace_for(0.0) == PACE_SLOW && pace_for(1000.0) == PACE_FAST && pace_for(20.0) < pace_for(5.0));
        // café só trabalhando há muito tempo, e não de novo logo em seguida
        let mut crab = Crab::new();
        assert!((0..300).all(|_| crab.pick(Mood::Busy) != Anim::Coffee));
        crab.tired = true;
        assert!((0..300).any(|_| crab.pick(Mood::Busy) == Anim::Coffee));
        crab.play(Anim::Coffee);
        assert!((0..300).all(|_| crab.pick(Mood::Busy) != Anim::Coffee));
    }

    #[test]
    fn zs_rise_grow_and_fade() {
        // um de cada vez: primeiro o pequeno, depois o grande, mais acima e à direita
        let small = zs_at(0.1);
        let big = zs_at(0.6);
        assert!(small.len() == 1 && big.len() == 1);
        assert!(small[0].0.len() < big[0].0.len(), "não cresce");
        assert!(big[0].1 > small[0].1 && big[0].2 < small[0].2, "não é uma escadinha");
        let mut alphas = Vec::new();
        for k in 0..=400 {
            let t = k as f32 / 400.0;
            let zs = zs_at(t);
            assert!(zs.len() <= 1, "dois \"z\" ao mesmo tempo em {t}");
            for &(glyph, x, y, alpha) in &zs {
                // nada encosta na cabeça nem nas garras (caídas, dormindo) e nada vai além do
                // espaço reservado (no grande, o título começa 5 quadradinhos depois da grade)
                let w = glyph[0].len() as i32;
                assert!(x > 15 && x + w <= COLS as i32 + 4 && y >= -3, "fora do lugar em {t}");
                assert!(y + glyph.len() as i32 <= 4, "encosta na garra em {t}");
                alphas.push((t, alpha));
            }
        }
        // sobe: o mesmo "z" fica mais alto no fim do que no começo
        for (born, dies) in [(0.0, 0.48), (0.5, 1.0)] {
            let (a, b) = (zs_at(born + 0.01)[0], zs_at(dies - 0.01)[0]);
            assert!(b.2 < a.2, "não sobe");
            assert!(a.3 < 0.95 && b.3 < 0.5, "não aparece nem some aos poucos");
        }
        // a opacidade muda em degraus (poucos quadros diferentes) e some no fim do cochilo
        let steps: std::collections::BTreeSet<_> = alphas.iter().map(|a| (a.1 * 100.0) as i32).collect();
        assert!(steps.len() <= 4, "{steps:?}");
        assert!(zs_at(1.0).is_empty());
        // e os de pixel art formam mesmo um "z": barras em cima e embaixo e uma diagonal da
        // direita para a esquerda no meio
        for g in [&Z_SMALL[..], &Z_BIG[..]] {
            let n = g[0].len();
            assert!(g.iter().all(|r| r.len() == n));
            assert!(g[0].bytes().all(|b| b == b'#') && g[g.len() - 1].bytes().all(|b| b == b'#'));
            let diag: Vec<_> = g[1..g.len() - 1].iter().map(|r| r.find('#').unwrap()).collect();
            assert!(diag.windows(2).all(|w| w[1] + 1 == w[0]) && diag[0] == n - 1, "{g:?}");
        }
    }

    #[test]
    fn reacts_to_the_mouse_on_the_first_frame() {
        // as animações do mouse (clique: pulo; chegar perto: aceno) já mudam o desenho no
        // primeiro quadro, em qualquer humor: a resposta aparece na hora, sem esperar o próximo
        for mood in [Mood::Normal, Mood::Sleepy, Mood::Busy, Mood::Worried] {
            let still = cells(&pose(mood, None, None));
            for anim in [Anim::Hop, Anim::Wave, Anim::Shake] {
                assert_ne!(cells(&pose(mood, Some((anim, 0.0)), None)), still, "{anim:?} com humor {mood:?} demora a aparecer");
            }
        }
        // e o próximo quadro vem logo
        let mut crab = Crab::new();
        crab.play(Anim::Hop);
        assert!(crab.is_playing(Anim::Hop));
        assert!(crab.next_tick() <= Instant::now() + Duration::from_millis(40));
    }

    #[test]
    fn writing_redraws_only_when_the_pencil_moves() {
        let mut crab = Crab::playing(Anim::Write, 0.33);
        crab.mood = Mood::Busy;
        assert!(crab.tick(Mood::Busy), "o primeiro quadro desenha");
        // logo em seguida o lápis ainda está na mesma letra: não redesenha
        assert!(!crab.tick(Mood::Busy), "redesenhou sem nada mudar");
        // começou a trabalhar: a escrita começa logo, sem esperar o intervalo de antes
        let mut crab = Crab::new();
        crab.next = Instant::now() + Duration::from_secs(14);
        crab.tick(Mood::Busy);
        assert!(crab.next_tick() <= Instant::now() + Duration::from_secs(1));
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
        crab.anim = Some((Anim::Blink, Instant::now() - Duration::from_secs(5), 0.2));
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
            let busy = [Anim::Write, Anim::Erase, Anim::Crumple, Anim::Think, Anim::Idea, Anim::Tap, Anim::Coffee, Anim::Done];
            assert!(![Anim::Snooze, Anim::Sweat, Anim::Shake].contains(&crab.pick(Mood::Normal)) && !busy.contains(&crab.pick(Mood::Normal)));
            let b = crab.pick(Mood::Busy);
            assert!(busy[..6].contains(&b) || [Anim::Blink, Anim::Look].contains(&b), "{b:?}");
            crab.inspired = false;
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
        // (o último bloco: o mesmo trabalho com o notebook no colo, como no pequeno)
        let busy = [Anim::Write, Anim::Erase, Anim::Crumple, Anim::Think, Anim::Idea, Anim::Tap, Anim::Coffee];
        let rows: Vec<(Mood, Option<Anim>, bool)> = std::iter::once((Mood::Normal, None, false))
            .chain([Anim::Blink, Anim::Look, Anim::Scuttle, Anim::Hop, Anim::Wave].map(|a| (Mood::Normal, Some(a), false)))
            .chain([(Mood::Sleepy, None, false), (Mood::Sleepy, Some(Anim::Snooze), false), (Mood::Busy, None, false)])
            .chain(busy.map(|a| (Mood::Busy, Some(a), false)))
            .chain([(Mood::Normal, Some(Anim::Done), false), (Mood::Worried, Some(Anim::Sweat), false), (Mood::Worried, Some(Anim::Shake), false)])
            .chain(std::iter::once((Mood::Busy, None, true)))
            .chain(busy.map(|a| (Mood::Busy, Some(a), true)))
            .chain(std::iter::once((Mood::Normal, Some(Anim::Done), true)))
            .collect();
        let (cell_w, cell_h, frames) = (110.0, 80.0, 16);
        let mut c = Canvas::new((cell_w * frames as f32) as u32, (cell_h * rows.len() as f32) as u32, 1.0);
        c.fill_round_rect(0.0, 0.0, c.w as f32, c.h as f32, 0.0, Rgba::rgb(60, 50, 90));
        for (r, (mood, anim, laptop)) in rows.iter().enumerate() {
            for f in 0..frames {
                let t = f as f32 / (frames - 1) as f32;
                let p = pose(*mood, anim.map(|a| (a, t)), None);
                let p = if *laptop { on_laptop(p, None) } else { p };
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
