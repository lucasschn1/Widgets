//! O Clawd, o caranguejo do Claude Code, em pixel art, com animações curtas.
//!
//! O desenho é o do logo que o Claude Code mostra no terminal, numa grade de 27 × 15
//! quadradinhos, com volume: uma linha de luz em cima e à esquerda, sombra à direita e embaixo,
//! brilho nos olhos, garras com pinça e patas de dois quadradinhos. Cada quadradinho tem 2/3 do
//! `unit` de cada lugar (o tamanho dos quadradinhos da grade antiga, de 18 × 10), então ele ocupa
//! o mesmo espaço de antes, e cai em pixels inteiros da tela: fica nítido em qualquer escala.
//!
//! As animações tocam em episódios curtos, com intervalos parados entre eles: parado, o widget
//! não redesenha nada. O humor muda o repertório:
//! - parado: pisca, olha para os lados, anda de lado, acena, pula e, às vezes, põe o fone e fica
//!   ouvindo música;
//! - dormindo (sem números, ou muito tempo sem o Claude Code): sentado, de gorro, cochila com os
//!   "z"; acorda devagar quando o Claude Code volta, ou num susto com o mouse;
//! - trabalhando: no notebook (de frente, no colo, no pequeno; de lado, com o código na tela, no
//!   médio, no grande e na barra de tarefas), digita, pensa, tem ideias e, depois de um bom tempo,
//!   toma um café; de vez em quando, a tarefa inteira de Sherlock Holmes (chapéu de detetive,
//!   lupa e, de lado, cachimbo); no fim, fecha o notebook, se espreguiça e mostra um ✓;
//! - perto do limite: corado, suando e tremendo;
//! - no limite: finca a plaquinha "VOLTO LOGO" do lado e fica lendo até o limite reiniciar;
//!   aí recolhe a placa e comemora com confete.

use std::time::{Duration, Instant};

use crate::gfx::Canvas;
use crate::theme::*;

/// A grade do desenho, em quadradinhos.
pub const COLS: f32 = 27.0;
pub const ROWS: f32 = 15.0;
/// A largura do desenho com o notebook de lado, à direita (até a coluna 36).
pub const SIDE_COLS: f32 = 37.0;
/// O tamanho de um quadradinho, em `unit` (o quadradinho da grade antiga, de 18 × 10): a grade
/// nova ocupa o mesmo espaço, 18 × 10 `unit`.
pub const CELL: f32 = 2.0 / 3.0;

// ---------- cores ----------
/// o corpo: a cor do Clawd no Claude Code, a luz e a sombra
const BODY: Rgba = Rgba::rgb(215, 119, 87);
const LIGHT: Rgba = Rgba::rgb(235, 152, 116);
const SHADE: Rgba = Rgba::rgb(182, 94, 64);
/// os olhos (com o brilho), a boca e a língua
const EYE: Rgba = Rgba::rgb(28, 20, 18);
const SHINE: Rgba = Rgba::rgb(255, 243, 234);
const MOUTH: Rgba = Rgba::rgb(91, 29, 26);
const TONGUE: Rgba = Rgba::rgb(226, 104, 122);
/// o gorro de dormir
const CAP: Rgba = Rgba::rgb(111, 141, 230);
const CAP_LIGHT: Rgba = Rgba::rgb(169, 188, 245);
const CAP_WHITE: Rgba = Rgba::rgb(243, 243, 250);
/// os notebooks: o alumínio, o teclado, o símbolo laranja, a moldura e a tela, o código
const ALU: Rgba = Rgba::rgb(201, 204, 214);
const ALU_LIGHT: Rgba = Rgba::rgb(230, 232, 240);
const ALU_DARK: Rgba = Rgba::rgb(139, 143, 156);
const KEYS: Rgba = Rgba::rgb(93, 96, 110);
const KEY_LIT: Rgba = Rgba::rgb(212, 216, 232);
const LOGO: Rgba = Rgba::rgb(242, 140, 40);
const BEZEL: Rgba = Rgba::rgb(42, 44, 54);
const SCREEN: Rgba = Rgba::rgb(27, 35, 54);
const CODE: [Rgba; 5] = [Rgba::rgb(242, 140, 40), Rgba::rgb(120, 196, 255), Rgba::rgb(112, 214, 128), Rgba::rgb(242, 132, 164), Rgba::rgb(255, 226, 92)];
/// o ✓, a lâmpada, o suor, o coração, o "!"
const CHECK: Rgba = Rgba::rgb(112, 214, 128);
const BULB: Rgba = Rgba::rgb(255, 226, 92);
const BULB_OFF: Rgba = Rgba::rgb(154, 154, 168);
const SWEAT: Rgba = Rgba::rgb(134, 203, 255);
const HEART: Rgba = Rgba::rgb(255, 111, 145);
const BANG: Rgba = Rgba::rgb(255, 226, 92);
/// a plaquinha "VOLTO LOGO"
const WOOD: Rgba = Rgba::rgb(217, 160, 102);
const WOOD_DARK: Rgba = Rgba::rgb(154, 100, 52);
const SIGN_TEXT: Rgba = Rgba::rgb(58, 36, 22);
/// o livro: as páginas, a capa, a lombada e o título dourado
const PAGE: Rgba = Rgba::rgb(246, 241, 228);
const COVER: Rgba = Rgba::rgb(92, 128, 214);
const COVER_DARK: Rgba = Rgba::rgb(62, 90, 168);
const GOLD: Rgba = Rgba::rgb(255, 209, 102);
/// o chapéu de detetive
const HAT: Rgba = Rgba::rgb(110, 93, 82);
const HAT_TWEED: Rgba = Rgba::rgb(127, 108, 95);
const HAT_LIGHT: Rgba = Rgba::rgb(147, 128, 111);
const HAT_SEAM: Rgba = Rgba::rgb(76, 63, 55);
const HAT_BAND: Rgba = Rgba::rgb(58, 47, 41);
const HAT_FLAP: Rgba = Rgba::rgb(133, 114, 99);
const HAT_PEAK: Rgba = Rgba::rgb(100, 83, 71);
const HAT_LINE: Rgba = Rgba::rgb(35, 28, 25);
/// a lupa: o aro, o vidro, o brilho e o cabo
const LENS: Rgba = Rgba::rgb(43, 43, 51);
const GLINT: Rgba = Rgba::rgb(255, 255, 255);
const HANDLE: Rgba = Rgba::rgb(107, 78, 51);
/// o cachimbo e a brasa
const PIPE_WOOD: Rgba = Rgba::rgb(138, 78, 43);
const PIPE_STEM: Rgba = Rgba::rgb(58, 36, 24);
const PIPE_RIM: Rgba = Rgba::rgb(46, 26, 16);
const EMBER: Rgba = Rgba::rgb(242, 140, 40);
const SMOKE: Rgba = Rgba::rgb(217, 217, 227);
/// a caneca: a borda, o café, a louça, a sombra e a asa; o vapor
const CUP_RIM: Rgba = Rgba::rgb(255, 248, 236);
const BREW: Rgba = Rgba::rgb(74, 42, 24);
const CUP: Rgba = Rgba::rgb(245, 233, 211);
const CUP_SHADE: Rgba = Rgba::rgb(214, 191, 152);
const CUP_HANDLE: Rgba = Rgba::rgb(230, 211, 178);
const STEAM: Rgba = Rgba::rgb(235, 203, 156);
/// o fone: o contorno, o azul, o azul claro e a espuma do arco; as notas
const PHONE_LINE: Rgba = Rgba::rgb(15, 35, 80);
const PHONE: Rgba = Rgba::rgb(36, 81, 158);
const PHONE_LIGHT: Rgba = Rgba::rgb(74, 130, 214);
const PHONE_PAD: Rgba = Rgba::rgb(242, 242, 246);
const NOTE: Rgba = Rgba::rgb(255, 209, 102);

fn glow() -> Rgba {
    Rgba::rgb(255, 196, 170).alpha(0.5)
}
fn blush(hot: bool) -> Rgba {
    if hot { Rgba::rgb(255, 72, 72).alpha(0.62) } else { Rgba::rgb(255, 128, 138).alpha(0.6) }
}
fn glass() -> Rgba {
    Rgba::rgb(222, 232, 246).alpha(0.82)
}

// ---------- desenhos em texto ----------
const Z1: [&str; 4] = ["###", "..#", ".#.", "###"];
const Z2: [&str; 5] = ["####", "...#", "..#.", ".#..", "####"];
const Z3: [&str; 6] = ["#####", "....#", "...#.", "..#..", ".#...", "#####"];
const BANG_MARK: [&str; 6] = ["##", "##", "##", "##", "..", "##"];
const HEART_S: [&str; 3] = ["#.#", "###", ".#."];
const HEART_L: [&str; 5] = [".#.#.", "#####", "#####", ".###.", "..#.."];
const CHECK_MARK: [&str; 6] = ["......#", ".....##", "#...##.", "##.##..", ".###...", "..#...."];
const BULB_GLASS: [&str; 4] = [".###.", "#####", "#####", ".###."];
const BULB_BASE: [&str; 2] = [".###.", "..#.."];
const LOGO_MARK: [&str; 5] = ["#.#.#", ".###.", "#####", ".###.", "#.#.#"];
/// as letras da plaquinha (3 × 4)
fn sign_letter(ch: char) -> [&'static str; 4] {
    match ch {
        'V' => ["#.#", "#.#", "#.#", ".#."],
        'O' => ["###", "#.#", "#.#", "###"],
        'L' => ["#..", "#..", "#..", "###"],
        'T' => ["###", ".#.", ".#.", ".#."],
        _ => ["###", "#..", "#.#", "###"], // G
    }
}
/// o gorro de dormir (desenhado espelhado: a ponta e o pompom caem para a esquerda): b azul,
/// l azul claro, w branco
const CAP_ART: [&str; 6] = ["......bbbbb", "....bbbblbbbb", "...bbblbbbbbbbb", "..bbbbbblbbbb..bb", ".wwwwwwwwwwwww..ww", "................ww"];
/// o chapéu de detetive (deerstalker), das colunas 2 a 24: o laço das abas amarrado em cima, a
/// copa em gomos de tweed (costura no meio, luz à esquerda), a faixa, as abas de orelha
/// dobradas para cima e a pala sobre a testa. o contorno, h tweed, t ponto do tweed, l luz,
/// m costura, d faixa, F aba de orelha, b pala
const HAT_ART: [&str; 9] = [
    "........oo...oo........",
    ".......ooohhhooo.......",
    ".....oohlhtmhhlhoo.....",
    "....ohlhthhmhthhlho....",
    "...ohlhhhhtmhhhhthho...",
    "..oFFhthhhhmhhthhhFFo..",
    "..oFFdddddddddddddFFo..",
    "...oo...obbbbbo...oo...",
    ".......................",
];
/// a lupa: lente redonda (7 × 7, com brilho) e o cabo descendo para a esquerda
const MAG_ART: [&str; 10] = ["....KKK..", "...KGGWK.", "..KGGGGWK", "..KGGGGGK", "..KGGGGGK", "...KGGGK.", "....KKK..", "...HH....", "..HH.....", ".HH......"];
/// o cachimbo no canto da boca
const PIPE_ART: [&str; 4] = ["oEo...", "PPPTTT", "PPP...", ".P...."];
/// a caneca, com a asa à direita (para o gole, espelhada)
const CUP_ART: [&str; 5] = ["RkkkR..", "MMMMMHH", "MMMMS.H", "MMMMSHH", ".SSS..."];
const CUP_SIP: [&str; 5] = ["..RkkkR", "HHMMMMM", "H.SMMMM", "HHSMMMM", "...SSS."];
/// o fone: o arco por cima da cabeça (colunas 1 a 25) e as conchas dos lados
const PHONES_ART: [&str; 10] = [
    ".......nnnnnnnnnnn.......",
    ".....nnbbbbbbbbbbbnn.....",
    "....nbbwwwwwwwwwwwbbn....",
    "...nbn.............nbn...",
    "..nbn...............nbn..",
    ".nn...................nn.",
    "nBbn.................nbBn",
    "nBbn.................nbBn",
    "nbbn.................nbbn",
    ".nn...................nn.",
];
/// a colcheia e a nota pequena
const NOTE8: [&str; 5] = [".##.", ".#.#", ".#..", "##..", "##.."];
const NOTE1: [&str; 1] = ["#"];
/// o livro aberto, visto por trás (ele lê olhando para baixo, os olhos espiando por cima)
const READ_ART: [&str; 7] = [".PPPP.PPPP.", "CCCCCSCCCCC", "C.g.CSCCCCC", "CgggCSCCCCC", "C.g.CSCCCCC", "CCCCCSCCCCC", "DDDDDSDDDDD"];

/// os "z": desenho, canto de cima, quando nasce e quando some (fração do ciclo)
const ZS: [(&[&str], i32, i32, f32, f32); 3] = [(&Z1, 21, -1, 0.0, 0.45), (&Z2, 22, -4, 0.28, 0.72), (&Z3, 22, -8, 0.55, 0.98)];
/// os pontinhos de quem pensa: (coluna, linha, tamanho)
const DOTS: [(i32, i32, i32); 3] = [(23, 1, 1), (25, -1, 2), (28, -4, 2)];
/// as linhas de código na tela do notebook de lado: (recuo, letras, cor)
const LINES: [(i32, i32, usize); 10] = [(0, 4, 0), (1, 3, 1), (1, 5, 2), (2, 3, 3), (1, 4, 1), (0, 2, 4), (1, 5, 0), (2, 4, 2), (1, 3, 1), (0, 4, 3)];
/// as letras de uma tela cheia
pub const CODE_TOTAL: i32 = 37;
/// as teclas que cada garra aperta no notebook de frente
const KEYS_LEFT: [i32; 6] = [8, 10, 7, 12, 9, 11];
const KEYS_RIGHT: [i32; 6] = [17, 15, 18, 14, 16, 19];
/// a lupa investigando: perto do rosto (de frente) ou passando pelo código na tela (de lado),
/// com a cor da linha aumentada dentro da lente
const MAG_FRONT: [(i32, i32); 4] = [(20, -2), (19, -1), (20, -1), (19, -2)];
const MAG_SIDE: [(i32, i32, usize); 6] = [(26, 2, 0), (27, 2, 1), (26, 3, 2), (27, 3, 3), (26, 4, 4), (27, 4, 0)];

/// Quantas vezes por segundo confere o quadro enquanto anima; só redesenha quando o desenho
/// muda, então conferir mais vezes deixa os movimentos no tempo certo sem custar desenhos.
const FPS: u64 = 30;
/// Segundos de digitação por vez, sem saber o ritmo do Claude Code; e o mais rápido e o mais
/// devagar.
pub const PACE: f32 = 4.2;
const PACE_FAST: f32 = 2.4;
const PACE_SLOW: f32 = 7.0;
/// Teclas por segundo, digitando.
const KEYS_PER_SEC: f32 = 6.0;
/// Trabalhando há tanto tempo, ele pode parar para um café; e não toma outro antes de tanto.
pub const COFFEE_AFTER: Duration = Duration::from_secs(20 * 60);
const COFFEE_EVERY: Duration = Duration::from_secs(12 * 60);
/// Uma tarefa em cada tantas é de Sherlock Holmes.
const SHERLOCK_ONE_IN: u64 = 4;

/// Como o Clawd está: muda as animações que ele faz.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Normal,
    /// sem números para mostrar (não conectado ou esperando o Claude Code): dorme
    Sleepy,
    /// o Claude Code está trabalhando agora: fica no notebook
    Busy,
    /// o diário perto do limite: sua e treme
    Worried,
    /// o diário chegou ao limite: a plaquinha "VOLTO LOGO" e um livro
    Limit,
}

/// De onde ele é visto trabalhando: o notebook de frente, no colo (o pequeno), ou de lado, no
/// chão, com a tela à mostra (o médio, o grande e a barra de tarefas).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Front,
    Side,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anim {
    Blink,
    Look,
    Scuttle,
    Hop,
    Wave,
    /// fecha os olhos sorrindo, fica corado e sobe um coração
    Love,
    /// põe o fone e fica curtindo, balançando no ritmo, com notinhas subindo
    Music,
    /// o limite reiniciou: dois pulinhos com confete
    Party,
    /// dormindo: respira, a boquinha abre e os "z" saem em fila
    Snooze,
    /// o Claude Code voltou: acorda devagar (um olho, bocejo, espreguiçada, o gorro sai)
    Wake,
    /// o mouse chegou com ele dormindo: acorda num susto e pula
    Startle,
    Sweat,
    Shake,
    /// digita (de frente, as garras se revezando; de lado, o código aparece na tela)
    Type,
    /// para, olha para cima e aparecem uns pontinhos
    Think,
    /// acende uma lâmpada; a digitação seguinte sai mais rápida
    Idea,
    /// toma um café: caneca do lado, olhos ^ ^, um gole
    Coffee,
    /// de Sherlock: põe o chapéu de detetive no começo da tarefa
    HatOn,
    /// de Sherlock: no lugar de pensar, investiga com a lupa
    Investigate,
    /// o Claude Code terminou: fecha o notebook, se espreguiça e mostra um ✓
    Done,
    /// chegou ao limite: leva um susto, anda de lado, finca a placa e abre o livro
    SignUp,
    /// o limite reiniciou: fecha o livro, recolhe a placa e volta para o meio
    SignDown,
    /// lendo: a página vira
    Read,
    /// ri de alguma parte do livro
    Laugh,
    /// cochila lendo e acorda
    Nod,
}

#[cfg(test)]
pub const ANIMS: [Anim; 25] = [
    Anim::Blink,
    Anim::Look,
    Anim::Scuttle,
    Anim::Hop,
    Anim::Wave,
    Anim::Love,
    Anim::Music,
    Anim::Party,
    Anim::Snooze,
    Anim::Wake,
    Anim::Startle,
    Anim::Sweat,
    Anim::Shake,
    Anim::Type,
    Anim::Think,
    Anim::Idea,
    Anim::Coffee,
    Anim::HatOn,
    Anim::Investigate,
    Anim::Done,
    Anim::SignUp,
    Anim::SignDown,
    Anim::Read,
    Anim::Laugh,
    Anim::Nod,
];

/// Os trechos (em ms) das animações de várias partes.
const MUSIC: [f32; 7] = [260.0, 300.0, 4800.0, 1600.0, 2800.0, 260.0, 700.0];
const WAKE: [f32; 13] = [550.0, 380.0, 120.0, 380.0, 700.0, 700.0, 260.0, 120.0, 260.0, 120.0, 520.0, 520.0, 900.0];
const STARTLE: [f32; 7] = [110.0, 420.0, 150.0, 500.0, 120.0, 900.0, 600.0];
const DONE_FRONT: [f32; 3] = [800.0, 900.0, 1400.0];
const DONE_SIDE: [f32; 3] = [1100.0, 900.0, 1100.0];
const SIGN_UP: [f32; 7] = [500.0, 700.0, 90.0, 90.0, 90.0, 800.0, 300.0];
const SIGN_DOWN: [f32; 6] = [600.0, 90.0, 90.0, 90.0, 700.0, 700.0];
const NOD: [f32; 3] = [900.0, 1300.0, 450.0];
/// a parte do pulo em que ele agacha antes de subir
const CROUCH: f32 = 0.1;

fn total(segs: &[f32]) -> f32 {
    segs.iter().sum::<f32>() / 1000.0
}

impl Anim {
    /// Duração, em segundos, digitando `pace` segundos por vez.
    pub fn secs(self, pace: f32) -> f32 {
        match self {
            Anim::Blink => 0.2,
            Anim::Look => 1.8,
            Anim::Scuttle => 1.7,
            Anim::Hop => 0.9,
            Anim::Wave => 1.1,
            Anim::Love => 2.2,
            Anim::Music => total(&MUSIC),
            Anim::Party => 3.2,
            Anim::Snooze => 5.6,
            Anim::Wake => total(&WAKE),
            Anim::Startle => total(&STARTLE),
            Anim::Sweat => 1.6,
            Anim::Shake => 0.8,
            Anim::Type => pace,
            Anim::Think => 1.5,
            Anim::Idea => 1.5,
            Anim::Coffee => 3.4,
            Anim::HatOn => 0.3,
            Anim::Investigate => 2.8,
            // (de frente e de lado, os mesmos 3,1 s)
            Anim::Done => total(&DONE_FRONT),
            Anim::SignUp => total(&SIGN_UP),
            Anim::SignDown => total(&SIGN_DOWN),
            Anim::Read => 3.2,
            Anim::Laugh => 1.1,
            Anim::Nod => total(&NOD),
        }
    }
}

/// Em que trecho de `segs` (durações em ms) está `ms`, e quanto dele já foi (0 a 1).
fn seg(ms: f32, segs: &[f32]) -> (usize, f32) {
    let mut acc = 0.0;
    for (i, &d) in segs.iter().enumerate() {
        if ms < acc + d || i == segs.len() - 1 {
            return (i, ((ms - acc) / d).clamp(0.0, 1.0));
        }
        acc += d;
    }
    (0, 0.0)
}

// ---------- o quadro ----------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Eyes {
    /// abertos, olhando para um lado: (-1, 0, 1) na horizontal e na vertical
    Open(i32, i32),
    Blink,
    /// dormindo: fechados em arco
    Sleep,
    /// felizes: ^ ^
    Happy,
    Half,
    /// um aberto e o outro fechado
    Peek,
    /// apertados: > <
    Squeeze,
    Wide,
    /// de detetive: o esquerdo bem aberto, o direito apertado
    Sherlock(i32, i32),
    /// fechados em traço, curtindo, com as bochechas coradas
    Chill,
}

impl Default for Eyes {
    fn default() -> Self {
        Eyes::Open(0, 0)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mouth {
    Yawn,
    O,
    Tiny,
    Smile,
    Grin,
    Wavy,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Legs {
    #[default]
    Stand,
    /// sentado: as patas dobradas
    Sit,
    /// no ar, encolhidas
    Tuck,
    Half,
    /// esticadas (se espreguiçando)
    Stretch,
    /// andando: um par levantado
    StepA,
    StepB,
    /// batendo o pé
    Tap,
}

impl Legs {
    fn heights(self) -> [i32; 4] {
        match self {
            Legs::Stand => [3, 3, 3, 3],
            Legs::Sit | Legs::Tuck => [1, 1, 1, 1],
            Legs::Half => [2, 2, 2, 2],
            Legs::Stretch => [4, 4, 4, 4],
            Legs::StepA => [2, 3, 2, 3],
            Legs::StepB => [3, 2, 3, 2],
            Legs::Tap => [3, 3, 3, 2],
        }
    }
}

/// Onde e como fica cada garra (a da direita é o espelho da da esquerda).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Claw {
    #[default]
    Rest,
    Up,
    High,
    /// caída no chão (dormindo)
    Limp,
    /// no teclado do notebook de frente, e apertando uma tecla
    Key,
    KeyDown,
    /// no teclado do notebook de lado
    SideKey,
    SideKeyDown,
    /// para baixo (pegando ou guardando a placa)
    Down,
    /// segurando a caneca do lado, e na frente da boca
    CupSide,
    CupFront,
    /// segurando o livro
    Book,
    /// fora do desenho (a da lupa vai junto com ela)
    Hidden,
}

impl Claw {
    fn art(self) -> Option<(&'static [&'static str], i32, i32)> {
        const REST: [&str; 3] = ["LBB", ".BB", "BBK"];
        const UP: [&str; 3] = ["L.B", "BBB", "BKK"];
        const FLAT: [&str; 2] = ["LBB", "BBK"];
        Some(match self {
            Claw::Rest => (&REST, 1, 6),
            Claw::Up => (&UP, 1, 2),
            Claw::High => (&UP, 1, 0),
            Claw::Limp => (&[".BBB", "KKKK"], 0, 11),
            Claw::Key => (&FLAT, 5, 11),
            Claw::KeyDown => (&FLAT, 5, 12),
            Claw::SideKey => (&["LBB", "KKK"], 1, 11),
            Claw::SideKeyDown => (&["LBB", "KKK"], 1, 12),
            Claw::Down => (&REST, 1, 9),
            Claw::CupSide => (&FLAT, 3, 6),
            Claw::CupFront => (&FLAT, 8, 6),
            Claw::Book => (&FLAT, 6, 8),
            Claw::Hidden => return None,
        })
    }
}

/// O notebook de frente, no colo.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Front {
    /// aberto (a tampa em pé) ou fechado, deitado
    pub open: bool,
    /// a tecla apertada agora (coluna)
    pub key: Option<i32>,
}

/// O notebook de lado, no chão à direita.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Side {
    pub open: bool,
    /// as letras de código na tela
    pub code: i32,
    /// o ✓ na tela (terminou)
    pub done: bool,
    pub key: Option<i32>,
}

/// A plaquinha: a altura (subindo do chão até 11), a coluna da esquerda e os pezinhos.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sign {
    pub h: i32,
    pub x: i32,
}

/// Um quadro do Clawd.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Pose {
    /// deslocamento, em quadradinhos
    pub dx: i32,
    pub dy: i32,
    /// o corpo um quadradinho mais alto (respirando)
    pub breath: bool,
    pub eyes: Eyes,
    pub mouth: Option<Mouth>,
    pub legs: Legs,
    pub claws: [Claw; 2],
    /// a luz da tela no rosto
    pub glow: bool,
    /// bochechas coradas (vermelhas de verdade, perto do limite)
    pub blush: Option<bool>,
    pub front: Option<Front>,
    pub side: Option<Side>,
    pub sign: Option<Sign>,
    /// o livro aberto: (quanto mais alto, a página virando: 0 parada, 1 a 3 passando)
    pub book: Option<(i32, u8)>,
    /// a caneca: (na boca, o vapor subindo)
    pub cup: Option<(bool, Option<f32>)>,
    /// o chapéu de detetive (e quanto acima da cabeça)
    pub hat: Option<i32>,
    /// a lupa: (coluna, linha, a cor da linha de código aumentada dentro dela)
    pub mag: Option<(i32, i32, Option<usize>)>,
    /// o cachimbo (com a fumaça subindo)
    pub pipe: Option<Option<f32>>,
    /// o fone (e quanto acima da cabeça) e as notas subindo
    pub phones: Option<i32>,
    pub notes: Option<f32>,
    /// o gorro de dormir: (quanto acima, opacidade)
    pub cap: Option<(i32, f32)>,
    /// os "z" (0 a 1 do ciclo) e quanto mais à direita
    pub zzz: Option<f32>,
    pub zzz_x: i32,
    pub bang: bool,
    pub sweat: Option<f32>,
    pub heart: Option<f32>,
    pub confetti: Option<f32>,
    pub check: bool,
    /// a lâmpada da ideia: (acesa, com os raios)
    pub bulb: Option<(bool, bool)>,
    /// pontinhos de quem está pensando (0 a 3)
    pub dots: u8,
}

/// De pé, olhando para a frente.
fn stand() -> Pose {
    Pose::default()
}

/// Dormindo, sentado: as garras caídas, de gorro; `ms` faz a respiração e os "z".
fn asleep(ms: Option<f32>) -> Pose {
    let mut p = Pose { dy: 2, legs: Legs::Sit, eyes: Eyes::Sleep, claws: [Claw::Limp; 2], cap: Some((0, 1.0)), ..stand() };
    if let Some(ms) = ms {
        let inhale = (ms / 1400.0) as i32 % 2 == 0;
        p.breath = inhale;
        p.mouth = (!inhale).then_some(Mouth::Tiny);
        p.zzz = Some((ms / 3600.0).fract());
    }
    p
}

/// No notebook de frente: olhando a tela, a luz no rosto, as garras no teclado.
fn front_base() -> Pose {
    Pose { eyes: Eyes::Open(0, 1), glow: true, front: Some(Front { open: true, key: None }), claws: [Claw::Key; 2], ..stand() }
}

/// No notebook de lado: olhando a tela, a garra direita no teclado.
fn side_base(code: i32) -> Pose {
    Pose { eyes: Eyes::Open(1, 0), side: Some(Side { open: true, code, done: false, key: None }), claws: [Claw::Rest, Claw::SideKey], ..stand() }
}

fn work_base(view: View, code: i32) -> Pose {
    match view {
        View::Front => front_base(),
        View::Side => side_base(code),
    }
}

/// Digitando: `n` teclas em `u` (0 a 1); de lado, o código vai de `a` a `b` letras.
fn typing(view: View, u: f32, n: i32, a: i32, b: i32) -> Pose {
    let k = (u * n as f32) as i32;
    let down = (u * n as f32).fract() < 0.55;
    match view {
        View::Front => {
            let left = k % 2 == 0;
            let mut p = front_base();
            let keys = if left { KEYS_LEFT } else { KEYS_RIGHT };
            p.front = Some(Front { open: true, key: down.then_some(keys[((k >> 1) % 6) as usize]) });
            p.claws = [if left && down { Claw::KeyDown } else { Claw::Key }, if !left && down { Claw::KeyDown } else { Claw::Key }];
            p
        }
        View::Side => {
            let mut p = side_base(a + ((b - a) as f32 * u).round() as i32);
            if let Some(s) = &mut p.side {
                s.key = down.then_some(25 + (k * 5) % 11);
            }
            p.claws[1] = if down { Claw::SideKeyDown } else { Claw::SideKey };
            p
        }
    }
}

/// A ideia: a lâmpada aparece apagada, pisca e acende de vez; ele levanta as garras.
fn idea(view: View, code: i32, u: f32) -> Pose {
    let lit = (0.25..0.32).contains(&u) || u >= 0.42;
    let up = (0.42..0.8).contains(&u);
    let mut p = work_base(view, code);
    p.eyes = if up { Eyes::Happy } else if lit { Eyes::Open(0, 0) } else { Eyes::Open(1, -1) };
    if u > 0.08 {
        p.bulb = Some((lit, lit && (u * 10.0) as i32 % 2 == 0));
    }
    if up {
        p.claws = [Claw::Up; 2];
    }
    p
}

/// O café: pega a caneca e a segura do lado (olhos ^ ^), dá um gole e solta o "ahh".
fn coffee(view: View, code: i32, u: f32, ms: f32) -> Pose {
    let mut p = work_base(view, code);
    let steam = Some((ms / 1500.0).fract());
    match u {
        u if u < 0.12 => {
            p.eyes = Eyes::Open(-1, 0);
            p.cup = Some((false, None));
            p.claws[0] = Claw::CupSide;
        }
        u if u < 0.42 => {
            p.eyes = Eyes::Happy;
            p.cup = Some((false, steam));
            p.claws[0] = Claw::CupSide;
        }
        u if u < 0.7 => {
            p.eyes = Eyes::Happy;
            p.blush = Some(false);
            p.cup = Some((true, steam));
            p.claws[0] = Claw::CupFront;
        }
        _ => {
            p.eyes = Eyes::Happy;
            p.mouth = Some(Mouth::Smile);
            p.cup = Some((false, steam));
            p.claws[0] = Claw::CupSide;
        }
    }
    p
}

/// Investigando com a lupa: perto do rosto (de frente) ou passando pelo código na tela (de
/// lado), com um olho apertado; no fim, uns pontinhos ("hmm...").
fn investigate(view: View, u: f32) -> Pose {
    let mut p = work_base(view, 20);
    p.mag = Some(match view {
        View::Front => {
            let (x, y) = MAG_FRONT[((u * MAG_FRONT.len() as f32) as usize).min(MAG_FRONT.len() - 1)];
            (x, y, None)
        }
        View::Side => {
            let (x, y, c) = MAG_SIDE[((u * MAG_SIDE.len() as f32) as usize).min(MAG_SIDE.len() - 1)];
            (x, y, Some(c))
        }
    });
    p.eyes = Eyes::Sherlock(1, 0);
    p.claws[1] = Claw::Hidden;
    if u > 0.86 {
        p.dots = (((u - 0.86) * 25.0) as u8).min(3);
    }
    p
}

/// Terminou: fecha o notebook (de lado, primeiro o ✓ na tela), se espreguiça e mostra o ✓.
fn done(view: View, ms: f32) -> Pose {
    let stretch = Pose { dy: -1, legs: Legs::Stretch, eyes: Eyes::Squeeze, mouth: Some(Mouth::Yawn), claws: [Claw::High; 2], ..stand() };
    match view {
        View::Front => {
            let closed = Some(Front { open: false, key: None });
            let (i, u) = seg(ms, &DONE_FRONT);
            match i {
                0 => Pose { eyes: Eyes::Open(0, 1), front: closed, ..stand() },
                1 => Pose { front: closed, ..stretch },
                _ => Pose { eyes: Eyes::Happy, mouth: Some(Mouth::Smile), check: !(0.4..0.48).contains(&u), front: closed, ..stand() },
            }
        }
        View::Side => {
            let closed = Some(Side { open: false, code: 0, done: false, key: None });
            let (i, _) = seg(ms, &DONE_SIDE);
            match i {
                0 => Pose { eyes: Eyes::Happy, mouth: Some(Mouth::Smile), claws: [Claw::Up; 2], side: Some(Side { open: true, code: CODE_TOTAL, done: true, key: None }), ..stand() },
                1 => Pose { side: closed, ..stretch },
                _ => Pose { mouth: Some(Mouth::Smile), side: closed, ..stand() },
            }
        }
    }
}

/// No limite, ele fica de lado para a placa caber: no pequeno, 6 colunas para a esquerda; no
/// médio e no grande (já mais à esquerda, por causa do notebook de lado), 4.
fn limit_shift(view: View) -> i32 {
    match view {
        View::Front => -6,
        View::Side => -4,
    }
}

/// O quadro `p` do limite: no lugar dele (`shift`: deslocado) e com a placa de altura `h`
/// do lado (0: sem placa).
fn at_limit(mut p: Pose, view: View, h: i32, shift: bool) -> Pose {
    if shift {
        p.dx += limit_shift(view);
    }
    if h > 0 {
        p.sign = Some(Sign { h, x: if view == View::Front { 15 } else { 17 } });
    }
    p
}

/// Lendo: o livro aberto seguro com as duas garras, os olhos espiando por cima.
fn reading() -> Pose {
    Pose { eyes: Eyes::Open(0, 1), book: Some((0, 0)), claws: [Claw::Book; 2], ..stand() }
}

/// A página virando, um pouco depois de `at` (0 a 1).
fn page_flip(u: f32, at: f32) -> u8 {
    if u > at && u < at + 0.12 { 1 + ((u - at) / 0.04) as u8 } else { 0 }
}

/// Andando de lado até o canto da placa (ou de volta, `back`).
fn side_step(view: View, u: f32, back: bool) -> Pose {
    let st = (u * 7.0) as i32;
    let n = -limit_shift(view);
    let off = n.min((st as f32 * n as f32 / 6.0).round() as i32);
    Pose { legs: if st % 2 == 1 { Legs::StepA } else { Legs::StepB }, eyes: Eyes::Open(if back { 1 } else { -1 }, 0), dx: if back { -n + off } else { -off }, ..stand() }
}

/// O balanço da música: um tempo para cada lado.
fn beat(ms: f32) -> bool {
    (ms / 460.0) as i32 % 2 == 1
}

/// Como ele fica parado, em cada humor (sem animação).
fn still(mood: Mood, scene: &Scene) -> Pose {
    match mood {
        Mood::Normal => stand(),
        Mood::Sleepy => asleep(None),
        Mood::Busy => work_base(scene.view, scene.code),
        Mood::Worried => Pose { mouth: Some(Mouth::Wavy), blush: Some(true), sweat: Some(0.0), ..stand() },
        Mood::Limit => at_limit(reading(), scene.view, 11, true),
    }
}

/// O que está em volta dele agora: de onde é visto, se está de Sherlock e o código na tela.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Scene {
    pub view: View,
    /// com o chapéu de detetive (a tarefa de agora é de Sherlock)
    pub hat: bool,
    /// as letras de código na tela do notebook de lado
    pub code: i32,
}

impl Scene {
    #[cfg(test)]
    pub fn new(view: View) -> Scene {
        Scene { view, hat: false, code: 0 }
    }
}

/// Uma animação tocando: qual, quanto já foi (0 a 1) e quanto dura (segundos).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Play {
    pub anim: Anim,
    pub t: f32,
    pub secs: f32,
}

/// O quadro de agora: com o humor, a animação tocando (se houver), o que está em volta e, se o
/// mouse está por perto, olhando para ele (`gaze`).
pub fn pose(mood: Mood, play: Option<Play>, scene: &Scene, gaze: Option<(i32, i32)>) -> Pose {
    let view = scene.view;
    let base = still(mood, scene);
    let mut p = match play {
        None => base,
        Some(Play { anim, t, secs }) => {
            let t = t.clamp(0.0, 1.0);
            frame(anim, t, secs, base, scene)
        }
    };
    // a fantasia de Sherlock: o chapéu no trabalho todo e, de lado, o cachimbo (menos bocejando,
    // falando ou tomando café)
    // (pulando, acordando ou levando um susto, ele está sem o notebook: sem a fantasia)
    let working = match play.map(|p| p.anim) {
        None => mood == Mood::Busy,
        Some(Anim::Done | Anim::HatOn) => true,
        Some(Anim::Hop | Anim::Love | Anim::Wake | Anim::Startle) => false,
        Some(_) => mood == Mood::Busy,
    };
    if scene.hat && working {
        if p.hat.is_none() {
            p.hat = Some(0);
        }
        if view == View::Side && p.mouth.is_none() && p.cup.is_none() {
            let ms = play.map(|p| p.t * p.secs * 1000.0);
            p.pipe = Some(ms.map(|ms| (ms / 2200.0).fract()));
        }
    }
    // os olhos no mouse, se estão abertos
    if let Some((gx, gy)) = gaze {
        p.eyes = match p.eyes {
            Eyes::Open(..) => Eyes::Open(gx, gy),
            Eyes::Sherlock(..) => Eyes::Sherlock(gx, gy),
            e => e,
        };
    }
    p
}

/// O quadro de `anim` a `t` (0 a 1) dos `secs` que ela dura, partindo de `base` (o quadro
/// parado).
fn frame(anim: Anim, t: f32, secs: f32, base: Pose, scene: &Scene) -> Pose {
    let view = scene.view;
    let ms = t * secs * 1000.0;
    let open_eyes = |p: &mut Pose, e: Eyes| {
        if matches!(p.eyes, Eyes::Open(..) | Eyes::Sherlock(..)) {
            p.eyes = e;
        }
    };
    match anim {
        Anim::Blink => {
            let mut p = base;
            open_eyes(&mut p, Eyes::Blink);
            p
        }
        Anim::Look => {
            let mut p = base;
            let x = match t {
                t if t < 0.4 => -1,
                t if t < 0.5 => 0,
                t if t < 0.9 => 1,
                _ => 0,
            };
            let y = if let Eyes::Open(_, y) = p.eyes { y } else { 0 };
            open_eyes(&mut p, Eyes::Open(x, y));
            p
        }
        Anim::Scuttle => {
            const PATH: [i32; 15] = [0, 1, 2, 3, 4, 4, 3, 2, 1, 0, -1, -2, -2, -1, 0];
            let st = ((t * 15.0) as usize).min(14);
            Pose { dx: PATH[st], legs: if st % 2 == 1 { Legs::StepA } else { Legs::StepB }, ..stand() }
        }
        Anim::Hop => {
            if t < CROUCH {
                // agacha antes de pular: já o primeiro quadro é outro (reage ao clique na hora)
                Pose { dy: 1, legs: Legs::Tuck, ..stand() }
            } else {
                let h = ((t - CROUCH) / (1.0 - CROUCH) * 2.0).fract();
                let dy = -((h * std::f32::consts::PI).sin() * 4.0).round() as i32;
                let up = dy <= -3;
                Pose { dy, legs: if dy < 0 { Legs::Tuck } else { Legs::Stand }, claws: if up { [Claw::Up; 2] } else { [Claw::Rest; 2] }, ..stand() }
            }
        }
        Anim::Wave => {
            // acena com uma garra de cada vez, sorrindo (trabalhando, sem largar o notebook)
            let mut p = base;
            let k = (t * 6.0) as i32 % 2;
            p.claws[k as usize] = Claw::Up;
            open_eyes(&mut p, Eyes::Happy);
            if p.mouth.is_none() {
                p.mouth = Some(Mouth::Smile);
            }
            p
        }
        Anim::Love => Pose {
            eyes: if t < 0.75 { Eyes::Happy } else { Eyes::Open(0, 0) },
            blush: Some(false),
            mouth: Some(Mouth::Smile),
            heart: Some(t),
            claws: if t < 0.5 { [Claw::Up; 2] } else { [Claw::Rest; 2] },
            ..stand()
        },
        Anim::Music => {
            let (i, _) = seg(ms, &MUSIC);
            let notes = Some((ms / 2600.0).fract());
            let b = beat(ms);
            match i {
                0 => Pose { eyes: Eyes::Open(0, -1), claws: [Claw::Up; 2], phones: Some(-3), ..stand() },
                1 => Pose { eyes: Eyes::Happy, claws: [Claw::Up; 2], phones: Some(0), ..stand() },
                2 | 4 => Pose { eyes: Eyes::Chill, phones: Some(0), notes, dx: b as i32, legs: if b { Legs::Tap } else { Legs::Stand }, ..stand() },
                3 => Pose { eyes: Eyes::Happy, mouth: Some(Mouth::Smile), phones: Some(0), notes, dx: b as i32, claws: if b { [Claw::Up, Claw::Rest] } else { [Claw::Rest, Claw::Up] }, ..stand() },
                5 => Pose { eyes: Eyes::Happy, claws: [Claw::Up; 2], phones: Some(-3), ..stand() },
                _ => stand(),
            }
        }
        Anim::Party => {
            let h = ((t / 0.6) * 2.0).fract();
            let dy = if t < 0.6 { -((h * std::f32::consts::PI).sin() * 4.0).round() as i32 } else { 0 };
            Pose { dy, legs: if dy < 0 { Legs::Tuck } else { Legs::Stand }, eyes: Eyes::Happy, mouth: Some(Mouth::Grin), claws: [Claw::Up; 2], confetti: Some(t), ..stand() }
        }
        Anim::Snooze => asleep(Some(ms)),
        Anim::Wake => {
            let (i, u) = seg(ms, &WAKE);
            let sitting = |eyes| Pose { eyes, ..asleep(None) };
            match i {
                0 => sitting(Eyes::Peek),
                1 | 3 => sitting(Eyes::Half),
                2 => sitting(Eyes::Sleep),
                4 => Pose { dy: 1, legs: Legs::Half, eyes: Eyes::Squeeze, mouth: Some(Mouth::Yawn), claws: [Claw::Up; 2], cap: Some((0, 1.0)), ..stand() },
                5 => Pose { dy: -1, legs: Legs::Stretch, eyes: Eyes::Squeeze, mouth: Some(Mouth::Yawn), claws: [Claw::High; 2], cap: Some((-(u.round() as i32), 1.0 - u * 0.6)), ..stand() },
                6 => Pose { cap: Some((-1, 0.4 - u * 0.4)), ..stand() },
                7 | 9 => Pose { eyes: Eyes::Blink, ..stand() },
                10 => Pose { eyes: Eyes::Open(-1, 0), ..stand() },
                11 => Pose { eyes: Eyes::Open(1, 0), ..stand() },
                12 => Pose { mouth: Some(Mouth::Smile), ..stand() },
                _ => stand(),
            }
        }
        Anim::Startle => {
            let (i, u) = seg(ms, &STARTLE);
            match i {
                0 => Pose { eyes: Eyes::Wide, mouth: Some(Mouth::O), bang: true, cap: Some((-1, 1.0)), zzz: None, ..asleep(None) },
                1 => {
                    let dy = -((u * std::f32::consts::PI).sin() * 3.0).round() as i32;
                    let cap = (u < 0.25).then_some((-1, 0.5 - u * 2.0));
                    Pose { dy, legs: Legs::Tuck, eyes: Eyes::Wide, mouth: Some(Mouth::O), claws: [Claw::Up; 2], bang: true, cap, ..stand() }
                }
                2 => Pose { dy: 2, legs: Legs::Tuck, eyes: Eyes::Wide, mouth: Some(Mouth::O), bang: true, ..stand() },
                3 => Pose { eyes: Eyes::Wide, bang: true, ..stand() },
                4 => Pose { eyes: Eyes::Blink, ..stand() },
                5 => Pose { eyes: Eyes::Open(1, 0), ..stand() },
                _ => Pose { eyes: Eyes::Open(1, 1), ..stand() },
            }
        }
        Anim::Sweat => Pose { sweat: Some(t), ..still(Mood::Worried, scene) },
        Anim::Shake => {
            let dx = if (t * 16.0) as i32 % 2 == 0 { 1 } else { -1 };
            Pose { dx, eyes: Eyes::Squeeze, sweat: Some(t), ..still(Mood::Worried, scene) }
        }
        Anim::Type => {
            let n = (secs * KEYS_PER_SEC).round().max(4.0) as i32;
            let chars = (n as f32 * 1.25) as i32;
            let a = scene.code;
            typing(view, t, n, a, (a + chars).min(CODE_TOTAL))
        }
        Anim::Think => {
            let mut p = work_base(view, scene.code);
            p.eyes = Eyes::Open(1, -1);
            p.dots = ((t * 4.0) as u8).min(3);
            p
        }
        Anim::Idea => idea(view, scene.code, t),
        Anim::Coffee => coffee(view, scene.code, t, ms),
        Anim::HatOn => {
            let mut p = work_base(view, scene.code);
            p.eyes = Eyes::Open(0, -1);
            p.claws = if t < 0.6 { [Claw::High; 2] } else { [Claw::Up; 2] };
            p.hat = Some(0);
            p
        }
        Anim::Investigate => {
            let mut p = investigate(view, t);
            if view == View::Side {
                if let Some(s) = &mut p.side {
                    s.code = scene.code.max(20);
                }
            }
            p
        }
        Anim::Done => done(view, ms),
        Anim::SignUp => {
            let (i, u) = seg(ms, &SIGN_UP);
            match i {
                0 => Pose { eyes: Eyes::Wide, mouth: Some(Mouth::O), bang: true, ..stand() },
                1 => side_step(view, u, false),
                2..=4 => {
                    let h = [3, 6, 9][i - 2];
                    let claws = if i < 4 { [Claw::Rest, Claw::Down] } else { [Claw::Rest; 2] };
                    at_limit(Pose { eyes: Eyes::Open(1, 1), claws, ..stand() }, view, h, true)
                }
                5 => at_limit(Pose { eyes: Eyes::Happy, mouth: Some(Mouth::Smile), ..stand() }, view, 11, true),
                _ => at_limit(Pose { eyes: Eyes::Open(0, 0), book: Some((-2, 0)), claws: [Claw::Up; 2], ..stand() }, view, 11, true),
            }
        }
        Anim::SignDown => {
            let (i, u) = seg(ms, &SIGN_DOWN);
            match i {
                0 => at_limit(Pose { eyes: Eyes::Wide, mouth: Some(Mouth::O), ..stand() }, view, 11, true),
                1..=3 => {
                    let h = [9, 6, 3][i - 1];
                    let claws = if i < 3 { [Claw::Rest, Claw::Down] } else { [Claw::Rest; 2] };
                    at_limit(Pose { eyes: Eyes::Open(1, 1), claws, ..stand() }, view, h, true)
                }
                4 => side_step(view, u, true),
                _ => Pose { eyes: Eyes::Happy, mouth: Some(Mouth::Smile), ..stand() },
            }
        }
        Anim::Read => {
            let blink = (0.3..0.33).contains(&t);
            let p = Pose { eyes: if blink { Eyes::Blink } else { Eyes::Open(0, 1) }, book: Some((0, page_flip(t, 0.62))), ..reading() };
            at_limit(p, view, 11, true)
        }
        Anim::Laugh => {
            let dy = if (ms / 170.0) as i32 % 2 == 1 { -1 } else { 0 };
            at_limit(Pose { eyes: Eyes::Happy, blush: Some(false), dy, ..reading() }, view, 11, true)
        }
        Anim::Nod => {
            let (i, u) = seg(ms, &NOD);
            let p = match i {
                0 => Pose { eyes: Eyes::Half, ..reading() },
                // um cochilo curto: só o começo do ciclo dos "z" (o grande nem chega a subir)
                1 => Pose { dy: 1, eyes: Eyes::Sleep, zzz: Some(0.1 + 0.5 * u), ..reading() },
                _ => Pose { eyes: Eyes::Wide, ..reading() },
            };
            at_limit(p, view, 11, true)
        }
    }
}

// ---------- o desenho ----------

/// Um retângulo do desenho, em quadradinhos: (x, y, largura, altura, cor).
pub type Cell = (i32, i32, i32, i32, Rgba);

/// Os quadradinhos de um desenho em texto com o canto de cima em (x, y): cada letra vira uma
/// cor (`.` fica vazio). Espelhado, a coluna x vai para 26 − x (a garra da direita é o espelho
/// da da esquerda). `att`: anda junto com ele (dx, dy).
#[allow(clippy::too_many_arguments)]
fn art(put: &mut impl FnMut(i32, i32, i32, i32, Rgba, bool), rows: &[&str], x: i32, y: i32, color: impl Fn(u8) -> Option<Rgba>, mirror: bool, att: bool) {
    for (j, row) in rows.iter().enumerate() {
        for (i, ch) in row.bytes().enumerate() {
            if ch == b'.' {
                continue;
            }
            let Some(c) = color(ch) else { continue };
            let cx = x + i as i32;
            put(if mirror { 26 - cx } else { cx }, y + j as i32, 1, 1, c, att);
        }
    }
}

/// Uma cor só para todas as letras.
fn one(c: Rgba) -> impl Fn(u8) -> Option<Rgba> {
    move |_| Some(c)
}

/// Os degraus de opacidade (pixel art; e o widget só redesenha quando algo muda).
fn steps(a: f32) -> f32 {
    (a.clamp(0.0, 1.0) * 4.0).round() / 4.0
}

/// Os retângulos do quadro, em quadradinhos (os adereços podem sair da grade: acima, em linhas
/// negativas; à direita, o notebook de lado e a placa).
pub fn cells(p: &Pose) -> Vec<Cell> {
    let mut out: Vec<Cell> = Vec::new();
    let (dx, dy) = (p.dx, p.dy);
    let mut put = |x: i32, y: i32, w: i32, h: i32, c: Rgba, att: bool| {
        let (x, y) = if att { (x + dx, y + dy) } else { (x, y) };
        // emenda com o último retângulo, se continua a mesma linha na mesma cor
        if let Some(last) = out.last_mut() {
            if h == 1 && last.3 == 1 && last.1 == y && last.4 == c {
                if last.0 + last.2 == x {
                    last.2 += w;
                    return;
                }
                if x + w == last.0 {
                    last.0 = x;
                    last.2 += w;
                    return;
                }
            }
        }
        out.push((x, y, w, h, c));
    };
    let top = -(p.breath as i32);

    // o corpo: luz em cima e à esquerda, sombra embaixo e à direita, cantos arredondados
    put(5, top, 17, 1, LIGHT, true);
    put(4, top + 1, 19, 10 - top, BODY, true);
    put(22, top + 1, 1, 10 - top, SHADE, true);
    put(5, 11, 17, 1, SHADE, true);
    put(5, top + 1, 2, 1, LIGHT, true);
    if p.glow {
        put(5, 5, 17, 1, glow(), true);
    }
    if let Some(hot) = p.blush {
        put(5, 6, 2, 1, blush(hot), true);
        put(20, 6, 2, 1, blush(hot), true);
    }

    // os olhos (o esquerdo nas colunas 7 e 8, o direito em 18 e 19)
    const EX: [i32; 2] = [7, 18];
    match p.eyes {
        Eyes::Open(lx, ly) => {
            let (lx, ly) = (lx.clamp(-1, 1), ly.clamp(-1, 1));
            for ex in EX {
                put(ex + lx, 3 + ly, 2, 3, EYE, true);
                put(ex + 1 + lx, 3 + ly, 1, 1, SHINE, true);
            }
        }
        Eyes::Blink => {
            for ex in EX {
                put(ex, 4, 2, 1, EYE, true);
            }
        }
        Eyes::Sleep => {
            for ex in EX {
                put(ex - 1, 4, 1, 1, EYE, true);
                put(ex + 2, 4, 1, 1, EYE, true);
                put(ex, 5, 2, 1, EYE, true);
            }
        }
        Eyes::Happy => {
            for ex in EX {
                put(ex - 1, 4, 1, 1, EYE, true);
                put(ex + 2, 4, 1, 1, EYE, true);
                put(ex, 3, 2, 1, EYE, true);
            }
        }
        Eyes::Half => {
            for ex in EX {
                put(ex, 4, 2, 2, EYE, true);
                put(ex - 1, 3, 4, 1, SHADE, true);
            }
        }
        Eyes::Peek => {
            put(7, 4, 2, 2, EYE, true);
            put(6, 3, 4, 1, SHADE, true);
            put(17, 4, 1, 1, EYE, true);
            put(20, 4, 1, 1, EYE, true);
            put(18, 5, 2, 1, EYE, true);
        }
        Eyes::Squeeze => {
            for (x, y) in [(6, 2), (7, 3), (8, 4), (7, 5), (6, 6), (20, 2), (19, 3), (18, 4), (19, 5), (20, 6)] {
                put(x, y, 1, 1, EYE, true);
            }
        }
        Eyes::Wide => {
            for ex in EX {
                put(ex, 2, 2, 4, EYE, true);
                put(ex + 1, 2, 1, 1, SHINE, true);
            }
        }
        Eyes::Sherlock(lx, ly) => {
            let (lx, ly) = (lx.clamp(-1, 1), ly.clamp(-1, 1));
            put(7 + lx, 3 + ly, 2, 3, EYE, true);
            put(8 + lx, 3 + ly, 1, 1, SHINE, true);
            put(17 + lx, 4 + ly, 3, 1, EYE, true);
        }
        Eyes::Chill => {
            put(6, 4, 3, 1, EYE, true);
            put(18, 4, 3, 1, EYE, true);
            let cheek = Rgba::rgb(255, 128, 138).alpha(0.45);
            put(7, 5, 2, 1, cheek, true);
            put(18, 5, 2, 1, cheek, true);
        }
    }

    // a boca (só em algumas expressões: parado, ele é como o logo, sem boca)
    match p.mouth {
        Some(Mouth::Yawn) => {
            put(12, 7, 3, 1, MOUTH, true);
            put(11, 8, 5, 2, MOUTH, true);
            put(12, 9, 3, 2, TONGUE, true);
        }
        Some(Mouth::O) => put(12, 8, 3, 2, MOUTH, true),
        Some(Mouth::Tiny) => put(13, 8, 1, 1, MOUTH, true),
        Some(Mouth::Smile) => {
            put(11, 7, 1, 1, EYE, true);
            put(15, 7, 1, 1, EYE, true);
            put(12, 8, 3, 1, EYE, true);
        }
        Some(Mouth::Grin) => {
            put(11, 7, 5, 1, MOUTH, true);
            put(12, 8, 3, 1, TONGUE, true);
        }
        Some(Mouth::Wavy) => {
            for (x, y) in [(10, 8), (11, 7), (12, 8), (13, 7), (14, 8), (15, 7), (16, 8)] {
                put(x, y, 1, 1, EYE, true);
            }
        }
        None => {}
    }

    // as patas: duas colunas cada, a da direita na sombra
    for (i, x) in [6, 9, 16, 19].into_iter().enumerate() {
        let h = p.legs.heights()[i];
        put(x, 12, 1, h, BODY, true);
        put(x + 1, 12, 1, h, SHADE, true);
    }

    // o notebook de frente, no colo: a tampa por trás (com o símbolo), o teclado e a base
    if let Some(Front { open, key }) = p.front {
        if open {
            put(6, 6, 15, 7, ALU, true);
            put(6, 6, 15, 1, ALU_LIGHT, true);
            put(20, 7, 1, 6, ALU_DARK, true);
            art(&mut put, &LOGO_MARK, 11, 7, one(LOGO), false, true);
            put(4, 13, 19, 1, KEYS, true);
            if let Some(k) = key {
                put(k, 13, 1, 1, KEY_LIT, true);
            }
        } else {
            put(4, 13, 19, 1, ALU, true);
        }
        put(4, 14, 19, 1, ALU_DARK, true);
    }
    // o notebook de lado, no chão à direita (não anda com ele): a moldura, a tela com o código
    // (ou o ✓), o teclado e a base
    if let Some(s) = p.side {
        if s.open {
            put(27, 2, 10, 11, BEZEL, false);
            put(28, 3, 8, 9, SCREEN, false);
            if s.done {
                art(&mut put, &CHECK_MARK, 28, 4, one(CHECK), false, false);
            } else {
                let mut shown = Vec::new();
                let mut left = s.code;
                for &(ind, len, col) in &LINES {
                    if left <= 0 {
                        break;
                    }
                    shown.push((ind, len.min(left), CODE[col]));
                    left -= len;
                }
                let view = &shown[shown.len().saturating_sub(4)..];
                for (i, &(ind, n, col)) in view.iter().enumerate() {
                    put(29 + ind, 4 + 2 * i as i32, n, 1, col, false);
                }
                let cur = view.last().map_or((29, 4), |&(ind, n, _)| (29 + ind + n, 4 + 2 * (view.len() as i32 - 1)));
                put(cur.0.min(35), cur.1, 1, 1, FG.alpha(0.9), false);
            }
            put(24, 13, 13, 1, KEYS, false);
            if let Some(k) = s.key {
                put(k, 13, 1, 1, KEY_LIT, false);
            }
        } else {
            put(24, 13, 13, 1, ALU, false);
        }
        put(24, 14, 13, 1, ALU_DARK, false);
    }

    // a plaquinha "VOLTO LOGO" do lado dele (não anda com ele): subindo do chão, só a madeira
    if let Some(Sign { h, x }) = p.sign {
        let y0 = 17 - h;
        put(x, y0, 21, h, WOOD, false);
        put(x, y0, 21, 1, WOOD_DARK, false);
        put(x, 16, 21, 1, WOOD_DARK, false);
        put(x, y0, 1, h, WOOD_DARK, false);
        put(x + 20, y0, 1, h, WOOD_DARK, false);
        if h >= 11 {
            for (word, wx, wy) in [("VOLTO", x + 1, y0 + 1), ("LOGO", x + 3, y0 + 6)] {
                for (i, ch) in word.chars().enumerate() {
                    art(&mut put, &sign_letter(ch), wx + 4 * i as i32, wy, one(SIGN_TEXT), false, false);
                }
            }
            put(x + 3, 17, 2, 1, WOOD_DARK, false);
            put(x + 16, 17, 2, 1, WOOD_DARK, false);
        }
    }
    // o livro aberto, seguro na frente; a página virando passa por cima
    if let Some((bdy, flip)) = p.book {
        let by = 5 + bdy;
        let color = |ch: u8| match ch {
            b'P' => Some(PAGE),
            b'C' => Some(COVER),
            b'g' => Some(GOLD),
            _ => Some(COVER_DARK),
        };
        art(&mut put, &READ_ART, 8, by, color, false, true);
        match flip {
            1 => put(14, by - 1, 3, 1, PAGE, true),
            2 => put(13, by - 3, 1, 3, PAGE, true),
            3 => put(10, by - 1, 3, 1, PAGE, true),
            _ => {}
        }
    }
    // o cachimbo e a fumacinha subindo para a esquerda
    if let Some(smoke) = p.pipe {
        let color = |ch: u8| match ch {
            b'o' => Some(PIPE_RIM),
            b'E' => Some(EMBER),
            b'P' => Some(PIPE_WOOD),
            _ => Some(PIPE_STEM),
        };
        art(&mut put, &PIPE_ART, 7, 7, color, false, true);
        if let Some(s) = smoke {
            for k in 0..3 {
                let a = (s + k as f32 / 3.0).fract();
                let al = steps((a / 0.2).min((1.0 - a) / 0.4)) * 0.8;
                if al > 0.0 {
                    let sz = if a > 0.5 { 2 } else { 1 };
                    put(7 - (5.0 * a).round() as i32, 5 - (6.0 * a).round() as i32, sz, sz, SMOKE.alpha(al), true);
                }
            }
        }
    }
    // a caneca (antes das garras: a garra fica por cima da asa, segurando) e o vapor
    if let Some((sip, steam)) = p.cup {
        let (cx, cy) = if sip { (9, 4) } else { (-2, 4) };
        let color = |ch: u8| match ch {
            b'R' => Some(CUP_RIM),
            b'k' => Some(BREW),
            b'M' => Some(CUP),
            b'S' => Some(CUP_SHADE),
            _ => Some(CUP_HANDLE),
        };
        art(&mut put, if sip { &CUP_SIP } else { &CUP_ART }, cx, cy, color, false, true);
        if let Some(s) = steam {
            let mid = cx + if sip { 4 } else { 2 };
            for k in 0..3 {
                let a = (s + k as f32 / 3.0).fract();
                let al = steps((a / 0.2).min((1.0 - a) / 0.4)) * 0.9;
                if al > 0.0 {
                    put(mid + (a * 6.0 + k as f32 * 2.0).sin().round() as i32, cy - 1 - (4.0 * a).round() as i32, 1, 1, STEAM.alpha(al), true);
                }
            }
        }
    }

    // as garras
    let claw_color = |ch: u8| match ch {
        b'L' => Some(LIGHT),
        b'B' => Some(BODY),
        _ => Some(SHADE),
    };
    for (side, claw) in p.claws.iter().enumerate() {
        if let Some((rows, x, y)) = claw.art() {
            art(&mut put, rows, x, y, claw_color, side == 1, true);
        }
    }

    // ---- adereços ----
    if let Some(hdy) = p.hat {
        let color = |ch: u8| match ch {
            b'o' => Some(HAT_LINE),
            b'h' => Some(HAT),
            b't' => Some(HAT_TWEED),
            b'l' => Some(HAT_LIGHT),
            b'm' => Some(HAT_SEAM),
            b'd' => Some(HAT_BAND),
            b'F' => Some(HAT_FLAP),
            _ => Some(HAT_PEAK),
        };
        art(&mut put, &HAT_ART, 2, -6 + top + hdy, color, false, true);
    }
    if let Some((mx, my, zoom)) = p.mag {
        // a lupa, segura pela garra direita na ponta do cabo; em cima da tela, o código aumentado
        let color = |ch: u8| match ch {
            b'K' => Some(LENS),
            b'G' => Some(glass()),
            b'W' => Some(GLINT),
            _ => Some(HANDLE),
        };
        art(&mut put, &MAG_ART, mx, my, color, false, true);
        if let Some(c) = zoom {
            put(mx + 3, my + 2, 4, 2, CODE[c % 5], true);
            put(mx + 3, my + 5, 2, 1, CODE[(c + 2) % 5], true);
        }
        art(&mut put, &["LBB", "BBK"], mx, my + 9, claw_color, false, true);
    }
    if let Some(pdy) = p.phones {
        let color = |ch: u8| match ch {
            b'n' => Some(PHONE_LINE),
            b'b' => Some(PHONE),
            b'B' => Some(PHONE_LIGHT),
            _ => Some(PHONE_PAD),
        };
        art(&mut put, &PHONES_ART, 1, -3 + top + pdy, color, false, true);
    }
    if let Some(n) = p.notes {
        // notas saindo do fone da direita, subindo em zigue-zague e sumindo
        for k in 0..3 {
            let a = (n + k as f32 / 3.0).fract();
            let glyph: &[&str] = if k == 1 { &NOTE1 } else { &NOTE8 };
            let al = steps((a / 0.15).min((1.0 - a) / 0.35));
            if al <= 0.0 {
                continue;
            }
            let x = 24 + (2.0 * a).round() as i32 + ((a * 6.0) as i32 % 2);
            let yb = 2 - (5.0 * a).round() as i32;
            art(&mut put, glyph, x, yb - glyph.len() as i32 + 1, one(NOTE.alpha(al)), false, true);
        }
    }
    if let Some((cdy, a)) = p.cap {
        if a > 0.0 {
            let color = move |ch: u8| match ch {
                b'b' => Some(CAP.alpha(a)),
                b'l' => Some(CAP_LIGHT.alpha(a)),
                _ => Some(CAP_WHITE.alpha(a)),
            };
            art(&mut put, &CAP_ART, 5, -4 + top + cdy, color, true, true);
        }
    }
    if let Some(z) = p.zzz {
        for (glyph, x, y, born, dies) in zs_at(z) {
            art(&mut put, glyph, x + p.zzz_x, y, one(FG.alpha(steps_alpha(z, born, dies))), false, true);
        }
    }
    if p.bang {
        art(&mut put, &BANG_MARK, 24, -5, one(BANG), false, false);
    }
    if let Some(s) = p.sweat {
        // duas gotas escorrendo, uma de cada lado
        for (ph, mirror) in [(0.0, false), (0.5, true)] {
            let t = (s + ph).fract();
            let y = -1 + (5.0 * t).round() as i32;
            let a = steps((1.0 - t) / 0.3);
            if a <= 0.0 {
                continue;
            }
            let color = move |ch: u8| if ch == b'w' { Some(SHINE.alpha(a)) } else { Some(SWEAT.alpha(a)) };
            art(&mut put, &[".s", "ss", "sw"], 23, y, color, mirror, true);
        }
    }
    if let Some(u) = p.heart {
        let glyph: &[&str] = if u < 0.25 { &HEART_S } else { &HEART_L };
        let yb = -1 - (2.0 * u).round() as i32;
        let a = steps((1.0 - u) / 0.3);
        if a > 0.0 {
            art(&mut put, glyph, 22, yb - glyph.len() as i32 + 1, one(HEART.alpha(a)), false, true);
        }
    }
    if let Some(u) = p.confetti {
        for i in 0..16 {
            let x0 = (i * 11 + 3) % 26;
            let y0 = -6 + (i * 7) % 4;
            let fall = 10 + (i * 5) % 9;
            let y = y0 + (u * fall as f32 * 1.4).round() as i32;
            if y > 14 {
                continue;
            }
            let x = x0 + ((u * 7.0 + i as f32).sin() * 1.2).round() as i32;
            let tall = ((u * 10.0) as i32 + i) % 2 == 0;
            put(x, y, 1, if tall { 2 } else { 1 }, CODE[i as usize % 5], false);
        }
    }
    if p.check {
        art(&mut put, &CHECK_MARK, if p.hat.is_some() { 24 } else { 21 }, -5, one(CHECK), false, true);
    }
    if let Some((lit, rays)) = p.bulb {
        art(&mut put, &BULB_GLASS, 23, -6, one(if lit { BULB } else { BULB_OFF }), false, true);
        art(&mut put, &BULB_BASE, 23, -2, one(BULB_OFF), false, true);
        if rays {
            for (x, y) in [(21, -5), (29, -5), (22, -7), (28, -7)] {
                put(x, y, 1, 1, BULB.alpha(0.85), true);
            }
        }
    }
    for &(x, y, s) in DOTS.iter().take(p.dots as usize) {
        put(x, y, s, s, FG, true);
    }
    out
}

/// A opacidade de um "z" nascido em `born` e que some em `dies` (frações do ciclo `z`).
fn steps_alpha(z: f32, born: f32, dies: f32) -> f32 {
    let age = (z - born) / (dies - born);
    steps((age / 0.15).min((1.0 - age) / 0.35)) * 0.95
}

/// Os "z" vivos em `z` (0 a 1 do ciclo), em escadinha (o pequeno do lado da cabeça, o médio
/// acima, o grande mais alto), cada um subindo um quadradinho no meio da vida: (desenho, coluna,
/// linha, quando nasce, quando some).
pub fn zs_at(z: f32) -> Vec<(&'static [&'static str], i32, i32, f32, f32)> {
    let mut out = Vec::new();
    for &(glyph, x, y, born, dies) in &ZS {
        let age = (z - born) / (dies - born);
        if (0.0..1.0).contains(&age) && steps_alpha(z, born, dies) > 0.0 {
            out.push((glyph, x, y - (age >= 0.5) as i32, born, dies));
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

/// O Clawd pulando de um lugar para outro (o widget mudando de tamanho, a ida para a barra de
/// tarefas): agachado antes de subir e ao pousar, de olhos apertados; no ar, com as patas
/// encolhidas e as garras para cima.
pub fn jump_pose(crouch: bool) -> Pose {
    if crouch {
        Pose { dy: 1, legs: Legs::Tuck, eyes: Eyes::Squeeze, ..stand() }
    } else {
        Pose { legs: Legs::Tuck, eyes: Eyes::Open(0, -1), claws: [Claw::Up; 2], ..stand() }
    }
}

/// O tamanho de um quadradinho, em pixels, num lugar de `unit` pontos na escala `s`: 2/3 do
/// quadradinho antigo (que era `unit` em pixels inteiros), também em pixels inteiros.
pub fn cell_px(s: f32, unit: f32) -> f32 {
    let old = (unit * s).round().max(1.0);
    (old * CELL).round().max(1.0)
}

/// O canto de cima da grade, em pontos (em pixels inteiros), e o tamanho de um quadradinho, com
/// o centro da grade em (cx, cy).
pub fn origin(s: f32, cx: f32, cy: f32, unit: f32) -> (f32, f32, f32) {
    let c = cell_px(s, unit) / s;
    let snap = |v: f32| (v * s).round() / s;
    (snap(cx - COLS / 2.0 * c), snap(cy - ROWS / 2.0 * c), c)
}

/// Desenha o Clawd com o centro da grade em (cx, cy), num lugar de `unit` pontos. Devolve a
/// largura da grade.
pub fn draw(c: &mut Canvas, cx: f32, cy: f32, unit: f32, p: &Pose) -> f32 {
    let (x0, y0, u) = origin(c.s, cx, cy, unit);
    for (x, y, cw, ch, col) in cells(p) {
        let (ax, ay) = (x0 + x as f32 * u, y0 + y as f32 * u);
        c.fill_round_rect(ax, ay, ax + cw as f32 * u, ay + ch as f32 * u, 0.0, col);
    }
    COLS * u
}

/// Segundos de digitação por vez, para o Claude Code dando `per_min` passos por minuto
/// (respostas e resultados de ferramentas): quanto mais rápido ele trabalha, mais rápido o Clawd
/// digita.
pub fn pace_for(per_min: f32) -> f32 {
    (PACE_SLOW - per_min * 0.12).clamp(PACE_FAST, PACE_SLOW)
}

/// As letras de código que uma digitação de `secs` segundos põe na tela.
fn chars_typed(secs: f32) -> i32 {
    ((secs * KEYS_PER_SEC).round().max(4.0) * 1.25) as i32
}

// ---------- quando e o que anima ----------

/// Quando e o que o Clawd anima.
pub struct Crab {
    /// a animação, quando começou e quanto dura (segundos)
    anim: Option<(Anim, Instant, f32)>,
    /// a que vem logo depois desta (o confete depois de recolher a placa)
    queue: Option<Anim>,
    next: Instant,
    seed: u64,
    /// o humor e o desenho do último tick (só redesenha quando o desenho muda)
    mood: Mood,
    last: Option<Vec<Cell>>,
    /// segundos de digitação por vez (o ritmo do Claude Code)
    pub pace: f32,
    /// teve uma ideia: a próxima digitação sai mais rápido
    inspired: bool,
    /// trabalhando há um bom tempo (pode parar para um café), e quando foi o último
    pub tired: bool,
    last_coffee: Option<Instant>,
    /// a tarefa de agora é de Sherlock, e se ele já pôs o chapéu
    sherlock: bool,
    hat: bool,
    /// as letras de código na tela do notebook de lado
    code: i32,
    /// pode pôr o fone (sem números, ele fica embaixo do anel, e as notas chegariam nele)
    pub music: bool,
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
            queue: None,
            next: Instant::now() + Duration::from_secs(3),
            seed,
            mood: Mood::Normal,
            last: None,
            pace: PACE,
            inspired: false,
            tired: false,
            last_coffee: None,
            sherlock: false,
            hat: false,
            code: 0,
            music: true,
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

    /// O mesmo, numa tarefa de Sherlock, já de chapéu (para as imagens e os testes).
    #[cfg(test)]
    pub fn dressed(mut self) -> Crab {
        self.sherlock = true;
        self.hat = true;
        self.code = 20;
        self
    }

    fn random(&mut self) -> u64 {
        // xorshift: basta para sortear a próxima animação
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// A animação tocando agora.
    fn current(&self) -> Option<Play> {
        let (anim, start, secs) = self.anim?;
        let t = start.elapsed().as_secs_f32() / secs;
        (t < 1.0).then_some(Play { anim, t, secs })
    }

    pub fn animating(&self) -> bool {
        self.current().is_some()
    }

    /// Está tocando `anim` agora?
    pub fn is_playing(&self, anim: Anim) -> bool {
        self.current().is_some_and(|p| p.anim == anim)
    }

    /// O que está em volta dele, visto de `view`.
    pub fn scene(&self, view: View) -> Scene {
        Scene { view, hat: self.hat, code: self.code.min(CODE_TOTAL) }
    }

    /// O quadro de agora, visto de `view`.
    pub fn pose(&self, mood: Mood, gaze: Option<(i32, i32)>, view: View) -> Pose {
        pose(mood, self.current(), &self.scene(view), gaze)
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
            let before = self.mood;
            self.mood = mood;
            self.last = None;
            if mood == Mood::Limit {
                // chegou ao limite: a placa e o livro
                self.queue = None;
                self.play(Anim::SignUp);
            } else if before == Mood::Limit {
                // o limite reiniciou: recolhe a placa e comemora
                self.play(Anim::SignDown);
                self.queue = Some(Anim::Party);
            } else if before == Mood::Sleepy && !self.animating() {
                // o Claude Code voltou: acorda devagar
                self.play(Anim::Wake);
            }
            if before == Mood::Busy && !self.is_playing(Anim::Done) {
                self.sherlock = false;
                self.hat = false;
            }
            if mood == Mood::Busy && before != Mood::Busy {
                // começou a trabalhar: pega o notebook logo; de vez em quando, de Sherlock
                self.sherlock = self.random() % SHERLOCK_ONE_IN == 0;
                self.hat = false;
                self.next = self.next.min(now + Duration::from_millis(600));
            }
        }
        if let Some((anim, _, secs)) = self.anim {
            if !self.animating() {
                // acabou: volta à pose parada e espera um pouco até a próxima
                self.anim = None;
                match anim {
                    Anim::Type => self.code = (self.code + chars_typed(secs)).min(CODE_TOTAL),
                    Anim::Done => {
                        self.sherlock = false;
                        self.hat = false;
                        self.code = 0;
                    }
                    _ => {}
                }
                self.last = None;
                if let Some(next) = self.queue.take() {
                    self.play(next);
                    return true;
                }
                let (lo, hi) = match mood {
                    Mood::Sleepy => (5, 9),
                    Mood::Busy => (1, 4),
                    Mood::Limit => (3, 8),
                    _ => (7, 16),
                };
                self.next = now + Duration::from_secs(lo + self.random() % (hi - lo));
                // com uma ideia na cabeça (ou o chapéu para pôr), volta ao trabalho logo
                if mood == Mood::Busy && (self.inspired || (self.sherlock && !self.hat)) {
                    self.next = now + Duration::from_millis(300);
                }
                return true;
            }
        } else if now >= self.next {
            let anim = self.pick(mood);
            self.play(anim);
        }
        // parado não redesenha; animando, só se o quadro mudou
        if self.anim.is_none() {
            return false;
        }
        let mut drawn = cells(&self.pose(mood, None, View::Front));
        drawn.extend(cells(&self.pose(mood, None, View::Side)));
        let changed = self.last.as_ref() != Some(&drawn);
        self.last = Some(drawn);
        changed
    }

    /// Toca `anim` agora (um clique nele, o mouse chegando perto, o Claude Code terminando),
    /// mesmo no meio de outra.
    pub fn play(&mut self, anim: Anim) {
        let mut secs = anim.secs(self.pace);
        match anim {
            Anim::Type => {
                // a tela cheia: começa outra
                if self.code >= CODE_TOTAL {
                    self.code = 0;
                }
                // a digitação depois de uma ideia sai na metade do tempo
                if self.inspired {
                    secs *= 0.5;
                    self.inspired = false;
                }
            }
            Anim::Idea => self.inspired = true,
            Anim::Coffee => self.last_coffee = Some(Instant::now()),
            Anim::HatOn => self.hat = true,
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
            Mood::Sleepy => Anim::Startle, // acorda num susto
            Mood::Worried => Anim::Shake,
            Mood::Busy => Anim::Wave, // acena sem largar o notebook
            Mood::Limit => Anim::Laugh,
            Mood::Normal => [Anim::Wave, Anim::Hop][(self.random() % 2) as usize],
        };
        self.play(anim);
    }

    /// O mouse chegou em cima dele: acena (parado, às vezes fica corado, com um coração).
    pub fn greet(&mut self, mood: Mood) {
        let anim = match mood {
            Mood::Normal => [Anim::Wave, Anim::Love][(self.random() % 2) as usize],
            Mood::Limit => Anim::Laugh,
            _ => Anim::Wave,
        };
        self.play(anim);
    }

    fn pick(&mut self, mood: Mood) -> Anim {
        match mood {
            Mood::Sleepy => Anim::Snooze,
            Mood::Busy => {
                if self.sherlock && !self.hat {
                    return Anim::HatOn;
                }
                if self.inspired {
                    return Anim::Type;
                }
                // um café de vez em quando, depois de muito tempo trabalhando
                let coffee_due = self.tired && self.last_coffee.is_none_or(|t| t.elapsed() >= COFFEE_EVERY);
                if coffee_due && self.random() % 3 == 0 {
                    return Anim::Coffee;
                }
                // quase sempre digitando; às vezes pensa (de Sherlock, investiga), tem uma ideia
                const BUSY: [Anim; 11] = [Anim::Type, Anim::Type, Anim::Type, Anim::Type, Anim::Type, Anim::Type, Anim::Think, Anim::Think, Anim::Idea, Anim::Blink, Anim::Look];
                match BUSY[(self.random() % BUSY.len() as u64) as usize] {
                    Anim::Think if self.hat => Anim::Investigate,
                    a => a,
                }
            }
            Mood::Worried => [Anim::Sweat, Anim::Shake, Anim::Sweat, Anim::Blink][(self.random() % 4) as usize],
            // lendo: quase sempre virando a página; às vezes ri ou cochila
            Mood::Limit => [Anim::Read, Anim::Read, Anim::Read, Anim::Laugh, Anim::Nod, Anim::Blink][(self.random() % 6) as usize],
            // piscar é o mais comum; pular, acenar e a música, os mais raros
            Mood::Normal => match [Anim::Blink, Anim::Blink, Anim::Blink, Anim::Look, Anim::Look, Anim::Scuttle, Anim::Scuttle, Anim::Hop, Anim::Wave, Anim::Music][(self.random() % 10) as usize] {
                Anim::Music if !self.music => Anim::Blink,
                a => a,
            },
        }
    }
}

/// As animações que podem tocar em cada humor (`data`: com números; sem eles, não trabalha nem
/// chega ao limite), para conferir o espaço de cada quadro.
#[cfg(test)]
pub fn anims_for(mood: Mood, data: bool) -> Vec<Anim> {
    use Anim::*;
    match mood {
        Mood::Normal if data => vec![Blink, Look, Scuttle, Hop, Wave, Love, Music, Party, Wake, Startle, Done, SignDown],
        Mood::Normal => vec![Blink, Look, Scuttle, Hop, Wave, Love, Wake, Startle],
        Mood::Sleepy => vec![Snooze, Startle, Wake],
        Mood::Busy => vec![Blink, Look, Wave, Hop, Type, Think, Idea, Coffee, HatOn, Investigate, Wake],
        Mood::Worried => vec![Sweat, Shake, Blink, Wave, Hop],
        Mood::Limit => vec![SignUp, Read, Laugh, Nod, Blink, Hop],
    }
}

/// Todos os desenhos diferentes que o Clawd pode fazer num humor, visto de `view`: parado e em
/// cada animação que toca nele (de 25 em 25 ms), de chapéu ou não e com a tela vazia, pela
/// metade ou cheia; cada um com a animação de onde veio (None: parado).
#[cfg(test)]
pub fn frames_for(mood: Mood, view: View, data: bool) -> Vec<(Option<Anim>, Vec<Cell>)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut add = |anim: Option<Anim>, cs: Vec<Cell>| {
        let key: Vec<_> = cs.iter().map(|&(x, y, w, h, c)| (x, y, w, h, c.0, c.1, c.2, c.3)).collect();
        if seen.insert((anim.map(|a| a as u8), key)) {
            out.push((anim, cs));
        }
    };
    for hat in [false, true] {
        for code in [0, 20, CODE_TOTAL] {
            let scene = Scene { view, hat, code };
            add(None, cells(&pose(mood, None, &scene, None)));
            for anim in anims_for(mood, data) {
                let secs = anim.secs(PACE);
                let n = (secs * 1000.0 / 25.0).ceil() as usize;
                for i in 0..n {
                    add(Some(anim), cells(&pose(mood, Some(Play { anim, t: i as f32 / n as f32, secs }), &scene, None)));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os quadradinhos ocupados (sem cor; os quase transparentes não contam), para comparar
    /// desenhos.
    fn grid(cs: &[Cell]) -> std::collections::BTreeSet<(i32, i32)> {
        let mut set = std::collections::BTreeSet::new();
        for &(x, y, w, h, c) in cs {
            if c.3 < 10 {
                continue;
            }
            for i in x..x + w {
                for j in y..y + h {
                    set.insert((i, j));
                }
            }
        }
        set
    }

    fn only(cs: &[Cell], colors: &[Rgba]) -> std::collections::BTreeSet<(i32, i32)> {
        grid(&cs.iter().filter(|c| colors.contains(&c.4)).cloned().collect::<Vec<_>>())
    }

    fn scene(view: View) -> Scene {
        Scene::new(view)
    }

    fn at(mood: Mood, anim: Anim, t: f32, sc: &Scene) -> Pose {
        pose(mood, Some(Play { anim, t, secs: anim.secs(PACE) }), sc, None)
    }

    #[test]
    fn standing_clawd_is_the_logo_with_volume() {
        let cs = cells(&pose(Mood::Normal, None, &scene(View::Front), None));
        let g = grid(&cs);
        // a cabeça (com a luz em cima) e os cantos arredondados; o corpo de 4 a 22
        let light = only(&cs, &[LIGHT]);
        assert!(light.contains(&(5, 0)) && light.contains(&(21, 0)) && !g.contains(&(4, 0)) && !g.contains(&(22, 0)));
        assert!(g.contains(&(4, 5)) && g.contains(&(22, 5)) && !g.contains(&(3, 5)));
        // a sombra à direita e embaixo
        let shade = only(&cs, &[SHADE]);
        assert!(shade.contains(&(22, 4)) && shade.contains(&(10, 11)));
        // os olhos com o brilho
        assert_eq!(only(&cs, &[EYE]).len(), 2 * 6);
        assert!(only(&cs, &[SHINE]) == [(8, 3), (19, 3)].into_iter().collect());
        // as garras dos lados, com a pinça aberta para fora, e as patas de dois quadradinhos
        assert!(g.contains(&(1, 6)) && g.contains(&(25, 6)) && !g.contains(&(1, 7)) && !g.contains(&(25, 7)) && g.contains(&(2, 7)));
        for x in [6, 7, 9, 10, 16, 17, 19, 20] {
            assert!((12..15).all(|y| g.contains(&(x, y))), "pata em {x}");
        }
        // parado, sem boca (como o logo), e tudo dentro da grade
        assert!(only(&cs, &[MOUTH]).is_empty());
        assert!(g.iter().all(|&(x, y)| (0..COLS as i32).contains(&x) && (0..ROWS as i32).contains(&y)));
    }

    #[test]
    fn the_drawings_in_text_are_rectangles() {
        let sets: [&[&str]; 13] = [&HAT_ART, &MAG_ART, &PIPE_ART, &CUP_ART, &CUP_SIP, &PHONES_ART, &READ_ART, &CHECK_MARK, &BULB_GLASS, &LOGO_MARK, &Z3, &NOTE8, &HEART_L];
        for rows in sets {
            assert!(rows.iter().all(|r| r.len() == rows[0].len()), "{rows:?}");
        }
        // o chapéu e o fone, de frente, são simétricos
        for rows in [&HAT_ART[..], &PHONES_ART[..]] {
            for r in rows {
                let shape: Vec<bool> = r.chars().map(|c| c != '.').collect();
                let back: Vec<bool> = shape.iter().rev().cloned().collect();
                assert_eq!(shape, back, "{r}");
            }
        }
    }

    #[test]
    fn every_animation_moves_and_stays_in_its_room() {
        for data in [true, false] {
            for mood in [Mood::Normal, Mood::Sleepy, Mood::Busy, Mood::Worried, Mood::Limit] {
                if !data && matches!(mood, Mood::Busy | Mood::Worried | Mood::Limit) {
                    continue;
                }
                for view in [View::Front, View::Side] {
                    let sc = scene(view);
                    let still = cells(&pose(mood, None, &sc, None));
                    for anim in anims_for(mood, data) {
                        let moves = (0..40).any(|k| cells(&at(mood, anim, k as f32 / 40.0, &sc)) != still);
                        assert!(moves, "{anim:?} com humor {mood:?} não mexe em nada");
                        // e nada sai do espaço em volta da grade
                        for k in 0..=80 {
                            let g = grid(&cells(&at(mood, anim, k as f32 / 80.0, &sc)));
                            assert!(g.iter().all(|&(x, y)| (-8..SIDE_COLS as i32 + 3).contains(&x) && (-9..ROWS as i32 + 3).contains(&y)), "{anim:?} {view:?} sai do lugar");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn types_on_the_laptop_with_one_claw_at_a_time() {
        // de frente: as garras se revezam e a tecla apertada acende
        let sc = scene(View::Front);
        let mut sides = Vec::new();
        for k in 0..=200 {
            let p = at(Mood::Busy, Anim::Type, k as f32 / 200.0, &sc);
            assert!(p.front.is_some_and(|f| f.open) && p.glow);
            let down = p.claws.map(|c| c == Claw::KeyDown);
            assert!(!(down[0] && down[1]), "as duas garras batem juntas");
            if down[0] || down[1] {
                assert!(p.front.unwrap().key.is_some(), "bate sem tecla acesa");
                if sides.last() != Some(&down[0]) {
                    sides.push(down[0]);
                }
            }
        }
        assert!(sides.len() >= 8, "não reveza: {sides:?}");
        // olhando para a tela, os olhos espiam por cima da tampa (ela cobre só a última linha)
        let cs = cells(&pose(Mood::Busy, None, &sc, None));
        let lid = cs.iter().position(|c| c.4 == ALU).unwrap();
        assert!(cs.iter().position(|c| c.4 == EYE).unwrap() < lid);
        assert!(only(&cs, &[EYE]).iter().filter(|&&(_, y)| y < 6).count() >= 8);
        // de lado: o código cresce na tela, sem pular
        let sc = Scene { code: 5, ..scene(View::Side) };
        let mut before = 5;
        for k in 0..=200 {
            let code = at(Mood::Busy, Anim::Type, k as f32 / 200.0, &sc).side.unwrap().code;
            assert!(code >= before && code - before <= 2, "o código pula");
            before = code;
        }
        assert!(before > 15);
        // a tela mostra no máximo 4 linhas, dentro dela, com o cursor também dentro
        for code in 0..=CODE_TOTAL {
            let cs = cells(&Pose { side: Some(Side { open: true, code, done: false, key: None }), ..stand() });
            let lines = only(&cs, &CODE);
            assert!(lines.iter().all(|&(x, y)| (29..36).contains(&x) && (3..12).contains(&y)), "{code}");
            assert!(lines.iter().map(|p| p.1).collect::<std::collections::BTreeSet<_>>().len() <= 4);
        }
    }

    #[test]
    fn coffee_is_held_to_the_side_and_sipped() {
        for view in [View::Front, View::Side] {
            let sc = scene(view);
            let hold = at(Mood::Busy, Anim::Coffee, 0.3, &sc);
            assert!(hold.cup.is_some_and(|(sip, steam)| !sip && steam.is_some()) && hold.eyes == Eyes::Happy);
            assert_eq!(hold.claws[0], Claw::CupSide);
            let sip = at(Mood::Busy, Anim::Coffee, 0.55, &sc);
            assert!(sip.cup.is_some_and(|c| c.0) && sip.blush.is_some() && sip.claws[0] == Claw::CupFront, "{view:?}: sem o gole");
            assert_eq!(at(Mood::Busy, Anim::Coffee, 0.9, &sc).mouth, Some(Mouth::Smile));
            // a garra fica por cima da asa da caneca
            let cs = cells(&hold);
            let cup = cs.iter().position(|c| c.4 == CUP).unwrap();
            let claw = cs.iter().rposition(|c| c.4 == LIGHT).unwrap();
            assert!(claw > cup);
        }
    }

    #[test]
    fn sherlock_wears_the_hat_for_the_whole_task_now_and_then() {
        // de vez em quando: mais ou menos uma tarefa em quatro
        let mut crab = Crab::new();
        let mut dressed = 0;
        for _ in 0..400 {
            crab.tick(Mood::Normal);
            crab.anim = None;
            crab.tick(Mood::Busy);
            crab.anim = None;
            dressed += crab.sherlock as i32;
        }
        assert!((50..160).contains(&dressed), "{dressed} de 400");
        // de Sherlock: primeiro põe o chapéu; daí em diante, investiga em vez de pensar
        let mut crab = Crab::new();
        crab.mood = Mood::Busy;
        crab.sherlock = true;
        assert_eq!(crab.pick(Mood::Busy), Anim::HatOn);
        crab.play(Anim::HatOn);
        assert!(crab.hat);
        assert!((0..300).all(|_| crab.pick(Mood::Busy) != Anim::Think));
        assert!((0..300).any(|_| crab.pick(Mood::Busy) == Anim::Investigate));
        // o chapéu em todos os quadros do trabalho; o cachimbo só de lado (de frente, a tampa
        // cobre a boca); a lupa investigando, com um olho apertado
        for view in [View::Front, View::Side] {
            let sc = Scene { hat: true, ..scene(view) };
            for anim in [Anim::Type, Anim::Idea, Anim::Investigate, Anim::Blink] {
                let p = at(Mood::Busy, anim, 0.5, &sc);
                assert!(p.hat.is_some(), "{view:?} {anim:?}: sem chapéu");
                assert_eq!(p.pipe.is_some(), view == View::Side, "{view:?} {anim:?}: cachimbo");
            }
            let inv = at(Mood::Busy, Anim::Investigate, 0.4, &sc);
            assert!(inv.mag.is_some() && inv.eyes == Eyes::Sherlock(1, 0));
            assert!(pose(Mood::Busy, None, &sc, None).hat.is_some());
            // sem a fantasia, nada disso; e o chapéu não sobra fora do trabalho
            let plain = at(Mood::Busy, Anim::Type, 0.5, &scene(view));
            assert!(plain.hat.is_none() && plain.pipe.is_none());
            assert!(pose(Mood::Normal, None, &sc, None).hat.is_none());
        }
        // a tarefa acabou: o chapéu sai junto com o ✓
        crab.play(Anim::Done);
        crab.anim = Some((Anim::Done, Instant::now() - Duration::from_secs(10), 3.1));
        crab.tick(Mood::Normal);
        assert!(!crab.hat && !crab.sherlock);
    }

    #[test]
    fn at_the_limit_puts_up_the_sign_and_reads_until_it_resets() {
        let mut crab = Crab::new();
        crab.tick(Mood::Limit);
        assert!(crab.is_playing(Anim::SignUp), "não pegou a placa");
        for view in [View::Front, View::Side] {
            let sc = scene(view);
            // começa no lugar, com o susto; termina de lado, com a placa em pé e o livro aberto
            let first = at(Mood::Limit, Anim::SignUp, 0.0, &sc);
            assert!(first.bang && first.sign.is_none() && first.dx == 0);
            let reading = pose(Mood::Limit, None, &sc, None);
            assert_eq!(reading.dx, limit_shift(view));
            assert!(reading.sign.is_some_and(|s| s.h == 11) && reading.book.is_some());
            let end = at(Mood::Limit, Anim::SignUp, 0.99, &sc);
            assert!(end.sign == reading.sign && end.dx == reading.dx, "{view:?}: pula ao terminar");
            // a placa fica do lado dele: os olhos e o livro não ficam atrás dela
            let cs = cells(&reading);
            let sign = only(&cs, &[WOOD, WOOD_DARK, SIGN_TEXT]);
            assert!(only(&cs, &[EYE]).iter().chain(only(&cs, &[COVER]).iter()).all(|e| !sign.contains(e)));
            // lendo, a página vira; rindo, pula; cochilando, os "z"
            assert!((0..100).any(|k| at(Mood::Limit, Anim::Read, k as f32 / 100.0, &sc).book.unwrap().1 > 0));
            assert!((0..20).any(|k| at(Mood::Limit, Anim::Laugh, k as f32 / 20.0, &sc).dy < 0));
            assert!(at(Mood::Limit, Anim::Nod, 0.5, &sc).zzz.is_some());
            // e volta para o meio, sem a placa
            let back = at(Mood::Normal, Anim::SignDown, 0.99, &sc);
            assert!(back.dx == 0 && back.sign.is_none());
        }
        // o limite reiniciou: recolhe a placa e, logo depois, o confete
        crab.anim = None;
        crab.tick(Mood::Normal);
        assert!(crab.is_playing(Anim::SignDown));
        crab.anim = Some((Anim::SignDown, Instant::now() - Duration::from_secs(10), 2.3));
        assert!(crab.tick(Mood::Normal));
        assert!(crab.is_playing(Anim::Party), "não comemorou");
    }

    #[test]
    fn wakes_slowly_when_claude_code_comes_back_and_with_a_start_at_the_mouse() {
        let mut crab = Crab::new();
        crab.tick(Mood::Sleepy);
        crab.tick(Mood::Normal);
        assert!(crab.is_playing(Anim::Wake));
        // a primeira parte ainda é sentado, de gorro; no fim, de pé e sorrindo
        let sc = scene(View::Front);
        let first = at(Mood::Normal, Anim::Wake, 0.01, &sc);
        assert!(first.cap.is_some() && first.legs == Legs::Sit && first.eyes == Eyes::Peek);
        assert!(at(Mood::Normal, Anim::Wake, 0.99, &sc).cap.is_none());
        // dormindo, o mouse chegando: o susto, já no primeiro quadro
        let mut crab = Crab::new();
        crab.poke(Mood::Sleepy);
        assert!(crab.is_playing(Anim::Startle));
        assert!(at(Mood::Sleepy, Anim::Startle, 0.0, &sc).bang);
        // dormindo: sentado, de gorro, e os "z" e a respiração só no cochilo
        let still = pose(Mood::Sleepy, None, &sc, None);
        assert!(still.cap.is_some() && still.zzz.is_none() && still.legs == Legs::Sit);
        assert!((0..20).any(|k| at(Mood::Sleepy, Anim::Snooze, k as f32 / 20.0, &sc).breath));
    }

    #[test]
    fn listens_to_music_with_the_headphones_on() {
        let sc = scene(View::Front);
        let mid = at(Mood::Normal, Anim::Music, 0.3, &sc);
        assert!(mid.phones == Some(0) && mid.notes.is_some() && mid.eyes == Eyes::Chill);
        // balança no ritmo
        let dxs: std::collections::BTreeSet<_> = (0..60).map(|k| at(Mood::Normal, Anim::Music, 0.1 + k as f32 / 200.0, &sc).dx).collect();
        assert_eq!(dxs.len(), 2, "{dxs:?}");
        // põe e tira o fone
        assert_eq!(at(Mood::Normal, Anim::Music, 0.01, &sc).phones, Some(-3));
        assert!(at(Mood::Normal, Anim::Music, 0.99, &sc).phones.is_none());
    }

    #[test]
    fn reacts_to_the_mouse_on_the_first_frame() {
        for mood in [Mood::Normal, Mood::Sleepy, Mood::Busy, Mood::Worried, Mood::Limit] {
            let sc = scene(View::Side);
            let still = cells(&pose(mood, None, &sc, None));
            for _ in 0..6 {
                let mut crab = Crab::new();
                crab.poke(mood);
                let anim = crab.current().unwrap().anim;
                assert_ne!(cells(&at(mood, anim, 0.0, &sc)), still, "{anim:?} com humor {mood:?} demora a aparecer");
                let mut crab = Crab::new();
                crab.greet(mood);
                let anim = crab.current().unwrap().anim;
                assert_ne!(cells(&at(mood, anim, 0.0, &sc)), still, "{anim:?} com humor {mood:?} demora a aparecer");
            }
            assert_ne!(cells(&at(mood, Anim::Hop, 0.0, &sc)), still, "o pulo do clique demora em {mood:?}");
        }
        let mut crab = Crab::new();
        crab.play(Anim::Hop);
        assert!(crab.next_tick() <= Instant::now() + Duration::from_millis(40));
    }

    #[test]
    fn rests_between_animations_and_redraws_only_when_something_changes() {
        let mut crab = Crab::new();
        assert!(!crab.animating(), "começa parado");
        assert!(!crab.tick(Mood::Normal), "parado não redesenha");
        assert!(crab.next_tick() > Instant::now() + Duration::from_secs(1));
        crab.poke(Mood::Normal);
        assert!(crab.animating() && crab.tick(Mood::Normal));
        crab.anim = Some((Anim::Blink, Instant::now() - Duration::from_secs(5), 0.2));
        assert!(crab.tick(Mood::Normal));
        assert!(!crab.animating());
        assert!(crab.next_tick().saturating_duration_since(Instant::now()) >= Duration::from_secs(6));
        // digitando, logo depois do primeiro quadro o desenho ainda é o mesmo: não redesenha
        let mut crab = Crab::playing(Anim::Type, 0.31);
        crab.mood = Mood::Busy;
        assert!(crab.tick(Mood::Busy));
        assert!(!crab.tick(Mood::Busy), "redesenhou sem nada mudar");
        // começou a trabalhar: pega o notebook logo
        let mut crab = Crab::new();
        crab.next = Instant::now() + Duration::from_secs(14);
        crab.tick(Mood::Busy);
        assert!(crab.next_tick() <= Instant::now() + Duration::from_secs(1));
    }

    #[test]
    fn an_idea_speeds_up_typing_and_coffee_waits_for_a_long_session() {
        let mut crab = Crab::new();
        crab.mood = Mood::Busy;
        crab.play(Anim::Idea);
        assert_eq!(crab.pick(Mood::Busy), Anim::Type, "depois da ideia, digita");
        crab.play(Anim::Type);
        let fast = crab.anim.unwrap().2;
        crab.play(Anim::Type);
        assert!((fast - crab.pace * 0.5).abs() < 1e-4 && (crab.anim.unwrap().2 - crab.pace).abs() < 1e-4);
        assert!(pace_for(0.0) == PACE_SLOW && pace_for(1000.0) == PACE_FAST && pace_for(20.0) < pace_for(5.0));
        let mut crab = Crab::new();
        assert!((0..300).all(|_| crab.pick(Mood::Busy) != Anim::Coffee));
        crab.tired = true;
        assert!((0..300).any(|_| crab.pick(Mood::Busy) == Anim::Coffee));
        crab.play(Anim::Coffee);
        assert!((0..300).all(|_| crab.pick(Mood::Busy) != Anim::Coffee));
        // a tela enche com as digitações e começa outra quando está cheia
        let mut crab = Crab::new();
        crab.mood = Mood::Busy;
        for _ in 0..6 {
            crab.play(Anim::Type);
            let secs = crab.anim.unwrap().2;
            crab.anim = Some((Anim::Type, Instant::now() - Duration::from_secs(20), secs));
            crab.tick(Mood::Busy);
        }
        assert_eq!(crab.code, CODE_TOTAL);
        crab.play(Anim::Type);
        assert_eq!(crab.code, 0);
    }

    #[test]
    fn moods_pick_their_own_animations() {
        let mut crab = Crab::new();
        for _ in 0..100 {
            assert_eq!(crab.pick(Mood::Sleepy), Anim::Snooze);
            assert!([Anim::Sweat, Anim::Shake, Anim::Blink].contains(&crab.pick(Mood::Worried)));
            assert!([Anim::Read, Anim::Laugh, Anim::Nod, Anim::Blink].contains(&crab.pick(Mood::Limit)));
            let n = crab.pick(Mood::Normal);
            assert!([Anim::Blink, Anim::Look, Anim::Scuttle, Anim::Hop, Anim::Wave, Anim::Music].contains(&n), "{n:?}");
            let b = crab.pick(Mood::Busy);
            assert!([Anim::Type, Anim::Think, Anim::Idea, Anim::Blink, Anim::Look].contains(&b), "{b:?}");
            crab.inspired = false;
        }
        // sem números, sem música
        assert!((0..300).any(|_| crab.pick(Mood::Normal) == Anim::Music));
        crab.music = false;
        assert!((0..300).all(|_| crab.pick(Mood::Normal) != Anim::Music));
    }

    #[test]
    fn zs_rise_and_fade_in_steps() {
        let mut alphas = std::collections::BTreeSet::new();
        for k in 0..=400 {
            let z = k as f32 / 400.0;
            for &(_, _, _, born, dies) in &zs_at(z) {
                alphas.insert((steps_alpha(z, born, dies) * 100.0) as i32);
            }
        }
        assert!(alphas.len() <= 4, "{alphas:?}");
        // crescem e sobem: o pequeno embaixo, o grande em cima
        assert!(ZS[0].0.len() < ZS[2].0.len() && ZS[2].2 < ZS[0].2);
        assert!(zs_at(0.999).is_empty());
    }

    #[test]
    fn cells_fall_on_whole_pixels() {
        // 2/3 do quadradinho antigo, em pixels inteiros: a 125%, no pequeno (2 pontos), 2 pixels
        // no lugar de 3; a 100%, 1 no lugar de 2; a 150% no médio (2,4 pontos), 3 no lugar de 4
        assert_eq!(cell_px(1.25, 2.0), 2.0);
        assert_eq!(cell_px(1.0, 2.0), 1.0);
        assert_eq!(cell_px(1.25, 2.4), 2.0);
        assert_eq!(cell_px(1.5, 2.4), 3.0);
        let (x0, y0, c) = origin(1.25, 77.0, 129.0, 2.0);
        assert!(((x0 * 1.25).round() - x0 * 1.25).abs() < 1e-3 && ((y0 * 1.25).round() - y0 * 1.25).abs() < 1e-3);
        assert!((c * 1.25 - 2.0).abs() < 1e-4);
    }

    #[test]
    fn drawn_on_whole_pixels() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let mut c = Canvas::new(80, 60, 1.25);
        draw(&mut c, 32.0, 24.0, 2.0, &pose(Mood::Normal, None, &scene(View::Front), None));
        let img = c.finish();
        let mut solid = 0;
        for p in img.px.chunks(4) {
            if p[3] == 0 {
                continue;
            }
            assert_eq!(p[3], 255, "pixel meio transparente na borda: {p:?}");
            solid += 1;
        }
        assert!(solid > 300);
    }

    /// Folha com os quadros de cada animação, ampliada (%TEMP%\clawd.png):
    ///   cargo test --release clawd_sheet -- --ignored --nocapture
    #[test]
    #[ignore]
    fn clawd_sheet() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let mut rows: Vec<(Mood, Option<Anim>, Scene)> = Vec::new();
        for mood in [Mood::Normal, Mood::Sleepy, Mood::Busy, Mood::Worried, Mood::Limit] {
            for view in [View::Front, View::Side] {
                rows.push((mood, None, scene(view)));
                for anim in anims_for(mood, true) {
                    rows.push((mood, Some(anim), scene(view)));
                    if mood == Mood::Busy {
                        rows.push((mood, Some(anim), Scene { hat: true, code: 20, ..scene(view) }));
                    }
                }
            }
        }
        let (cell_w, cell_h, frames) = (130.0, 72.0, 14);
        let mut c = Canvas::new((cell_w * frames as f32) as u32, (cell_h * rows.len() as f32) as u32, 1.0);
        c.fill_round_rect(0.0, 0.0, c.w as f32, c.h as f32, 0.0, Rgba::rgb(60, 50, 90));
        for (r, (mood, anim, sc)) in rows.iter().enumerate() {
            for f in 0..frames {
                let t = f as f32 / (frames - 1) as f32;
                let p = pose(*mood, anim.map(|a| Play { anim: a, t, secs: a.secs(PACE) }), sc, None);
                draw(&mut c, cell_w * f as f32 + 40.0, cell_h * (r as f32 + 0.62), 2.6, &p);
            }
        }
        let path = std::env::temp_dir().join("clawd.png");
        crate::gfx::save_png(&c.finish(), &path).unwrap();
        println!("{}", path.display());
    }
}
