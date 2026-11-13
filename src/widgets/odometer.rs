//! Números que rolam como num hodômetro quando mudam: cada dígito que mudou é uma coluna de 0
//! a 9 que sobe (o número aumentou) ou desce (diminuiu) até o valor novo, passando pelos do meio.
//!
//! - `draw_rolling`: escreve um texto rolando de outro, a `t` (0 a 1) do caminho;
//! - `Odometer`: um texto de um widget que rola sozinho quando muda (o widget só escreve o texto
//!   de agora e pede quadros enquanto `animating`);
//! - `draw_counting`: um número contando de um valor a outro junto com um arco ou uma barra (o
//!   valor quebrado é o caminho: 43,6 é o 3 rolando para o 4).

use std::cell::RefCell;
use std::time::Instant;

use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::Rgba;

/// Quanto dura o rolar, normalmente (ms).
pub const ROLL_MS: f32 = 650.0;
/// Quanto um dígito anda (em em) de uma casa para a outra: menos que a altura dele, para o que
/// entra e o que sai se cruzarem esmaecendo.
const STEP: f32 = 0.62;
/// Quanto depois do fim ainda pede quadros (o último, parado).
const LAST_FRAME_MS: f32 = 60.0;

/// Um texto que rola quando muda. Percebe a mudança na hora de desenhar (por isso a célula): o
/// texto de antes fica guardado e os dígitos rolam dele até o novo por `ms` milissegundos.
#[derive(Clone, Debug)]
pub struct Odometer {
    ms: f32,
    state: RefCell<Option<Shown>>,
}

/// O que está escrito, o que estava antes (de onde rola) e quando mudou (ou, nas imagens e nos
/// testes, um ponto fixo do caminho).
#[derive(Clone, Debug)]
struct Shown {
    text: String,
    before: Option<String>,
    at: Instant,
    frozen: Option<f32>,
}

impl Default for Odometer {
    fn default() -> Self {
        Odometer::new(ROLL_MS)
    }
}

impl Odometer {
    /// Um hodômetro que rola em `ms` milissegundos.
    pub fn new(ms: f32) -> Odometer {
        Odometer { ms, state: RefCell::new(None) }
    }

    /// Escreve `text` como `Canvas::text`; se ele mudou desde o último desenho, os dígitos rolam
    /// do que estava escrito (o primeiro texto só aparece). Devolve a largura escrita.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, c: &mut Canvas, x: f32, y: f32, text: &str, face: Face, size: f32, color: Rgba, anchor: Anchor, max_w: Option<f32>) -> f32 {
        let (before, t) = self.track(text);
        draw_rolling(c, x, y, text, before.as_deref(), t, face, size, color, anchor, max_w)
    }

    /// Está rolando agora (o widget precisa de quadros)? Um pouco além do fim, para o último
    /// quadro sair com os dígitos parados no lugar.
    pub fn animating(&self) -> bool {
        self.state.borrow().as_ref().is_some_and(|s| s.before.is_some() && ms_since(s.at) < self.ms + LAST_FRAME_MS)
    }

    /// Guarda `text` como o escrito agora e diz de onde ele rola: (o de antes, 0 a 1 do caminho).
    fn track(&self, text: &str) -> (Option<String>, f32) {
        let mut state = self.state.borrow_mut();
        match state.as_mut() {
            None => {
                *state = Some(Shown { text: text.to_string(), before: None, at: Instant::now(), frozen: None });
                (None, 1.0)
            }
            Some(s) if s.text != text => {
                s.before = Some(std::mem::replace(&mut s.text, text.to_string()));
                s.at = Instant::now();
                s.frozen = None;
                (s.before.clone(), 0.0)
            }
            Some(Shown { before: Some(b), frozen: Some(t), .. }) => (Some(b.clone()), *t),
            Some(s) => match &s.before {
                Some(b) if ms_since(s.at) < self.ms => (Some(b.clone()), ease_out(ms_since(s.at) / self.ms)),
                _ => (None, 1.0),
            },
        }
    }

    /// O mesmo, como se tivesse mudado de `before` para `text` há `ms` milissegundos (para os
    /// testes).
    #[cfg(test)]
    pub fn rolled(before: &str, text: &str, ms: f32) -> Odometer {
        let o = Odometer::default();
        let at = Instant::now() - std::time::Duration::from_secs_f32(ms / 1000.0);
        *o.state.borrow_mut() = Some(Shown { text: text.to_string(), before: Some(before.to_string()), at, frozen: None });
        o
    }

    /// Rolando de `before` para `text`, parado a `t` (0 a 1) do caminho (para as imagens e os
    /// testes de desenho, que comparam quadros desenhados em momentos diferentes).
    #[cfg(test)]
    pub fn frozen(before: &str, text: &str, t: f32) -> Odometer {
        let o = Odometer::default();
        *o.state.borrow_mut() = Some(Shown { text: text.to_string(), before: Some(before.to_string()), at: Instant::now(), frozen: Some(t) });
        o
    }
}

/// Escreve o número `at` contando (seguido de `suffix`, como "%"): parado num inteiro, ou no
/// meio do caminho entre dois, com os dígitos que mudam rolando (43,6: o 3 a 60% do caminho para o
/// 4). Devolve a largura escrita.
#[allow(clippy::too_many_arguments)]
pub fn draw_counting(c: &mut Canvas, x: f32, y: f32, at: f64, suffix: &str, face: Face, size: f32, color: Rgba, anchor: Anchor) -> f32 {
    let (lo, t) = counting_step(at);
    let text = |n: i64| format!("{n}{suffix}");
    if t <= 0.0 {
        return c.text(x, y, &text(lo), face, size, color, anchor, None);
    }
    draw_rolling(c, x, y, &text(lo + 1), Some(&text(lo)), t, face, size, color, anchor, None)
}

/// O inteiro de baixo e quanto do caminho até o de cima (0 a 1) num número contando.
fn counting_step(at: f64) -> (i64, f32) {
    // (quase inteiro: parado, sem um fiapo do próximo dígito aparecendo)
    let near = at.round();
    if (at - near).abs() < 0.005 {
        return (near as i64, 0.0);
    }
    let lo = at.floor();
    (lo as i64, (at - lo) as f32)
}

fn ms_since(t: Instant) -> f32 {
    t.elapsed().as_secs_f32() * 1000.0
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Os dois textos têm o mesmo formato (os mesmos caracteres, com dígitos nos mesmos lugares)?
/// Só aí os dígitos rolam: "3 h 20 restantes" → "3 h 05 restantes" sim; "99" → "100" não.
fn same_shape(a: &str, b: &str) -> bool {
    let shape = |s: &str| s.chars().map(|c| if c.is_ascii_digit() { '0' } else { c }).collect::<String>();
    shape(a) == shape(b)
}

/// Escreve `text` como `Canvas::text`, mas com os dígitos que mudaram desde `before` rolando como
/// num hodômetro (`t`, de 0 a 1, do caminho): cada dígito é uma coluna de 0 a 9 que sobe quando
/// ele aumenta e desce quando diminui, passando pelos do meio, como no mockup. Sem `before`, ou
/// de outro formato, escreve parado. Devolve a largura escrita.
#[allow(clippy::too_many_arguments)]
pub fn draw_rolling(c: &mut Canvas, x: f32, y: f32, text: &str, before: Option<&str>, t: f32, face: Face, size: f32, color: Rgba, anchor: Anchor, max_w: Option<f32>) -> f32 {
    let total = c.text_width(text, face, size);
    let Some(before) = before.filter(|b| t < 1.0 && b != &text && same_shape(b, text) && max_w.is_none_or(|m| total <= m)) else {
        return c.text(x, y, text, face, size, color, anchor, max_w);
    };
    let (h, v) = anchor.parts();
    let x0 = match h {
        'm' => x - total / 2.0,
        'r' => x - total,
        _ => x,
    };
    // os dígitos que rolam são escritos pela linha de base; os parados, como o resto do texto
    let (base, left_anchor) = match v {
        'a' => (y + c.baseline(face, size), Anchor::LA),
        'm' => (y + size * 0.36, Anchor::LM),
        _ => (y, Anchor::LS),
    };
    // uma coluna com um dígito a cada `STEP` em, vista por uma janela em volta do dígito: o que
    // sai sobe (ou desce) esmaecendo e o que entra aparece do outro lado, sem pedaços duros de
    // dois números ao mesmo tempo no meio do caminho
    let step = size * STEP;
    // (dígitos não descem da linha de base: embaixo, a janela termina logo nela, senão o topo
    // do dígito seguinte aparece embaixo do número no fim do caminho)
    let (top, bottom) = (base - size * 0.9, base + size * 0.05);
    let digit_w = (0..10).map(|d| c.text_width(&d.to_string(), face, size)).fold(0.0, f32::max);
    let rising = digits_of(text) > digits_of(before);
    for ((i, ch), old) in text.char_indices().zip(before.chars()) {
        let left = x0 + c.text_width(&text[..i], face, size);
        let digits = if ch != old { ch.to_digit(10).zip(old.to_digit(10)) } else { None };
        let Some((to, from)) = digits else {
            c.text(left, y, ch.encode_utf8(&mut [0; 4]), face, size, color, left_anchor, None);
            continue;
        };
        let cx = left + c.text_width(ch.encode_utf8(&mut [0; 4]), face, size) / 2.0;
        let at = from as f32 + odometer_steps(from, to, rising) as f32 * t;
        c.push_round_clip(cx - digit_w / 2.0 - 1.0, top, cx + digit_w / 2.0 + 1.0, bottom, 0.0);
        let lo = at.floor() as i32;
        for d in [lo, lo + 1] {
            let off = d as f32 - at;
            let fade = (1.0 - off.abs()).clamp(0.0, 1.0).powf(1.6);
            if fade > 0.01 {
                let col = color.with_a((color.3 as f32 * fade).round() as u8);
                c.text(cx, base + off * step, &d.rem_euclid(10).to_string(), face, size, col, Anchor::MS, None);
            }
        }
        c.pop_clip();
    }
    total
}

/// Só os dígitos de um texto, para saber se o número subiu ou desceu.
fn digits_of(s: &str) -> String {
    s.chars().filter(char::is_ascii_digit).collect()
}

/// Quantas casas um dígito anda de `from` até `to`, como num hodômetro: todos para o mesmo lado
/// (para cima quando o número sobe, para baixo quando desce), dando a volta no 9/0. (Antes, de
/// 2 h 59 para 3 h 00 o 9 voltava até o 0 passando por todos, em vez de andar uma casa.)
fn odometer_steps(from: u32, to: u32, rising: bool) -> i32 {
    let up = (to as i32 - from as i32).rem_euclid(10);
    if up == 0 {
        0
    } else if rising {
        up
    } else {
        up - 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_texts_of_the_same_shape_roll() {
        assert!(same_shape("73", "68") && same_shape("3 h 20 restantes", "3 h 05 restantes"));
        assert!(!same_shape("99", "100") && !same_shape("45 min restantes", "1 h 05 restantes"));
    }

    /// Todos os dígitos andam para o mesmo lado, dando a volta no 9/0.
    #[test]
    fn digits_turn_like_an_odometer() {
        assert_eq!((odometer_steps(2, 3, true), odometer_steps(5, 0, true), odometer_steps(9, 0, true)), (1, 5, 1));
        assert_eq!((odometer_steps(3, 2, false), odometer_steps(0, 5, false), odometer_steps(0, 9, false)), (-1, -5, -1));
        assert_eq!(odometer_steps(4, 4, true), 0);
        assert!(digits_of("3 h 00 restantes") > digits_of("2 h 59 restantes"));
    }

    /// Contando: parado nos inteiros, rolando entre eles.
    #[test]
    fn counting_rolls_between_whole_numbers() {
        assert_eq!(counting_step(43.0), (43, 0.0));
        assert_eq!(counting_step(42.999), (43, 0.0));
        let (lo, t) = counting_step(43.6);
        assert_eq!(lo, 43);
        assert!((t - 0.6).abs() < 1e-4);
        assert_eq!(counting_step(7.25).0, 7);
    }

    /// O primeiro texto só aparece; um texto novo rola do anterior por `ms` e depois para.
    #[test]
    fn rolls_from_what_was_written() {
        let o = Odometer::new(200.0);
        assert_eq!(o.track("25°"), (None, 1.0));
        assert!(!o.animating());
        assert_eq!(o.track("25°"), (None, 1.0), "sem mudança, parado");
        assert_eq!(o.track("26°"), (Some("25°".into()), 0.0));
        assert!(o.animating());
        let (before, t) = o.track("26°");
        assert_eq!(before.as_deref(), Some("25°"));
        assert!((0.0..1.0).contains(&t));
        // mudou de novo no meio do caminho: rola do que estava escrito
        assert_eq!(o.track("27°").0.as_deref(), Some("26°"));
        o.state.borrow_mut().as_mut().unwrap().at -= std::time::Duration::from_millis(400);
        assert!(!o.animating());
        assert_eq!(o.track("27°"), (None, 1.0));
    }
}
