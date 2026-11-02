//! Gera as imagens do README com os widgets de verdade (dados de exemplo), em vidro sobre um
//! papel de parede em degradê:
//!
//!   cargo test readme -- --ignored --nocapture
//!
//! As imagens paradas vão direto para docs/images; os quadros das animações vão para
//! %TEMP%\widgets-readme (o packaging/readme_anim.py junta em .gif e .webp).

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::backdrop::Backdrop;
use crate::gfx::{self, Anchor, Canvas, Face, Image};
use crate::imgops::blur;
use crate::theme::*;
use crate::widgets::github::GitHub;
use crate::widgets::media::Media;
use crate::widgets::pomodoro::Pomodoro;
use crate::widgets::system::System;
use crate::widgets::tempo::Tempo;
use crate::widgets::{Env, Kind, Layout, Mode};

/// escala das imagens (125%, como num notebook comum)
const S: f32 = 1.25;
const GLASS_TINT: f64 = 0.35;
const BLUR: f64 = 30.0;

fn docs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs").join("images")
}

fn frames_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("widgets-readme").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Degradê azul, roxo, rosa e laranja (parecido com os papéis de parede do Windows 11).
fn wallpaper(w: u32, h: u32) -> Image {
    let mut c = Canvas::new(w, h, 1.0);
    let (wf, hf) = (w as f32, h as f32);
    c.gradient_round_rect(
        0.0,
        0.0,
        wf,
        hf,
        0.0,
        &[(0.0, Rgba::rgb(92, 84, 246)), (0.45, Rgba::rgb(176, 92, 222)), (0.78, Rgba::rgb(234, 118, 138)), (1.0, Rgba::rgb(250, 152, 88))],
        0.0,
        hf,
    );
    c.gradient_h(0.0, 0.0, wf, hf, 0.0, &[(0.0, Rgba(36, 110, 246, 190)), (0.6, Rgba(36, 110, 246, 0)), (1.0, Rgba(36, 110, 246, 0))], 0.0, wf);
    c.finish()
}

/// O conteúdo do widget (sem o vidro) e o raio dos cantos, em pixels.
fn content(kind: &dyn Kind, mode: Mode, cfg: &Value) -> (Image, f32) {
    let l = Layout::of(mode);
    let mut c = Canvas::new((l.w * S).round() as u32, (l.h * S).round() as u32, S);
    kind.draw(&mut c, mode, &Env { cfg, key: kind.key() });
    (c.finish(), l.radius * S)
}

/// Um cartão pronto para pôr na cena: conteúdo, raio, posição.
struct Item {
    img: Image,
    radius: f32,
    x: i32,
    y: i32,
}

/// Uma legenda branca em cima de um cartão.
struct Label {
    text: String,
    x: f32,
    y: f32,
}

fn compose(wall: &Image, bd: &Backdrop, items: &[Item], labels: &[Label]) -> Image {
    let mut c = Canvas::new(wall.w, wall.h, 1.0);
    c.image(wall, 0.0, 0.0);
    for it in items {
        // sombra suave embaixo do cartão
        let pad = 36.0;
        let (w, h) = (it.img.w as f32, it.img.h as f32);
        let mut sh = Canvas::new((w + 2.0 * pad) as u32, (h + 2.0 * pad) as u32, 1.0);
        sh.fill_round_rect(pad, pad + 10.0, pad + w, pad + h + 10.0, it.radius, Rgba(20, 10, 40, 70));
        let mut shadow = sh.finish();
        let (sw, shh) = (shadow.w, shadow.h);
        blur(&mut shadow.px, sw, shh, 4, 16.0);
        c.image(&shadow, it.x as f32 - pad, it.y as f32 - pad);
        let glass = bd.glass(&it.img, it.radius, it.x, it.y, GLASS_TINT);
        c.image(&glass, it.x as f32, it.y as f32);
    }
    for l in labels {
        c.text(l.x, l.y, &l.text, Face::Semibold, 17.0, WHITE.alpha(0.92), Anchor::MS, None);
    }
    c.finish()
}

fn save(img: &Image, path: &PathBuf) {
    gfx::save_png(img, path).unwrap();
    println!("{}", path.display());
}

fn empty_cfg() -> Value {
    json!({"tempo": {}, "sistema": {}, "midia": {}, "github": {}, "pomodoro": {}, "app": {}})
}

fn pomodoro_cfg() -> Value {
    // rodando: 13:27 para acabar o terceiro foco
    json!({"pomodoro": {"estado": {"fase": "foco", "ciclo": 2, "total": 1500.0, "fim": crate::win::now() + 807.5}}, "app": {}})
}

/// Capas inventadas para as músicas de exemplo.
fn cover(n: usize) -> Arc<Image> {
    let mut a = Canvas::new(300, 300, 1.0);
    match n {
        0 => {
            // pôr do sol
            a.gradient_round_rect(0.0, 0.0, 300.0, 300.0, 0.0, &[(0.0, Rgba::rgb(58, 24, 112)), (0.55, Rgba::rgb(214, 64, 120)), (1.0, Rgba::rgb(255, 140, 66))], 0.0, 300.0);
            a.fill_circle(150.0, 170.0, 78.0, Rgba::rgb(255, 210, 92));
            for k in 0..6 {
                let y = 190.0 + k as f32 * 18.0;
                a.fill_round_rect(0.0, y, 300.0, y + 6.0 + k as f32, 0.0, Rgba::rgb(58, 24, 112));
            }
        }
        1 => {
            // mar à noite
            a.gradient_round_rect(0.0, 0.0, 300.0, 300.0, 0.0, &[(0.0, Rgba::rgb(10, 34, 92)), (0.6, Rgba::rgb(16, 120, 170)), (1.0, Rgba::rgb(64, 214, 200))], 0.0, 300.0);
            a.fill_circle(210.0, 80.0, 38.0, Rgba::rgb(236, 244, 255));
            for k in 0..4 {
                let y = 190.0 + k as f32 * 30.0;
                for i in 0..4 {
                    a.fill_circle(40.0 + i as f32 * 80.0, y + 60.0, 52.0, Rgba(255, 255, 255, 26 + 10 * k as u8));
                }
            }
        }
        2 => {
            // floresta com vaga-lumes
            a.gradient_round_rect(0.0, 0.0, 300.0, 300.0, 0.0, &[(0.0, Rgba::rgb(8, 40, 30)), (1.0, Rgba::rgb(30, 120, 70))], 0.0, 300.0);
            for (x, y, r) in [(40.0, 280.0, 110.0), (180.0, 300.0, 130.0), (290.0, 260.0, 90.0)] {
                a.fill_circle(x, y, r, Rgba::rgb(10, 56, 36));
            }
            for (x, y) in [(60.0, 70.0), (120.0, 120.0), (220.0, 60.0), (250.0, 150.0), (90.0, 180.0), (170.0, 90.0), (200.0, 200.0)] {
                a.fill_circle(x, y, 14.0, Rgba(255, 236, 90, 70));
                a.fill_circle(x, y, 5.0, Rgba::rgb(255, 240, 120));
            }
        }
        _ => {
            // rosa dos ventos
            a.gradient_round_rect(0.0, 0.0, 300.0, 300.0, 0.0, &[(0.0, Rgba::rgb(255, 120, 160)), (1.0, Rgba::rgb(190, 24, 70))], 0.0, 300.0);
            a.stroke_circle(150.0, 150.0, 92.0, WHITE.alpha(0.85), 5.0);
            a.stroke_circle(150.0, 150.0, 60.0, WHITE.alpha(0.5), 3.0);
            a.line(&[(150.0, 40.0), (150.0, 260.0)], WHITE.alpha(0.9), 5.0);
            a.line(&[(40.0, 150.0), (260.0, 150.0)], WHITE.alpha(0.9), 5.0);
            a.fill_circle(150.0, 150.0, 14.0, Rgba::rgb(255, 214, 160));
        }
    }
    Arc::new(a.finish())
}

const SONGS: [(&str, &str, f64, f64); 4] = [
    ("Sunset Drive", "Neon Avenue  ·  Night Shift", 96.0, 231.0),
    ("Maré Alta", "Lua Serena  ·  Litoral", 41.0, 198.0),
    ("Floresta Elétrica", "Os Vaga-lumes  ·  Verde Neon", 128.0, 244.0),
    ("Rosa dos Ventos", "Clara Duarte  ·  Bússola", 63.0, 207.0),
];

fn song(n: usize, extra: f64) -> Media {
    let (title, artist, pos, dur) = SONGS[n];
    Media::sample_song("spotify", title, artist, cover(n), &format!("capa{n}"), (pos + extra).min(dur), dur)
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 }
}

#[test]
#[ignore]
fn readme() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    let cfg = empty_cfg();

    // ---------- área de trabalho ----------
    {
        let gap = (WIDGET_GAP * S).round() as i32;
        let (w, top) = (1260, 42);
        let right = w - 40 - (320.0 * S) as i32;
        let left = right - gap - (320.0 * S) as i32;
        let column = |x: i32, cards: Vec<(Box<dyn Kind>, Mode, Value)>| {
            let mut y = top;
            let mut out = Vec::new();
            for (kind, mode, c) in cards {
                let (img, radius) = content(kind.as_ref(), mode, &c);
                let h = img.h as i32;
                out.push(Item { img, radius, x, y });
                y += h + gap;
            }
            (out, y)
        };
        let (mut items, y1) = column(
            right,
            vec![(Box::new(Tempo::sample("inicio", 0, 0.0, Mode::G)), Mode::G, cfg.clone()), (Box::new(System::sample()), Mode::M, cfg.clone())],
        );
        let (more, y2) = column(
            left,
            vec![
                (Box::new(GitHub::sample(&cfg, &json!({}))), Mode::G, cfg.clone()),
                (Box::new(song(0, 0.0)), Mode::M, cfg.clone()),
                (Box::new(Pomodoro::new(&pomodoro_cfg())), Mode::M, pomodoro_cfg()),
            ],
        );
        items.extend(more);
        let h = y1.max(y2) - gap + 42;
        let wall = wallpaper(w as u32, h as u32);
        let bd = Backdrop::from_picture(&wall, BLUR, S);
        save(&compose(&wall, &bd, &items, &[]), &docs().join("desktop.png"));
    }

    // ---------- tamanhos: pequeno, médio e grande ----------
    {
        let rows: Vec<Vec<(Box<dyn Kind>, Mode)>> = vec![
            vec![
                (Box::new(Tempo::sample("inicio", 0, 0.0, Mode::P)), Mode::P),
                (Box::new(Tempo::sample("inicio", 0, 0.0, Mode::M)), Mode::M),
                (Box::new(Tempo::sample("inicio", 0, 0.0, Mode::G)), Mode::G),
            ],
            vec![(Box::new(System::sample()), Mode::P), (Box::new(System::sample()), Mode::M), (Box::new(System::sample_flipped(None, Mode::G)), Mode::G)],
        ];
        let (margin, gap) = (60, 56);
        let mut items = Vec::new();
        let mut y = 0;
        let mut width = 0;
        for row in rows {
            let imgs: Vec<(Image, f32)> = row.iter().map(|(k, m)| content(k.as_ref(), *m, &cfg)).collect();
            let row_h = imgs.iter().map(|i| i.0.h as i32).max().unwrap() + 2 * margin;
            let mut x = margin;
            for (img, radius) in imgs {
                let (iw, ih) = (img.w as i32, img.h as i32);
                items.push(Item { img, radius, x, y: y + (row_h - ih) / 2 });
                x += iw + gap;
            }
            width = width.max(x - gap + margin);
            y += row_h;
        }
        let wall = wallpaper(width as u32, y as u32);
        let bd = Backdrop::from_picture(&wall, BLUR, S);
        save(&compose(&wall, &bd, &items, &[]), &docs().join("tamanhos.png"));
    }

    // ---------- Tempo navegável ----------
    {
        let views: [(Box<dyn Kind>, Mode, &str); 3] = [
            (Box::new(Tempo::sample("inicio", 0, 0.0, Mode::G)), Mode::G, "Início"),
            (Box::new(Tempo::sample("detalhes", 0, 0.0, Mode::G)), Mode::G, "Detalhes de agora"),
            (Box::new(Tempo::sample("dia3", 0, 0.0, Mode::G)), Mode::G, "Um dia da semana"),
        ];
        let (margin, gap, label_h) = (48, 44, 40);
        let mut items = Vec::new();
        let mut labels = Vec::new();
        let mut x = margin;
        let mut h = 0;
        for (kind, mode, label) in views {
            let (img, radius) = content(kind.as_ref(), mode, &cfg);
            let (iw, ih) = (img.w as i32, img.h as i32);
            labels.push(Label { text: label.into(), x: (x + iw / 2) as f32, y: (margin + label_h - 14) as f32 });
            items.push(Item { img, radius, x, y: margin + label_h });
            x += iw + gap;
            h = h.max(margin + label_h + ih + margin);
        }
        let wall = wallpaper((x - gap + margin) as u32, h as u32);
        let bd = Backdrop::from_picture(&wall, BLUR, S);
        save(&compose(&wall, &bd, &items, &labels), &docs().join("tempo-navegavel.png"));
    }

    // ---------- Sistema: valores e processos ----------
    {
        let views: [(Box<dyn Kind>, Mode, &str); 3] = [
            (Box::new(System::sample_flipped(None, Mode::P)), Mode::P, "Pequeno"),
            (Box::new(System::sample_flipped(Some(1), Mode::P)), Mode::P, "Clique num anel"),
            (Box::new(System::sample_flipped(Some(0), Mode::M)), Mode::M, "Médio, mostrando a CPU"),
        ];
        let (margin, gap, label_h) = (48, 44, 40);
        let mut items = Vec::new();
        let mut labels = Vec::new();
        let mut x = margin;
        let mut h = 0;
        for (kind, mode, label) in views {
            let (img, radius) = content(kind.as_ref(), mode, &cfg);
            let (iw, ih) = (img.w as i32, img.h as i32);
            labels.push(Label { text: label.into(), x: (x + iw / 2) as f32, y: (margin + label_h - 14) as f32 });
            items.push(Item { img, radius, x, y: margin + label_h });
            x += iw + gap;
            h = h.max(margin + label_h + ih + margin);
        }
        let wall = wallpaper((x - gap + margin) as u32, h as u32);
        let bd = Backdrop::from_picture(&wall, BLUR, S);
        save(&compose(&wall, &bd, &items, &labels), &docs().join("sistema-processos.png"));
    }

    // ---------- animação: Tocando agora trocando de música ----------
    {
        let dir = frames_dir("musica");
        let (iw, ih) = ((320.0 * S).round() as u32, 0);
        let _ = ih;
        let probe = content(&song(0, 0.0), Mode::M, &cfg).0;
        let margin = 36;
        let (w, h) = (iw + 2 * margin, probe.h + 2 * margin);
        let wall = wallpaper(w * 3, h * 3); // um pedaço calmo do degradê
        let wall = crop(&wall, w, h, w as i32, h as i32);
        let bd = Backdrop::from_picture(&wall, BLUR, S);
        let frame = |n: usize, extra: f64| {
            let (img, radius) = content(&song(n, extra), Mode::M, &cfg);
            compose(&wall, &bd, &[Item { img, radius, x: margin as i32, y: margin as i32 }], &[])
        };
        let mut k = 0;
        let mut list = String::new();
        for n in 0..SONGS.len() {
            // tocando: a barra anda
            for step in 0..6 {
                let path = dir.join(format!("{k:03}.png"));
                gfx::save_png(&frame(n, step as f64 * 4.0), &path).unwrap();
                list.push_str(&format!("{k:03}.png 380\n"));
                k += 1;
            }
            // troca de música, como no widget: a capa e o nome trocam na hora e as cores do fundo
            // passam devagar da capa anterior para a nova
            let next = (n + 1) % SONGS.len();
            for step in 1..=9 {
                let t = step as f32 / 10.0;
                let m = song(next, 0.0).fading_from(&song(n, 24.0), Mode::M, S, t);
                let (img, radius) = content(&m, Mode::M, &cfg);
                let path = dir.join(format!("{k:03}.png"));
                gfx::save_png(&compose(&wall, &bd, &[Item { img, radius, x: margin as i32, y: margin as i32 }], &[]), &path).unwrap();
                list.push_str(&format!("{k:03}.png 60\n"));
                k += 1;
            }
        }
        std::fs::write(dir.join("frames.txt"), list).unwrap();
        println!("{}", dir.display());
    }

    // ---------- ilha flutuante: as três formas e a animação ----------
    {
        let info = crate::widgets::IslandInfo {
            title: SONGS[0].0.into(),
            artist: SONGS[0].1.into(),
            app: "Spotify".into(),
            art: Some(cover(0)),
            colors: [Rgba::rgb(255, 170, 90), Rgba::rgb(244, 96, 150)],
            playing: true,
            position: SONGS[0].2,
            duration: SONGS[0].3,
            can_prev: true,
            can_toggle: true,
            can_next: true,
            can_seek: true,
        };
        // imagem parada: parada, com o mouse em cima e aberta
        let shapes = crate::app::island::readme_shapes(&info, S);
        let labels = ["Parada", "Com o mouse em cima", "Aberta, com um clique"];
        let (margin, label_h, gap) = (40i32, 34i32, 26i32);
        let width = shapes.iter().map(|i| i.w as i32).max().unwrap() + 2 * margin;
        let height = shapes.iter().map(|i| i.h as i32 + label_h + gap).sum::<i32>() + 2 * margin - gap;
        let wall = wallpaper(width as u32, height as u32);
        let mut c = Canvas::new(width as u32, height as u32, 1.0);
        c.image(&wall, 0.0, 0.0);
        let mut y = margin;
        for (img, label) in shapes.iter().zip(labels) {
            c.text(width as f32 / 2.0, (y + label_h - 12) as f32, label, Face::Semibold, 16.0, WHITE.alpha(0.92), Anchor::MS, None);
            y += label_h;
            c.image(img, ((width - img.w as i32) / 2) as f32, y as f32);
            y += img.h as i32 + gap;
        }
        save(&c.finish(), &docs().join("ilha.png"));

        // animação: o cartão vira ilha, estica, abre, recolhe e volta para o lugar
        let dir = frames_dir("ilha");
        let (w, h) = (820u32, 400u32);
        let wall = wallpaper(w, h);
        let bd = Backdrop::from_picture(&wall, BLUR, S);
        let (card_img, radius) = content(&song(0, 0.0), Mode::M, &cfg);
        let (cx, cy) = ((w - 40 - card_img.w) as i32, (h - 40 - card_img.h) as i32);
        let card_geo = (cx as f32, cy as f32, card_img.w as f32, card_img.h as f32, radius);
        // o cartão como aparece na tela: a ilha se transforma nele (e a partir dele)
        let face = bd.glass(&card_img, radius, cx, cy, GLASS_TINT);
        let with_card = compose(&wall, &bd, &[Item { img: card_img, radius, x: cx, y: cy }], &[]);
        let frames = crate::app::island::readme_frames(&info, card_geo, &face, (w as f32 / 2.0, 14.0), S);
        let mut list = String::new();
        for (k, (pill, ms)) in frames.into_iter().enumerate() {
            let img = match pill {
                None => with_card.clone(),
                Some((img, (x, y))) => {
                    let mut c = Canvas::new(w, h, 1.0);
                    c.image(&wall, 0.0, 0.0);
                    c.image(&img, x as f32, y as f32);
                    c.finish()
                }
            };
            gfx::save_png(&img, &dir.join(format!("{k:03}.png"))).unwrap();
            list.push_str(&format!("{k:03}.png {ms}\n"));
        }
        std::fs::write(dir.join("frames.txt"), list).unwrap();
        println!("{}", dir.display());
    }

    // ---------- animação: mudando de tamanho ----------
    {
        let dir = frames_dir("tamanhos");
        let modes = [Mode::P, Mode::M, Mode::G, Mode::P];
        let imgs: Vec<(Image, f32)> = modes.iter().map(|&m| content(&Tempo::sample("inicio", 0, 0.0, m), m, &cfg)).collect();
        let margin = 40;
        let (w, h) = (imgs.iter().map(|i| i.0.w).max().unwrap() + 2 * margin, imgs.iter().map(|i| i.0.h).max().unwrap() + 2 * margin);
        let wall = wallpaper(w, h);
        let bd = Backdrop::from_picture(&wall, BLUR, S);
        let mut k = 0;
        let mut list = String::new();
        let mut push = |img: Image, ms: u32, list: &mut String| {
            gfx::save_png(&img, &dir.join(format!("{k:03}.png"))).unwrap();
            list.push_str(&format!("{k:03}.png {ms}\n"));
            k += 1;
        };
        for pair in imgs.windows(2) {
            let ((old, r0), (new, r1)) = (&pair[0], &pair[1]);
            push(compose(&wall, &bd, &[Item { img: old.clone(), radius: *r0, x: margin as i32, y: margin as i32 }], &[]), 1300, &mut list);
            // como o cartão faz: cresce preso no canto, um desenho sumindo e o outro aparecendo
            for step in 1..=10 {
                let t = step as f32 / 10.0;
                let e = ease_in_out(t);
                let cw = (old.w as f32 + (new.w as f32 - old.w as f32) * e).round() as u32;
                let ch = (old.h as f32 + (new.h as f32 - old.h as f32) * e).round() as u32;
                let place = |card: &Image| {
                    let mut layer = Image::new(cw, ch);
                    layer.over(card, 0, 0);
                    layer
                };
                let fade = ease_in_out(((t - 0.1) / 0.7).clamp(0.0, 1.0));
                let layer = Image::blend(&place(old), &place(new), fade);
                let radius = r0 + (r1 - r0) * e;
                push(compose(&wall, &bd, &[Item { img: layer, radius, x: margin as i32, y: margin as i32 }], &[]), 30, &mut list);
            }
        }
        std::fs::write(dir.join("frames.txt"), list).unwrap();
        println!("{}", dir.display());
    }
}

/// Recorte w×h de uma imagem a partir de (x, y).
fn crop(img: &Image, w: u32, h: u32, x: i32, y: i32) -> Image {
    let mut out = Image::new(w, h);
    out.over(img, -x, -y);
    out
}

/// Folha de conferência: os cinco widgets nos três tamanhos (e algumas vistas extras), lado a
/// lado e alinhados pelo topo, para ver de uma vez se todos têm as mesmas medidas e se nada
/// fica cortado. Vai para %TEMP%\widgets-tamanhos.png:
///
///   cargo test sizes_sheet -- --ignored --nocapture
#[test]
#[ignore]
fn sizes_sheet() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    let cfg = empty_cfg();
    let pomodoro = pomodoro_cfg();
    type Make = Box<dyn Fn(Mode) -> (Box<dyn Kind>, Value)>;
    let rows: Vec<(&str, Make)> = vec![
        ("Tempo", Box::new(|m| (Box::new(Tempo::sample("inicio", 0, 0.0, m)) as Box<dyn Kind>, empty_cfg()))),
        ("Tempo: detalhes", Box::new(|m| (Box::new(Tempo::sample("detalhes", 0, 0.0, m)) as Box<dyn Kind>, empty_cfg()))),
        ("Tempo: um dia", Box::new(|m| (Box::new(Tempo::sample("dia3", 0, 0.0, m)) as Box<dyn Kind>, empty_cfg()))),
        ("Sistema", Box::new(|m| (Box::new(System::sample_flipped(None, m)) as Box<dyn Kind>, empty_cfg()))),
        ("Sistema: virado", Box::new(|m| (Box::new(System::sample_flipped(Some(1), m)) as Box<dyn Kind>, empty_cfg()))),
        ("Tocando agora", Box::new(|m| (Box::new(song(0, 0.0)) as Box<dyn Kind>, empty_cfg()))),
        ("Tocando agora: nada", Box::new(|_| (Box::new(Media::sample_idle()) as Box<dyn Kind>, empty_cfg()))),
        ("GitHub", Box::new(|_| (Box::new(GitHub::sample(&empty_cfg(), &json!({}))) as Box<dyn Kind>, empty_cfg()))),
        ("Pomodoro", Box::new(move |_| (Box::new(Pomodoro::new(&pomodoro)) as Box<dyn Kind>, pomodoro.clone()))),
    ];
    let (margin, gap, label_w) = (40, 40, 220);
    let mut items = Vec::new();
    let mut labels = Vec::new();
    let mut y = margin;
    let mut width = 0;
    for (name, make) in &rows {
        let mut x = margin + label_w;
        let mut row_h = 0;
        for mode in Mode::ALL {
            let (kind, c) = make(mode);
            let (img, radius) = content(kind.as_ref(), mode, &c);
            let (iw, ih) = (img.w as i32, img.h as i32);
            items.push(Item { img, radius, x, y });
            x += iw + gap;
            row_h = row_h.max(ih);
        }
        labels.push(Label { text: name.to_string(), x: (margin + label_w / 2) as f32, y: (y + 24) as f32 });
        width = width.max(x - gap + margin);
        y += row_h + gap;
    }
    let _ = cfg;
    let wall = wallpaper(width as u32, (y - gap + margin) as u32);
    let bd = Backdrop::from_picture(&wall, BLUR, S);
    save(&compose(&wall, &bd, &items, &labels), &std::env::temp_dir().join("widgets-tamanhos.png"));
}

/// O widget "Uso do Claude" nos três tamanhos (uso normal, perto do limite e ainda sem
/// conectar), cada caso numa imagem: grande à esquerda, médio e pequeno à direita.
///   MOCK_OUT=pasta cargo test claude_widget_images -- --ignored
#[test]
#[ignore]
fn claude_widget_images() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    use crate::widgets::claude::Claude;
    let out = std::env::var("MOCK_OUT").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    std::fs::create_dir_all(&out).unwrap();
    let cfg = empty_cfg();
    let gap = (WIDGET_GAP * S).round() as i32;
    let cases = [("claude", Claude::sample as fn() -> Claude), ("claude_limite", Claude::sample_limit), ("claude_conectar", Claude::sample_unlinked)];
    for (name, make) in cases {
        let (g, gr) = content(&make(), Mode::G, &cfg);
        let (m, mr) = content(&make(), Mode::M, &cfg);
        let (p, pr) = content(&make(), Mode::P, &cfg);
        let x2 = 40 + g.w as i32 + gap;
        let (gh, mh, ph, mw) = (g.h as i32, m.h as i32, p.h as i32, m.w as i32);
        let (w, h) = ((x2 + mw + 40) as u32, (40 + gh.max(mh + gap + ph) + 40) as u32);
        let items = vec![Item { img: g, radius: gr, x: 40, y: 40 }, Item { img: m, radius: mr, x: x2, y: 40 }, Item { img: p, radius: pr, x: x2, y: 40 + mh + gap }];
        let wall = wallpaper(w, h);
        let bd = Backdrop::from_picture(&wall, BLUR, S);
        let path = out.join(format!("{name}.png"));
        gfx::save_png(&compose(&wall, &bd, &items, &[]), &path).unwrap();
        println!("{}", path.display());
    }
}

