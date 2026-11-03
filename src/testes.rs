//! Testes de integração dentro do app: os widgets inteiros (dados de exemplo, desenho e
//! cliques), sem janela. Pegam o tipo de erro que mais quebra o app na mão do usuário: um
//! desenho ou um clique que entra em pânico num tamanho ou num estado que ninguém abriu.
//!
//!   cargo test testes

use serde_json::{json, Value};

use crate::gfx::{Canvas, Image};
use crate::widgets::claude::Claude;
use crate::widgets::github::GitHub;
use crate::widgets::media::Media;
use crate::widgets::pomodoro::Pomodoro;
use crate::widgets::system::System;
use crate::widgets::tempo::Tempo;
use crate::widgets::{self, Env, Kind, Layout, Mode};

const S: f32 = 1.25;

fn com() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
}

fn empty_cfg() -> Value {
    json!({"tempo": {}, "sistema": {}, "midia": {}, "github": {}, "pomodoro": {}, "claude": {}, "app": {}})
}

fn pomodoro_cfg(fase: &str, running: bool) -> Value {
    let estado = if running {
        json!({"fase": fase, "ciclo": 2, "total": 1500.0, "fim": crate::win::now() + 600.0})
    } else {
        json!({"fase": fase, "ciclo": 1, "total": 300.0, "restante": 120.0})
    };
    json!({"pomodoro": {"estado": estado}, "app": {}})
}

/// Cada widget em cada estado que dá para montar sem rede nem janela: (nome, widget, opções).
fn cases() -> Vec<(String, Box<dyn Kind>, Value)> {
    let cfg = empty_cfg();
    let mut out: Vec<(String, Box<dyn Kind>, Value)> = Vec::new();
    for mode in Mode::ALL {
        let m = mode.code();
        out.push((format!("tempo início {m}"), Box::new(Tempo::sample("inicio", 0, 0.0, mode)), cfg.clone()));
        out.push((format!("tempo rolado {m}"), Box::new(Tempo::sample("inicio", 0, 9.5, mode)), cfg.clone()));
        out.push((format!("tempo chovendo {m}"), Box::new(Tempo::sample_raining(3.0, mode)), cfg.clone()));
        for page in 0..4 {
            out.push((format!("tempo detalhes p{page} {m}"), Box::new(Tempo::sample("detalhes", page, 0.0, mode)), cfg.clone()));
        }
        for day in 0..7 {
            out.push((format!("tempo dia{day} {m}"), Box::new(Tempo::sample(&format!("dia{day}"), 0, 0.0, mode)), cfg.clone()));
        }
        for which in [None, Some(0), Some(1), Some(2)] {
            out.push((format!("sistema virado {which:?} {m}"), Box::new(System::sample_flipped(which, mode)), cfg.clone()));
        }
    }
    let tempo_cfg = json!({"tempo": {"city": "Florianópolis"}, "app": {}});
    out.push(("tempo sem cidade".into(), Box::new(Tempo::new(&cfg, &json!({}))), cfg.clone()));
    out.push(("tempo sem dados".into(), Box::new(Tempo::new(&tempo_cfg, &json!({}))), tempo_cfg));
    out.push(("sistema".into(), Box::new(System::sample()), cfg.clone()));
    out.push(("sistema no limite".into(), Box::new(System::sample_values([1.0, 1.0, 1.0])), cfg.clone()));
    out.push(("sistema zerado".into(), Box::new(System::sample_values([0.0, 0.0, 0.0])), cfg.clone()));
    out.push(("música".into(), Box::new(Media::sample()), cfg.clone()));
    out.push(("música parada".into(), Box::new(Media::sample_idle()), cfg.clone()));
    out.push(("github".into(), Box::new(GitHub::sample(&cfg, &json!({}))), cfg.clone()));
    out.push(("github sem usuário".into(), Box::new(GitHub::new(&cfg, &json!({}))), cfg.clone()));
    for fase in ["foco", "pausa", "longa"] {
        for running in [true, false] {
            let c = pomodoro_cfg(fase, running);
            out.push((format!("pomodoro {fase} {}", if running { "rodando" } else { "pausado" }), Box::new(Pomodoro::new(&c)), c));
        }
    }
    out.push(("pomodoro novo".into(), Box::new(Pomodoro::new(&cfg)), cfg.clone()));
    out.push(("claude".into(), Box::new(Claude::sample()), cfg.clone()));
    out.push(("claude no limite".into(), Box::new(Claude::sample_limit()), cfg.clone()));
    out.push(("claude sem conectar".into(), Box::new(Claude::sample_unlinked()), cfg.clone()));
    out.push(("claude lendo".into(), Box::new(Claude::sample_reading()), cfg.clone()));
    out.push(("claude subindo".into(), Box::new(Claude::sample_growing(0.2, 300)), cfg));
    out
}

fn draw(kind: &dyn Kind, mode: Mode, cfg: &Value) -> Image {
    let l = Layout::of(mode);
    let mut c = Canvas::new((l.w * S).round() as u32, (l.h * S).round() as u32, S);
    kind.draw(&mut c, mode, &Env { cfg, key: kind.key() });
    kind.overlay(&mut c, mode, 0.0, 0.0);
    c.finish()
}

/// Quanto da imagem tem alguma tinta (0 a 1).
fn ink(img: &Image) -> f32 {
    let painted = img.px.chunks_exact(4).filter(|p| p[3] > 8).count();
    painted as f32 / (img.w * img.h).max(1) as f32
}

#[test]
fn every_widget_draws_in_every_size() {
    com();
    let mut empty = Vec::new();
    for (name, kind, cfg) in cases() {
        for &mode in kind.sizes() {
            let img = draw(kind.as_ref(), mode, &cfg);
            let l = Layout::of(mode);
            assert_eq!((img.w, img.h), ((l.w * S).round() as u32, (l.h * S).round() as u32), "{name} {mode:?}");
            // o conteúdo vai por cima do vidro: um widget que não desenha nada está quebrado
            // (um aviso curto, como o "Carregando…" do GitHub no grande, ocupa uns 0,5%)
            if ink(&img) < 0.002 {
                empty.push(format!("{name} {mode:?}"));
            }
        }
    }
    assert!(empty.is_empty(), "widgets em branco: {empty:?}");
}

#[test]
fn every_widget_answers_the_mouse_everywhere() {
    com();
    for (name, kind, cfg) in cases() {
        let env = Env { cfg: &cfg, key: kind.key() };
        for &mode in kind.sizes() {
            // os desenhos guardam o tamanho visto por último: desenha antes, como o app faz
            let _ = draw(kind.as_ref(), mode, &cfg);
            let l = Layout::of(mode);
            let mut y = -4.0;
            while y <= l.h + 4.0 {
                let mut x = -4.0;
                while x <= l.w + 4.0 {
                    let _ = kind.hit(x, y, mode, &env);
                    let _ = kind.tooltip(x, y, mode, &env);
                    let _ = kind.hot_rect(x, y, mode, &env);
                    let _ = kind.hover_style(x, y, mode, &env);
                    x += 7.0;
                }
                y += 7.0;
            }
        }
        let _ = kind.menu(&env);
        let _ = kind.status();
        let _ = kind.island();
        let _ = name;
    }
}

#[test]
fn every_widget_has_its_registry_entries() {
    let cfg = empty_cfg();
    for key in widgets::KEYS {
        assert_ne!(widgets::title(key), "Widget", "{key} sem título em widgets::title");
        assert!(!widgets::description(key).is_empty(), "{key} sem descrição");
        assert_ne!(widgets::icon_name(key), "widgets", "{key} sem ícone em widgets::icon_name");
        assert_ne!(crate::theme::icon(widgets::icon_name(key)), "", "{key}: ícone sem código em theme::icon");
        assert!(widgets::FIRST_LAYOUT.iter().any(|col| col.contains(key)), "{key} fora de FIRST_LAYOUT");
        let kind = widgets::sample(key, &cfg, &json!({})).unwrap_or_else(|| panic!("{key} sem exemplo em widgets::sample"));
        assert_eq!(kind.key(), *key);
        assert_eq!(kind.title(), widgets::title(key));
        assert!(!kind.sizes().is_empty());
        assert!(kind.sizes().contains(&kind.default_mode()), "{key}: o tamanho padrão não está entre os tamanhos");
    }
}
