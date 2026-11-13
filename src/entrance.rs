//! A entrada dos widgets: ao abrir o app, ao sair do modo foco e quando um jogo (ou vídeo) em
//! tela cheia fecha, cada widget aparece subindo um pouco, crescendo de leve e clareando, um logo
//! depois do outro, na ordem de leitura (de cima para baixo, da esquerda para a direita).

use std::time::{Duration, Instant};

use crate::theme::{ease_settle, ease_spring};
use crate::win::Rect;

/// Quanto dura a entrada de um widget (ms).
pub const ENTRANCE_MS: f32 = 560.0;
/// Quanto depois do anterior cada widget começa a entrar (ms).
pub const STAGGER_MS: f32 = 70.0;
/// Quanto menor o widget começa (fração do tamanho).
const SHRINK: f32 = 0.05;
/// De quantos pontos abaixo ele sobe (cabe na margem transparente em volta do cartão).
const LIFT: f32 = 6.0;
/// A parte da entrada em que ele clareia (o resto é a mola assentando).
const FADE_PART: f32 = 0.55;

/// Como o widget aparece num momento da entrada: o tamanho (1: normal), a opacidade (0 a 1) e
/// quantos pontos abaixo do lugar ele está.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    pub scale: f32,
    pub alpha: f32,
    pub lift: f32,
}

impl Look {
    /// Parado no lugar.
    pub const STILL: Look = Look { scale: 1.0, alpha: 1.0, lift: 0.0 };
}

/// A entrada de um widget: quando ela começa (pode ser daqui a pouco, esperando a vez dele).
#[derive(Clone, Copy, Debug)]
pub struct Entrance {
    start: Instant,
}

impl Entrance {
    /// Uma entrada que começa daqui a `delay_ms` milissegundos.
    pub fn after(delay_ms: f32) -> Entrance {
        Entrance { start: Instant::now() + Duration::from_secs_f32(delay_ms.max(0.0) / 1000.0) }
    }

    /// Quanto da entrada já foi (0 a 1; 0 enquanto espera a vez).
    fn progress(&self) -> f32 {
        let ms = Instant::now().saturating_duration_since(self.start).as_secs_f32() * 1000.0;
        (ms / ENTRANCE_MS).clamp(0.0, 1.0)
    }

    /// Uma entrada que já terminou (para os testes).
    #[cfg(test)]
    pub fn finished() -> Entrance {
        Entrance { start: Instant::now() - Duration::from_secs(5) }
    }

    pub fn done(&self) -> bool {
        self.progress() >= 1.0
    }

    /// Como o widget aparece agora.
    pub fn look(&self) -> Look {
        look_at(self.progress())
    }
}

/// Como o widget aparece a `t` (0 a 1) da entrada: clareia logo no começo, sobe assentando suave
/// e cresce com uma mola leve.
pub fn look_at(t: f32) -> Look {
    if t >= 1.0 {
        return Look::STILL;
    }
    let fade = (t / FADE_PART).clamp(0.0, 1.0);
    Look { scale: 1.0 - SHRINK * (1.0 - ease_spring(t)), alpha: fade * fade * (3.0 - 2.0 * fade), lift: LIFT * (1.0 - ease_settle(t)) }
}

/// A espera de cada widget (ms), pela ordem de leitura dos cartões: de cima para baixo e, na
/// mesma altura (mais ou menos meio cartão), da esquerda para a direita.
pub fn delays(rects: &[Rect]) -> Vec<f32> {
    let mut order: Vec<usize> = (0..rects.len()).collect();
    let row = |r: &Rect| r.1 / ((r.3 - r.1).max(1) / 2).max(1);
    order.sort_by_key(|&i| (row(&rects[i]), rects[i].0));
    let mut out = vec![0.0; rects.len()];
    for (rank, i) in order.into_iter().enumerate() {
        out[i] = rank as f32 * STAGGER_MS;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Começa invisível, um pouco menor e mais embaixo; termina exatamente parado no lugar.
    #[test]
    fn starts_hidden_and_ends_in_place() {
        let start = look_at(0.0);
        assert_eq!(start.alpha, 0.0);
        assert!(start.scale < 1.0 && start.lift > 0.0);
        assert_eq!(look_at(1.0), Look::STILL);
        // clareia antes de terminar de assentar, sem voltar a escurecer
        let mut last = 0.0;
        for k in 0..=100 {
            let l = look_at(k as f32 / 100.0);
            assert!(l.alpha >= last, "escureceu em {k}");
            assert!((0.0..=LIFT).contains(&l.lift) && l.scale > 0.9 && l.scale < 1.02);
            last = l.alpha;
        }
        assert_eq!(look_at(FADE_PART).alpha, 1.0);
    }

    /// Quem espera a vez fica invisível até ela chegar.
    #[test]
    fn waits_its_turn() {
        let e = Entrance::after(10_000.0);
        assert_eq!(e.look().alpha, 0.0);
        assert!(!e.done());
        let e = Entrance { start: Instant::now() - Duration::from_secs(5) };
        assert!(e.done());
        assert_eq!(e.look(), Look::STILL);
    }

    /// A ordem de leitura: de cima para baixo e, na mesma linha, da esquerda para a direita.
    #[test]
    fn enters_in_reading_order() {
        // duas colunas: a da direita com dois cartões, a da esquerda com um no meio da altura
        let rects = [(1200, 100, 1520, 254), (1200, 270, 1520, 424), (860, 100, 1180, 254), (860, 400, 1180, 554)];
        let d = delays(&rects);
        assert_eq!(d[2], 0.0, "o de cima à esquerda primeiro");
        assert_eq!(d[0], STAGGER_MS, "depois o do lado");
        assert!(d[1] < d[3], "e descendo");
        let mut sorted = d.clone();
        sorted.sort_by(f32::total_cmp);
        assert_eq!(sorted, (0..4).map(|k| k as f32 * STAGGER_MS).collect::<Vec<_>>());
    }
}
