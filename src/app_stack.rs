//! Pilhas de widgets, como as do iPhone e do Mac: vários widgets no mesmo lugar, um por vez,
//! trocando com a roda do mouse (os pontinhos na borda direita dizem qual é qual). Para empilhar,
//! arraste um widget e solte no meio de outro; para tirar, clique direito → "Tirar da pilha".
//!
//! Cada widget continua sendo um cartão completo, com a janela dele: os de baixo ficam escondidos
//! no mesmo lugar e no mesmo tamanho do de cima, sem redesenhar, e continuam funcionando (o
//! Pomodoro conta, a música segue na ilha, os avisos saem). Na arrumação (encaixar, abrir espaço,
//! organizar), só o de cima conta.
//! (Módulo filho de `app`: mexe direto no estado do App.)

use serde_json::{json, Value};

use super::*;

/// Uma pilha: os widgets, na ordem da roda do mouse, e qual está em cima.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub keys: Vec<&'static str>,
    pub top: usize,
}

impl Stack {
    pub fn top_key(&self) -> &'static str {
        self.keys[self.top]
    }
}

/// As pilhas guardadas (cfg["app"]["pilhas"]): só widgets conhecidos, cada um numa pilha só, e
/// pelo menos dois em cada.
pub fn load(cfg: &Value) -> Vec<Stack> {
    let mut seen: Vec<&'static str> = Vec::new();
    let mut out = Vec::new();
    for s in cfg["app"]["pilhas"].as_array().into_iter().flatten() {
        let mut keys: Vec<&'static str> = Vec::new();
        for k in s["widgets"].as_array().into_iter().flatten() {
            if let Some(&key) = widgets::KEYS.iter().find(|w| Some(**w) == k.as_str()) {
                if !seen.contains(&key) && !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        if keys.len() >= 2 {
            seen.extend(&keys);
            let top = (s["atual"].as_u64().unwrap_or(0) as usize).min(keys.len() - 1);
            out.push(Stack { keys, top });
        }
    }
    out
}

/// O que guardar no config.
pub fn store(stacks: &[Stack]) -> Value {
    json!(stacks.iter().map(|s| json!({"widgets": s.keys, "atual": s.top})).collect::<Vec<_>>())
}

/// Soltando um widget com o centro dentro desta fração do outro (de cada borda para dentro), os
/// dois se empilham; mais perto da borda, encaixa ao lado, como sempre.
const DROP_INSET: f32 = 0.25;

/// O centro de `r` cai no meio de `t`?
pub fn drops_onto(r: Rect, t: Rect) -> bool {
    let (cx, cy) = ((r.0 + r.2) / 2, (r.1 + r.3) / 2);
    let (dx, dy) = (((t.2 - t.0) as f32 * DROP_INSET) as i32, ((t.3 - t.1) as f32 * DROP_INSET) as i32);
    cx > t.0 + dx && cx < t.2 - dx && cy > t.1 + dy && cy < t.3 - dy
}

/// Um lugar longe de tudo: o retângulo dos escondidos numa pilha, para não contarem na
/// arrumação.
pub const NOWHERE: Rect = (-1_000_000, -1_000_000, -999_999, -999_999);

/// O próximo na pilha de `n` widgets, a partir de `top`, rolando `dir` (1: o próximo, -1: o
/// anterior), dando a volta.
pub fn next_top(top: usize, n: usize, dir: i32) -> usize {
    (top as i32 + dir).rem_euclid(n as i32) as usize
}

/// passos da roda do mouse para trocar de widget (a roda de um touchpad manda muitos pequenos)
const WHEEL_STEP: f32 = 1.0;

impl App {
    /// A pilha do widget `key`, se ele estiver numa.
    pub(super) fn stack_of(&self, key: &str) -> Option<usize> {
        self.stacks.iter().position(|s| s.keys.contains(&key))
    }

    /// O retângulo do cartão `j` para a arrumação (os escondidos numa pilha não contam).
    pub(super) fn layout_rect(&self, j: usize) -> Rect {
        if self.cards[j].stacked { NOWHERE } else { self.cards[j].card_rect() }
    }

    fn save_stacks(&mut self) {
        if self.stacks.is_empty() {
            self.sh.cfg.obj_mut("app").remove("pilhas");
        } else {
            self.sh.cfg.obj_mut("app").insert("pilhas".into(), store(&self.stacks));
        }
        self.sh.save();
    }

    /// Ao abrir: as pilhas guardadas, com os de baixo escondidos embaixo do de cima.
    pub(super) fn restore_stacks(&mut self) {
        self.stacks = load(&self.sh.cfg);
        self.apply_stacks();
    }

    /// Pode mostrar a janela do cartão `i` agora? (Nem no modo foco, nem com o Tocando agora
    /// virado ilha.)
    fn may_show(&self, i: usize) -> bool {
        !self.sh.focus && !(self.island.is_some() && self.cards[i].key() == "midia")
    }

    /// Acerta os cartões pelas pilhas: o de cima aparece (com os pontinhos), os outros ficam
    /// escondidos no lugar e no tamanho dele. Uma pilha com menos de dois widgets abertos se
    /// desfaz.
    pub(super) fn apply_stacks(&mut self) {
        let open: Vec<&'static str> = self.cards.iter().map(|c| c.key()).collect();
        for s in &mut self.stacks {
            let top_key = s.keys[s.top.min(s.keys.len() - 1)];
            s.keys.retain(|k| open.contains(k));
            s.top = s.keys.iter().position(|k| *k == top_key).unwrap_or(0);
        }
        self.stacks.retain(|s| s.keys.len() >= 2);
        for i in 0..self.cards.len() {
            let key = self.cards[i].key();
            let (stacked, dots) = match self.stack_of(key) {
                Some(n) => {
                    let s = &self.stacks[n];
                    (s.top_key() != key, Some((s.top, s.keys.len())))
                }
                None => (false, None),
            };
            let was = self.cards[i].stacked;
            self.cards[i].stacked = stacked;
            self.cards[i].dots = dots;
            if stacked && !was {
                events::show(self.cards[i].hwnd, false);
            } else if !stacked && was {
                self.cards[i].refresh(&self.sh);
                self.cards[i].present(&self.sh, true);
                if self.may_show(i) {
                    events::show(self.cards[i].hwnd, true);
                    self.send_to_back(i);
                }
            }
        }
        self.sync_stacks();
        self.save_stacks();
    }

    /// Os de baixo de cada pilha acompanham o de cima: o mesmo lugar, a mesma escala e o mesmo
    /// tamanho (se o widget tiver esse tamanho).
    pub(super) fn sync_stacks(&mut self) {
        let mut dirty = false;
        for n in 0..self.stacks.len() {
            let Some(t) = self.find(self.stacks[n].top_key()) else { continue };
            if self.cards[t].animating() || self.cards[t].temp || self.cards[t].drag.is_some() {
                continue;
            }
            let pos = self.cards[t].slide.as_ref().map_or(self.cards[t].pos, |s| s.to);
            let (mode, s) = (self.cards[t].mode, self.cards[t].s);
            for k in 0..self.stacks[n].keys.len() {
                let Some(j) = self.find(self.stacks[n].keys[k]) else { continue };
                if j == t || self.cards[j].swap.is_some() {
                    continue;
                }
                let key = self.cards[j].key();
                let c = &mut self.cards[j];
                if (c.s - s).abs() > 0.01 {
                    c.set_scale(s);
                    c.stale = true;
                }
                if c.pos != pos {
                    c.pos = pos;
                    c.slide = None;
                    let w = self.sh.cfg.obj_mut(key);
                    w.insert("x".into(), json!(pos.0));
                    w.insert("y".into(), json!(pos.1));
                    dirty = true;
                }
                let c = &mut self.cards[j];
                if c.mode != mode && c.kind.sizes().contains(&mode) {
                    c.mode = mode;
                    c.stale = true;
                    widgets::store_size(self.sh.cfg.obj_mut(key), mode);
                    dirty = true;
                }
            }
        }
        if dirty {
            self.sh.save();
        }
    }

    /// As trocas que terminaram: o que saiu fica escondido de vez.
    pub(super) fn finish_swaps(&mut self) {
        for i in 0..self.cards.len() {
            if self.cards[i].swap_done() {
                self.cards[i].swap = None;
                if self.cards[i].stacked {
                    events::show(self.cards[i].hwnd, false);
                } else {
                    self.cards[i].present(&self.sh, false);
                }
            }
        }
    }

    /// A roda do mouse no widget `i`, em (lx, ly) pontos: numa pilha, troca de widget (menos onde
    /// o widget usa a roda). Devolve true se a roda foi para a pilha.
    pub(super) fn stack_wheel(&mut self, i: usize, steps: f32, lx: f32, ly: f32) -> bool {
        let key = self.cards[i].key();
        let Some(n) = self.stack_of(key) else { return false };
        if self.cards[i].temp || self.cards[i].kind.uses_wheel(lx, ly, self.cards[i].mode) {
            return false;
        }
        // uma troca por vez: a roda que continua girando durante a troca não pula outro
        if self.stack_wheel.1.elapsed().as_secs_f32() * 1000.0 < crate::card::SWAP_MS * 0.8 {
            self.stack_wheel.0 = 0.0;
            return true;
        }
        self.stack_wheel.0 += steps;
        if self.stack_wheel.0.abs() < WHEEL_STEP {
            return true;
        }
        // rolar para baixo (passos negativos) traz o próximo, como no iPhone
        let dir = if self.stack_wheel.0 < 0.0 { 1 } else { -1 };
        self.stack_wheel = (0.0, Instant::now());
        self.switch_stack(n, dir);
        true
    }

    /// Troca o widget de cima da pilha `n` pelo próximo (`dir` = 1) ou pelo anterior (-1).
    pub(super) fn switch_stack(&mut self, n: usize, dir: i32) {
        let len = self.stacks[n].keys.len();
        let old_key = self.stacks[n].top_key();
        self.stacks[n].top = next_top(self.stacks[n].top, len, dir);
        let top = self.stacks[n].top;
        let new_key = self.stacks[n].top_key();
        let (Some(a), Some(b)) = (self.find(old_key), self.find(new_key)) else { return };
        let animate = self.sh.animations;
        // o novo no lugar e no tamanho do que estava em cima
        let (pos, mode) = (self.cards[a].pos, self.cards[a].mode);
        self.cards[b].pos = pos;
        if self.cards[b].mode != mode && self.cards[b].kind.sizes().contains(&mode) {
            self.cards[b].mode = mode;
        }
        let now = Instant::now();
        let hover = self.cards[a].hover;
        for (j, entering) in [(a, false), (b, true)] {
            let c = &mut self.cards[j];
            c.stacked = !entering;
            c.dots = Some((top, len));
            c.swap = animate.then_some((now, entering));
            c.hot = None;
            c.pressing = false;
        }
        // o mouse está em cima: o novo já entra como se o mouse tivesse entrado nele
        self.cards[b].hover = hover;
        self.cards[b].set_hover(true);
        self.cards[a].set_hover(false);
        self.cards[b].refresh(&self.sh);
        self.cards[b].present(&self.sh, true);
        if self.may_show(b) {
            events::show(self.cards[b].hwnd, true);
        }
        // o novo por cima do que sai
        self.send_to_back(b);
        self.send_to_back(a);
        if !animate {
            events::show(self.cards[a].hwnd, false);
        }
        self.tooltip.hide();
        self.save_stacks();
    }

    /// Arrastando o widget `i` com o cartão em `pos`: o widget em que ele seria empilhado ao
    /// soltar (o centro dele no meio do outro, que tem o mesmo tamanho disponível).
    pub(super) fn stack_target(&self, i: usize, pos: (i32, i32)) -> Option<usize> {
        // uma pilha arrastada anda inteira (não se empilha em outra)
        if self.stack_of(self.cards[i].key()).is_some() {
            return None;
        }
        let rect = self.cards[i].card_rect_at(pos, None);
        (0..self.cards.len()).find(|&j| {
            let o = &self.cards[j];
            j != i && !o.stacked && !o.temp && !o.animating() && self.cards[i].kind.sizes().contains(&o.mode) && drops_onto(rect, o.card_rect())
        })
    }

    /// Marca (ou desmarca) o widget em que o arrastado seria empilhado.
    pub(super) fn show_drop_hint(&mut self, target: Option<usize>) {
        for j in 0..self.cards.len() {
            let on = Some(j) == target;
            if self.cards[j].drop_hint != on {
                self.cards[j].drop_hint = on;
                self.cards[j].present(&self.sh, false);
            }
        }
    }

    /// Solta o widget `i` em cima do `j`: os dois viram uma pilha (ou ele entra na pilha do
    /// outro), com o que chegou em cima, no lugar e no tamanho do outro.
    pub(super) fn stack_onto(&mut self, i: usize, j: usize) {
        let (ki, kj) = (self.cards[i].key(), self.cards[j].key());
        match self.stack_of(kj) {
            Some(n) => {
                self.stacks[n].keys.push(ki);
                self.stacks[n].top = self.stacks[n].keys.len() - 1;
            }
            None => self.stacks.push(Stack { keys: vec![kj, ki], top: 1 }),
        }
        // (o de baixo pode estar deslizando para abrir espaço: vale para onde ele vai)
        let pos = self.cards[j].slide.as_ref().map_or(self.cards[j].pos, |s| s.to);
        let mode = self.cards[j].mode;
        self.cards[i].slide = None;
        self.cards[i].pos = pos;
        if self.cards[i].mode != mode {
            self.cards[i].mode = mode;
            widgets::store_size(self.sh.cfg.obj_mut(ki), mode);
        }
        self.show_drop_hint(None);
        self.apply_stacks();
        self.cards[i].refresh(&self.sh);
        self.cards[i].present(&self.sh, true);
        self.moved(i);
        let n = self.stack_of(ki).map_or(2, |n| self.stacks[n].keys.len());
        let text = if n == 2 { format!("{} e {} empilhados", widgets::title(kj), widgets::title(ki)) } else { format!("{} entrou na pilha", widgets::title(ki)) };
        self.toast(&text, Some("Role a roda do mouse em cima para trocar"), "check", WHITE, None);
    }

    /// Tira o widget `i` da pilha dele: o próximo fica em cima, e ele vai para logo abaixo da
    /// pilha (os widgets encaixados embaixo descem para abrir espaço).
    pub(super) fn unstack(&mut self, i: usize) {
        let key = self.cards[i].key();
        let Some(n) = self.stack_of(key) else { return };
        let at = self.stacks[n].keys.iter().position(|k| *k == key).unwrap();
        self.stacks[n].keys.remove(at);
        let len = self.stacks[n].keys.len();
        self.stacks[n].top = self.stacks[n].top.min(len - 1);
        let next = self.stacks[n].top_key();
        self.cards[i].dots = None;
        self.cards[i].stacked = false;
        self.apply_stacks();
        let Some(t) = self.find(next) else { return };
        // quem está encaixado embaixo da pilha desce para dar lugar a ele
        let limit = ((WIDGET_GAP + 8.0) * self.cards[t].s) as i32;
        let rects: Vec<Rect> = (0..self.cards.len()).map(|j| if j == i { NOWHERE } else { self.layout_rect(j) }).collect();
        let below = stack_in(&rects, t, rects[t], limit);
        let shift = self.cards[i].card_size().1 + (WIDGET_GAP * self.cards[i].s).round() as i32;
        for j in below {
            let to = (self.cards[j].pos.0, self.cards[j].pos.1 + shift);
            self.cards[j].slide_to(to, ANIM_MS, &self.sh);
        }
        let to = self.cards[i].below(&self.cards[t]);
        self.cards[i].slide_to(to, ANIM_MS, &self.sh);
        self.cards[i].refresh(&self.sh);
        self.send_to_back(t);
        self.later_save(i);
        self.toast(&format!("{} saiu da pilha", widgets::title(key)), None, "check", WHITE, None);
    }

    /// Guarda a posição do widget `i` depois que ele terminar de deslizar.
    fn later_save(&mut self, i: usize) {
        if let Some(to) = self.cards[i].slide.as_ref().map(|s| s.to) {
            let key = self.cards[i].key();
            let w = self.sh.cfg.obj_mut(key);
            w.insert("x".into(), json!(to.0));
            w.insert("y".into(), json!(to.1));
            self.sh.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As pilhas guardadas voltam como eram, sem widgets desconhecidos, repetidos ou sozinhos.
    #[test]
    fn stacks_load_and_store() {
        let cfg = json!({"app": {"pilhas": [
            {"widgets": ["tempo", "sistema", "tempo", "nada"], "atual": 1},
            {"widgets": ["sistema", "midia"]},
            {"widgets": ["github"], "atual": 0},
            {"widgets": ["pomodoro", "bateria"], "atual": 9}
        ]}});
        let s = load(&cfg);
        assert_eq!(s, vec![Stack { keys: vec!["tempo", "sistema"], top: 1 }, Stack { keys: vec!["pomodoro", "bateria"], top: 1 }]);
        let back = json!({"app": {"pilhas": store(&s)}});
        assert_eq!(load(&back), s);
        assert!(load(&json!({"app": {}})).is_empty());
    }

    /// Solta no meio do outro: empilha; perto da borda: não (encaixa ao lado, como sempre).
    #[test]
    fn dropping_in_the_middle_stacks() {
        let t = (100, 100, 420, 254);
        assert!(drops_onto((110, 110, 430, 264), t), "quase em cima");
        assert!(!drops_onto((330, 100, 650, 254), t), "ao lado, encostando");
        assert!(!drops_onto((100, 230, 420, 384), t), "embaixo, encostando");
    }

    /// A roda dá a volta: depois do último vem o primeiro.
    #[test]
    fn the_wheel_goes_around() {
        assert_eq!(next_top(0, 3, 1), 1);
        assert_eq!(next_top(2, 3, 1), 0);
        assert_eq!(next_top(0, 3, -1), 2);
    }
}
