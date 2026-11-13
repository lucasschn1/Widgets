//! A arrumação automática ("Organizar widgets"): os widgets de um monitor em colunas a partir da
//! direita da tela, como na tela de início do iPhone, sem nenhum encostar em outro:
//!
//! - cada coluna tem a largura de um widget largo; dois pequenos dividem a mesma linha, lado a
//!   lado (o primeiro à direita);
//! - a ordem é a de agora (coluna por coluna, da direita para a esquerda; de cima para baixo);
//! - quem não cabe embaixo vai para o alto da coluna seguinte, à esquerda;
//! - o que já ocupa a tela e não se mexe (a ilha do Tocando agora) fica livre: a linha desce até
//!   passar dele;
//! - sem mais colunas na tela, o widget vai para o primeiro lugar livre que sobrou (e, se não há
//!   nenhum, para o canto, por cima: melhor que fora da tela).
//!
//! Tudo em pixels da tela; os retângulos são os dos cartões (sem a margem transparente).

use crate::app::screens::{free_spot, hits};
use crate::theme::{EDGE_GAP, WIDGET_GAP};
use crate::widgets::{SMALL, WIDE};
use crate::win::Rect;

/// Um widget a arrumar: quem é, o tamanho do cartão e onde ele está agora (para manter a ordem).
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub id: usize,
    pub size: (i32, i32),
    pub at: Rect,
}

/// Uma linha de dois pequenos com o lugar da esquerda ainda livre: (topo, altura).
type OpenRow = Option<(i32, i32)>;

/// Para onde vai cada widget (`id`, retângulo do cartão) na área `work` de um monitor de escala
/// `s`, sem encostar em outro nem em `fixed`.
pub fn plan(items: &[Item], work: Rect, s: f32, fixed: &[Rect]) -> Vec<(usize, Rect)> {
    let px = |v: f32| (v * s).round() as i32;
    let (edge, gap, small, wide) = (px(EDGE_GAP), px(WIDGET_GAP), px(SMALL), px(WIDE));
    let (top, bottom, left) = (work.1 + edge, work.3 - edge, work.0 + edge);
    let mut placed: Vec<(usize, Rect)> = Vec::new();
    let mut taken: Vec<Rect> = fixed.to_vec();
    let (mut right, mut y, mut col_w, mut open): (i32, i32, i32, OpenRow) = (work.2 - edge, top, 0, None);
    let mut full = false;
    for item in reading_order(items, work.2, wide + gap) {
        let (w, h) = item.size;
        let is_small = w <= small + 1;
        // o lugar da esquerda de uma linha de dois pequenos
        if let (true, Some((row_y, row_h))) = (is_small, open) {
            let r = (right - small - gap - w, row_y, right - small - gap, row_y + h);
            if r.3 <= bottom && !blocked(r, &taken, gap) {
                open = None;
                y = row_y + row_h.max(h) + gap;
                placed.push((item.id, r));
                taken.push(r);
                continue;
            }
        }
        // (a linha aberta fecha: o próximo começa embaixo dela)
        if let Some((row_y, row_h)) = open.take() {
            y = y.max(row_y + row_h + gap);
        }
        let spot = if full { None } else { column_spot(w, h, (&mut right, &mut y, &mut col_w), (top, bottom, left), &taken, gap) };
        let r = match spot {
            Some(r) => r,
            None => {
                // sem coluna livre na tela: o primeiro lugar livre que sobrou
                full = true;
                free_spot(w, h, work, &taken, true, s)
            }
        };
        if !full {
            col_w = col_w.max(if is_small { wide } else { w });
            if is_small {
                open = Some((r.1, h));
            } else {
                y = r.3 + gap;
            }
        }
        placed.push((item.id, r));
        taken.push(r);
    }
    placed
}

/// A ordem dos widgets: coluna por coluna, da direita para a esquerda (cada `col` pixels a partir
/// da borda `right` da tela), e de cima para baixo em cada uma; na mesma linha, o da direita
/// primeiro (é o lugar da direita de uma linha de dois pequenos).
fn reading_order(items: &[Item], right: i32, col: i32) -> Vec<Item> {
    let mut out = items.to_vec();
    let column = |r: &Rect| ((right - r.2).max(0) + col / 3) / col.max(1);
    out.sort_by_key(|it| (column(&it.at), it.at.1, -it.at.2));
    out
}

/// O lugar do próximo widget `w` × `h` na coluna de agora (`right`, `y`, `col_w`), ou no alto das
/// seguintes, à esquerda; desce para passar do que está em `taken`. None: não há mais colunas na
/// tela.
fn column_spot(w: i32, h: i32, (right, y, col_w): (&mut i32, &mut i32, &mut i32), (top, bottom, left): (i32, i32, i32), taken: &[Rect], gap: i32) -> Option<Rect> {
    loop {
        if *right - w < left {
            return None;
        }
        let r = (*right - w, *y, *right, *y + h);
        // não cabe embaixo (e a coluna já tem alguém): a próxima coluna
        if r.3 > bottom && (*col_w > 0 || *y > top) {
            *right -= (*col_w).max(w) + gap;
            *y = top;
            *col_w = 0;
            continue;
        }
        match taken.iter().filter(|o| hits(grown(r, gap), **o)).map(|o| o.3).max() {
            Some(below) => *y = below + gap,
            None => return Some(r),
        }
    }
}

/// `r` encosta (ou fica a menos de `gap`) em alguém de `taken`?
fn blocked(r: Rect, taken: &[Rect], gap: i32) -> bool {
    taken.iter().any(|o| hits(grown(r, gap), *o))
}

/// `r` com `by` pixels a mais de cada lado, menos um (encostar com o espaço certo não conta).
fn grown(r: Rect, by: i32) -> Rect {
    let by = (by - 1).max(0);
    (r.0 - by, r.1 - by, r.2 + by, r.3 + by)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = (0, 0, 1536, 816);
    const S: f32 = 1.0;

    fn px(v: f32) -> i32 {
        (v * S).round() as i32
    }

    /// Um widget de `size` (em pontos) que está agora em (x, y).
    fn item(id: usize, size: (f32, f32), x: i32, y: i32) -> Item {
        let (w, h) = (px(size.0), px(size.1));
        Item { id, size: (w, h), at: (x, y, x + w, y + h) }
    }

    /// Ninguém encosta em ninguém (nem com menos que o espaço entre widgets), nem nos fixos, e
    /// todos ficam dentro da tela.
    fn assert_tidy(plan: &[(usize, Rect)], fixed: &[Rect]) {
        let gap = px(WIDGET_GAP);
        for (a, (ia, ra)) in plan.iter().enumerate() {
            assert!(ra.0 >= WORK.0 && ra.1 >= WORK.1 && ra.2 <= WORK.2 && ra.3 <= WORK.3, "{ia} fora da tela: {ra:?}");
            for (ib, rb) in plan.iter().skip(a + 1) {
                assert!(!hits(grown(*ra, gap), *rb), "{ia} e {ib} encostam: {ra:?} {rb:?}");
            }
            for f in fixed {
                assert!(!hits(*ra, *f), "{ia} por cima do fixo {f:?}");
            }
        }
    }

    fn rect_of(plan: &[(usize, Rect)], id: usize) -> Rect {
        plan.iter().find(|p| p.0 == id).unwrap().1
    }

    /// Os seis widgets de sempre, de vários tamanhos e embaralhados: nada encosta, tudo na tela.
    #[test]
    fn nothing_touches() {
        let items = [
            item(0, (WIDE, SMALL), 300, 500),
            item(1, (SMALL, SMALL), 900, 40),
            item(2, (WIDE, 448.0), 100, 100),
            item(3, (SMALL, SMALL), 1200, 600),
            item(4, (WIDE, SMALL), 1180, 300),
            item(5, (SMALL, SMALL), 20, 20),
        ];
        let p = plan(&items, WORK, S, &[]);
        assert_eq!(p.len(), items.len());
        assert_tidy(&p, &[]);
    }

    /// Dois pequenos seguidos dividem a linha: o primeiro à direita, o segundo à esquerda dele.
    #[test]
    fn two_small_share_a_row() {
        let items = [item(0, (SMALL, SMALL), 1300, 30), item(1, (SMALL, SMALL), 1300, 200), item(2, (WIDE, SMALL), 1200, 400)];
        let p = plan(&items, WORK, S, &[]);
        let (a, b, c) = (rect_of(&p, 0), rect_of(&p, 1), rect_of(&p, 2));
        assert_eq!(a.1, b.1, "não ficaram na mesma linha");
        assert!(b.2 < a.0, "o segundo não ficou à esquerda");
        assert_eq!(a.2, WORK.2 - px(EDGE_GAP), "o primeiro não ficou na borda direita");
        assert_eq!(c.1, a.3 + px(WIDGET_GAP), "o largo não veio logo embaixo");
        assert_eq!((c.0, c.2), (b.0, a.2), "o largo não ocupou a largura da linha");
        assert_tidy(&p, &[]);
    }

    /// A ordem de agora fica: quem estava mais à direita e mais em cima vem primeiro.
    #[test]
    fn keeps_the_order() {
        let items = [item(0, (WIDE, SMALL), 1150, 400), item(1, (WIDE, SMALL), 1150, 30), item(2, (WIDE, SMALL), 700, 30)];
        let p = plan(&items, WORK, S, &[]);
        assert!(rect_of(&p, 1).1 < rect_of(&p, 0).1, "o de cima desceu");
        assert!(rect_of(&p, 0).1 < rect_of(&p, 2).1 || rect_of(&p, 2).0 < rect_of(&p, 0).0, "o da coluna da esquerda passou na frente");
    }

    /// Quem não cabe embaixo vai para o alto da coluna seguinte, à esquerda.
    #[test]
    fn full_column_starts_another() {
        let items: Vec<Item> = (0..6).map(|i| item(i, (WIDE, 200.0), 1200, 30 + 210 * i as i32)).collect();
        let p = plan(&items, WORK, S, &[]);
        assert_tidy(&p, &[]);
        let lefts: std::collections::BTreeSet<i32> = p.iter().map(|(_, r)| r.0).collect();
        assert_eq!(lefts.len(), 2, "não abriu outra coluna: {p:?}");
        let second = p.iter().map(|(_, r)| *r).filter(|r| r.0 == *lefts.iter().next().unwrap()).min_by_key(|r| r.1).unwrap();
        assert_eq!(second.1, WORK.1 + px(EDGE_GAP), "a coluna nova não começou no alto");
    }

    /// A ilha do Tocando agora (fixa no alto) fica livre: o widget desce até passar dela.
    #[test]
    fn steers_clear_of_the_island() {
        let island = (WORK.2 - 300, 0, WORK.2 - 100, 60);
        let items = [item(0, (WIDE, SMALL), 1200, 10), item(1, (SMALL, SMALL), 1200, 200)];
        let p = plan(&items, WORK, S, &[island]);
        assert_tidy(&p, &[island]);
        assert!(rect_of(&p, 0).1 >= island.3 + px(WIDGET_GAP));
    }

    /// Sem espaço para todos, ninguém sai da tela (e quem couber não encosta em ninguém).
    #[test]
    fn too_many_stay_on_screen() {
        let items: Vec<Item> = (0..30).map(|i| item(i, (WIDE, 448.0), 1200, i as i32)).collect();
        let p = plan(&items, WORK, S, &[]);
        assert_eq!(p.len(), items.len());
        for (_, r) in &p {
            assert!(r.0 >= WORK.0 && r.2 <= WORK.2 && r.1 >= WORK.1, "fora da tela: {r:?}");
        }
    }

    /// Vários tamanhos e ordens que cabem na tela: nunca encostam, em monitores de várias escalas.
    #[test]
    fn many_mixes_never_touch() {
        let sizes = [(SMALL, SMALL), (WIDE, SMALL), (WIDE, 448.0), (WIDE, 230.0)];
        let mut seed = 7u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        // (até quatro: numa tela de notebook, cabem quatro grandes lado a lado)
        for s in [1.0f32, 1.25, 1.5] {
            let work = (0, 0, (1536.0 * s).round() as i32, (816.0 * s).round() as i32);
            for _ in 0..300 {
                let n = 2 + (next() % 3) as usize;
                let items: Vec<Item> = (0..n)
                    .map(|i| {
                        let (w, h) = sizes[(next() % 4) as usize];
                        let (w, h) = ((w * s).round() as i32, (h * s).round() as i32);
                        let (x, y) = ((next() % 1500) as i32, (next() % 700) as i32);
                        Item { id: i, size: (w, h), at: (x, y, x + w, y + h) }
                    })
                    .collect();
                let p = plan(&items, work, s, &[]);
                let gap = (WIDGET_GAP * s).round() as i32;
                for (a, (_, ra)) in p.iter().enumerate() {
                    assert!(ra.2 <= work.2 && ra.0 >= work.0 && ra.3 <= work.3, "fora: {ra:?} em {s}");
                    for (_, rb) in p.iter().skip(a + 1) {
                        assert!(!hits(grown(*ra, gap), *rb), "encostam em {s}: {ra:?} {rb:?}");
                    }
                }
            }
        }
    }
}
