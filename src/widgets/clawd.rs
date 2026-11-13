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
//! - programando: no notebook (de frente, no colo, no pequeno; de lado, com o código na tela, no
//!   médio, no grande e na barra de tarefas), digita, pensa, tem ideias e, depois de um bom tempo,
//!   toma um café;
//! - investigando (o Claude Code lendo e procurando no código): guarda o notebook e vira Sherlock
//!   Holmes (chapéu de detetive, cachimbo e lupa): examina com a lupa (o olho aumentado nela),
//!   segue pegadas no chão e pensa fumando o cachimbo; volta ao notebook quando o Claude Code
//!   volta a editar. No fim de qualquer tarefa, se espreguiça e mostra um ✓;
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

const CROSS: Rgba = Rgba::rgb(255, 99, 99);
const SAND: Rgba = Rgba::rgb(242, 196, 110);
const PAGE_EDGE: Rgba = Rgba::rgb(196, 186, 166);
const PENCIL: Rgba = Rgba::rgb(255, 209, 102);
const ERASER: Rgba = Rgba::rgb(242, 132, 164);
const BOARD: Rgba = Rgba::rgb(176, 122, 74);
const CLIP: Rgba = Rgba::rgb(139, 143, 156);

const SANTA: Rgba = Rgba::rgb(214, 48, 49);
const SANTA_LIGHT: Rgba = Rgba::rgb(240, 92, 84);
const SANTA_DARK: Rgba = Rgba::rgb(160, 30, 36);
const FUR: Rgba = Rgba::rgb(246, 244, 240);
const FUR_SHADE: Rgba = Rgba::rgb(206, 208, 222);
const SNOW: Rgba = Rgba::rgb(246, 248, 255);

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

/// o ✗ de quando um comando falha
const CROSS_MARK: [&str; 5] = ["#...#", ".#.#.", "..#..", ".#.#.", "#...#"];
/// o binóculo, na frente dos olhos
const BINOC_ART: [&str; 4] = [".KKKK.....KKKK.", "KGGWGKKKKKGGWGK", "KGGGGK...KGGGGK", ".KKKK.....KKKK."];
/// um ajudante (subagente): um Clawd pequenininho
const HELPER_ART: [&str; 4] = [".LLL.", "LEBEB", "BBBBS", "S.S.S"];
/// o gorro de Papai Noel, das colunas 3 a 26: a copa redonda, com luz à esquerda e sombra à
/// direita, deitando para a direita até o pompom; e a barra de pelo grossa, um pouco mais larga
/// que a cabeça, cobrindo o alto dela. l luz, r vermelho, d sombra, W pelo, g sombra do pelo
/// (baixo: pulando, mais alto encostaria no nome do anel do pequeno)
const SANTA_ART: [&str; 5] = [
    ".......llrrrrrdd.....WW.",
    ".....llrrrrrrrrrrdddWWWg",
    "...lrrrrrrrrrrrrrrdd.gg.",
    "WWWWWWWWWWWWWWWWWWWWW...",
    "gWWWgWWWWgWWWWWgWWWWg...",
];
/// a neve do Natal: quantos flocos, quantos quadros por segundo (na camada animada do cartão) e
/// a faixa onde cai, em quadradinhos (das colunas `SNOW_LEFT` a `SNOW_LEFT + SNOW_WIDE`, das
/// linhas `SNOW_TOP` a `SNOW_TOP + SNOW_FALL`)
const FLAKES: i32 = 15;
pub const SNOW_FPS: u32 = 30;
const SNOW_LEFT: f32 = -2.0;
const SNOW_WIDE: i32 = 31;
const SNOW_TOP: f32 = -5.0;
const SNOW_FALL: f32 = 20.0;
/// os ajudantes: em cima do notebook de lado, ou no chão à direita (um em cima do outro)
const HELPERS_LAPTOP: [(i32, i32); 2] = [(28, -2), (32, -2)];
const HELPERS_GROUND: [(i32, i32); 2] = [(23, 11), (23, 7)];
/// quantos ajudantes aparecem (mais que isso, o balão conta)
pub const HELPERS_SHOWN: u8 = 2;
/// a pilha de papéis da conversa enchendo: até tantas folhas, no chão à esquerda
pub const PILE_MAX: u8 = 4;

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
/// a lupa (o canto de cima de `MAG_ART`): segura do lado, na frente do olho direito e baixa,
/// examinando o chão; e os caminhos de um lugar ao outro, um quadradinho por quadro
const MAG_REST: (i32, i32) = (21, 0);
const MAG_EYE: (i32, i32) = (14, 0);
const MAG_LOW: (i32, i32) = (19, 4);
const MAG_UP: [(i32, i32); 8] = [(21, 0), (20, 0), (19, 0), (18, 0), (17, 0), (16, 0), (15, 0), (14, 0)];
const MAG_DOWN: [(i32, i32); 6] = [(14, 0), (15, 1), (16, 2), (17, 3), (18, 4), (19, 4)];
const MAG_BACK: [(i32, i32); 4] = [(19, 4), (20, 3), (21, 2), (21, 1)];
/// as pegadas no chão, seguidas com a lupa: (coluna, linha), de frente e de lado
const PRINTS_FRONT: [(i32, i32); 2] = [(23, 14), (27, 14)];
const PRINTS_SIDE: [(i32, i32); 3] = [(25, 14), (29, 14), (33, 14)];
const PRINT: Rgba = Rgba::rgb(170, 122, 86);
/// uma pegada: três dedinhos em cima e o meio embaixo
const PRINT_ART: [&str; 2] = ["#.#", ".#."];

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
/// Sem saber o que o Claude Code está fazendo (sem as conversas), uma tarefa em cada tantas é de
/// investigar.
const SHERLOCK_ONE_IN: u64 = 4;
/// Investigando ou programando, fica pelo menos tanto tempo antes de trocar (o Claude Code
/// alterna leituras e edições a todo instante).
const SWITCH_AFTER: Duration = Duration::from_secs(8);

/// Como o Clawd está: muda as animações que ele faz.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Normal,
    /// sem números para mostrar (não conectado ou esperando o Claude Code): dorme
    Sleepy,
    /// o Claude Code está trabalhando agora: no notebook ou, investigando, de Sherlock
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
    /// vai investigar: fecha e guarda o notebook, põe o chapéu de detetive, pega a lupa
    HatOn,
    /// volta a programar: guarda a lupa, tira o chapéu e abre o notebook
    HatOff,
    /// de Sherlock: examina com a lupa (o olho aumentado nela) e depois o chão
    Investigate,
    /// de Sherlock: segue as pegadas no chão com a lupa
    Trail,
    /// de Sherlock: pensa fumando o cachimbo
    Puff,
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
    /// o Claude Code precisa de você: acena com um "!" em cima
    Call,
    /// um comando falhou: a garra na testa, suando, com um ✗
    Oops,
    /// o comando passou depois de falhar: comemora
    Cheer,
    /// um comando demorado: olha a ampulheta, batendo o pé
    Wait,
    /// pesquisando na web: olha longe com o binóculo
    Scout,
    /// planejando: escreve numa prancheta
    Plan,
    /// a conversa foi compactada: arruma a pilha de papéis
    Tidy,
}

#[cfg(test)]
pub const ANIMS: [Anim; 35] = [
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
    Anim::HatOff,
    Anim::Investigate,
    Anim::Trail,
    Anim::Puff,
    Anim::Done,
    Anim::SignUp,
    Anim::SignDown,
    Anim::Read,
    Anim::Laugh,
    Anim::Nod,
    Anim::Call,
    Anim::Oops,
    Anim::Cheer,
    Anim::Wait,
    Anim::Scout,
    Anim::Plan,
    Anim::Tidy,
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
/// fecha o notebook, guarda, põe o chapéu (descendo) e pega a lupa; e o caminho de volta
const HAT_ON: [f32; 4] = [350.0, 250.0, 600.0, 400.0];
const HAT_OFF: [f32; 4] = [350.0, 550.0, 250.0, 250.0];
/// terminou investigando: abaixa a lupa, se espreguiça e mostra o ✓ (os mesmos 3,1 s)
const DONE_SLEUTH: [f32; 3] = [800.0, 900.0, 1400.0];
/// a parte do pulo em que ele agacha antes de subir
const CROUCH: f32 = 0.1;

fn total(segs: &[f32]) -> f32 {
    segs.iter().sum::<f32>() / 1000.0
}

impl Anim {
    /// Duração, em segundos, digitando `pace` segundos por vez.
    pub fn secs(self, pace: f32) -> f32 {
        match self {
            Anim::Blink => 0.24,
            Anim::Look => 1.8,
            Anim::Scuttle => 1.7,
            Anim::Hop => 0.9,
            Anim::Wave => 1.3,
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
            Anim::HatOn => total(&HAT_ON),
            Anim::HatOff => total(&HAT_OFF),
            Anim::Investigate => 3.2,
            Anim::Trail => 3.6,
            Anim::Puff => 2.6,
            // (de frente e de lado, os mesmos 3,1 s)
            Anim::Done => total(&DONE_FRONT),
            Anim::SignUp => total(&SIGN_UP),
            Anim::SignDown => total(&SIGN_DOWN),
            Anim::Read => 3.2,
            Anim::Laugh => 1.1,
            Anim::Nod => total(&NOD),
            Anim::Call => 1.6,
            Anim::Oops => 2.0,
            Anim::Cheer => 1.6,
            Anim::Wait => 3.0,
            Anim::Scout => 3.2,
            Anim::Plan => 3.4,
            Anim::Tidy => 2.0,
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
    /// de detetive: o esquerdo bem aberto, o direito apertado (ou atrás da lupa, aumentado)
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
    /// no meio do caminho entre parada e levantada
    Mid,
    Up,
    /// levantada e um quadradinho para fora (acenando, balança entre esta e `Up`)
    Out,
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
    /// segurando a caneca do lado, no meio do caminho e na frente da boca
    CupSide,
    CupMid,
    CupFront,
    /// segurando o livro
    Book,
    /// na testa, cobrindo o olho (um comando falhou)
    Palm,
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
            Claw::Mid => (&REST, 1, 4),
            Claw::Up => (&UP, 1, 2),
            Claw::Out => (&UP, 0, 2),
            Claw::High => (&UP, 1, 0),
            Claw::Limp => (&[".BBB", "KKKK"], 0, 11),
            Claw::Key => (&FLAT, 5, 11),
            Claw::KeyDown => (&FLAT, 5, 12),
            Claw::SideKey => (&["LBB", "KKK"], 1, 11),
            Claw::SideKeyDown => (&["LBB", "KKK"], 1, 12),
            Claw::Down => (&REST, 1, 9),
            Claw::CupSide => (&FLAT, 3, 6),
            Claw::Palm => (&FLAT, 6, 2),
            Claw::CupMid => (&FLAT, 5, 5),
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

/// Onde está a caneca do café.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cup {
    Side,
    Mid,
    Sip,
}

/// O que aparece aumentado na lupa.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lens {
    Glass,
    /// o olho direito, olhando para um lado (-1, 0, 1)
    Eye(i32),
    /// uma pegada
    Print,
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
    /// a caneca: (onde: do lado, no meio do caminho ou na boca; o vapor subindo)
    pub cup: Option<(Cup, Option<f32>)>,
    /// o chapéu de detetive (e quanto acima da cabeça)
    pub hat: Option<i32>,
    /// a lupa: (coluna, linha, o que aparece aumentado nela)
    pub mag: Option<(i32, i32, Lens)>,
    /// as pegadas no chão, de quem investiga (`PRINTS_FRONT` ou `PRINTS_SIDE`)
    pub prints: Option<&'static [(i32, i32)]>,
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
    /// o ✗ vermelho (um comando falhou)
    pub cross: bool,
    /// a ampulheta, segura do lado: quanto da areia já caiu (0 a 1)
    pub hourglass: Option<f32>,
    /// o binóculo nos olhos (e para que lado ele olha: -1, 0, 1)
    pub binoculars: Option<i32>,
    /// a prancheta: as letras já escritas
    pub clipboard: Option<i32>,
    /// os ajudantes (subagentes) e onde ficam
    pub helpers: Option<(u8, &'static [(i32, i32)])>,
    /// a pilha de papéis da conversa enchendo (0 a `PILE_MAX` folhas)
    pub pile: u8,
    /// o tema de Natal: o gorro de Papai Noel (a neve cai na camada animada, `draw_snow`)
    pub xmas: bool,
    /// há quanto tempo (ms) a animação de agora começou (a neve caindo, os ajudantes mexendo)
    pub ms: Option<f32>,
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

/// A ideia, partindo de `base` (no notebook ou de Sherlock): a lâmpada aparece apagada, pisca e
/// acende de vez; ele levanta as garras (de Sherlock, só a que não segura a lupa).
fn idea(base: Pose, u: f32) -> Pose {
    let lit = (0.25..0.32).contains(&u) || u >= 0.42;
    let claw = match u {
        u if (0.38..0.42).contains(&u) || (0.8..0.85).contains(&u) => Some(Claw::Mid),
        u if (0.42..0.8).contains(&u) => Some(Claw::Up),
        _ => None,
    };
    let mut p = base;
    p.eyes = if claw == Some(Claw::Up) { Eyes::Happy } else if lit { Eyes::Open(0, 0) } else { Eyes::Open(1, -1) };
    if u > 0.08 {
        p.bulb = Some((lit, lit && (u * 10.0) as i32 % 2 == 0));
    }
    if let Some(c) = claw {
        p.claws[0] = c;
        if p.mag.is_none() {
            p.claws[1] = c;
        }
    }
    p
}

/// O café: pega a caneca e a segura do lado (olhos ^ ^), leva à boca (passando pelo meio do
/// caminho), dá um gole e solta o "ahh".
fn coffee(view: View, code: i32, u: f32, ms: f32) -> Pose {
    let mut p = work_base(view, code);
    let steam = Some((ms / 1500.0).fract());
    let (eyes, cup, claw) = match u {
        u if u < 0.12 => (Eyes::Open(-1, 0), (Cup::Side, None), Claw::CupSide),
        u if u < 0.4 => (Eyes::Happy, (Cup::Side, steam), Claw::CupSide),
        u if u < 0.44 => (Eyes::Open(-1, 0), (Cup::Mid, steam), Claw::CupMid),
        u if u < 0.7 => {
            p.blush = Some(false);
            (Eyes::Happy, (Cup::Sip, steam), Claw::CupFront)
        }
        u if u < 0.74 => (Eyes::Happy, (Cup::Mid, steam), Claw::CupMid),
        _ => {
            p.mouth = Some(Mouth::Smile);
            (Eyes::Happy, (Cup::Side, steam), Claw::CupSide)
        }
    };
    p.eyes = eyes;
    p.cup = Some(cup);
    p.claws[0] = claw;
    p
}

/// O ponto de `path` a `u` (0 a 1) do caminho.
fn along(path: &[(i32, i32)], u: f32) -> (i32, i32) {
    path[((u.max(0.0) * path.len() as f32) as usize).min(path.len() - 1)]
}

/// De Sherlock, parado: a lupa segura do lado com a garra direita (o chapéu e o cachimbo vêm
/// com a fantasia, em `pose`).
fn sleuth_base() -> Pose {
    Pose { mag: Some((MAG_REST.0, MAG_REST.1, Lens::Glass)), claws: [Claw::Rest, Claw::Hidden], ..stand() }
}

/// Examinando com a lupa: ela sobe até o olho direito (que aparece aumentado nela, olhando para
/// os lados), desce até o chão (uma pegada na lente) e volta; no fim, uns pontinhos ("hmm...").
fn investigate(u: f32) -> Pose {
    let mut p = sleuth_base();
    let (x, y, lens, eyes) = match u {
        u if u < 0.18 => {
            let (x, y) = along(&MAG_UP, u / 0.18);
            (x, y, Lens::Glass, Eyes::Open(1, 0))
        }
        u if u < 0.55 => {
            // o olho aumentado vai de um lado ao outro; a lupa balança um pouco junto
            let k = ((u - 0.18) / 0.37 * 7.0) as usize;
            let look = [0, -1, -1, 0, 1, 1, 0][k.min(6)];
            (MAG_EYE.0 - (k % 2) as i32, MAG_EYE.1, Lens::Eye(look), Eyes::Sherlock(look, 0))
        }
        u if u < 0.7 => {
            let (x, y) = along(&MAG_DOWN, (u - 0.55) / 0.15);
            (x, y, Lens::Glass, Eyes::Open(1, 1))
        }
        u if u < 0.8 => (MAG_LOW.0, MAG_LOW.1, Lens::Print, Eyes::Sherlock(1, 1)),
        u if u < 0.88 => {
            let (x, y) = along(&MAG_BACK, (u - 0.8) / 0.08);
            (x, y, Lens::Glass, Eyes::Open(1, 0))
        }
        _ => (MAG_REST.0, MAG_REST.1, Lens::Glass, Eyes::Open(0, -1)),
    };
    p.mag = Some((x, y, lens));
    p.eyes = eyes;
    if u > 0.88 {
        p.dots = (((u - 0.88) * 30.0) as u8).min(3);
    }
    p
}

/// Seguindo as pegadas: abaixa a lupa, anda de lado até elas olhando o chão, para em cima da
/// última examinando e volta.
fn trail(view: View, u: f32) -> Pose {
    let far = if view == View::Side { 6 } else { 3 };
    let mut p = sleuth_base();
    p.prints = Some(if view == View::Side { &PRINTS_SIDE } else { &PRINTS_FRONT });
    let walk = |p: &mut Pose, dx: i32, st: i32| {
        p.dx = dx;
        p.legs = if st % 2 == 1 { Legs::StepA } else { Legs::StepB };
    };
    match u {
        u if u < 0.1 => {
            let (x, y) = along(&[(21, 0), (21, 1), (20, 2), (20, 3), (19, 4)], u / 0.1);
            p.mag = Some((x, y, Lens::Glass));
            p.eyes = Eyes::Open(1, 1);
        }
        // anda até as pegadas, um passo por quadro
        u if u < 0.45 => {
            let st = ((u - 0.1) / 0.35 * (far * 2) as f32) as i32;
            walk(&mut p, (st + 1) / 2, st);
            p.mag = Some((MAG_LOW.0, MAG_LOW.1, Lens::Print));
            p.eyes = Eyes::Sherlock(1, 1);
        }
        // examina a última
        u if u < 0.65 => {
            p.dx = far;
            let bob = ((u - 0.45) / 0.2 * 4.0) as i32 % 2;
            p.mag = Some((MAG_LOW.0 + bob, MAG_LOW.1, Lens::Print));
            p.eyes = Eyes::Sherlock(1, 1);
            if u > 0.57 {
                p.dots = (((u - 0.57) * 40.0) as u8).min(3);
            }
        }
        // volta, de lupa um pouco mais alta
        u if u < 0.92 => {
            let st = ((u - 0.65) / 0.27 * (far * 2) as f32) as i32;
            walk(&mut p, far - (st + 1) / 2, st);
            p.mag = Some((MAG_LOW.0, MAG_LOW.1 - 2, Lens::Glass));
            p.eyes = Eyes::Open(-1, 0);
        }
        _ => {
            let (x, y) = along(&[(20, 1), (21, 0)], (u - 0.92) / 0.04);
            p.mag = Some((x, y, Lens::Glass));
            p.eyes = Eyes::Open(0, 0);
        }
    }
    p
}

/// Pensando fumando o cachimbo: olha para cima, puxa a fumaça (os olhos meio fechados), a garra
/// livre no queixo, e solta em baforadas; no fim, uns pontinhos.
fn puff(u: f32, ms: f32) -> Pose {
    let mut p = sleuth_base();
    p.pipe = Some(Some((ms / 1100.0).fract()));
    p.eyes = match u {
        u if u < 0.15 => Eyes::Open(-1, -1),
        u if u < 0.45 => Eyes::Half,
        u if u < 0.5 => Eyes::Blink,
        _ => Eyes::Open(-1, -1),
    };
    if (0.1..0.85).contains(&u) {
        p.claws[0] = if u < 0.14 || u > 0.81 { Claw::Mid } else { Claw::Up };
    }
    if u > 0.7 {
        p.dots = (((u - 0.7) * 20.0) as u8).min(3);
    }
    p
}

/// Chamando você: um "!" em cima e a garra da esquerda balançando depressa, de olhos arregalados
/// (sem largar o notebook nem a lupa).
fn call(base: Pose, ms: f32) -> Pose {
    let mut p = base;
    let beat = (ms / 120.0) as i32 % 2 == 0;
    p.claws[0] = if beat { Claw::Up } else { Claw::Out };
    p.bang = (ms / 400.0) as i32 % 4 != 3;
    if matches!(p.eyes, Eyes::Open(..) | Eyes::Sherlock(..)) {
        p.eyes = if (ms / 400.0) as i32 % 2 == 0 { Eyes::Wide } else { Eyes::Open(0, 0) };
    }
    p.glow = false;
    p
}

/// Um comando falhou: leva a garra à testa (apertando os olhos), com um ✗ em cima, e fica
/// suando, de boca torta.
fn oops(base: Pose, u: f32) -> Pose {
    let mut p = base;
    p.cross = u < 0.85;
    p.claws[0] = match u {
        u if u < 0.08 || (0.6..0.68).contains(&u) => Claw::Mid,
        u if u < 0.6 => Claw::Palm,
        _ => p.claws[0],
    };
    p.eyes = if u < 0.6 { Eyes::Squeeze } else { Eyes::Open(0, 1) };
    p.mouth = Some(Mouth::Wavy);
    p.sweat = Some(u);
    p
}

/// O comando passou depois de falhar: levanta as garras (as livres), dá dois pulinhos e sorri.
fn cheer(base: Pose, u: f32, ms: f32) -> Pose {
    let mut p = base;
    let wave = if (ms / 140.0) as i32 % 2 == 0 { Claw::Up } else { Claw::Out };
    let claw = if u < 0.08 || u > 0.92 { Claw::Mid } else { wave };
    p.claws[0] = claw;
    if p.mag.is_none() {
        p.claws[1] = claw;
    }
    p.dy = if (0.15..0.3).contains(&u) || (0.45..0.6).contains(&u) { -1 } else { 0 };
    p.eyes = Eyes::Happy;
    p.mouth = Some(Mouth::Grin);
    p.check = (0.1..0.9).contains(&u);
    p
}

/// Esperando um comando demorado: segura a ampulheta do lado, olhando a areia cair, batendo o pé.
fn wait(base: Pose, u: f32, ms: f32) -> Pose {
    let mut p = base;
    p.claws[0] = if u < 0.06 || u > 0.94 { Claw::Mid } else { Claw::Up };
    if u >= 0.06 && u <= 0.94 {
        p.hourglass = Some(((u - 0.06) / 0.88).clamp(0.0, 1.0));
    }
    p.eyes = if (0.45..0.5).contains(&u) { Eyes::Blink } else { Eyes::Open(-1, 0) };
    p.legs = if (ms / 300.0) as i32 % 2 == 0 { Legs::Tap } else { Legs::Stand };
    p.glow = false;
    p
}

/// Pesquisando na web: guarda a lupa e olha longe com o binóculo, para um lado e para o outro;
/// no fim, uns pontinhos.
fn scout(u: f32) -> Pose {
    let mut p = sleuth_base();
    p.mag = None;
    let pan = match u {
        u if u < 0.1 || u > 0.92 => None,
        u => Some([0, -1, -1, 0, 1, 1, 0][(((u - 0.1) / 0.82) * 7.0) as usize % 7]),
    };
    p.binoculars = pan;
    p.claws = if pan.is_some() { [Claw::Up; 2] } else { [Claw::Mid; 2] };
    if u > 0.84 {
        p.dots = (((u - 0.84) * 30.0) as u8).min(3);
    }
    p
}

/// Planejando: guarda a lupa e escreve numa prancheta, olhando para ela; no fim, confere.
fn plan(u: f32) -> Pose {
    let mut p = sleuth_base();
    p.mag = None;
    p.claws = [Claw::Book, Claw::Hidden];
    p.clipboard = Some(((u / 0.85).min(1.0) * 15.0) as i32);
    p.eyes = if u < 0.85 { Eyes::Open(-1, 1) } else { Eyes::Open(0, 0) };
    p
}

/// Arrumando a pilha de papéis (a conversa foi compactada): as folhas saem uma a uma, com as
/// garras se revezando; no fim, sorri.
fn tidy(pile: u8, u: f32) -> Pose {
    let start = pile.max(2) as f32;
    let left = (start * (1.0 - u / 0.8)).ceil().max(0.0) as u8;
    let mut p = stand();
    p.pile = left;
    if u < 0.8 {
        let k = (u * 12.0) as usize % 2;
        p.claws[k] = Claw::Down;
        p.eyes = Eyes::Open(-1, 1);
    } else {
        p.eyes = Eyes::Happy;
        p.mouth = Some(Mouth::Smile);
    }
    p
}

/// Indo investigar: fecha o notebook, guarda (as garras sobem para o chapéu), o chapéu desce até
/// a cabeça e ele pega a lupa, piscando um olho.
fn hat_on(view: View, code: i32, ms: f32) -> Pose {
    let (i, u) = seg(ms, &HAT_ON);
    match i {
        0 => {
            let mut p = work_base(view, code);
            p.front = p.front.map(|f| Front { open: false, ..f });
            p.side = p.side.map(|s| Side { open: false, key: None, ..s });
            p.eyes = Eyes::Open(1, 1);
            p.glow = false;
            p.claws = [Claw::Rest; 2];
            p
        }
        1 => Pose { eyes: Eyes::Open(0, -1), claws: [if u < 0.5 { Claw::Mid } else { Claw::Up }; 2], ..stand() },
        2 => {
            // (mais alto que um quadradinho, no pequeno, encostaria no nome do anel)
            let hdy = if u < 0.5 { -1 } else { 0 };
            Pose { eyes: Eyes::Open(0, -1), claws: [if hdy < 0 { Claw::High } else { Claw::Up }; 2], hat: Some(hdy), ..stand() }
        }
        _ => {
            let (x, y) = along(&[(21, 3), (21, 2), (21, 1), (21, 0)], u / 0.6);
            Pose { eyes: Eyes::Sherlock(0, 0), mag: Some((x, y, Lens::Glass)), claws: [Claw::Rest, Claw::Hidden], hat: Some(0), pipe: (u > 0.5).then_some(None), ..stand() }
        }
    }
}

/// Voltando a programar: guarda a lupa, tira o chapéu (sobe e some), pega o notebook fechado e
/// abre.
fn hat_off(view: View, code: i32, ms: f32) -> Pose {
    let (i, u) = seg(ms, &HAT_OFF);
    match i {
        0 => {
            let mag = (u < 0.75).then(|| {
                let (x, y) = along(&[(21, 0), (21, 1), (21, 2), (21, 3)], u / 0.75);
                (x, y, Lens::Glass)
            });
            Pose { mag, claws: [Claw::Rest, if mag.is_some() { Claw::Hidden } else { Claw::Rest }], hat: Some(0), pipe: Some(None), ..stand() }
        }
        1 => {
            let claws = [if u < 0.15 { Claw::Up } else { Claw::High }; 2];
            Pose { eyes: Eyes::Open(0, -1), claws, hat: (u < 0.7).then_some(-((u / 0.35) as i32).min(1)), ..stand() }
        }
        2 => {
            let mut p = work_base(view, code);
            p.front = p.front.map(|f| Front { open: false, ..f });
            p.side = p.side.map(|s| Side { open: false, key: None, ..s });
            p.glow = false;
            p.claws = [Claw::Rest; 2];
            p
        }
        _ => {
            let mut p = work_base(view, code);
            p.eyes = Eyes::Open(0, 0);
            p
        }
    }
}

/// Terminou: fecha o notebook (de lado, primeiro o ✓ na tela) ou abaixa a lupa, se espreguiça e
/// mostra o ✓.
fn done(view: View, ms: f32, sleuth: bool) -> Pose {
    let stretch = Pose { dy: -1, legs: Legs::Stretch, eyes: Eyes::Squeeze, mouth: Some(Mouth::Yawn), claws: [Claw::High; 2], ..stand() };
    if sleuth {
        let (i, u) = seg(ms, &DONE_SLEUTH);
        return match i {
            0 => {
                let mag = (u < 0.6).then(|| {
                    let (x, y) = along(&[(21, 0), (21, 1), (21, 2), (21, 3)], u / 0.6);
                    (x, y, Lens::Glass)
                });
                Pose { eyes: Eyes::Happy, mag, claws: [Claw::Rest, if mag.is_some() { Claw::Hidden } else { Claw::Rest }], ..stand() }
            }
            1 => stretch,
            _ => Pose { eyes: Eyes::Happy, mouth: Some(Mouth::Smile), check: !(0.4..0.48).contains(&u), ..stand() },
        };
    }
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
        Mood::Busy if scene.sleuth => sleuth_base(),
        Mood::Busy => work_base(scene.view, scene.code),
        Mood::Worried => Pose { mouth: Some(Mouth::Wavy), blush: Some(true), sweat: Some(0.0), ..stand() },
        Mood::Limit => at_limit(reading(), scene.view, 11, true),
    }
}

/// O que está em volta dele agora: de onde é visto, se está investigando (de Sherlock), o
/// código na tela, os ajudantes, a pilha de papéis e o tema de Natal.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Scene {
    pub view: View,
    /// investigando, de Sherlock (sem o notebook)
    pub sleuth: bool,
    /// as letras de código na tela do notebook de lado
    pub code: i32,
    /// subagentes trabalhando
    pub helpers: u8,
    /// a pilha de papéis da conversa enchendo (0 a `PILE_MAX`)
    pub pile: u8,
    pub xmas: bool,
}

impl Scene {
    #[cfg(test)]
    pub fn new(view: View) -> Scene {
        Scene { view, sleuth: false, code: 0, helpers: 0, pile: 0, xmas: false }
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
    let base = still(mood, scene);
    let mut p = match play {
        None => base,
        Some(Play { anim, t, secs }) => {
            let t = t.clamp(0.0, 1.0);
            frame(anim, t, secs, base, scene)
        }
    };
    // a fantasia de Sherlock, investigando: o chapéu e o cachimbo (menos bocejando ou falando)
    // (pulando, acordando ou levando um susto, sem a fantasia; pondo ou tirando, ela vem no
    // desenho da animação)
    let dressed = match play.map(|p| p.anim) {
        None => mood == Mood::Busy,
        Some(Anim::Done) => true,
        Some(Anim::HatOn | Anim::HatOff | Anim::Hop | Anim::Love | Anim::Wake | Anim::Startle) => false,
        Some(_) => mood == Mood::Busy,
    };
    if scene.sleuth && dressed {
        if p.hat.is_none() {
            p.hat = Some(0);
        }
        if p.mouth.is_none() && p.pipe.is_none() {
            let ms = play.map(|p| p.t * p.secs * 1000.0);
            p.pipe = Some(ms.map(|ms| (ms / 2200.0).fract()));
        }
    }
    // em volta: os ajudantes trabalhando junto (em cima do notebook de lado, ou no chão), a pilha
    // de papéis da conversa e o Natal
    p.xmas = scene.xmas;
    let around = mood == Mood::Busy;
    if around && scene.helpers > 0 {
        let spots: &'static [(i32, i32)] = if p.side.is_some_and(|s| s.open) { &HELPERS_LAPTOP } else { &HELPERS_GROUND };
        p.helpers = Some((scene.helpers.min(HELPERS_SHOWN), spots));
    }
    if around && play.map(|p| p.anim) != Some(Anim::Tidy) {
        p.pile = scene.pile.min(PILE_MAX);
    }
    // (o tempo só entra no quadro quando algo depende dele: os ajudantes)
    if p.helpers.is_some() {
        p.ms = play.map(|p| p.t.clamp(0.0, 1.0) * p.secs * 1000.0);
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
            // fecha e abre passando pela metade
            let mut p = base;
            open_eyes(&mut p, if (0.3..0.7).contains(&t) { Eyes::Blink } else { Eyes::Half });
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
                Pose { dy: 1, legs: Legs::Tuck, claws: [Claw::Mid; 2], ..stand() }
            } else {
                // dois pulos, cada um subindo e descendo um quadradinho por quadro e amortecendo
                // no chão
                let h = ((t - CROUCH) / (1.0 - CROUCH) * 2.0).fract();
                if h > 0.86 {
                    return Pose { dy: 1, legs: Legs::Half, eyes: Eyes::Squeeze, claws: [Claw::Mid; 2], ..stand() };
                }
                let dy = -((h / 0.86 * std::f32::consts::PI).sin() * 4.0).round() as i32;
                let claws = match dy {
                    d if d <= -3 => Claw::Up,
                    d if d < 0 => Claw::Mid,
                    _ => Claw::Rest,
                };
                Pose { dy, legs: if dy < 0 { Legs::Tuck } else { Legs::Stand }, claws: [claws; 2], ..stand() }
            }
        }
        Anim::Wave => {
            // levanta a garra da esquerda, balança de um lado para o outro e abaixa, sorrindo
            // (trabalhando, sem largar o notebook nem a lupa)
            let mut p = base;
            p.claws[0] = match t {
                t if t < 0.08 || (0.86..0.94).contains(&t) => Claw::Mid,
                t if t < 0.86 => {
                    if ((t - 0.08) * secs * 1000.0 / 140.0) as i32 % 2 == 0 {
                        Claw::Up
                    } else {
                        Claw::Out
                    }
                }
                _ => p.claws[0],
            };
            open_eyes(&mut p, Eyes::Happy);
            // (de Sherlock, sorri com o cachimbo na boca)
            if p.mouth.is_none() && !scene.sleuth {
                p.mouth = Some(Mouth::Smile);
            }
            p
        }
        Anim::Love => Pose {
            eyes: if t < 0.75 { Eyes::Happy } else { Eyes::Open(0, 0) },
            blush: Some(false),
            mouth: Some(Mouth::Smile),
            heart: Some(t),
            claws: [match t {
                t if t < 0.06 || (0.45..0.53).contains(&t) => Claw::Mid,
                t if t < 0.45 => Claw::Up,
                _ => Claw::Rest,
            }; 2],
            ..stand()
        },
        Anim::Music => {
            let (i, u) = seg(ms, &MUSIC);
            let notes = Some((ms / 2600.0).fract());
            let b = beat(ms);
            match i {
                0 => Pose { eyes: Eyes::Open(0, -1), claws: [if u < 0.4 { Claw::Mid } else { Claw::Up }; 2], phones: Some(-3), ..stand() },
                // o fone desce até a cabeça
                1 => Pose { eyes: Eyes::Happy, claws: [Claw::Up; 2], phones: Some(-2 + (u * 3.0).min(2.0) as i32), ..stand() },
                2 | 4 => Pose { eyes: Eyes::Chill, phones: Some(0), notes, dx: b as i32, legs: if b { Legs::Tap } else { Legs::Stand }, ..stand() },
                3 => Pose { eyes: Eyes::Happy, mouth: Some(Mouth::Smile), phones: Some(0), notes, dx: b as i32, claws: if b { [Claw::Up, Claw::Rest] } else { [Claw::Rest, Claw::Up] }, ..stand() },
                5 => Pose { eyes: Eyes::Happy, claws: [Claw::Up; 2], phones: Some(-1 - (u * 3.0).min(2.0) as i32), ..stand() },
                _ => Pose { claws: [if u < 0.2 { Claw::Mid } else { Claw::Rest }; 2], ..stand() },
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
        Anim::Idea => idea(base, t),
        Anim::Coffee => coffee(view, scene.code, t, ms),
        Anim::HatOn => hat_on(view, scene.code, ms),
        Anim::HatOff => hat_off(view, scene.code, ms),
        Anim::Investigate => investigate(t),
        Anim::Trail => trail(view, t),
        Anim::Puff => puff(t, ms),
        Anim::Done => done(view, ms, scene.sleuth),
        Anim::Call => call(base, ms),
        Anim::Oops => oops(base, t),
        Anim::Cheer => cheer(base, t, ms),
        Anim::Wait => wait(base, t, ms),
        Anim::Scout => scout(t),
        Anim::Plan => plan(t),
        Anim::Tidy => tidy(scene.pile, t),
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

/// Os flocos de neve a `secs` segundos: (x, y, lado, opacidade), em quadradinhos, com posições
/// quebradas (a neve desliza, sem andar de quadradinho em quadradinho). Três camadas: os da
/// frente, maiores e claros, caem mais rápido e balançam mais; os do fundo, menores e apagados,
/// devagar. Cada um aparece aos poucos lá em cima, some perto do chão e, a cada volta, cai numa
/// coluna nova (a neve não fica repetindo o mesmo desenho).
pub fn flakes(secs: f32) -> impl Iterator<Item = (f32, f32, f32, f32)> {
    (0..FLAKES).map(move |i| {
        let layer = (i % 3) as usize;
        // quadradinhos por segundo
        let speed = [3.6, 2.5, 1.7][layer];
        let fall = secs * speed + (i * 7) as f32 % SNOW_FALL + i as f32 * 0.37;
        let lap = (fall / SNOW_FALL).floor() as i32;
        let row = fall.rem_euclid(SNOW_FALL);
        let col = (i * 11 + 3 + lap * 13).rem_euclid(SNOW_WIDE) as f32;
        // o balanço: devagar, de um lado para o outro
        let sway = (secs * [1.9, 1.5, 1.1][layer] + i as f32 * 1.9).sin() * [1.2, 0.9, 0.6][layer];
        let size = [1.0, 0.8, 0.6][layer];
        // aparece nas duas primeiras linhas e some nas três últimas
        let fade = (row / 2.0).min((SNOW_FALL - row) / 3.0).clamp(0.0, 1.0);
        (SNOW_LEFT + col + sway, SNOW_TOP + row, size, [0.95, 0.7, 0.5][layer] * fade)
    })
}

/// Os flocos que aparecem com ele em `p`: os que passam por trás dele (e do que ele segura) ficam
/// escondidos, como se a neve caísse atrás.
pub fn visible_flakes(p: &Pose, secs: f32) -> Vec<(f32, f32, f32, f32)> {
    let body = cells(p);
    flakes(secs)
        .filter(|&(x, y, size, a)| {
            let (mx, my) = (x + size / 2.0, y + size / 2.0);
            a > 0.02 && !body.iter().any(|&(bx, by, bw, bh, _)| mx >= bx as f32 && mx < (bx + bw) as f32 && my >= by as f32 && my < (by + bh) as f32)
        })
        .collect()
}

/// A neve caindo em volta dele (centro da grade em (cx, cy), lugar de `unit` pontos), na camada
/// animada do cartão: redesenhada a cada quadro, sem redesenhar o cartão.
pub fn draw_snow(c: &mut Canvas, cx: f32, cy: f32, unit: f32, p: &Pose, secs: f32) {
    let (x0, y0, u) = origin(c.s, cx, cy, unit);
    for (x, y, size, a) in visible_flakes(p, secs) {
        let (ax, ay) = (x0 + x * u, y0 + y * u);
        c.fill_round_rect(ax, ay, ax + size * u, ay + size * u, size * u * 0.25, SNOW.alpha(a));
    }
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


    // a pilha de papéis no chão à esquerda: uma folha (com a beirada) por vez, meio torta
    for k in 0..p.pile.min(PILE_MAX) as i32 {
        let (x, y) = (-3 + (k % 2), 13 - 2 * k);
        put(x, y, 4, 1, PAGE, false);
        put(x, y + 1, 4, 1, PAGE_EDGE, false);
    }

    // as pegadas no chão (por baixo dele, que passa por cima), sem andar junto com ele
    if let Some(prints) = p.prints {
        for &(x, y) in prints {
            art(&mut put, &PRINT_ART, x, y - 1, one(PRINT), false, false);
        }
    }

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
    if let Some((at, steam)) = p.cup {
        let sip = at == Cup::Sip;
        let (cx, cy) = match at {
            Cup::Side => (-2, 4),
            Cup::Mid => (2, 3),
            Cup::Sip => (9, 4),
        };
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
    if let Some((mx, my, lens)) = p.mag {
        // a lupa, segura pela garra direita na ponta do cabo; dentro dela, o olho ou a pegada
        // aumentados (o brilho do vidro fica por cima)
        let color = |ch: u8| match ch {
            b'K' => Some(LENS),
            b'G' => Some(glass()),
            b'W' => Some(GLINT),
            _ => Some(HANDLE),
        };
        art(&mut put, &MAG_ART, mx, my, color, false, true);
        match lens {
            Lens::Glass => {}
            Lens::Eye(look) => {
                // o olho direito por trás da lente: o dobro de alto, com o brilho
                let ex = mx + 4 + look.clamp(-1, 1);
                put(ex, my + 1, 2, 5, EYE, true);
                put(ex + 1, my + 1, 1, 2, SHINE, true);
            }
            Lens::Print => {
                art(&mut put, &[".##.", "#..#", "#.##", ".#.."], mx + 3, my + 1, one(PRINT), false, true);
            }
        }
        art(&mut put, &["W", ".W"], mx + 6, my + 1, one(GLINT.alpha(0.9)), false, true);
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
    // o gorro de Papai Noel (no Natal, quando não há outro chapéu; dormindo, é ele o gorro de
    // dormir, e sai do mesmo jeito ao acordar)
    let santa = |a: f32| {
        move |ch: u8| match ch {
            b'l' => Some(SANTA_LIGHT.alpha(a)),
            b'r' => Some(SANTA.alpha(a)),
            b'd' => Some(SANTA_DARK.alpha(a)),
            b'g' => Some(FUR_SHADE.alpha(a)),
            _ => Some(FUR.alpha(a)),
        }
    };
    if p.xmas && p.hat.is_none() && p.cap.is_none() && p.phones.is_none() {
        art(&mut put, &SANTA_ART, 3, -3 + top, santa(1.0), false, true);
    }
    if let Some((cdy, a)) = p.cap {
        if a > 0.0 && p.xmas {
            art(&mut put, &SANTA_ART, 3, -3 + top + cdy, santa(a), false, true);
        } else if a > 0.0 {
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
    if p.cross {
        art(&mut put, &CROSS_MARK, 23, -5, one(CROSS), false, true);
    }
    if let Some(fell) = p.hourglass {
        // a ampulheta na garra da esquerda: a areia de cima vai para baixo, um grão caindo
        let (hx, hy) = (-2, 0);
        put(hx, hy, 5, 1, BOARD, true);
        put(hx, hy + 6, 5, 1, BOARD, true);
        put(hx + 1, hy + 1, 3, 2, glass(), true);
        put(hx + 2, hy + 3, 1, 1, glass(), true);
        put(hx + 1, hy + 4, 3, 2, glass(), true);
        let top_sand = (3.0 * (1.0 - fell)).round() as i32;
        let bottom_sand = 3 - top_sand;
        if top_sand >= 1 {
            put(hx + 1, hy + 2, top_sand.min(3), 1, SAND, true);
        }
        if top_sand >= 3 {
            put(hx + 2, hy + 1, 1, 1, SAND, true);
        }
        if bottom_sand >= 1 {
            put(hx + 1, hy + 5, bottom_sand.min(3), 1, SAND, true);
        }
        if bottom_sand >= 3 {
            put(hx + 2, hy + 4, 1, 1, SAND, true);
        }
        if fell < 0.98 {
            put(hx + 2, hy + 3, 1, 1, SAND, true);
        }
    }
    if let Some(pan) = p.binoculars {
        let color = |ch: u8| match ch {
            b'K' => Some(LENS),
            b'W' => Some(GLINT),
            _ => Some(glass()),
        };
        art(&mut put, &BINOC_ART, 5 + pan, 2, color, false, true);
    }
    if let Some(written) = p.clipboard {
        // a prancheta na frente, com o papel; as linhas aparecem enquanto ele escreve
        put(7, 5, 8, 9, BOARD, true);
        put(9, 4, 4, 1, CLIP, true);
        put(8, 6, 6, 7, PAGE, true);
        let mut left = written;
        for row in 0..3 {
            let n = left.clamp(0, 5);
            if n > 0 {
                put(8, 7 + 2 * row, n, 1, CODE[(row as usize + 1) % 5], true);
            }
            left -= 5;
        }
        // o lápis na garra da direita, na ponta do que está sendo escrito
        let row = (written.clamp(0, 14) / 5).min(2);
        let col = 8 + (written - row * 5).clamp(0, 5);
        let (px, py) = (col, 6 + 2 * row);
        put(px, py, 1, 1, EYE, true);
        put(px + 1, py - 1, 1, 1, PENCIL, true);
        put(px + 2, py - 2, 1, 1, PENCIL, true);
        put(px + 3, py - 3, 1, 1, ERASER, true);
        art(&mut put, &["LBB", "BBK"], px + 3, py - 2, claw_color, false, true);
    }
    if let Some((n, spots)) = p.helpers {
        // os ajudantes, mexendo quando ele mexe (cada um no seu tempo)
        let color = |ch: u8| match ch {
            b'L' => Some(LIGHT),
            b'E' => Some(EYE),
            b'S' => Some(SHADE),
            _ => Some(BODY),
        };
        for (k, &(x, y)) in spots.iter().take(n as usize).enumerate() {
            let bob = p.ms.map_or(0, |ms| ((ms / 220.0) as i32 + k as i32) % 2);
            art(&mut put, &HELPER_ART, x, y - bob, color, false, false);
        }
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
    /// investigando, de Sherlock (sem o notebook); e quando trocou por último
    sleuth: bool,
    switched: Option<Instant>,
    /// o que o Claude Code está fazendo: lendo e procurando (true) ou editando e rodando
    /// comandos (false); None: não dá para saber (aí vale o sorteio de cada tarefa, `guess`)
    pub investigating: Option<bool>,
    guess: bool,
    /// o Claude Code precisa de você (pedindo permissão, perguntando)
    pub calling: bool,
    /// pesquisando na web, planejando, esperando um comando demorado
    pub web: bool,
    pub planning: bool,
    pub running: bool,
    /// subagentes trabalhando e a pilha de papéis da conversa enchendo
    pub helpers: u8,
    pub pile: u8,
    /// o tema de Natal (mudado também na hora de desenhar, quando a opção muda) e desde quando a
    /// neve cai
    pub xmas: std::cell::Cell<bool>,
    snow_since: Instant,
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
            sleuth: false,
            switched: None,
            investigating: None,
            guess: false,
            calling: false,
            web: false,
            planning: false,
            running: false,
            helpers: 0,
            pile: 0,
            xmas: std::cell::Cell::new(false),
            snow_since: Instant::now(),
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

    /// O mesmo, investigando, já de Sherlock (para as imagens e os testes).
    #[cfg(test)]
    pub fn dressed(mut self) -> Crab {
        self.sleuth = true;
        self.guess = true;
        self.investigating = Some(true);
        self.switched = Some(Instant::now());
        self.code = 20;
        self
    }

    /// Está investigando (de Sherlock)?
    pub fn sleuthing(&self) -> bool {
        self.sleuth
    }

    /// Investigar ou programar mudou e já está na hora de trocar (de roupa)?
    fn switch_due(&self) -> bool {
        self.investigating.unwrap_or(self.guess) != self.sleuth && self.switched.is_none_or(|t| t.elapsed() >= SWITCH_AFTER)
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
        Scene { view, sleuth: self.sleuth, code: self.code.min(CODE_TOTAL), helpers: self.helpers, pile: self.pile, xmas: self.xmas.get() }
    }

    /// Com a neve caindo há `secs` segundos (para as imagens do README).
    #[cfg(test)]
    pub fn snow_from(&mut self, secs: f32) {
        self.snow_since = Instant::now() - Duration::from_secs_f32(secs);
    }

    /// Há quantos segundos a neve cai (dá a volta a cada hora, sem perder a precisão).
    pub fn snow_secs(&self) -> f32 {
        self.snow_since.elapsed().as_secs_f32() % 3600.0
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
                self.sleuth = false;
            }
            if mood == Mood::Busy && before != Mood::Busy {
                // começou a trabalhar: pega o notebook logo (ou, investigando, vira Sherlock)
                self.guess = self.random() % SHERLOCK_ONE_IN == 0;
                self.sleuth = false;
                self.switched = None;
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
                        self.sleuth = false;
                        self.code = 0;
                    }
                    Anim::HatOn | Anim::HatOff => {
                        self.sleuth = anim == Anim::HatOn;
                        self.switched = Some(now);
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
                // com uma ideia na cabeça (ou a roupa para trocar), volta ao trabalho logo; e,
                // chamando você, continua chamando
                if mood == Mood::Busy && ((self.inspired && !self.sleuth) || self.switch_due()) {
                    self.next = now + Duration::from_millis(300);
                }
                if mood == Mood::Busy && self.calling {
                    self.next = now + Duration::from_millis(700);
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
                if self.calling {
                    return Anim::Call;
                }
                if self.switch_due() {
                    return if self.sleuth { Anim::HatOff } else { Anim::HatOn };
                }
                if self.sleuth {
                    // planejando: a prancheta; na web: o binóculo; investigando o código: quase
                    // sempre a lupa; às vezes segue pegadas, pensa fumando o cachimbo ou tem
                    // uma ideia
                    const PLANNING: [Anim; 6] = [Anim::Plan, Anim::Plan, Anim::Plan, Anim::Puff, Anim::Investigate, Anim::Blink];
                    const WEB: [Anim; 5] = [Anim::Scout, Anim::Scout, Anim::Scout, Anim::Puff, Anim::Blink];
                    const SLEUTH: [Anim; 10] = [Anim::Investigate, Anim::Investigate, Anim::Investigate, Anim::Trail, Anim::Trail, Anim::Puff, Anim::Puff, Anim::Idea, Anim::Blink, Anim::Look];
                    let list: &[Anim] = if self.planning { &PLANNING } else if self.web { &WEB } else { &SLEUTH };
                    return list[(self.random() % list.len() as u64) as usize];
                }
                // um comando demorado: espera olhando a ampulheta
                if self.running {
                    const WAITING: [Anim; 5] = [Anim::Wait, Anim::Wait, Anim::Wait, Anim::Look, Anim::Blink];
                    return WAITING[(self.random() % WAITING.len() as u64) as usize];
                }
                if self.inspired {
                    return Anim::Type;
                }
                // um café de vez em quando, depois de muito tempo trabalhando
                let coffee_due = self.tired && self.last_coffee.is_none_or(|t| t.elapsed() >= COFFEE_EVERY);
                if coffee_due && self.random() % 3 == 0 {
                    return Anim::Coffee;
                }
                // quase sempre digitando; às vezes pensa ou tem uma ideia
                const BUSY: [Anim; 11] = [Anim::Type, Anim::Type, Anim::Type, Anim::Type, Anim::Type, Anim::Type, Anim::Think, Anim::Think, Anim::Idea, Anim::Blink, Anim::Look];
                BUSY[(self.random() % BUSY.len() as u64) as usize]
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
        Mood::Busy => vec![Blink, Look, Wave, Hop, Type, Think, Idea, Coffee, HatOn, HatOff, Investigate, Trail, Puff, Wake, Call, Oops, Cheer, Wait, Scout, Plan, Tidy],
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
    // de Sherlock ou não, com a tela vazia, pela metade ou cheia; e, com a tela pela metade, com
    // tudo em volta (os ajudantes, a pilha de papéis cheia e o Natal)
    let mut scenes = Vec::new();
    for sleuth in [false, true] {
        for code in [0, 20, CODE_TOTAL] {
            scenes.push(Scene { view, sleuth, code, helpers: 0, pile: 0, xmas: false });
        }
        scenes.push(Scene { view, sleuth, code: 20, helpers: HELPERS_SHOWN, pile: PILE_MAX, xmas: true });
    }
    for scene in scenes {
        {
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
            assert!(hold.cup.is_some_and(|(at, steam)| at == Cup::Side && steam.is_some()) && hold.eyes == Eyes::Happy);
            assert_eq!(hold.claws[0], Claw::CupSide);
            let sip = at(Mood::Busy, Anim::Coffee, 0.55, &sc);
            assert!(sip.cup.is_some_and(|c| c.0 == Cup::Sip) && sip.blush.is_some() && sip.claws[0] == Claw::CupFront, "{view:?}: sem o gole");
            // a caneca passa pelo meio do caminho, indo e voltando
            for t in [0.42, 0.72] {
                let mid = at(Mood::Busy, Anim::Coffee, t, &sc);
                assert!(mid.cup.is_some_and(|c| c.0 == Cup::Mid) && mid.claws[0] == Claw::CupMid, "{view:?} a {t}");
            }
            assert_eq!(at(Mood::Busy, Anim::Coffee, 0.9, &sc).mouth, Some(Mouth::Smile));
            // a garra fica por cima da asa da caneca
            let cs = cells(&hold);
            let cup = cs.iter().position(|c| c.4 == CUP).unwrap();
            let claw = cs.iter().rposition(|c| c.4 == LIGHT).unwrap();
            assert!(claw > cup);
        }
    }

    /// Termina a animação de agora, como se o tempo dela tivesse passado.
    fn finish(crab: &mut Crab, mood: Mood) {
        let (anim, _, secs) = crab.anim.unwrap();
        crab.anim = Some((anim, Instant::now() - Duration::from_secs_f32(secs + 1.0), secs));
        crab.tick(mood);
    }

    #[test]
    fn investigates_as_sherlock_and_codes_as_himself() {
        // sem saber o que o Claude Code faz: mais ou menos uma tarefa em quatro é de investigar
        let mut crab = Crab::new();
        let mut sleuths = 0;
        for _ in 0..400 {
            crab.tick(Mood::Normal);
            crab.anim = None;
            crab.tick(Mood::Busy);
            crab.anim = None;
            sleuths += (crab.pick(Mood::Busy) == Anim::HatOn) as i32;
        }
        assert!((50..160).contains(&sleuths), "{sleuths} de 400");
        // o Claude Code lendo e procurando: guarda o notebook e vira Sherlock
        let mut crab = Crab::new();
        crab.tick(Mood::Busy);
        crab.investigating = Some(true);
        assert_eq!(crab.pick(Mood::Busy), Anim::HatOn);
        crab.play(Anim::HatOn);
        assert!(!crab.sleuthing(), "só depois de pôr o chapéu");
        finish(&mut crab, Mood::Busy);
        assert!(crab.sleuthing());
        // de Sherlock, só investiga: nada de digitar, pensar no notebook ou café
        crab.tired = true;
        let picks: Vec<Anim> = (0..400).map(|_| crab.pick(Mood::Busy)).collect();
        for a in [Anim::Investigate, Anim::Trail, Anim::Puff] {
            assert!(picks.contains(&a), "{a:?}");
        }
        for a in [Anim::Type, Anim::Think, Anim::Coffee, Anim::HatOn, Anim::HatOff] {
            assert!(!picks.contains(&a), "{a:?} de Sherlock");
        }
        // voltou a editar: fica mais um tempo investigando (o Claude Code alterna muito), depois
        // tira o chapéu e volta ao notebook
        crab.investigating = Some(false);
        assert_ne!(crab.pick(Mood::Busy), Anim::HatOff, "trocou logo");
        crab.switched = Some(Instant::now() - SWITCH_AFTER);
        assert_eq!(crab.pick(Mood::Busy), Anim::HatOff);
        crab.play(Anim::HatOff);
        finish(&mut crab, Mood::Busy);
        assert!(!crab.sleuthing());
        let picks: Vec<Anim> = (0..400).map(|_| crab.pick(Mood::Busy)).collect();
        assert!(picks.contains(&Anim::Type));
        for a in [Anim::Investigate, Anim::Trail, Anim::Puff] {
            assert!(!picks.contains(&a), "{a:?} programando");
        }
        // investigando, de chapéu, cachimbo e lupa e sem o notebook em todos os quadros; e nada
        // disso programando ou fora do trabalho
        for view in [View::Front, View::Side] {
            let sc = Scene { sleuth: true, ..scene(view) };
            for anim in [Anim::Investigate, Anim::Trail, Anim::Puff, Anim::Idea, Anim::Blink, Anim::Look, Anim::Wave] {
                for k in 0..=20 {
                    let p = at(Mood::Busy, anim, k as f32 / 20.0, &sc);
                    assert!(p.hat.is_some() && p.pipe.is_some(), "{view:?} {anim:?}: sem a fantasia");
                    assert!(p.front.is_none() && p.side.is_none(), "{view:?} {anim:?}: de Sherlock no notebook");
                }
            }
            let still = pose(Mood::Busy, None, &sc, None);
            assert!(still.hat.is_some() && still.mag.is_some() && still.side.is_none() && still.front.is_none());
            for anim in [Anim::Type, Anim::Think, Anim::Idea, Anim::Coffee] {
                let p = at(Mood::Busy, anim, 0.5, &scene(view));
                assert!(p.hat.is_none() && p.pipe.is_none() && p.mag.is_none(), "{view:?} {anim:?}: fantasia programando");
                assert!(p.front.is_some() || p.side.is_some(), "{view:?} {anim:?}: sem o notebook");
            }
            assert!(pose(Mood::Normal, None, &sc, None).hat.is_none());
            // a lupa sobe até o olho, que aparece aumentado nela, e desce até a pegada no chão
            let eye = at(Mood::Busy, Anim::Investigate, 0.4, &sc);
            assert!(matches!(eye.mag, Some((_, _, Lens::Eye(_)))) && matches!(eye.eyes, Eyes::Sherlock(..)));
            assert!(matches!(at(Mood::Busy, Anim::Investigate, 0.75, &sc).mag, Some((_, _, Lens::Print))));
            // seguindo as pegadas, anda até elas e volta
            let far = at(Mood::Busy, Anim::Trail, 0.55, &sc);
            assert!(far.dx > 0 && far.prints.is_some());
            assert_eq!(at(Mood::Busy, Anim::Trail, 0.99, &sc).dx, 0);
        }
        // a tarefa acabou investigando: o ✓ ainda de chapéu, sem o notebook; depois, sem a fantasia
        crab.investigating = Some(true);
        crab.switched = None;
        crab.play(Anim::HatOn);
        finish(&mut crab, Mood::Busy);
        crab.play(Anim::Done);
        let p = crab.pose(Mood::Normal, None, View::Side);
        assert!(p.hat.is_some() && p.side.is_none());
        finish(&mut crab, Mood::Normal);
        assert!(!crab.sleuthing());
    }

    /// O que o Claude Code está fazendo aparece no desenho: chamando você, um comando que falha
    /// ou passa, um comando demorado, a web, o planejamento, os ajudantes, a pilha de papéis da
    /// conversa e a compactação.
    #[test]
    fn shows_what_claude_code_is_doing() {
        let mut crab = Crab::new();
        crab.tick(Mood::Busy);
        // programando (sem isso, o sorteio de cada tarefa às vezes manda investigar)
        crab.investigating = Some(false);
        // precisa de você: chama, sem largar o notebook, e continua chamando
        crab.calling = true;
        assert_eq!(crab.pick(Mood::Busy), Anim::Call);
        for view in [View::Front, View::Side] {
            let sc = Scene { code: 20, ..scene(view) };
            let p = at(Mood::Busy, Anim::Call, 0.1, &sc);
            assert!(p.bang && (p.front.is_some() || p.side.is_some()), "{view:?}");
            // falhou: a garra na testa e o ✗; passou: comemora com o ✓
            let oops = at(Mood::Busy, Anim::Oops, 0.3, &sc);
            assert!(oops.cross && oops.claws[0] == Claw::Palm && oops.sweat.is_some());
            let cheer = at(Mood::Busy, Anim::Cheer, 0.5, &sc);
            assert!(cheer.check && cheer.eyes == Eyes::Happy);
            // esperando: a ampulheta com a areia caindo
            let sand: Vec<f32> = [0.2, 0.5, 0.9].iter().map(|&t| at(Mood::Busy, Anim::Wait, t, &sc).hourglass.unwrap()).collect();
            assert!(sand.windows(2).all(|w| w[0] < w[1]), "{sand:?}");
            // de Sherlock na web: o binóculo no lugar da lupa; planejando: a prancheta, escrevendo
            let sleuth = Scene { sleuth: true, ..sc };
            let scout = at(Mood::Busy, Anim::Scout, 0.5, &sleuth);
            assert!(scout.binoculars.is_some() && scout.mag.is_none() && scout.hat.is_some());
            let (a, b) = (at(Mood::Busy, Anim::Plan, 0.2, &sleuth).clipboard.unwrap(), at(Mood::Busy, Anim::Plan, 0.8, &sleuth).clipboard.unwrap());
            assert!(b > a, "escrevendo: {a} {b}");
            // os ajudantes: em cima do notebook de lado, ou no chão; no máximo dois
            let busy = Scene { helpers: 3, ..sc };
            let (n, spots) = pose(Mood::Busy, None, &busy, None).helpers.unwrap();
            assert_eq!(n, HELPERS_SHOWN);
            assert_eq!(spots, if view == View::Side { &HELPERS_LAPTOP[..] } else { &HELPERS_GROUND[..] });
            assert!(pose(Mood::Normal, None, &busy, None).helpers.is_none(), "sem trabalho, sem ajudantes");
            // a pilha de papéis; compactando, ela some aos poucos
            let full = Scene { pile: PILE_MAX, ..sc };
            assert_eq!(pose(Mood::Busy, None, &full, None).pile, PILE_MAX);
            let piles: Vec<u8> = (0..=10).map(|k| at(Mood::Busy, Anim::Tidy, k as f32 / 10.0, &full).pile).collect();
            assert!(piles.windows(2).all(|w| w[1] <= w[0]) && piles[0] == PILE_MAX && *piles.last().unwrap() == 0, "{piles:?}");
        }
        // no modo de cada coisa, o repertório dela
        crab.calling = false;
        crab.running = true;
        assert!((0..200).any(|_| crab.pick(Mood::Busy) == Anim::Wait) && (0..200).all(|_| crab.pick(Mood::Busy) != Anim::Type));
        crab.running = false;
        crab.investigating = Some(true);
        crab.play(Anim::HatOn);
        finish(&mut crab, Mood::Busy);
        crab.web = true;
        assert!((0..200).any(|_| crab.pick(Mood::Busy) == Anim::Scout) && (0..200).all(|_| crab.pick(Mood::Busy) != Anim::Investigate));
        crab.web = false;
        crab.planning = true;
        assert!((0..200).any(|_| crab.pick(Mood::Busy) == Anim::Plan));
    }

    /// O tema de Natal: só o gorro de Papai Noel (menos com outro chapéu: o de Sherlock, o fone;
    /// o gorro de dormir fica vermelho); a neve fica na camada animada.
    #[test]
    fn christmas_dresses_him_up() {
        let sc = Scene { xmas: true, ..scene(View::Side) };
        let has = |p: &Pose, c: Rgba| cells(p).iter().any(|cell| cell.4 == c);
        let still = pose(Mood::Normal, None, &sc, None);
        assert!(has(&still, SANTA) && has(&still, FUR));
        // só o gorro: o resto dele fica igual a fora do Natal
        let body = |cs: Vec<Cell>| cs.into_iter().filter(|c| ![SANTA, SANTA_LIGHT, SANTA_DARK, FUR, FUR_SHADE].contains(&c.4)).collect::<Vec<_>>();
        assert_eq!(body(cells(&still)), body(cells(&pose(Mood::Normal, None, &scene(View::Side), None))), "algo além do gorro");
        assert!(!has(&pose(Mood::Normal, None, &scene(View::Side), None), SANTA), "fora do Natal");
        // de Sherlock, o chapéu de detetive; ouvindo música, o fone; dormindo, o gorro vermelho
        assert!(!has(&pose(Mood::Busy, None, &Scene { sleuth: true, ..sc }, None), SANTA));
        assert!(!has(&at(Mood::Normal, Anim::Music, 0.5, &sc), FUR));
        assert!(has(&pose(Mood::Sleepy, None, &sc, None), SANTA));
    }

    /// Os flocos deslizam: de um quadro para o outro (30 por segundo), cada um anda só um
    /// pedacinho de quadradinho (ou recomeça lá em cima, em outra coluna), sempre dentro da faixa
    /// da neve; aparecem e somem aos poucos; e a volta seguinte cai em outro lugar.
    #[test]
    fn snow_falls_softly() {
        let dt = 1.0 / SNOW_FPS as f32;
        let at = |k: u32| flakes(k as f32 * dt).collect::<Vec<_>>();
        for k in 0..3000 {
            let (now, next) = (at(k), at(k + 1));
            for (a, b) in now.iter().zip(&next) {
                assert!((SNOW_LEFT - 1.5..SNOW_LEFT + SNOW_WIDE as f32 + 1.5).contains(&a.0), "fora da faixa: {a:?}");
                assert!((SNOW_TOP..SNOW_TOP + SNOW_FALL).contains(&a.1), "fora da faixa: {a:?}");
                let restarted = b.1 < a.1;
                if !restarted {
                    assert!(b.1 - a.1 < 0.2 && (b.0 - a.0).abs() < 0.15, "pulou: {a:?} -> {b:?}");
                } else {
                    // recomeçou: estava sumido embaixo e nasce sumido em cima
                    assert!(a.3 < 0.08 && b.3 < 0.08, "apareceu ou sumiu de uma vez: {a:?} -> {b:?}");
                }
            }
        }
        // claros e apagados
        let all: Vec<_> = (0..300).flat_map(at).collect();
        assert!(all.iter().any(|f| f.3 > 0.9) && all.iter().any(|f| f.3 > 0.1 && f.3 < 0.6));
        let cols = |secs: f32| flakes(secs).map(|f| f.0.round() as i32).collect::<Vec<_>>();
        assert_ne!(cols(0.0), cols(SNOW_FALL / 1.7 * 2.0));
    }

    /// A neve cai por trás dele: nenhum floco aparece em cima do corpo (nem do notebook), e os
    /// outros aparecem.
    #[test]
    fn snow_falls_behind_him() {
        for view in [View::Front, View::Side] {
            let p = pose(Mood::Busy, None, &Scene { xmas: true, code: 20, ..scene(view) }, None);
            let body = cells(&p);
            let mut hidden = false;
            for k in 0..200 {
                let secs = k as f32 * 0.1;
                let shown = visible_flakes(&p, secs);
                assert!(!shown.is_empty(), "{view:?}: sem neve");
                hidden |= shown.len() < flakes(secs).filter(|f| f.3 > 0.02).count();
                for (x, y, size, _) in shown {
                    let (mx, my) = (x + size / 2.0, y + size / 2.0);
                    assert!(!body.iter().any(|b| mx >= b.0 as f32 && mx < (b.0 + b.2) as f32 && my >= b.1 as f32 && my < (b.1 + b.3) as f32), "{view:?}: floco na frente dele");
                }
            }
            assert!(hidden, "{view:?}: nenhum floco passou por trás");
        }
    }

    /// As trocas de pose andam um quadradinho por quadro: o chapéu desce até a cabeça (só um: mais
    /// alto, no pequeno, encostaria no nome do anel), a lupa
    /// sobe até o olho, as garras passam pelo meio do caminho, o fone desce aos poucos.
    #[test]
    fn moves_one_step_at_a_time() {
        let sc = scene(View::Side);
        let sleuth = Scene { sleuth: true, ..sc };
        // o que `get` dá em cada quadro da animação (de 10 em 10 ms), sem repetir em seguida
        let path = |mood: Mood, anim: Anim, sc: &Scene, get: &dyn Fn(&Pose) -> Option<i32>| {
            let n = (anim.secs(PACE) * 100.0) as usize;
            let mut out: Vec<i32> = Vec::new();
            for k in 0..=n {
                if let Some(v) = get(&at(mood, anim, k as f32 / n as f32, sc)) {
                    if out.last() != Some(&v) {
                        out.push(v);
                    }
                }
            }
            out
        };
        let steady = |v: &[i32], what: &str| {
            assert!(v.len() >= 2, "{what}: {v:?}");
            assert!(v.windows(2).all(|w| (w[0] - w[1]).abs() <= 1), "{what} pula: {v:?}");
        };
        steady(&path(Mood::Busy, Anim::HatOn, &sc, &|p| p.hat), "o chapéu descendo");
        steady(&path(Mood::Busy, Anim::HatOff, &sleuth, &|p| p.hat), "o chapéu subindo");
        steady(&path(Mood::Busy, Anim::Investigate, &sleuth, &|p| p.mag.map(|m| m.0)), "a lupa (coluna)");
        steady(&path(Mood::Busy, Anim::Investigate, &sleuth, &|p| p.mag.map(|m| m.1)), "a lupa (linha)");
        steady(&path(Mood::Busy, Anim::Trail, &sleuth, &|p| Some(p.dx)), "andando atrás das pegadas");
        steady(&path(Mood::Normal, Anim::Music, &sc, &|p| p.phones), "o fone");
        steady(&path(Mood::Normal, Anim::Hop, &sc, &|p| Some(p.dy)), "o pulo");
        // a garra: parada (6), no meio (4), levantada (2), nunca direto de uma ponta à outra
        let claw_y = |c: Claw| c.art().map(|a| a.2);
        for (mood, anim) in [(Mood::Normal, Anim::Wave), (Mood::Normal, Anim::Hop), (Mood::Normal, Anim::Love), (Mood::Busy, Anim::Idea)] {
            let ys = path(mood, anim, &sc, &|p| claw_y(p.claws[0]));
            assert!(ys.windows(2).all(|w| !(w[0] == 6 && w[1] == 2) && !(w[0] == 2 && w[1] == 6)), "{anim:?}: a garra pula {ys:?}");
        }
        // o piscar passa pela metade
        let eyes: Vec<Eyes> = (0..=10).map(|k| at(Mood::Normal, Anim::Blink, k as f32 / 10.0, &sc).eyes).collect();
        assert!(eyes.contains(&Eyes::Half) && eyes.contains(&Eyes::Blink));
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
    /// Só algumas animações, maiores e com mais quadros (CLAWD_ZOOM: o tamanho dos quadradinhos;
    /// CLAWD_FRAMES: quantos quadros por linha):
    ///   $env:CLAWD_ANIMS="Wave,Hop"; $env:CLAWD_ZOOM="9"; $env:CLAWD_FRAMES="16"; cargo test ...
    #[test]
    #[ignore]
    fn clawd_sheet() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let only: Option<Vec<String>> = std::env::var("CLAWD_ANIMS").ok().map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
        let zoom: f32 = std::env::var("CLAWD_ZOOM").ok().and_then(|v| v.parse().ok()).unwrap_or(2.6);
        let frames: usize = std::env::var("CLAWD_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(14);
        let wanted = |anim: Option<Anim>| match (&only, anim) {
            (None, _) => true,
            (Some(names), Some(a)) => names.contains(&format!("{a:?}")),
            (Some(names), None) => names.iter().any(|n| n == "None"),
        };
        let mut rows: Vec<(Mood, Option<Anim>, Scene)> = Vec::new();
        for mood in [Mood::Normal, Mood::Sleepy, Mood::Busy, Mood::Worried, Mood::Limit] {
            for view in [View::Front, View::Side] {
                if wanted(None) {
                    rows.push((mood, None, scene(view)));
                    if mood == Mood::Busy {
                        rows.push((mood, None, Scene { sleuth: true, ..scene(view) }));
                    }
                }
                for anim in anims_for(mood, true) {
                    if !wanted(Some(anim)) {
                        continue;
                    }
                    let sleuthing = matches!(anim, Anim::Investigate | Anim::Trail | Anim::Puff | Anim::HatOff | Anim::Scout | Anim::Plan);
                    if !sleuthing && anim != Anim::HatOn {
                        rows.push((mood, Some(anim), Scene { code: 20, ..scene(view) }));
                    }
                    if mood == Mood::Busy && (sleuthing || matches!(anim, Anim::Blink | Anim::Look | Anim::Wave | Anim::Idea | Anim::Done)) {
                        rows.push((mood, Some(anim), Scene { sleuth: true, ..scene(view) }));
                    }
                    if anim == Anim::HatOn {
                        rows.push((mood, Some(anim), Scene { code: 20, ..scene(view) }));
                    }
                }
            }
        }
        // CLAWD_TUDO=1: com os ajudantes, a pilha de papéis cheia e o Natal
        if std::env::var("CLAWD_TUDO").is_ok() {
            for r in rows.iter_mut() {
                r.2 = Scene { helpers: HELPERS_SHOWN, pile: PILE_MAX, xmas: true, ..r.2 };
            }
        }
        let (cell_w, cell_h) = (50.0 * zoom, 28.0 * zoom);
        let mut c = Canvas::new((cell_w * frames as f32) as u32, (cell_h * rows.len() as f32) as u32, 1.0);
        c.fill_round_rect(0.0, 0.0, c.w as f32, c.h as f32, 0.0, Rgba::rgb(60, 50, 90));
        for (r, (mood, anim, sc)) in rows.iter().enumerate() {
            for f in 0..frames {
                let t = f as f32 / (frames - 1) as f32;
                let p = pose(*mood, anim.map(|a| Play { anim: a, t, secs: a.secs(PACE) }), sc, None);
                draw(&mut c, cell_w * f as f32 + 15.0 * zoom, cell_h * (r as f32 + 0.62), zoom, &p);
            }
        }
        let path = std::env::temp_dir().join("clawd.png");
        crate::gfx::save_png(&c.finish(), &path).unwrap();
        println!("{}", path.display());
    }
}

