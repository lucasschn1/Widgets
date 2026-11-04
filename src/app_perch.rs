//! O Clawd na barra de tarefas: um clique nele e ele pula do widget do Uso do Claude para cima
//! da barra de tarefas, colado ao relógio. Lá, trabalha no notebook de lado, com um balão que diz
//! o que o Claude Code está fazendo agora. Outro clique (ou o clique direito, "Voltar para o
//! widget") e ele pula de volta. Fica por cima das janelas, como a ilha do Tocando agora, e some
//! em tela cheia.
//! (Módulo filho de `app`: mexe direto no estado do App.)

use std::time::{Duration, Instant};

use serde_json::json;

use super::*;
use crate::gfx::{Canvas, Image};
use crate::widgets::claude::{show_names, PERCH_CRAB, PERCH_H, PERCH_UNIT, PERCH_W};
use crate::widgets::clawd;
use crate::widgets::Env;

const KEY: &str = "claude";
/// o pulo entre o widget e a barra
const FLY_MS: f32 = 800.0;
/// agachado antes de subir e ao pousar (partes do pulo)
const CROUCH: f32 = 0.12;
const LAND: f32 = 0.9;
/// a altura do arco do pulo, em pontos (acima do mais alto dos dois lugares)
const ARC: f32 = 90.0;
/// a distância da borda direita da tela (o relógio fica logo abaixo)
const RIGHT_GAP: f32 = 10.0;
/// a janela do pulo: só o Clawd, com o centro do corpo em (`FLY_BOX.0 / 2`, `FLY_BOX.1 / 2`)
const FLY_BOX: (f32, f32) = (80.0, 60.0);

pub const M_PERCH_BACK: u32 = 300_050;
pub const M_PERCH_NAMES: u32 = 300_051;

/// O pulo: de onde e para onde vai o centro do corpo (pixels da tela), quando começou e se está
/// voltando para o widget.
struct Flight {
    from: (f32, f32),
    to: (f32, f32),
    start: Instant,
    back: bool,
}

impl Flight {
    fn progress(&self) -> f32 {
        (self.start.elapsed().as_secs_f32() * 1000.0 / FLY_MS).min(1.0)
    }
}

pub struct Perch {
    pub hwnd: HWND,
    s: f32,
    /// o canto de cima à esquerda da janela parada na barra, em pixels da tela
    home: (i32, i32),
    flight: Option<Flight>,
    hidden: bool,
    /// o desenho de agora (só redesenha quando muda)
    last: Option<u64>,
    pressed: bool,
}

/// Onde a janela fica na barra: no canto de baixo à direita da área de trabalho do monitor (em
/// cima da barra de tarefas, colada ao relógio; com a barra escondida ou de lado, no canto da
/// tela). Devolve (canto da janela, escala).
fn home_for(near: Rect) -> ((i32, i32), f32) {
    let work = win::area_for(near);
    let s = win::scale_at(work.2 - 20, work.3 - 20);
    let x = work.2 - ((PERCH_W + RIGHT_GAP) * s).round() as i32;
    let y = work.3 - (PERCH_H * s).round() as i32;
    ((x, y), s)
}

/// O centro do corpo do Clawd na janela parada em `home`, em pixels da tela.
fn crab_on_bar(home: (i32, i32), s: f32) -> (f32, f32) {
    (home.0 as f32 + PERCH_CRAB.0 * s, home.1 as f32 + PERCH_CRAB.1 * s)
}

/// Onde o Clawd está, a `p` do pulo de `from` para `to`: um arco que sobe acima dos dois
/// lugares; e se está agachado (antes de subir e ao pousar).
fn flight_at(from: (f32, f32), to: (f32, f32), p: f32, s: f32) -> ((f32, f32), bool) {
    if p < CROUCH {
        return (from, true);
    }
    if p >= LAND {
        return (to, true);
    }
    let e = ease_in_out((p - CROUCH) / (LAND - CROUCH));
    let x = from.0 + (to.0 - from.0) * e;
    // a altura: a linha reta entre os dois, mais uma parábola que sobe `ARC` acima do mais alto
    let line = from.1 + (to.1 - from.1) * e;
    let top = from.1.min(to.1) - ARC * s;
    let lift = (line - top) * 4.0 * e * (1.0 - e);
    ((x, line - lift), false)
}

impl Perch {
    /// A janela parada na barra: o balão e o Clawd.
    fn render(&self, kind: &dyn Kind, env: &Env) -> Image {
        let mut c = Canvas::new((PERCH_W * self.s).round() as u32, (PERCH_H * self.s).round() as u32, self.s);
        kind.draw_perch(&mut c, env);
        c.finish()
    }

    /// Um quadro do pulo: a janela pequena, só com o Clawd, no lugar do arco; devolve a imagem e
    /// o canto dela na tela.
    fn render_flight(&self, f: &Flight) -> (Image, (i32, i32)) {
        let ((x, y), crouch) = flight_at(f.from, f.to, f.progress(), self.s);
        let s = self.s;
        let mut c = Canvas::new((FLY_BOX.0 * s).round() as u32, (FLY_BOX.1 * s).round() as u32, s);
        clawd::draw(&mut c, FLY_BOX.0 / 2.0, FLY_BOX.1 / 2.0, PERCH_UNIT, &clawd::jump_pose(crouch));
        let pos = ((x - FLY_BOX.0 / 2.0 * s).round() as i32, (y - FLY_BOX.1 / 2.0 * s).round() as i32);
        (c.finish(), pos)
    }
}

impl App {
    pub(super) fn perch_owns(&self, hwnd: isize) -> bool {
        self.perch.as_ref().is_some_and(|p| p.hwnd.0 as isize == hwnd)
    }

    fn claude_index(&self) -> Option<usize> {
        self.find(KEY)
    }

    /// O centro do Clawd no widget agora, em pixels da tela.
    fn crab_on_card(&self, j: usize) -> Option<(f32, f32)> {
        let card = &self.cards[j];
        let (cx, cy) = card.kind.perch_from(card.mode)?;
        let r = card.card_rect();
        Some((r.0 as f32 + cx * card.s, r.1 as f32 + cy * card.s))
    }

    /// O Clawd vai para a barra de tarefas (`animate`: pulando do widget até lá).
    pub(super) fn open_perch(&mut self, animate: bool) {
        if self.perch.is_some() {
            return;
        }
        let Some(j) = self.claude_index() else { return };
        let Some(from) = self.crab_on_card(j) else { return };
        let (home, s) = home_for(self.cards[j].card_rect());
        let hwnd = events::create_layered(CARD_CLASS, false);
        let animate = animate && self.sh.animations;
        self.perch = Some(Perch {
            hwnd,
            s,
            home,
            flight: animate.then(|| Flight { from, to: crab_on_bar(home, s), start: Instant::now(), back: false }),
            hidden: false,
            last: None,
            pressed: false,
        });
        self.with_kind(j, |k, _| k.set_perched(true));
        self.cards[j].refresh(&self.sh);
        self.draw_perch_now();
        if let Some(p) = &self.perch {
            win::set_z(p.hwnd, Z::Topmost);
            events::show(p.hwnd, !self.sh.focus);
            events::set_cursor(p.hwnd, Cursor::Hand);
        }
        self.sh.cfg.obj_mut(KEY).insert("na_barra".into(), json!(true));
        self.sh.save();
    }

    /// O Clawd volta para o widget (`animate`: pulando de volta).
    pub(super) fn close_perch(&mut self, animate: bool) {
        let to = self.claude_index().and_then(|j| self.crab_on_card(j));
        let animations = self.sh.animations;
        let Some(p) = &mut self.perch else { return };
        if p.flight.as_ref().is_some_and(|f| f.back) {
            return;
        }
        match to {
            Some(to) if animate && animations && !p.hidden => {
                let from = match &p.flight {
                    // ainda indo: volta de onde está
                    Some(f) => flight_at(f.from, f.to, f.progress(), p.s).0,
                    None => crab_on_bar(p.home, p.s),
                };
                p.flight = Some(Flight { from, to, start: Instant::now(), back: true });
                p.pressed = false;
            }
            _ => self.finish_perch(),
        }
    }

    /// Pousou de volta no widget: a janela some e o Clawd reaparece no lugar dele.
    fn finish_perch(&mut self) {
        self.drop_perch();
        self.sh.cfg.obj_mut(KEY).insert("na_barra".into(), json!(false));
        self.sh.save();
        if let Some(j) = self.claude_index() {
            self.with_kind(j, |k, _| k.set_perched(false));
            self.cards[j].refresh(&self.sh);
        }
    }

    /// Some com a janela sem animação (o widget foi fechado, o app vai sair...).
    pub(super) fn drop_perch(&mut self) {
        if let Some(p) = self.perch.take() {
            events::destroy(p.hwnd);
        }
    }

    pub(super) fn perch_deadline(&self) -> Option<Instant> {
        let p = self.perch.as_ref()?;
        p.flight.as_ref().map(|_| Instant::now() + Duration::from_millis(FRAME_MS))
    }

    /// Desenha a janela agora (no pulo, o quadro do pulo; parada, só se o desenho mudou).
    fn draw_perch_now(&mut self) {
        let Some(j) = self.claude_index() else { return };
        let env = Env { cfg: &self.sh.cfg, key: KEY };
        let kind = self.cards[j].kind.as_ref();
        let Some(p) = &mut self.perch else { return };
        if p.hidden {
            return;
        }
        if let Some(f) = &p.flight {
            let (img, pos) = p.render_flight(f);
            win::blit(p.hwnd, &img, Some(pos), 255);
            p.last = None;
            return;
        }
        let key = kind.perch_key(&env);
        if p.last == Some(key) {
            return;
        }
        p.last = Some(key);
        let img = p.render(kind, &env);
        win::blit(p.hwnd, &img, Some(p.home), 255);
    }

    /// A cada volta do relógio do app: o pulo, esconder em tela cheia e redesenhar se mudou.
    pub(super) fn perch_tick(&mut self) {
        let Some(p) = &mut self.perch else { return };
        if let Some(f) = &p.flight {
            if f.progress() >= 1.0 {
                if f.back {
                    self.finish_perch();
                    return;
                }
                p.flight = None;
            }
        }
        // em tela cheia no monitor dela (ou no modo foco), some
        let (cx, cy) = crab_on_bar(p.home, p.s);
        let hidden = self.sh.focus || self.sh.paused.iter().any(|r| (cx as i32) >= r.0 && (cx as i32) < r.2 && (cy as i32) >= r.1 && (cy as i32) < r.3);
        if hidden != p.hidden {
            p.hidden = hidden;
            p.last = None;
            events::show(p.hwnd, !hidden);
            if !hidden {
                win::set_z(p.hwnd, Z::Topmost);
            }
        }
        self.draw_perch_now();
    }

    pub(super) fn perch_mouse(&mut self, kind: MouseKind, sx: i32, sy: i32) {
        let Some(p) = &mut self.perch else { return };
        match kind {
            MouseKind::Down => p.pressed = true,
            MouseKind::Up => {
                if std::mem::take(&mut p.pressed) && p.flight.is_none() {
                    self.close_perch(true);
                }
            }
            MouseKind::Right => {
                let items = vec![
                    MenuItem::Cmd("Voltar para o widget".into(), M_PERCH_BACK),
                    MenuItem::Sep,
                    MenuItem::Check("Mostrar nomes de arquivos no balão".into(), show_names(&self.sh.cfg), M_PERCH_NAMES),
                ];
                self.menu.popup(items, sx, sy, Some(KEY));
            }
            MouseKind::Leave => p.pressed = false,
            _ => {}
        }
    }

    /// Comandos do menu da janela da barra (clique direito).
    pub(super) fn perch_menu(&mut self, id: u32) -> bool {
        match id {
            M_PERCH_BACK => self.close_perch(true),
            M_PERCH_NAMES => {
                let on = show_names(&self.sh.cfg);
                self.sh.cfg.obj_mut(KEY).insert("balao_nomes".into(), json!(!on));
                self.sh.save();
                if let Some(p) = &mut self.perch {
                    p.last = None;
                }
                self.draw_perch_now();
            }
            _ => return false,
        }
        true
    }

    /// Depois de mudar a combinação de monitores (ou a barra de lugar): volta para o canto.
    pub(super) fn perch_screens_changed(&mut self) {
        let Some(j) = self.claude_index() else { return };
        let (home, s) = home_for(self.cards[j].card_rect());
        if let Some(p) = &mut self.perch {
            p.home = home;
            p.s = s;
            p.last = None;
        }
        self.draw_perch_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_jump_arcs_above_both_places_and_lands_on_the_bar() {
        let (from, to) = ((1500.0, 200.0), (1700.0, 1000.0));
        // agachado no lugar de antes e no de depois
        assert_eq!(flight_at(from, to, 0.05, 1.25), (from, true));
        assert_eq!(flight_at(from, to, 0.95, 1.25), (to, true));
        // no ar: sobe acima do mais alto dos dois e anda sempre para o mesmo lado
        let path: Vec<(f32, f32)> = (0..=40).map(|k| flight_at(from, to, CROUCH + (LAND - CROUCH) * k as f32 / 40.0, 1.25).0).collect();
        assert!(path.iter().any(|p| p.1 < from.1 - 50.0), "não sobe");
        assert!(path.windows(2).all(|w| w[1].0 >= w[0].0), "volta para trás");
        assert!((path[0].0 - from.0).abs() < 1.0 && (path[40].0 - to.0).abs() < 1.0, "não emenda com os pousos");
        // e de volta, o mesmo arco ao contrário
        assert!(flight_at(to, from, 0.5, 1.25).0 .1 < from.1);
    }

    #[test]
    fn sits_on_the_taskbar_next_to_the_clock() {
        // a janela fica dentro da área de trabalho, no canto de baixo à direita, com as patas do
        // Clawd na borda de baixo (em cima da barra)
        let near = (100, 100, 400, 300);
        let work = win::area_for(near);
        let (home, s) = home_for(near);
        assert!(home.0 >= work.0 && home.0 + (PERCH_W * s) as i32 <= work.2);
        assert_eq!(home.1 + (PERCH_H * s).round() as i32, work.3);
        assert!(work.2 - (home.0 + (PERCH_W * s) as i32) <= (RIGHT_GAP * s).ceil() as i32 + 1);
        let (_, cy) = crab_on_bar(home, s);
        assert!((cy + 5.0 * PERCH_UNIT * s - work.3 as f32).abs() <= 1.0, "o Clawd não está em cima da barra");
    }
}
