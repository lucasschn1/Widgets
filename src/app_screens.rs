//! Vários monitores: o lugar dos widgets em cada combinação de telas ("só o notebook",
//! "notebook + monitor externo"...). Quando a combinação muda, cada widget vai para onde estava
//! da última vez nessa combinação; numa combinação nova, fica no mesmo canto do mesmo monitor
//! (ou do principal, se o monitor dele saiu), com a escala do monitor novo.
//! (Módulo filho de `app`: mexe direto no estado do App.)

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use super::*;
use crate::win::Monitor;

/// Espera a combinação de monitores ficar parada antes de mexer: ao acordar o PC, trocar a
/// entrada do monitor ou ligar no dock, as telas somem e voltam em poucos segundos.
pub const SETTLE: Duration = Duration::from_secs(3);
pub const SCREENS_EVERY: Duration = Duration::from_secs(2);
const SCREENS_WAITING: Duration = Duration::from_millis(500);
/// combinações lembradas (as usadas há mais tempo são esquecidas)
const KEEP: usize = 10;

/// Nome curto da combinação de monitores (aparelhos, posições e escalas).
pub fn signature(mons: &[Monitor]) -> String {
    let mut parts: Vec<String> = mons.iter().map(|m| format!("{}@{:?}@{}", m.id, m.screen, (m.scale * 100.0).round() as i32)).collect();
    parts.sort();
    // FNV-1a: só para a chave ficar curta no config.json
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in parts.join("|").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub fn monitors_to_json(mons: &[Monitor]) -> Value {
    let r = |r: Rect| json!([r.0, r.1, r.2, r.3]);
    Value::Array(mons.iter().map(|m| json!({"id": m.id, "tela": r(m.screen), "area": r(m.work), "escala": m.scale})).collect())
}

pub fn monitors_from_json(v: &Value) -> Vec<Monitor> {
    let rect = |v: &Value| -> Option<Rect> {
        let n: Vec<i32> = v.as_array()?.iter().filter_map(Value::as_i64).map(|x| x as i32).collect();
        (n.len() == 4).then(|| (n[0], n[1], n[2], n[3]))
    };
    let one = |m: &Value| -> Option<Monitor> {
        Some(Monitor {
            id: m.get("id")?.as_str()?.to_string(),
            screen: rect(m.get("tela")?)?,
            work: rect(m.get("area")?)?,
            scale: m.get("escala")?.as_f64()? as f32,
        })
    };
    v.as_array().map(|a| a.iter().filter_map(one).collect()).unwrap_or_default()
}

fn pair(v: &Value) -> Option<(i32, i32)> {
    let a = v.as_array()?;
    Some((a.first()?.as_i64()? as i32, a.get(1)?.as_i64()? as i32))
}

/// Um widget a posicionar: posição da janela e tamanho do cartão em unidades lógicas.
pub struct Spot {
    pub i: usize,
    pub pos: (i32, i32),
    pub size: (f32, f32),
}

fn margin(s: f32) -> i32 {
    (MARGIN * s).round() as i32
}

/// Retângulo do cartão (sem a margem) com a janela em `pos`, na escala `s`.
fn rect_at(pos: (i32, i32), size: (f32, f32), s: f32) -> Rect {
    let m = margin(s);
    let (w, h) = ((size.0 * s).round() as i32, (size.1 * s).round() as i32);
    (pos.0 + m, pos.1 + m, pos.0 + m + w, pos.1 + m + h)
}

pub fn bbox(rects: &[Rect]) -> Rect {
    rects.iter().fold((i32::MAX, i32::MAX, i32::MIN, i32::MIN), |b, r| (b.0.min(r.0), b.1.min(r.1), b.2.max(r.2), b.3.max(r.3)))
}

pub fn hits(a: Rect, b: Rect) -> bool {
    let (w, h) = win::overlap(a, b);
    w > 0 && h > 0
}

fn collides(rects: &[Rect], others: &[Rect]) -> bool {
    rects.iter().any(|r| others.iter().any(|o| hits(*r, *o)))
}

pub fn shifted(rects: &[Rect], dx: i32, dy: i32) -> Vec<Rect> {
    rects.iter().map(|r| (r.0 + dx, r.1 + dy, r.2 + dx, r.3 + dy)).collect()
}

pub fn inside(r: Rect, area: Rect) -> bool {
    r.0 >= area.0 && r.1 >= area.1 && r.2 <= area.2 && r.3 <= area.3
}

/// Quanto mover o grupo para caber na área; se não couber, fica preso ao lado da âncora.
fn fit(bb: Rect, area: Rect, right: bool, bottom: bool) -> (i32, i32) {
    let axis = |lo: i32, hi: i32, a_lo: i32, a_hi: i32, far: bool| {
        if hi - lo > a_hi - a_lo {
            if far { a_hi - hi } else { a_lo - lo }
        } else if lo < a_lo {
            a_lo - lo
        } else if hi > a_hi {
            a_hi - hi
        } else {
            0
        }
    };
    (axis(bb.0, bb.2, area.0, area.2, right), axis(bb.1, bb.3, area.1, area.3, bottom))
}

/// Primeiro lugar livre para um cartão w×h, em colunas a partir do lado da âncora.
pub fn free_spot(w: i32, h: i32, area: Rect, taken: &[Rect], right: bool, s: f32) -> Rect {
    let (edge, gap) = ((EDGE_GAP * s).round() as i32, (WIDGET_GAP * s).round() as i32);
    let step = ((8.0 * s).round() as i32).max(1);
    let (x_min, x_max) = (area.0 + edge, area.2 - edge - w);
    let mut x = if right { x_max } else { x_min };
    while x >= x_min && x <= x_max {
        let mut y = area.1 + edge;
        while y + h <= area.3 - edge {
            let r = (x, y, x + w, y + h);
            let roomy = (r.0 - gap + 1, r.1 - gap + 1, r.2 + gap - 1, r.3 + gap - 1);
            match taken.iter().filter(|o| hits(roomy, **o)).map(|o| o.3).max() {
                Some(bottom) => y = bottom + gap,
                None => return r,
            }
        }
        x += if right { -step } else { step };
    }
    // não há espaço livre: fica no canto, mesmo por cima de outro
    let x = if right { x_max } else { x_min };
    (x, area.1 + edge, x + w, area.1 + edge + h)
}

/// Para onde vai cada widget ao passar dos monitores `old` para `new`: (índice, posição, escala).
/// `saved`: onde cada widget ficava da última vez na combinação nova (vale mais que a conta).
pub fn plan(spots: &[Spot], keys: &[&str], old: &[Monitor], new: &[Monitor], saved: Option<&Map<String, Value>>) -> Vec<(usize, (i32, i32), f32)> {
    let mut out = Vec::new();
    if new.is_empty() {
        return out;
    }
    // (monitor novo, retângulo) de quem já tem lugar
    let mut taken: Vec<(usize, Rect)> = Vec::new();
    let mut rest: Vec<&Spot> = Vec::new();
    for sp in spots {
        let back = saved.and_then(|m| m.get(keys[sp.i])).and_then(pair).and_then(|pos| {
            let n = holder(new, rect_at(pos, sp.size, 1.0))?;
            let s = new[n].scale;
            let r = rect_at(pos, sp.size, s);
            let (w, h) = win::overlap(r, new[n].screen);
            (w.min(h) >= (60.0 * s) as i32).then_some((n, pos, s, r))
        });
        match back {
            Some((n, pos, s, r)) => {
                out.push((sp.i, pos, s));
                taken.push((n, r));
            }
            None => rest.push(sp),
        }
    }
    // os outros andam em grupo com os que estavam no mesmo monitor
    let old = if old.is_empty() { new } else { old };
    let mut groups: Vec<(usize, Vec<&Spot>)> = Vec::new();
    for sp in rest {
        let o = holder(old, rect_at(sp.pos, sp.size, 1.0)).unwrap_or(0);
        match groups.iter_mut().find(|g| g.0 == o) {
            Some(g) => g.1.push(sp),
            None => groups.push((o, vec![sp])),
        }
    }
    // primeiro os monitores que continuam ligados; depois os que saíram
    groups.sort_by_key(|(o, _)| !new.iter().any(|m| m.id == old[*o].id));
    for (o, group) in groups {
        let om = &old[o];
        let tn = new.iter().position(|m| m.id == om.id).unwrap_or(0);
        let nm = &new[tn];
        let (so, sn) = (om.scale, nm.scale);
        let rects: Vec<Rect> = group.iter().map(|sp| rect_at(sp.pos, sp.size, so)).collect();
        let bb = bbox(&rects);
        let (wo, wn) = (om.work, nm.work);
        // preso ao canto da tela mais perto do grupo
        let right = (wo.2 - bb.2) < (bb.0 - wo.0);
        let bottom = (wo.3 - bb.3) < (bb.1 - wo.1);
        let co = (if right { wo.2 } else { wo.0 }, if bottom { wo.3 } else { wo.1 });
        let cn = (if right { wn.2 } else { wn.0 }, if bottom { wn.3 } else { wn.1 });
        let mut placed: Vec<Rect> = rects
            .iter()
            .zip(&group)
            .map(|(r, sp)| {
                let x = cn.0 + ((r.0 - co.0) as f32 / so * sn).round() as i32;
                let y = cn.1 + ((r.1 - co.1) as f32 / so * sn).round() as i32;
                (x, y, x + (sp.size.0 * sn).round() as i32, y + (sp.size.1 * sn).round() as i32)
            })
            .collect();
        let (dx, dy) = fit(bbox(&placed), wn, right, bottom);
        placed = shifted(&placed, dx, dy);
        let others: Vec<Rect> = taken.iter().filter(|t| t.0 == tn).map(|t| t.1).collect();
        if collides(&placed, &others) {
            // ao lado dos que já estão nesse monitor, mantendo o arranjo do grupo
            let gap = (WIDGET_GAP * sn).round() as i32;
            let bb = bbox(&placed);
            let dx = if right { others.iter().map(|r| r.0).min().unwrap() - gap - bb.2 } else { others.iter().map(|r| r.2).max().unwrap() + gap - bb.0 };
            let beside = shifted(&placed, dx, 0);
            if inside(bbox(&beside), wn) && !collides(&beside, &others) {
                placed = beside;
            } else {
                // não cabe junto: cada um no primeiro lugar livre
                let mut all = others.clone();
                for r in placed.iter_mut() {
                    *r = free_spot(r.2 - r.0, r.3 - r.1, wn, &all, right, sn);
                    all.push(*r);
                }
            }
        }
        for (sp, r) in group.iter().zip(placed) {
            out.push((sp.i, (r.0 - margin(sn), r.1 - margin(sn)), sn));
            taken.push((tn, r));
        }
    }
    out
}

/// Depois de um widget `i` mudar de tamanho (de `old` para `new`): os vizinhos saem da frente,
/// para ninguém ficar por cima ou por baixo de outro nem cortado pela borda da tela.
/// `rects`: cartões de todos (onde estão ou para onde já vão); `start`: para onde alguns já iam
/// (os encaixados embaixo); `below`: quem está encaixado embaixo de cada um; `area_for`: área de
/// trabalho do monitor de um retângulo. Devolve (índice, retângulo final) de quem muda de lugar.
#[allow(clippy::too_many_arguments)]
pub fn make_room(
    i: usize,
    old: Rect,
    new: Rect,
    rects: &[Rect],
    start: &HashMap<usize, Rect>,
    below: &[Vec<usize>],
    s: f32,
    area_for: impl Fn(Rect) -> Rect,
) -> Vec<(usize, Rect)> {
    let n = rects.len();
    let mut cur: Vec<Rect> = (0..n).map(|j| start.get(&j).copied().unwrap_or(rects[j])).collect();
    let gap = (WIDGET_GAP * s).round() as i32;
    // empurra primeiro para os lados em que o widget cresceu
    let mut dirs: Vec<(i32, i32)> = Vec::new();
    if new.0 < old.0 {
        dirs.push((-1, 0));
    }
    if new.2 > old.2 {
        dirs.push((1, 0));
    }
    if new.3 > old.3 {
        dirs.push((0, 1));
    }
    if new.1 < old.1 {
        dirs.push((0, -1));
    }
    let all = [(0, 1), (-1, 0), (1, 0), (0, -1)];
    // (índice, retângulo final, mudou de lugar)
    let mut settled: Vec<(usize, Rect, bool)> = vec![(i, new, true)];
    let mut done = vec![false; n];
    done[i] = true;
    let far = |r: Rect| (new.0 - r.2).max(r.0 - new.2).max(0) + (new.1 - r.3).max(r.1 - new.3).max(0);
    let mut order: Vec<usize> = (0..n).filter(|&j| j != i).collect();
    order.sort_by_key(|&j| (!start.contains_key(&j), far(cur[j])));
    for j in order {
        if done[j] {
            continue;
        }
        let r = cur[j];
        let area = area_for(rects[j]);
        // só sai da frente de quem mudou (sobreposições que já existiam ficam como estão)
        let blocked = settled.iter().any(|(_, o, changed)| *changed && hits(r, *o));
        let cut = start.contains_key(&j) && !inside(r, area);
        if !blocked && !cut {
            settled.push((j, r, start.contains_key(&j)));
            done[j] = true;
            continue;
        }
        // quanto o grupo anda na direção `d` para não tocar em ninguém (e caber na tela)
        let clear = |group: &[usize], d: (i32, i32)| -> Option<(i32, i32)> {
            let rs: Vec<Rect> = group.iter().map(|&g| cur[g]).collect();
            let mut off = (0, 0);
            for _ in 0..=n + 1 {
                let moved = shifted(&rs, off.0, off.1);
                let mut step: Option<i32> = None;
                for (_, o, _) in &settled {
                    for m in moved.iter().filter(|m| hits(**m, *o)) {
                        let need = match d {
                            (0, 1) => o.3 + gap - m.1,
                            (0, -1) => o.1 - gap - m.3,
                            (1, 0) => o.2 + gap - m.0,
                            _ => o.0 - gap - m.2,
                        };
                        step = Some(match step {
                            Some(v) if d.0 + d.1 > 0 => v.max(need),
                            Some(v) => v.min(need),
                            None => need,
                        });
                    }
                }
                match step {
                    None => return inside(bbox(&moved), area).then_some(off),
                    Some(v) => {
                        off.0 += v * d.0.abs();
                        off.1 += v * d.1.abs();
                    }
                }
            }
            None
        };
        // vai junto: os encaixados embaixo; de lado, a coluna inteira (para continuar alinhada)
        let group_for = |d: (i32, i32)| -> Vec<usize> {
            let mut g = vec![j];
            g.extend(below[j].iter().copied());
            if d.0 != 0 {
                g.extend((0..n).filter(|&k| below[k].contains(&j)));
            }
            g.sort();
            g.dedup();
            g.retain(|&k| !done[k] && k != i);
            g
        };
        let try_dirs = |list: &[(i32, i32)]| {
            list.iter()
                .filter_map(|&d| {
                    let g = group_for(d);
                    clear(&g, d).filter(|off| *off != (0, 0) || !cut).map(|off| (off, g))
                })
                .min_by_key(|(off, _)| off.0.abs() + off.1.abs())
        };
        match try_dirs(&dirs).or_else(|| try_dirs(&all)) {
            Some((off, group)) => {
                for g in group {
                    cur[g] = shifted(&[cur[g]], off.0, off.1)[0];
                    settled.push((g, cur[g], true));
                    done[g] = true;
                }
            }
            None => {
                // não há como empurrar: vai para o primeiro lugar livre do monitor
                let mut taken: Vec<Rect> = settled.iter().map(|t| t.1).collect();
                taken.extend((0..n).filter(|&k| !done[k] && k != j).map(|k| cur[k]));
                let right = r.0 + r.2 > area.0 + area.2;
                cur[j] = free_spot(r.2 - r.0, r.3 - r.1, area, &taken, right, s);
                settled.push((j, cur[j], true));
                done[j] = true;
            }
        }
    }
    (0..n).filter(|&j| j != i && (cur[j] != rects[j] || start.contains_key(&j))).map(|j| (j, cur[j])).collect()
}

/// Monitor com a maior parte do retângulo (None se não toca em nenhum).
fn holder(mons: &[Monitor], rect: Rect) -> Option<usize> {
    mons.iter()
        .enumerate()
        .map(|(n, m)| {
            let (w, h) = win::overlap(rect, m.screen);
            (n, w as i64 * h as i64)
        })
        .filter(|(_, a)| *a > 0)
        .max_by_key(|(_, a)| *a)
        .map(|(n, _)| n)
}

impl App {
    /// Ao abrir: as posições guardadas são de qual combinação de monitores? Devolve os monitores
    /// de agora se forem outros (aí `finish_screens_startup` leva os widgets para o lugar certo).
    pub(super) fn begin_screens_startup(&mut self) -> Option<(Vec<Monitor>, String)> {
        let mons = win::monitor_list();
        let sig = signature(&mons);
        let stored = self.sh.cfg["app"].str_of("tela").unwrap_or_default().to_string();
        let stored_mons = monitors_from_json(&self.sh.cfg["app"]["telas"][&stored]["monitores"]);
        if !stored.is_empty() && stored != sig && !stored_mons.is_empty() && !mons.is_empty() {
            // os widgets nascem onde estavam (mesmo fora da tela) e depois mudam de lugar
            self.screens = stored_mons;
            self.screens_sig = stored;
            self.keep_saved_pos = true;
            return Some((mons, sig));
        }
        self.screens = mons;
        self.screens_sig = sig;
        None
    }

    pub(super) fn finish_screens_startup(&mut self, change: Option<(Vec<Monitor>, String)>) {
        self.keep_saved_pos = false;
        match change {
            Some((mons, sig)) => self.screens_changed(mons, sig),
            None => {
                let sig = self.screens_sig.clone();
                self.sh.cfg.obj_mut("app").insert("tela".into(), json!(sig));
                self.remember_screens();
            }
        }
    }

    /// Confere os monitores; só reorganiza quando a combinação nova fica parada por `SETTLE`.
    pub(super) fn check_screens(&mut self) {
        let mons = win::monitor_list();
        let sig = signature(&mons);
        if mons.is_empty() || sig == self.screens_sig {
            if self.screens_pending.take().is_some() {
                // voltou como estava: põe cada janela no lugar (o Windows pode ter mexido nelas)
                for c in &mut self.cards {
                    c.present(&self.sh, true);
                }
            }
            self.next_screens = Instant::now() + SCREENS_EVERY;
            return;
        }
        self.next_screens = Instant::now() + SCREENS_WAITING;
        match &self.screens_pending {
            Some((pending, since)) if *pending == sig => {
                if since.elapsed() >= SETTLE {
                    self.screens_pending = None;
                    self.next_screens = Instant::now() + SCREENS_EVERY;
                    self.screens_changed(mons, sig);
                }
            }
            _ => self.screens_pending = Some((sig, Instant::now())),
        }
    }

    /// Guarda onde cada widget fica na combinação de monitores atual.
    fn remember_screens(&mut self) {
        if self.screens_sig.is_empty() || self.screens.is_empty() {
            return;
        }
        let mut widgets = Map::new();
        for c in &self.cards {
            let w = &self.sh.cfg[c.key()];
            if let (Some(x), Some(y)) = (w.i64_of("x"), w.i64_of("y")) {
                widgets.insert(c.key().into(), json!([x, y]));
            }
        }
        let entry = json!({"monitores": monitors_to_json(&self.screens), "widgets": widgets, "usado": win::now()});
        let sig = self.screens_sig.clone();
        let app = self.sh.cfg.obj_mut("app");
        let telas = app.entry("telas").or_insert(json!({}));
        if !telas.is_object() {
            *telas = json!({});
        }
        let map = telas.as_object_mut().unwrap();
        map.insert(sig, entry);
        while map.len() > KEEP {
            let oldest = map.iter().min_by(|a, b| a.1.f64_of("usado").unwrap_or(0.0).total_cmp(&b.1.f64_of("usado").unwrap_or(0.0))).map(|(k, _)| k.clone());
            match oldest {
                Some(k) => map.remove(&k),
                None => break,
            };
        }
    }

    /// A combinação de monitores mudou: guarda o lugar de cada widget na que saiu e leva cada um
    /// para o lugar certo na nova.
    pub(super) fn screens_changed(&mut self, mons: Vec<Monitor>, sig: String) {
        debug_log!("monitores: {} -> {} ({} telas)", self.screens_sig, sig, mons.len());
        self.remember_screens();
        self.tooltip.hide();
        self.guides.hide();
        self.later.clear();
        for c in &mut self.cards {
            c.temp_home = None;
            if c.temp {
                // aberto só pelo mouse: volta ao pequeno antes da conta
                c.temp = false;
                c.mode = Mode::P;
                c.content = None;
            }
            c.anim = None;
            c.slide = None;
            c.drag = None;
            c.press = None;
        }
        let spots: Vec<Spot> = (0..self.cards.len())
            .map(|i| {
                let c = &self.cards[i];
                let w = &self.sh.cfg[c.key()];
                let pos = match (w.i64_of("x"), w.i64_of("y")) {
                    (Some(x), Some(y)) => (x as i32, y as i32),
                    _ => c.pos,
                };
                let l = c.layout(c.mode);
                Spot { i, pos, size: (l.w, l.h) }
            })
            .collect();
        let keys: Vec<&str> = self.cards.iter().map(|c| c.key()).collect();
        let saved = self.sh.cfg["app"]["telas"][&sig]["widgets"].as_object().cloned();
        let moves = plan(&spots, &keys, &self.screens, &mons, saved.as_ref());
        self.screens = mons;
        self.screens_sig = sig.clone();
        self.sh.cfg.obj_mut("app").insert("tela".into(), json!(sig));
        for (i, pos, s) in moves {
            let c = &mut self.cards[i];
            c.pos = pos;
            if (s - c.s).abs() > 0.01 || c.content.is_none() {
                c.set_scale(s);
                c.refresh(&self.sh);
            }
            c.present(&self.sh, true);
            let key = c.key();
            let w = self.sh.cfg.obj_mut(key);
            w.insert("x".into(), json!(pos.0));
            w.insert("y".into(), json!(pos.1));
            widgets::store_size(w, self.cards[i].mode);
            w.remove("compact");
        }
        self.remember_screens();
        self.island_screens_changed();
        self.perch_screens_changed();
        self.sh.save();
    }

    /// Layout salvo com outra combinação de monitores: onde cada widget aberto fica nesta.
    /// (chave, posição, escala); vazio se o layout é desta combinação (ou de antes de guardá-la).
    pub(super) fn layout_moves(&self, snapshot: &Value) -> Vec<(&'static str, (i32, i32), f32)> {
        let old = monitors_from_json(&snapshot["_monitores"]);
        if old.is_empty() || signature(&old) == self.screens_sig {
            return Vec::new();
        }
        let mut spots = Vec::new();
        for (i, c) in self.cards.iter().enumerate() {
            let state = &snapshot[c.key()];
            if !state.bool_of("visible").unwrap_or(false) {
                continue;
            }
            let (Some(x), Some(y)) = (state.i64_of("x"), state.i64_of("y")) else { continue };
            let mode = state.str_of("tamanho").and_then(Mode::from_code).filter(|m| c.kind.sizes().contains(m)).unwrap_or(c.mode);
            let l = c.layout(mode);
            spots.push(Spot { i, pos: (x as i32, y as i32), size: (l.w, l.h) });
        }
        let keys: Vec<&str> = self.cards.iter().map(|c| c.key()).collect();
        plan(&spots, &keys, &old, &self.screens, None).into_iter().map(|(i, pos, s)| (self.cards[i].key(), pos, s)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, screen: Rect, scale: f32) -> Monitor {
        // barra de tarefas de 48 px embaixo
        Monitor { id: id.into(), screen, work: (screen.0, screen.1, screen.2, screen.3 - 48), scale }
    }

    fn card_rect(m: &(usize, (i32, i32), f32), size: (f32, f32)) -> Rect {
        rect_at(m.1, size, m.2)
    }

    const SIZE: (f32, f32) = (320.0, 160.0);

    /// notebook sozinho (150%) e notebook à direita de um externo principal (100%)
    fn setups() -> (Vec<Monitor>, Vec<Monitor>) {
        let alone = vec![mon("note", (0, 0, 1920, 1200), 1.5)];
        let docked = vec![mon("ext", (0, 0, 2560, 1440), 1.0), mon("note", (2560, 0, 4480, 1200), 1.5)];
        (alone, docked)
    }

    #[test]
    fn same_screens_change_nothing() {
        let (alone, _) = setups();
        let pos = (1920 - 36 - 480 - 12, 24);
        let out = plan(&[Spot { i: 0, pos, size: SIZE }], &["a"], &alone, &alone, None);
        assert_eq!(out, vec![(0, pos, 1.5)]);
    }

    #[test]
    fn follows_monitor_when_it_becomes_secondary() {
        let (alone, docked) = setups();
        // canto superior direito do notebook
        let r = (1920 - 36 - 480, 36, 1920 - 36, 36 + 240);
        let pos = (r.0 - 12, r.1 - 12);
        let out = plan(&[Spot { i: 0, pos, size: SIZE }], &["a"], &alone, &docked, None);
        let nr = card_rect(&out[0], SIZE);
        assert_eq!(out[0].2, 1.5);
        assert_eq!(nr, (4480 - 36 - 480, 36, 4480 - 36, 276), "continua no canto do notebook");
    }

    #[test]
    fn vanished_monitor_moves_group_and_scale() {
        let (alone, docked) = setups();
        // dois widgets empilhados no canto direito do externo (100%)
        let a = (2560 - 24 - 320, 24);
        let b = (2560 - 24 - 320, 24 + 160 + 12);
        let spots = [Spot { i: 0, pos: (a.0 - 8, a.1 - 8), size: SIZE }, Spot { i: 1, pos: (b.0 - 8, b.1 - 8), size: SIZE }];
        let out = plan(&spots, &["a", "b"], &docked, &alone, None);
        let (ra, rb) = (card_rect(&out[0], SIZE), card_rect(&out[1], SIZE));
        assert!(out.iter().all(|m| m.2 == 1.5), "pega a escala do notebook");
        assert_eq!(ra, (1920 - 36 - 480, 36, 1920 - 36, 276));
        assert_eq!(rb.1 - ra.3, 18, "o espaço entre eles cresce junto com a escala");
        assert_eq!(ra.0, rb.0);
    }

    #[test]
    fn vanished_monitor_avoids_widgets_already_there() {
        let (alone, docked) = setups();
        // um no canto direito do externo e outro no mesmo canto do notebook
        let ext = (2560 - 24 - 320 - 8, 24 - 8);
        let note = (4480 - 36 - 480 - 12, 36 - 12);
        let spots = [Spot { i: 0, pos: ext, size: SIZE }, Spot { i: 1, pos: note, size: SIZE }];
        let out = plan(&spots, &["a", "b"], &docked, &alone, None);
        let get = |i| card_rect(out.iter().find(|m| m.0 == i).unwrap(), SIZE);
        let (ra, rb) = (get(0), get(1));
        assert_eq!(rb, (1920 - 36 - 480, 36, 1920 - 36, 276), "o do notebook fica no lugar");
        assert!(!hits(ra, rb), "o que veio do externo não fica por cima");
        assert!(inside(ra, alone[0].work));
    }

    #[test]
    fn saved_positions_come_back() {
        let (alone, docked) = setups();
        let mut saved = Map::new();
        saved.insert("a".into(), json!([100, 200]));
        let out = plan(&[Spot { i: 0, pos: (50, 50), size: SIZE }], &["a"], &alone, &docked, Some(&saved));
        assert_eq!(out, vec![(0, (100, 200), 1.0)]);
    }

    #[test]
    fn saved_position_off_screen_is_ignored() {
        let (alone, docked) = setups();
        let mut saved = Map::new();
        saved.insert("a".into(), json!([9000, 200]));
        let out = plan(&[Spot { i: 0, pos: (50, 50), size: SIZE }], &["a"], &docked, &alone, Some(&saved));
        assert!(inside(card_rect(&out[0], SIZE), alone[0].work));
    }

    #[test]
    fn too_many_widgets_still_fit() {
        let (alone, docked) = setups();
        // coluna cheia no externo e outra no notebook: no notebook sozinho não cabe tudo junto
        let mut spots = Vec::new();
        for k in 0..6 {
            spots.push(Spot { i: k, pos: (2560 - 24 - 320 - 8, 24 + k as i32 * 172 - 8), size: SIZE });
        }
        for k in 0..4 {
            spots.push(Spot { i: 6 + k, pos: (4480 - 36 - 480 - 12, 36 + k as i32 * 258 - 12), size: SIZE });
        }
        let keys = ["a"; 10];
        let out = plan(&spots, &keys, &docked, &alone, None);
        assert_eq!(out.len(), 10);
        for m in &out {
            assert!(inside(card_rect(m, SIZE), alone[0].work), "{m:?}");
        }
    }

    #[test]
    fn signature_ignores_order() {
        let (_, docked) = setups();
        let rev: Vec<Monitor> = docked.iter().rev().cloned().collect();
        assert_eq!(signature(&docked), signature(&rev));
        let (alone, _) = setups();
        assert_ne!(signature(&docked), signature(&alone));
        assert_eq!(monitors_from_json(&monitors_to_json(&docked)), docked);
    }

    // ---------- abrir espaço ao mudar de tamanho ----------

    const AREA: Rect = (0, 0, 1920, 1032);

    fn room(i: usize, new: Rect, rects: &[Rect], below: &[Vec<usize>], dy_followers: bool) -> Vec<Rect> {
        let old = rects[i];
        let mut start = HashMap::new();
        if dy_followers {
            for &j in &below[i] {
                let r = rects[j];
                start.insert(j, (r.0, r.1 + new.3 - old.3, r.2, r.3 + new.3 - old.3));
            }
        }
        let mut out = rects.to_vec();
        out[i] = new;
        for (j, r) in make_room(i, old, new, rects, &start, below, 1.0, |_| AREA) {
            out[j] = r;
        }
        out
    }

    fn no_overlaps(rs: &[Rect]) {
        for a in 0..rs.len() {
            assert!(inside(rs[a], AREA), "cortado: {:?}", rs[a]);
            for b in a + 1..rs.len() {
                assert!(!hits(rs[a], rs[b]), "{:?} por cima de {:?}", rs[a], rs[b]);
            }
        }
    }

    fn card(x: i32, y: i32, w: i32, h: i32) -> Rect {
        (x, y, x + w, y + h)
    }

    #[test]
    fn wider_pushes_left_column_whole() {
        // duas colunas à direita; o de cima da direita fica mais largo (cresce para a esquerda)
        let rects = [card(1576, 24, 320, 160), card(1576, 196, 320, 160), card(1244, 24, 320, 160), card(1244, 196, 320, 160)];
        let below = [vec![1], vec![], vec![3], vec![]];
        let out = room(0, card(1416, 24, 480, 320), &rects, &below, true);
        no_overlaps(&out);
        assert_eq!(out[1].1, 24 + 320 + 12, "o encaixado embaixo acompanha");
        assert_eq!(out[2].2, 1416 - 12, "a coluna da esquerda abre espaço");
        assert_eq!(out[2].0, out[3].0, "e continua alinhada");
    }

    #[test]
    fn taller_pushes_loose_card_below() {
        // o de baixo está solto (longe demais para contar como encaixado)
        let rects = [card(1576, 24, 320, 160), card(1576, 260, 320, 160)];
        let below = [vec![], vec![]];
        let out = room(0, card(1576, 24, 320, 400), &rects, &below, true);
        no_overlaps(&out);
        assert_eq!(out[1].1, 24 + 400 + 12);
    }

    #[test]
    fn stack_past_bottom_wraps_instead_of_cutting() {
        let rects: Vec<Rect> = (0..5).map(|k| card(1576, 24 + k * 172, 320, 160)).collect();
        let below: Vec<Vec<usize>> = (0..5).map(|k| ((k + 1)..5).collect()).collect();
        let out = room(0, card(1576, 24, 320, 400), &rects, &below, true);
        no_overlaps(&out);
    }

    #[test]
    fn shrinking_pulls_stack_up_and_leaves_others() {
        let rects = [card(1576, 24, 320, 400), card(1576, 436, 320, 160), card(1244, 24, 320, 160)];
        let below = [vec![1], vec![], vec![]];
        let out = room(0, card(1576, 24, 320, 160), &rects, &below, true);
        assert_eq!(out[1].1, 24 + 160 + 12);
        assert_eq!(out[2], rects[2], "quem não estava no caminho fica parado");
    }

    #[test]
    fn old_overlaps_are_left_alone() {
        // dois widgets que já estavam um por cima do outro, longe de quem mudou
        let rects = [card(1576, 24, 320, 160), card(100, 100, 320, 160), card(200, 150, 320, 160)];
        let below = [vec![], vec![], vec![]];
        let out = room(0, card(1576, 24, 320, 300), &rects, &below, true);
        assert_eq!(out[1], rects[1]);
        assert_eq!(out[2], rects[2]);
    }

    #[test]
    fn full_screen_falls_back_to_free_spot() {
        // coluna da esquerda encostada na borda: não dá para empurrar de lado
        let rects = [card(344, 24, 320, 160), card(12, 24, 320, 160), card(12, 196, 320, 160)];
        let below = [vec![], vec![2], vec![]];
        let out = room(0, card(184, 24, 480, 160), &rects, &below, true);
        no_overlaps(&out);
    }
}
