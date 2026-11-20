//! O fim do expediente na ilha: na última hora (de segunda a sexta), uma contagem que vai ficando
//! dourada; no horário, um "bom descanso" por alguns segundos. O horário fica em Configurações →
//! Geral (padrão 17:00).

use serde_json::Value;

use super::{draw_buttons, hit, Clock, OK};
use crate::config::Get;
use crate::gfx::{Anchor, Canvas, Face};
use crate::theme::{icon, Rgba, FG, MUTED};
use crate::widgets::island_ui as iu;
use crate::widgets::{alert_hash, Ctx, Env, IslandForm, IslandItem, Kind, Mode, Rect4};

/// O horário padrão do fim do expediente.
pub const DEFAULT_END: &str = "17:00";
/// Os horários que dá para escolher nas Configurações.
pub const END_CHOICES: [&str; 5] = ["16:00", "17:00", "18:00", "19:00", "20:00"];
const GOLD: Rgba = Rgba::rgb(255, 199, 64);
/// a contagem começa este tanto antes
const LEAD: f64 = 3600.0;
/// nos últimos minutos, a contagem passa na frente da música
const LAST: f64 = 10.0 * 60.0;
/// o "bom descanso" fica este tanto na ilha
const DONE_FOR: f64 = 12.0;

pub struct Workday {
    clock: Clock,
    ok: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    /// faltam tantos segundos
    Left(f64),
    Done,
}

/// O horário do fim (segundos desde a meia-noite) gravado nas opções.
pub fn end_of(cfg: &Value) -> f64 {
    let text = cfg["expediente"].str_of("fim").unwrap_or(DEFAULT_END);
    let (h, m) = text.split_once(':').unwrap_or(("17", "00"));
    let (h, m) = (h.trim().parse::<f64>().unwrap_or(17.0), m.trim().parse::<f64>().unwrap_or(0.0));
    (h.clamp(0.0, 23.0) * 60.0 + m.clamp(0.0, 59.0)) * 60.0
}

impl Workday {
    pub fn new() -> Workday {
        Workday { clock: Clock::default(), ok: Vec::new() }
    }

    /// Uma segunda-feira, `from_end` segundos do fim do expediente (negativo: antes).
    #[cfg(test)]
    pub fn sample(from_end: f64) -> Workday {
        let monday = crate::util::days_from_civil(2026, 11, 23) as f64 * 86400.0;
        Workday { clock: Clock::at(monday + 17.0 * 3600.0 + 3.0 * 3600.0 + from_end, -3.0 * 3600.0), ..Workday::new() }
    }

    /// Onde estamos agora (None: fora da última hora, ou fim de semana) e o horário do fim.
    fn phase(&self, cfg: &Value) -> Option<(Phase, f64, String)> {
        let now = self.clock.now();
        if self.clock.weekday(now) >= 5 {
            return None;
        }
        let day = self.clock.day(now);
        let end = self.clock.midnight(day) + end_of(cfg);
        let key = format!("expediente|{day}");
        if self.ok.contains(&key) {
            return None;
        }
        let phase = if now >= end - LEAD && now < end {
            Phase::Left(end - now)
        } else if now >= end && now < end + DONE_FOR {
            Phase::Done
        } else {
            return None;
        };
        Some((phase, end, key))
    }
}

/// "47 min", "1 h"
fn left_text(secs: f64) -> String {
    let m = (secs / 60.0).ceil() as i64;
    if m >= 60 { "1 h".into() } else { format!("{m} min") }
}

/// O dourado vai chegando com o fim (0 = uma hora antes, 1 = no fim).
fn warmth(secs: f64) -> f32 {
    (1.0 - secs / LEAD).clamp(0.0, 1.0) as f32
}

impl Kind for Workday {
    fn key(&self) -> &'static str {
        "expediente"
    }
    fn title(&self) -> &'static str {
        super::title("expediente")
    }
    fn draw(&self, _c: &mut Canvas, _mode: Mode, _env: &Env) {}

    fn island_item(&self, env: &Env) -> Option<IslandItem> {
        let (phase, _, key) = self.phase(env.cfg)?;
        Some(match phase {
            // a contagem fica atrás da música e do Pomodoro, menos nos últimos minutos
            Phase::Left(s) => IslandItem { idle: s > LAST, ..IslandItem::live(MUTED.mix(GOLD, warmth(s))) },
            Phase::Done => IslandItem::alert(GOLD, alert_hash(&key)),
        })
    }

    fn island_draw(&self, c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, env: &Env) {
        let Some((phase, end, _)) = self.phase(env.cfg) else { return };
        let (x, y, w, h) = r;
        let cy = y + h / 2.0;
        let end_text = self.clock.hhmm(end);
        let (color, frac) = match phase {
            Phase::Left(s) => (MUTED.mix(GOLD, warmth(s)), warmth(s)),
            Phase::Done => (GOLD, 1.0),
        };
        let ring = |c: &mut Canvas, cx: f32, cy: f32, rr: f32, width: f32| {
            c.arc(cx, cy, rr, 0.0, 359.9, Rgba(255, 255, 255, 30), width);
            if frac > 0.002 {
                c.arc(cx, cy, rr, -90.0, -90.0 + 360.0 * frac, color, width);
            }
        };
        let glyph = |c: &mut Canvas, cx: f32, cy: f32, size: f32| {
            let g = if phase == Phase::Done { icon("check") } else { icon("pomodoro") };
            c.text(cx, cy, g, Face::Icons, size, color, Anchor::MM, None);
        };
        match form {
            IslandForm::Bubble => {
                ring(c, x + w / 2.0, cy, 12.0, 3.0);
                glyph(c, x + w / 2.0, cy, 10.0);
            }
            IslandForm::Compact | IslandForm::Banner => {
                ring(c, x + 26.0, cy, 11.0, 2.6);
                glyph(c, x + 26.0, cy, 9.5);
                match phase {
                    Phase::Left(s) => iu::compact_text(c, x + 46.0, r, if form == IslandForm::Banner { "Fim do expediente em" } else { "Expediente" }, &left_text(s), color),
                    Phase::Done if form == IslandForm::Banner => iu::compact_text(c, x + 46.0, r, "Bom descanso! O expediente acabou", "", GOLD),
                    Phase::Done => iu::compact_text(c, x + 46.0, r, "Bom descanso!", &end_text, GOLD),
                }
            }
            IslandForm::Medium => {
                ring(c, x + 39.0, cy, 19.0, 4.0);
                glyph(c, x + 39.0, cy, 15.0);
                let (title, sub) = match phase {
                    Phase::Left(s) => (format!("Fim do expediente às {end_text}"), format!("Faltam {}", left_text(s))),
                    Phase::Done => ("Bom descanso!".to_string(), format!("O expediente acabou às {end_text}")),
                };
                iu::two_lines(c, x + 74.0, cy, w - 74.0 - 86.0, &title, &sub, color);
                draw_buttons(c, form, r, hot, None);
            }
            IslandForm::Large => {
                let verb = if phase == Phase::Done { "terminou" } else { "termina" };
                iu::header(c, r, "EXPEDIENTE", color, &format!("{verb} às {end_text}"), MUTED);
                match phase {
                    Phase::Left(s) => {
                        let secs = s.ceil() as i64;
                        c.text(x + 22.0, y + 78.0, &format!("{}:{:02}", secs / 60, secs % 60), Face::Semibold, 44.0, FG, Anchor::LM, None);
                        c.text(x + 22.0, y + 112.0, if s <= LAST { "Reta final. Hora de fechar as abas." } else { "Falta pouco para o fim do dia." }, Face::Regular, 12.0, MUTED, Anchor::LM, Some(w - 44.0));
                    }
                    Phase::Done => {
                        c.text(x + 22.0, y + 78.0, "Bom descanso!", Face::Semibold, 30.0, GOLD, Anchor::LM, Some(w - 44.0));
                        c.text(x + 22.0, y + 112.0, "O expediente acabou. Até amanhã.", Face::Regular, 12.0, MUTED, Anchor::LM, Some(w - 44.0));
                    }
                }
                iu::bar(c, x + 22.0, x + w - 22.0, y + 140.0, frac, color);
                draw_buttons(c, form, r, hot, None);
            }
        }
    }

    fn island_hit(&self, form: IslandForm, x: f32, y: f32, w: f32, h: f32, _env: &Env) -> Option<u32> {
        hit(form, x, y, w, h, false)
    }

    /// Aberta, os segundos correm.
    fn island_fps(&self, form: IslandForm) -> u32 {
        if form == IslandForm::Large { 2 } else { 0 }
    }

    /// OK: some até amanhã.
    fn act(&mut self, action: u32, ctx: &mut Ctx) {
        if action == OK {
            if let Some((_, _, key)) = self.phase(ctx.cfg) {
                self.ok.push(key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Na última hora antes das 17:00, de segunda a sexta: a contagem (atrás da música até os
    /// últimos 10 minutos); no horário, o "bom descanso" por alguns segundos; depois, nada. Fim
    /// de semana, nada; outro horário escolhido, a contagem acompanha.
    #[test]
    fn counts_down_the_last_hour() {
        let cfg = json!({"app": {}});
        let env = Env { cfg: &cfg, key: "expediente" };
        for (from_end, shows) in [(-61.0 * 60.0, false), (-59.0 * 60.0, true), (-1.0, true), (2.0, true), (DONE_FOR + 1.0, false)] {
            assert_eq!(Workday::sample(from_end).island_item(&env).is_some(), shows, "{from_end}");
        }
        let early = Workday::sample(-40.0 * 60.0).island_item(&env).unwrap();
        assert!(early.idle && !early.alert);
        let late = Workday::sample(-5.0 * 60.0).island_item(&env).unwrap();
        assert!(!late.idle, "nos últimos minutos, na frente");
        assert!(Workday::sample(3.0).island_item(&env).unwrap().alert, "bom descanso: um aviso");
        // sábado
        let mut sat = Workday::sample(-10.0 * 60.0);
        sat.clock = Clock::at(sat.clock.now() + 5.0 * 86400.0, -3.0 * 3600.0);
        assert!(sat.island_item(&env).is_none());
        // com o fim às 18:00, a última hora vai das 17:00 às 18:00
        let cfg18 = json!({"expediente": {"fim": "18:00"}, "app": {}});
        let env18 = Env { cfg: &cfg18, key: "expediente" };
        assert!(Workday::sample(-10.0 * 60.0).island_item(&env18).is_none(), "16:50 com fim às 18:00: ainda não");
        assert!(Workday::sample(30.0 * 60.0).island_item(&env18).is_some(), "17:30 com fim às 18:00");
        assert_eq!(end_of(&cfg18), 18.0 * 3600.0);
        assert_eq!(end_of(&cfg), 17.0 * 3600.0);
    }

    #[test]
    fn ok_hides_until_tomorrow() {
        let cfg = json!({"app": {}});
        let mut w = Workday::sample(-30.0 * 60.0);
        let (mut cfg2, mut cache, mut out) = (cfg.clone(), json!({}), Vec::new());
        w.act(OK, &mut Ctx { cfg: &mut cfg2, cache: &mut cache, key: "expediente", out: &mut out });
        assert!(w.island_item(&Env { cfg: &cfg, key: "expediente" }).is_none());
        assert_eq!(left_text(47.0 * 60.0 - 5.0), "47 min");
        assert_eq!(left_text(3600.0), "1 h");
    }
}
