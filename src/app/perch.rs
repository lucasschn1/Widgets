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
use crate::widgets::claude::{self, show_names, PERCH_CRAB, PERCH_H, PERCH_UNIT, PERCH_W};
use crate::widgets::clawd;
use crate::widgets::Env;

const KEY: &str = "claude";
/// Quanto dura o pulo entre o widget e a barra: uma base mais um tanto por ponto de distância,
/// dentro de um mínimo e um máximo (com a duração fixa, o pulo do alto da tela até a barra
/// parecia um teletransporte, e um pulo curto se arrastava).
const FLY_BASE_MS: f32 = 480.0;
const FLY_MS_PER_PT: f32 = 0.3;
const FLY_MIN_MS: f32 = 620.0;
const FLY_MAX_MS: f32 = 1150.0;
/// Agachado antes de subir e ao pousar, em milissegundos (o mesmo em qualquer distância).
const CROUCH_MS: f32 = 110.0;
const LAND_MS: f32 = 120.0;
/// A altura do arco, em pontos acima do mais alto dos dois lugares: uma parte da distância,
/// dentro de um mínimo e um máximo (pulos longos sobem mais).
const ARC_PER_PT: f32 = 0.18;
const ARC_MIN: f32 = 44.0;
const ARC_MAX: f32 = 140.0;
/// a distância da borda direita da tela (o relógio fica logo abaixo)
const RIGHT_GAP: f32 = 10.0;
/// a janela do pulo: só o Clawd, com o centro do corpo em (`FLY_BOX.0 / 2`, `FLY_BOX.1 / 2`)
const FLY_BOX: (f32, f32) = (80.0, 60.0);

// (longe da faixa do modo foco: 300_050 era o "30 minutos" do modo foco)
pub const M_PERCH_BACK: u32 = 303_000;
pub const M_PERCH_NAMES: u32 = 303_001;

/// O pulo: de onde e para onde vai o centro do corpo (pixels da tela), o tamanho de um
/// quadradinho do Clawd em cada ponta (pixels da tela: no cartão e na barra podem ser
/// diferentes), quando começou, quanto dura, a altura do arco (pixels) e se está voltando para o
/// widget.
struct Flight {
    from: (f32, f32),
    to: (f32, f32),
    from_unit: f32,
    to_unit: f32,
    start: Instant,
    ms: f32,
    arc: f32,
    back: bool,
}

impl Flight {
    /// Um pulo de `from` a `to` numa tela de escala `s`, com o Clawd de `from_unit` a `to_unit`
    /// pixels por quadradinho; a duração e o arco vêm da distância.
    fn new(from: (f32, f32), to: (f32, f32), from_unit: f32, to_unit: f32, s: f32, back: bool) -> Flight {
        let distance = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt() / s.max(0.5);
        let ms = (FLY_BASE_MS + distance * FLY_MS_PER_PT).clamp(FLY_MIN_MS, FLY_MAX_MS);
        let arc = (distance * ARC_PER_PT).clamp(ARC_MIN, ARC_MAX) * s;
        Flight { from, to, from_unit, to_unit, start: Instant::now(), ms, arc, back }
    }

    /// Quanto do pulo já foi (0 a 1).
    fn progress(&self) -> f32 {
        (self.start.elapsed().as_secs_f32() * 1000.0 / self.ms).min(1.0)
    }

    /// Onde o Clawd está, a `progress` do pulo: agachado no lugar de antes, um arco que sobe
    /// acima dos dois lugares e agachado no de depois; e se está agachado.
    fn at(&self, progress: f32) -> ((f32, f32), bool) {
        let (crouch, land) = (CROUCH_MS / self.ms, 1.0 - LAND_MS / self.ms);
        if progress < crouch {
            return (self.from, true);
        }
        if progress >= land {
            return (self.to, true);
        }
        let along = ease_in_out((progress - crouch) / (land - crouch));
        let x = self.from.0 + (self.to.0 - self.from.0) * along;
        // a altura: a linha reta entre os dois, mais uma parábola que sobe `arc` acima do mais alto
        let line = self.from.1 + (self.to.1 - self.from.1) * along;
        let top = self.from.1.min(self.to.1) - self.arc;
        let lift = (line - top) * 4.0 * along * (1.0 - along);
        ((x, line - lift), false)
    }

    /// O tamanho de um quadradinho (pixels da tela) a `progress` do pulo: o do lugar de antes até
    /// o alto do arco, o do lugar de depois dali em diante (como na troca de tamanho do widget:
    /// em pixels inteiros, crescer aos poucos faria o desenho tremer).
    fn unit_at(&self, progress: f32) -> f32 {
        let (crouch, land) = (CROUCH_MS / self.ms, 1.0 - LAND_MS / self.ms);
        let along = ((progress - crouch) / (land - crouch)).clamp(0.0, 1.0);
        if along < 0.5 { self.from_unit } else { self.to_unit }
    }
}

/// A janela do Clawd na barra de tarefas.
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


impl Perch {
    /// A janela parada na barra: o balão e o Clawd.
    fn render(&self, kind: &dyn Kind, env: &Env) -> Image {
        let mut c = Canvas::new((PERCH_W * self.s).round() as u32, (PERCH_H * self.s).round() as u32, self.s);
        kind.draw_perch(&mut c, env);
        c.finish()
    }

    /// Um quadro do pulo: a janela pequena, só com o Clawd, no lugar do arco; devolve a imagem e
    /// o canto dela na tela.
    fn render_flight(&self, flight: &Flight, xmas: bool) -> (Image, (i32, i32)) {
        let progress = flight.progress();
        let ((x, y), crouch) = flight.at(progress);
        let s = self.s;
        let mut c = Canvas::new((FLY_BOX.0 * s).round() as u32, (FLY_BOX.1 * s).round() as u32, s);
        // o quadradinho em pontos desta janela (que está na escala da barra)
        let unit = flight.unit_at(progress) / s;
        clawd::draw(&mut c, FLY_BOX.0 / 2.0, FLY_BOX.1 / 2.0, unit, &clawd::jump_pose(crouch, xmas));
        let pos = ((x - FLY_BOX.0 / 2.0 * s).round() as i32, (y - FLY_BOX.1 / 2.0 * s).round() as i32);
        (c.finish(), pos)
    }
}

impl App {
    /// A janela `hwnd` é a do Clawd na barra?
    pub(super) fn perch_owns(&self, hwnd: isize) -> bool {
        self.perch.as_ref().is_some_and(|perch| perch.hwnd.0 as isize == hwnd)
    }

    /// O cartão do Uso do Claude.
    fn claude_index(&self) -> Option<usize> {
        self.find(KEY)
    }

    /// O centro do Clawd no widget agora e o tamanho de um quadradinho dele, em pixels da tela.
    fn crab_on_card(&self, j: usize) -> Option<((f32, f32), f32)> {
        let card = &self.cards[j];
        let (cx, cy, unit) = card.kind.perch_from(card.mode)?;
        let rect = card.card_rect();
        Some(((rect.0 as f32 + cx * card.s, rect.1 as f32 + cy * card.s), unit * card.s))
    }

    /// O Clawd vai para a barra de tarefas (`animate`: pulando do widget até lá).
    pub(super) fn open_perch(&mut self, animate: bool) {
        if self.perch.is_some() {
            return;
        }
        let Some(j) = self.claude_index() else { return };
        let Some((from, from_unit)) = self.crab_on_card(j) else { return };
        let (home, s) = home_for(self.cards[j].card_rect());
        let hwnd = events::create_layered(CARD_CLASS, false);
        let animate = animate && self.sh.animations;
        self.perch = Some(Perch {
            hwnd,
            s,
            home,
            flight: animate.then(|| Flight::new(from, crab_on_bar(home, s), from_unit, PERCH_UNIT * s, s, false)),
            hidden: false,
            last: None,
            pressed: false,
        });
        self.with_kind(j, |kind, _| kind.set_perched(true));
        self.cards[j].refresh(&self.sh);
        self.draw_perch_now();
        if let Some(perch) = &self.perch {
            win::set_z(perch.hwnd, Z::Topmost);
            events::show(perch.hwnd, !self.sh.focus);
            events::set_cursor(perch.hwnd, Cursor::Hand);
        }
        self.sh.cfg.obj_mut(KEY).insert("na_barra".into(), json!(true));
        self.sh.save();
    }

    /// O Clawd volta para o widget (`animate`: pulando de volta).
    pub(super) fn close_perch(&mut self, animate: bool) {
        let to = self.claude_index().and_then(|j| self.crab_on_card(j));
        let animations = self.sh.animations;
        let Some(perch) = &mut self.perch else { return };
        if perch.flight.as_ref().is_some_and(|flight| flight.back) {
            return;
        }
        match to {
            Some((to, to_unit)) if animate && animations && !perch.hidden => {
                let (from, from_unit) = match &perch.flight {
                    // ainda indo: volta de onde está, do tamanho que está
                    Some(flight) => (flight.at(flight.progress()).0, flight.unit_at(flight.progress())),
                    None => (crab_on_bar(perch.home, perch.s), PERCH_UNIT * perch.s),
                };
                perch.flight = Some(Flight::new(from, to, from_unit, to_unit, perch.s, true));
                perch.pressed = false;
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
            self.with_kind(j, |kind, _| kind.set_perched(false));
            self.cards[j].refresh(&self.sh);
        }
    }

    /// Some com a janela sem animação (o widget foi fechado, o app vai sair...).
    pub(super) fn drop_perch(&mut self) {
        if let Some(perch) = self.perch.take() {
            events::destroy(perch.hwnd);
        }
    }

    /// Quando o próximo quadro do pulo precisa vir.
    pub(super) fn perch_deadline(&self) -> Option<Instant> {
        let perch = self.perch.as_ref()?;
        perch.flight.as_ref().map(|_| Instant::now() + Duration::from_millis(FRAME_MS))
    }

    /// Desenha a janela agora (no pulo, o quadro do pulo; parada, só se o desenho mudou).
    fn draw_perch_now(&mut self) {
        let Some(j) = self.claude_index() else { return };
        let env = Env { cfg: &self.sh.cfg, key: KEY };
        let kind = self.cards[j].kind.as_ref();
        let Some(perch) = &mut self.perch else { return };
        if perch.hidden {
            return;
        }
        if let Some(flight) = &perch.flight {
            let (img, pos) = perch.render_flight(flight, claude::xmas(&self.sh.cfg));
            win::blit(perch.hwnd, &img, Some(pos), 255);
            perch.last = None;
            return;
        }
        let key = kind.perch_key(&env);
        if perch.last == Some(key) {
            return;
        }
        perch.last = Some(key);
        let img = perch.render(kind, &env);
        win::blit(perch.hwnd, &img, Some(perch.home), 255);
    }

    /// A cada volta do relógio do app: o pulo, esconder em tela cheia e redesenhar se mudou.
    pub(super) fn perch_tick(&mut self) {
        let Some(perch) = &mut self.perch else { return };
        if let Some(flight) = &perch.flight {
            if flight.progress() >= 1.0 {
                if flight.back {
                    self.finish_perch();
                    return;
                }
                perch.flight = None;
            }
        }
        // em tela cheia no monitor dela (ou no modo foco), some
        let (cx, cy) = crab_on_bar(perch.home, perch.s);
        let hidden = self.sh.focus || self.sh.paused.iter().any(|screen| (cx as i32) >= screen.0 && (cx as i32) < screen.2 && (cy as i32) >= screen.1 && (cy as i32) < screen.3);
        if hidden != perch.hidden {
            perch.hidden = hidden;
            perch.last = None;
            events::show(perch.hwnd, !hidden);
            if !hidden {
                win::set_z(perch.hwnd, Z::Topmost);
            }
        }
        self.draw_perch_now();
    }

    /// O mouse na janela da barra: um clique leva o Clawd de volta; o direito abre o menu.
    pub(super) fn perch_mouse(&mut self, kind: MouseKind, sx: i32, sy: i32) {
        let Some(perch) = &mut self.perch else { return };
        match kind {
            MouseKind::Down => perch.pressed = true,
            MouseKind::Up => {
                if std::mem::take(&mut perch.pressed) && perch.flight.is_none() {
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
            MouseKind::Leave => perch.pressed = false,
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
                if let Some(perch) = &mut self.perch {
                    perch.last = None;
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
        if let Some(perch) = &mut self.perch {
            perch.home = home;
            perch.s = s;
            perch.last = None;
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
        let flight = Flight::new(from, to, 3.0, 3.0, 1.25, false);
        let (crouch, land) = (CROUCH_MS / flight.ms, 1.0 - LAND_MS / flight.ms);
        // agachado no lugar de antes e no de depois
        assert_eq!(flight.at(crouch / 2.0), (from, true));
        assert_eq!(flight.at((land + 1.0) / 2.0), (to, true));
        // no ar: sobe acima do mais alto dos dois e anda sempre para o mesmo lado
        let path: Vec<(f32, f32)> = (0..=40).map(|k| flight.at(crouch + (land - crouch) * k as f32 / 40.0).0).collect();
        assert!(path.iter().any(|p| p.1 < from.1 - 50.0), "não sobe");
        assert!(path.windows(2).all(|w| w[1].0 >= w[0].0), "volta para trás");
        assert!((path[0].0 - from.0).abs() < 1.0 && (path[40].0 - to.0).abs() < 1.0, "não emenda com os pousos");
        // e de volta, o mesmo arco ao contrário
        assert!(Flight::new(to, from, 3.0, 3.0, 1.25, true).at(0.5).0 .1 < from.1);
    }

    /// Pulos longos duram mais e sobem mais que os curtos (dentro dos limites), e o agachar e o
    /// pousar levam o mesmo tempo em qualquer distância.
    #[test]
    fn longer_jumps_take_longer_and_arc_higher() {
        let short = Flight::new((1700.0, 900.0), (1800.0, 1000.0), 3.0, 3.0, 1.25, false);
        let long = Flight::new((100.0, 100.0), (1800.0, 1000.0), 3.0, 3.0, 1.25, false);
        assert!(long.ms > short.ms && long.arc > short.arc, "{} {} / {} {}", short.ms, short.arc, long.ms, long.arc);
        assert!(short.ms >= FLY_MIN_MS && long.ms <= FLY_MAX_MS);
        assert!(short.arc >= ARC_MIN * 1.25 - 0.01 && long.arc <= ARC_MAX * 1.25 + 0.01);
        // o agachar: o mesmo tempo, então uma parte menor do pulo longo
        let still = |flight: &Flight, ms: f32| flight.at(ms / flight.ms).1;
        assert!(still(&short, CROUCH_MS - 10.0) && still(&long, CROUCH_MS - 10.0));
        assert!(!still(&short, CROUCH_MS + 40.0) && !still(&long, CROUCH_MS + 40.0));
    }

    /// O Clawd sai do tamanho que tem no cartão e chega do tamanho da barra, trocando no alto do
    /// arco (de uma vez: em pixels inteiros, crescer aos poucos faria o desenho tremer).
    #[test]
    fn the_size_changes_at_the_top_of_the_jump() {
        let flight = Flight::new((100.0, 500.0), (1800.0, 1000.0), 4.0, 3.0, 1.25, false);
        assert_eq!(flight.unit_at(0.0), 4.0, "sai do tamanho do cartão");
        assert_eq!(flight.unit_at(0.3), 4.0);
        assert_eq!(flight.unit_at(0.7), 3.0);
        assert_eq!(flight.unit_at(1.0), 3.0, "chega do tamanho da barra");
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
