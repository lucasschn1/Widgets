//! Quando os widgets aparecem de novo (ao abrir o app, ao sair do modo foco e quando o jogo ou o
//! vídeo em tela cheia de um monitor fecha), eles entram um depois do outro, na ordem de leitura
//! (veja `entrance`).
//! (Módulo filho de `app`: mexe direto no estado do App.)

use super::*;
use crate::card::entrance;

impl App {
    /// Os cartões `which` entram um depois do outro, na ordem de leitura (os escondidos numa
    /// pilha, ou no lugar do Tocando agora virado ilha, não contam).
    pub(super) fn play_entrances(&mut self, which: impl Fn(&Card) -> bool) {
        let chosen: Vec<usize> = (0..self.cards.len()).filter(|&i| !self.cards[i].stacked && self.may_show(i) && which(&self.cards[i])).collect();
        let rects: Vec<win::Rect> = chosen.iter().map(|&i| self.cards[i].card_rect()).collect();
        for (&i, delay) in chosen.iter().zip(entrance::delays(&rects)) {
            self.cards[i].enter(delay, &self.sh);
            self.cards[i].present(&self.sh, false);
        }
    }

    /// Os monitores que estavam pausados (com algo em tela cheia) e voltaram: os widgets deles
    /// entram de novo.
    pub(super) fn enter_unpaused(&mut self, was: &[win::Rect]) {
        let back: Vec<win::Rect> = was.iter().filter(|screen| !self.sh.paused.contains(screen)).copied().collect();
        if back.is_empty() {
            return;
        }
        self.play_entrances(|card| {
            let (x0, y0, x1, y1) = card.card_rect();
            let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
            back.iter().any(|screen| cx >= screen.0 && cx < screen.2 && cy >= screen.1 && cy < screen.3)
        });
    }
}
