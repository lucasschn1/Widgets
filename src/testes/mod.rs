//! Testes de integração dentro do app: os widgets inteiros (dados de exemplo, desenho e
//! cliques), sem janela. Pegam o tipo de erro que mais quebra o app na mão do usuário: um
//! desenho ou um clique que entra em pânico num tamanho ou num estado que ninguém abriu.
//!
//!   cargo test testes
//!
//! Na pasta: `perf` (a bancada de desempenho) e `readme_images` (as imagens do README).

pub mod perf;
pub mod readme_images;

use serde_json::{json, Value};

use crate::gfx::{Canvas, Image};
use crate::widgets::battery::Battery;
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
    json!({"tempo": {}, "sistema": {}, "midia": {}, "github": {}, "pomodoro": {}, "claude": {}, "bateria": {}, "app": {}})
}

fn pomodoro_cfg(fase: &str, running: bool) -> Value {
    let estado = if running {
        json!({"fase": fase, "ciclo": 2, "total": 1500.0, "fim": crate::platform::win::now() + 600.0})
    } else {
        json!({"fase": fase, "ciclo": 1, "total": 300.0, "restante": 120.0})
    };
    json!({"pomodoro": {"estado": estado}, "app": {}})
}

/// Cada widget em cada estado que dá para montar sem rede nem janela: (nome, widget, opções).
pub(crate) fn cases() -> Vec<(String, Box<dyn Kind>, Value)> {
    let cfg = empty_cfg();
    let mut out: Vec<(String, Box<dyn Kind>, Value)> = Vec::new();
    for mode in Mode::ALL {
        let m = mode.code();
        out.push((format!("tempo início {m}"), Box::new(Tempo::sample("inicio", 0, 0.0, mode)), cfg.clone()));
        out.push((format!("tempo rolado {m}"), Box::new(Tempo::sample("inicio", 0, 9.5, mode)), cfg.clone()));
        out.push((format!("tempo chovendo {m}"), Box::new(Tempo::sample_raining(3.0, mode)), cfg.clone()));
        for at in [0.3, 0.7] {
            out.push((format!("tempo temperatura rolando {at} {m}"), Box::new(Tempo::sample_temp_rolling(at, mode)), cfg.clone()));
        }
        // cada clima do app Tempo da Apple, em horas diferentes (a trovoada com um raio caindo)
        for (name, code, hour, windy) in [
            ("céu limpo de madrugada", 0, 3.0, false),
            ("nascer do sol", 1, 6.0, false),
            ("pôr do sol", 0, 18.6, false),
            ("poucas nuvens", 2, 9.0, false),
            ("nublado", 3, 11.0, false),
            ("nublado de noite", 3, 23.0, false),
            ("neblina", 45, 8.0, false),
            ("garoa", 53, 21.0, false),
            ("chuva forte", 65, 16.0, false),
            ("trovoada", 95, 19.0, false),
            ("granizo", 99, 15.0, false),
            ("neve", 73, 10.0, false),
            ("ventania", 1, 12.0, true),
        ] {
            out.push((format!("tempo {name} {m}"), Box::new(Tempo::sample_sky(code, hour, windy, mode)), cfg.clone()));
        }
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
    out.push(("tempo buscando pela primeira vez".into(), Box::new(Tempo::sample_loading()), cfg.clone()));
    out.push(("sistema".into(), Box::new(System::sample()), cfg.clone()));
    for which in [None, Some(1)] {
        for mode in Mode::ALL {
            let m = mode.code();
            out.push((format!("sistema com ícones {which:?} {m}"), Box::new(System::sample_with_icons(which, mode)), cfg.clone()));
        }
    }
    out.push(("sistema no limite".into(), Box::new(System::sample_values([1.0, 1.0, 1.0])), cfg.clone()));
    out.push(("sistema zerado".into(), Box::new(System::sample_values([0.0, 0.0, 0.0])), cfg.clone()));
    out.push(("sistema com a CPU alta".into(), Box::new(System::sample_alarm()), cfg.clone()));
    out.push(("música".into(), Box::new(Media::sample()), cfg.clone()));
    out.push(("música parada".into(), Box::new(Media::sample_idle()), cfg.clone()));
    out.push(("música com capa quadrada".into(), Box::new(Media::sample_art("spotify", 300, 300)), cfg.clone()));
    out.push(("vídeo com miniatura do navegador".into(), Box::new(Media::sample_art("youtube", 150, 84)), cfg.clone()));
    out.push(("vídeo com miniatura em alta".into(), Box::new(Media::sample_art("youtube", 1280, 720)), cfg.clone()));
    out.push(("github".into(), Box::new(GitHub::sample(&cfg, &json!({}))), cfg.clone()));
    out.push(("github sem usuário".into(), Box::new(GitHub::new(&cfg, &json!({}))), cfg.clone()));
    out.push(("github buscando pela primeira vez".into(), Box::new(GitHub::sample_loading(&cfg)), cfg.clone()));
    out.push(("github com revisões".into(), Box::new(GitHub::sample_reviews(&cfg)), cfg.clone()));
    for fase in ["foco", "pausa", "longa"] {
        for running in [true, false] {
            let c = pomodoro_cfg(fase, running);
            out.push((format!("pomodoro {fase} {}", if running { "rodando" } else { "pausado" }), Box::new(Pomodoro::new(&c)), c));
        }
    }
    out.push(("pomodoro novo".into(), Box::new(Pomodoro::new(&cfg)), cfg.clone()));
    for at in [0.0, 0.4, 0.8] {
        let c = pomodoro_cfg("foco", false);
        out.push((format!("pomodoro rolando {at}"), Box::new(Pomodoro::sample_rolling(&c, at)), c));
    }
    for ms in [300, 450] {
        out.push((format!("sistema contando {ms}"), Box::new(System::sample_counting(ms)), cfg.clone()));
    }
    out.push(("claude".into(), Box::new(Claude::sample()), cfg.clone()));
    out.push(("claude no limite".into(), Box::new(Claude::sample_limit()), cfg.clone()));
    out.push(("claude sem conectar".into(), Box::new(Claude::sample_unlinked()), cfg.clone()));
    out.push(("claude lendo".into(), Box::new(Claude::sample_reading()), cfg.clone()));
    out.push(("claude no limite atingido".into(), Box::new(Claude::sample_ko()), cfg.clone()));
    out.push(("claude esperando os números".into(), Box::new(Claude::sample_waiting()), cfg.clone()));
    // o Clawd em cada animação, em várias partes dela, em cada humor
    for anim in crate::widgets::clawd::ANIMS {
        out.push((format!("clawd {anim:?}"), Box::new(Claude::sample().with_crab(anim, 0.5)), cfg.clone()));
        out.push((format!("clawd dormindo {anim:?}"), Box::new(Claude::sample_unlinked().with_crab(anim, 0.5)), cfg.clone()));
        out.push((format!("clawd suando {anim:?}"), Box::new(Claude::sample_limit().with_crab(anim, 0.3)), cfg.clone()));
        for t in [0.15, 0.5, 0.85] {
            out.push((format!("clawd trabalhando {anim:?} {t}"), Box::new(Claude::sample_busy().with_crab(anim, t)), cfg.clone()));
            out.push((format!("clawd de Sherlock {anim:?} {t}"), Box::new(Claude::sample_sherlock().with_crab(anim, t)), cfg.clone()));
            out.push((format!("clawd no limite {anim:?} {t}"), Box::new(Claude::sample_ko().with_crab(anim, t)), cfg.clone()));
        }
    }
    out.push(("claude trabalhando".into(), Box::new(Claude::sample_busy()), cfg.clone()));
    // a bateria em cada estado, e no meio de cada animação
    out.push(("bateria".into(), Box::new(Battery::sample()), cfg.clone()));
    out.push(("bateria carregando".into(), Box::new(Battery::sample_charging()), cfg.clone()));
    out.push(("bateria baixa".into(), Box::new(Battery::sample_low()), cfg.clone()));
    out.push(("bateria pc de mesa".into(), Box::new(Battery::sample_desktop()), cfg.clone()));
    out.push(("bateria só o notebook".into(), Box::new(Battery::sample_alone()), cfg.clone()));
    out.push(("bateria sem nada".into(), Box::new(Battery::sample_nothing()), cfg.clone()));
    for alert in ["chuva", "trovoada", "frio"] {
        out.push((format!("tempo aviso {alert}"), Box::new(Tempo::sample_island(alert)), cfg.clone()));
    }
    out.push(("bateria entrou na tomada".into(), Box::new(Battery::sample_plug_event(true)), cfg.clone()));
    out.push(("bateria saiu da tomada".into(), Box::new(Battery::sample_plug_event(false)), cfg.clone()));
    for ms in [0, 150, 400, 800] {
        out.push((format!("bateria ligando na tomada {ms}"), Box::new(Battery::sample_plugging(ms)), cfg.clone()));
        out.push((format!("bateria cheia {ms}"), Box::new(Battery::sample_full(ms)), cfg.clone()));
        out.push((format!("bateria aparelhos mudando {ms}"), Box::new(Battery::sample_changing(ms)), cfg.clone()));
        out.push((format!("bateria números rolando {ms}"), Box::new(Battery::sample_rolling(ms)), cfg.clone()));
        out.push((format!("bateria aparelho conectando {ms}"), Box::new(Battery::sample_connecting(ms)), cfg.clone()));
    }
    out.push(("claude de Sherlock".into(), Box::new(Claude::sample_sherlock()), cfg.clone()));
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

/// O esqueleto pulsando só aparece na primeira busca (sem nada para mostrar), e os blocos
/// ficam dentro do cartão, respeitando a margem de todos os widgets.
#[test]
fn skeletons_only_while_loading_for_the_first_time_and_inside_the_card() {
    use crate::widgets::CARD_PAD;
    // (alguns exemplos desenham imagens: precisam do COM, que nem sempre outro teste já ligou nesta
    // thread; no CI, sem ele, o Direct2D não abria)
    com();
    let mut shown = 0;
    for (name, kind, _) in cases() {
        for &mode in kind.sizes() {
            let l = Layout::of(mode);
            let blocks = kind.skeleton(mode);
            if !blocks.is_empty() {
                shown += 1;
                assert!(kind.status().loading, "{name} {mode:?}: esqueleto sem estar buscando");
            }
            for (x0, y0, x1, y1, _) in blocks {
                let inside = x0 >= CARD_PAD - 2.5 && y0 >= 8.0 && x1 <= l.w - CARD_PAD + 2.5 && y1 <= l.h - 8.0 && x0 < x1 && y0 < y1;
                assert!(inside, "{name} {mode:?}: {:?}", (x0, y0, x1, y1));
            }
        }
    }
    assert!(shown >= 6, "o Tempo e o GitHub buscando, nos três tamanhos: {shown}");
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
            // (um aviso curto, como o "Sem conexão" do GitHub no grande, ocupa uns 0,5%; buscando
            // pela primeira vez, o esqueleto pulsando é o cartão quem desenha)
            if ink(&img) < 0.002 && kind.skeleton(mode).is_empty() {
                empty.push(format!("{name} {mode:?}"));
            }
        }
    }
    assert!(empty.is_empty(), "widgets em branco: {empty:?}");
}

#[test]
fn widgets_that_draw_their_size_change_draw_every_frame() {
    com();
    let mut morphing = 0;
    for (name, kind, cfg) in cases() {
        for &from in kind.sizes() {
            for &to in kind.sizes() {
                if from == to || !kind.morphs(from, to) {
                    continue;
                }
                morphing += 1;
                let (a, b) = (Layout::of(from), Layout::of(to));
                for k in 0..=8 {
                    // o cartão no meio do caminho (com a mola, passa um pouco do tamanho novo)
                    let p = k as f32 / 8.0;
                    let e = crate::ui::theme::ease_spring(p);
                    let (w, h) = (a.w + (b.w - a.w) * e, a.h + (b.h - a.h) * e);
                    let mut c = Canvas::new((w * S).round().max(1.0) as u32, (h * S).round().max(1.0) as u32, S);
                    kind.draw_between(&mut c, from, to, p, &Env { cfg: &cfg, key: kind.key() });
                    assert!(ink(&c.finish()) > 0.002, "{name} {from:?} → {to:?} em {p}: em branco");
                }
            }
        }
    }
    assert!(morphing > 0, "nenhum widget desenha a troca de tamanho");
}

/// Os rótulos dos cabeçalhos ("GITHUB", "SISTEMA", "POMODORO", "SPOTIFY"...) têm todos a mesma
/// letra, em cor (nunca cinza); os do alto do cartão ficam todos na mesma altura (lado a lado,
/// alinhados).
#[test]
fn header_labels_look_the_same_everywhere() {
    use crate::gfx::{Face, Op};
    com();
    let labels = ["GITHUB", "SISTEMA", "POMODORO", "SPOTIFY", "YOUTUBE", "MÍDIA"];
    let mut top_base: Option<f32> = None;
    let mut seen = std::collections::HashSet::new();
    for (name, kind, cfg) in cases() {
        let env = Env { cfg: &cfg, key: kind.key() };
        for &mode in kind.sizes() {
            let _ = draw(kind.as_ref(), mode, &cfg);
            for rec in crate::card::morph::record(kind.as_ref(), mode, 1.0, 1.0, &env).ops {
                let Op::Text { base, text, face, size, color, .. } = rec.op else { continue };
                if !labels.contains(&text.as_str()) {
                    continue;
                }
                seen.insert(text.clone());
                assert!(face == Face::Semibold && size == widgets::LABEL_SIZE, "{name} {mode:?}: {text} em {face:?} {size}");
                assert!(color.0.abs_diff(color.1).max(color.1.abs_diff(color.2)) > 20, "{name} {mode:?}: {text} sem cor");
                // os que ficam no alto do cartão (o da música fica junto do texto dela)
                if base < 30.0 && !["SPOTIFY", "YOUTUBE", "MÍDIA"].contains(&text.as_str()) {
                    let first = *top_base.get_or_insert(base);
                    assert!((base - first).abs() < 0.01, "{name} {mode:?}: {text} em {base}, os outros em {first}");
                }
            }
        }
    }
    for label in ["GITHUB", "SISTEMA", "POMODORO"] {
        assert!(seen.contains(label), "nenhum caso desenhou {label}");
    }
}

/// Uma regra só para os rótulos: no médio e no grande, todo widget que tem rótulo o mostra; no
/// pequeno, nenhum (fora o aviso de um widget sem dados, que precisa dizer de quem é).
#[test]
fn labels_on_medium_and_large_never_on_small() {
    use crate::gfx::Op;
    com();
    let labels = ["GITHUB", "SISTEMA", "POMODORO", "SPOTIFY", "YOUTUBE", "MÍDIA"];
    let mut checked = 0;
    for (name, kind, cfg) in cases() {
        // as vistas de dentro (o Sistema virado) e os avisos sem dados têm o próprio título
        if !["github", "sistema", "pomodoro", "midia"].contains(&kind.key()) || name.contains("virado") || name.contains("sem usuário") {
            continue;
        }
        let env = Env { cfg: &cfg, key: kind.key() };
        for &mode in kind.sizes() {
            let _ = draw(kind.as_ref(), mode, &cfg);
            let has = crate::card::morph::record(kind.as_ref(), mode, 1.0, 1.0, &env).ops.iter().any(|rec| matches!(&rec.op, Op::Text { text, .. } if labels.contains(&text.as_str())));
            assert_eq!(has, mode != Mode::P, "{name} {mode:?}: rótulo {}", if has { "no pequeno" } else { "faltando" });
            checked += 1;
        }
    }
    assert!(checked >= 12, "poucos casos conferidos: {checked}");
}

/// O GitHub é só o mapa: em todos os tamanhos, nenhum texto grande disputa com os quadrados (o
/// total vai pequeno, em cima), e os quadrados ocupam a maior parte do cartão.
#[test]
fn github_is_just_the_map() {
    use crate::gfx::Op;
    com();
    let cfg = json!({});
    let kind = GitHub::sample(&cfg, &json!({}));
    let env = Env { cfg: &cfg, key: kind.key() };
    for &mode in kind.sizes() {
        let ops = crate::card::morph::record(&kind, mode, 1.0, 1.0, &env).ops;
        let big: Vec<_> = ops.iter().filter_map(|r| match &r.op { Op::Text { size, text, .. } if *size > 13.0 => Some(text.clone()), _ => None }).collect();
        assert!(big.is_empty(), "{mode:?}: texto grande {big:?}");
        let shapes = ops.iter().filter(|r| !matches!(r.op, Op::Text { .. })).count();
        assert!(shapes >= 50, "{mode:?}: só {shapes} quadrados"); // o pequeno: 7 semanas cheias e a de hoje
    }
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

/// Os widgets que entram na ilha: aparecem quando devem (o Pomodoro correndo, a CPU alta, um
/// pedido de revisão, um aparelho quase sem bateria) e, ali, desenham alguma coisa em todas as
/// formas e respondem ao mouse em toda a área, sem pânico. Botão da ilha é sempre uma ação do
/// próprio widget.
#[test]
fn island_widgets_draw_and_answer_in_every_form() {
    com();
    let mut on_island = Vec::new();
    for (name, kind, cfg) in cases() {
        if check_island(&name, kind.as_ref(), &cfg) {
            on_island.push(name.clone());
        }
    }
    for expected in ["pomodoro foco rodando", "sistema com a CPU alta", "github com revisões", "bateria", "bateria entrou na tomada", "bateria saiu da tomada", "tempo aviso chuva", "tempo aviso trovoada", "tempo aviso frio"] {
        assert!(on_island.iter().any(|n| n == expected), "{expected} devia estar na ilha: {on_island:?}");
    }
    for never in ["pomodoro novo", "sistema", "github", "bateria sem nada", "tempo início m", "tempo chovendo m"] {
        assert!(!on_island.iter().any(|n| n == never), "{never} não tem nada para a ilha");
    }
}

/// O mundo lá fora (o pôr do sol, a lua, a ISS, a cotação, o expediente): cada fonte, em cada
/// estado, está na ilha, desenha em todas as formas e responde ao mouse em toda a área; cada uma
/// tem nome e descrição nas Configurações.
#[test]
fn outside_feeds_draw_and_answer_in_every_form() {
    use crate::widgets::outside;
    com();
    for (name, kind, cfg) in outside::samples() {
        assert!(check_island(&name, kind.as_ref(), &cfg), "{name} devia estar na ilha");
        assert!(outside::KEYS.contains(&kind.key()) || kind.key() == outside::REST, "{name}");
    }
    // as fontes e, por último, a ilha em repouso (que não tem interruptor no mundo lá fora)
    let created: Vec<&str> = outside::create_all(&json!({})).iter().map(|k| k.key()).collect();
    assert_eq!(created, [outside::KEYS.as_slice(), &[outside::REST]].concat());
    assert_eq!(widgets::title(outside::REST), "Ilha em repouso");
    for key in outside::KEYS {
        assert!(!outside::title(key).is_empty() && !outside::description(key).is_empty(), "{key}");
        assert_eq!(widgets::title(key), outside::title(key));
        assert!(!widgets::KEYS.contains(&key), "{key} não é um widget da área de trabalho");
    }
}

/// O que `kind` mostra na ilha (se mostra algo): desenha em todas as formas e responde ao mouse
/// em toda a área de cada uma. Devolve se ele está na ilha.
fn check_island(name: &str, kind: &dyn Kind, cfg: &Value) -> bool {
    use crate::widgets::island_ui;
    use crate::widgets::IslandForm;
    let forms = [IslandForm::Bubble, IslandForm::Compact, IslandForm::Banner, IslandForm::Medium, IslandForm::Large];
    let env = Env { cfg, key: kind.key() };
    let Some(item) = kind.island_item(&env) else { return false };
    assert!(item.alert || !item.urgent, "{name}: só aviso é urgente");
    for form in forms {
        let island_ui::FormSize { w, h, .. } = island_ui::size(form);
        let mut c = Canvas::new((w * S).round() as u32, (h * S).round() as u32, S);
        c.fill_round_rect(0.0, 0.0, w, h, h / 2.0, crate::ui::theme::Rgba::rgb(0, 0, 0));
        let before = ink(&{
            let mut b = Canvas::new((w * S).round() as u32, (h * S).round() as u32, S);
            b.fill_round_rect(0.0, 0.0, w, h, h / 2.0, crate::ui::theme::Rgba::rgb(0, 0, 0));
            b.finish()
        });
        kind.island_draw(&mut c, form, (0.0, 0.0, w, h), None, &env);
        let img = c.finish();
        let lit = img.px.chunks_exact(4).filter(|p| p[0] > 40 || p[1] > 40 || p[2] > 40).count();
        assert!(lit > 20, "{name} {form:?}: nada desenhado na ilha ({before})");
        let mut y = -2.0;
        while y <= h + 2.0 {
            let mut x = -2.0;
            while x <= w + 2.0 {
                let _ = kind.island_hit(form, x, y, w, h, &env);
                x += 3.0;
            }
            y += 3.0;
        }
    }
    true
}

#[test]
fn every_widget_has_its_registry_entries() {
    let cfg = empty_cfg();
    for key in widgets::KEYS {
        assert_ne!(widgets::title(key), "Widget", "{key} sem título em widgets::title");
        assert!(!widgets::description(key).is_empty(), "{key} sem descrição");
        assert_ne!(widgets::icon_name(key), "widgets", "{key} sem ícone em widgets::icon_name");
        assert_ne!(crate::ui::theme::icon(widgets::icon_name(key)), "", "{key}: ícone sem código em theme::icon");
        assert!(widgets::FIRST_LAYOUT.iter().any(|col| col.contains(key)), "{key} fora de FIRST_LAYOUT");
        let kind = widgets::sample(key, &cfg, &json!({})).unwrap_or_else(|| panic!("{key} sem exemplo em widgets::sample"));
        assert_eq!(kind.key(), *key);
        assert_eq!(kind.title(), widgets::title(key));
        assert!(!kind.sizes().is_empty());
        assert!(kind.sizes().contains(&kind.default_mode()), "{key}: o tamanho padrão não está entre os tamanhos");
    }
}

/// Numa pilha, a roda troca de widget, menos onde ele mesmo usa a roda: as horas do Tempo, a
/// lista do GitHub, o volume do Tocando agora e o anel do Pomodoro. Nenhum widget quebra ao ser
/// perguntado em qualquer ponto.
#[test]
fn widgets_say_where_they_use_the_wheel() {
    com();
    for (_, kind, _) in cases() {
        for &mode in kind.sizes() {
            let l = Layout::of(mode);
            for k in 0..=20 {
                let _ = kind.uses_wheel(l.w * k as f32 / 20.0, l.h * k as f32 / 20.0, mode);
            }
        }
    }
    let tempo = Tempo::sample("inicio", 0, 0.0, Mode::M);
    assert!(tempo.uses_wheel(160.0, 110.0, Mode::M), "em cima das horas");
    assert!(!tempo.uses_wheel(160.0, 30.0, Mode::M), "na temperatura");
    assert!(!tempo.uses_wheel(77.0, 77.0, Mode::P), "o pequeno não rola");
    let cfg = empty_cfg();
    let pomodoro = Pomodoro::new(&cfg);
    assert!(pomodoro.uses_wheel(77.0, 62.0, Mode::P), "no anel");
    assert!(!pomodoro.uses_wheel(4.0, 150.0, Mode::P), "no canto");
    let github = GitHub::sample(&cfg, &json!({}));
    assert!(!github.uses_wheel(160.0, 300.0, Mode::G), "o GitHub não tem mais lista para rolar");
    assert!(!Media::sample().uses_wheel(160.0, 100.0, Mode::M), "o volume pela roda só na barra do grande");
}

/// Todos os widgets, em todos os estados, trocam de tamanho peça a peça sem quebrar; e o último
/// quadro da troca é o desenho do tamanho novo (nada a mais, nada faltando).
#[test]
fn every_widget_morphs_between_every_size() {
    com();
    for (name, kind, cfg) in cases() {
        let env = Env { cfg: &cfg, key: kind.key() };
        for &from in kind.sizes() {
            for &to in kind.sizes().iter().filter(|&&m| m != from) {
                let (a, b) = (crate::card::morph::record(kind.as_ref(), from, S, 1.0, &env), crate::card::morph::record(kind.as_ref(), to, S, 1.0, &env));
                for p in [0.5] {
                    let l = Layout::of(if p < 0.5 { from } else { to });
                    let mut c = Canvas::new((l.w * S).round() as u32, (l.h * S).round() as u32, S);
                    crate::card::morph::draw(&mut c, &a, &b, p);
                    let _ = c.finish();
                }
                // no fim, igual ao desenho de sempre do tamanho novo
                let l = Layout::of(to);
                let mut c = Canvas::new((l.w * S).round() as u32, (l.h * S).round() as u32, S);
                crate::card::morph::draw(&mut c, &a, &b, 1.0);
                let end = c.finish();
                let mut c = Canvas::new((l.w * S).round() as u32, (l.h * S).round() as u32, S);
                kind.draw(&mut c, to, &env);
                let normal = c.finish();
                let diff = end.px.iter().zip(&normal.px).filter(|(x, y)| (**x as i32 - **y as i32).abs() > 24).count();
                assert!(diff * 100 <= end.px.len(), "{name} {from:?} → {to:?}: o fim da troca difere do desenho ({diff} de {})", end.px.len());
            }
        }
    }
}

/// Um widget com fundo próprio cobrindo o cartão (a capa viva da Música) continua coberto durante
/// toda a troca de tamanho: o fundo cresce junto com o cartão, em vez de sumir e reaparecer
/// esmaecendo (o que deixava o cartão transparente no meio da troca).
#[test]
fn a_covered_card_stays_covered_while_changing_size() {
    use crate::ui::theme::ease_spring;
    com();
    // os pixels quase transparentes do miolo do cartão (longe dos cantos arredondados)
    let holes = |img: &crate::gfx::Image| {
        let inset = (32.0 * S) as u32;
        let mut n = 0;
        for y in inset..img.h.saturating_sub(inset) {
            for x in inset..img.w.saturating_sub(inset) {
                if img.px[((y * img.w + x) * 4 + 3) as usize] < 230 {
                    n += 1;
                }
            }
        }
        n
    };
    // cada widget uma vez, mais todo estado com uma imagem de fundo cobrindo o cartão (todos os
    // estados do Tempo levariam tempo demais)
    let (mut seen, mut backdrops) = (std::collections::HashSet::new(), 0);
    for (name, kind, cfg) in cases() {
        let env = Env { cfg: &cfg, key: kind.key() };
        let first = seen.insert(kind.key());
        // só os tamanhos em que o widget cobre o cartão sozinho
        let covered: Vec<Mode> = kind
            .sizes()
            .iter()
            .copied()
            .filter(|&m| {
                let l = Layout::of(m);
                let mut c = Canvas::new((l.w * S).round() as u32, (l.h * S).round() as u32, S);
                kind.draw(&mut c, m, &env);
                holes(&c.finish()) == 0
            })
            .collect();
        for &from in &covered {
            for &to in covered.iter().filter(|&&m| m != from) {
                let (a, b) = (crate::card::morph::record(kind.as_ref(), from, S, 1.0, &env), crate::card::morph::record(kind.as_ref(), to, S, 1.0, &env));
                let backdrop = crate::card::morph::has_backdrop(&a) && crate::card::morph::has_backdrop(&b);
                if !first && !backdrop {
                    continue;
                }
                backdrops += backdrop as usize;
                let (la, lb) = (Layout::of(from), Layout::of(to));
                for i in 0..=10 {
                    let p = i as f32 / 10.0;
                    // o cartão do tamanho do meio do caminho, como em `Card::anim_step`
                    let e = ease_spring(p);
                    let (w, h) = (la.w + (lb.w - la.w) * e, la.h + (lb.h - la.h) * e);
                    let mut c = Canvas::new((w * S).round() as u32, (h * S).round() as u32, S);
                    crate::card::morph::draw(&mut c, &a, &b, p);
                    let n = holes(&c.finish());
                    assert_eq!(n, 0, "{name} {from:?} → {to:?} a {p}: {n} pixels transparentes no meio do cartão");
                }
            }
        }
    }
    assert!(backdrops > 0, "nenhum widget com imagem de fundo para conferir (a capa viva da Música)");
}

/// Cada widget trocando de tamanho (pequeno → médio → grande → pequeno), em quadros, num PNG
/// por widget na pasta temporária (para conferir de olho as trocas peça a peça).
///
///   cargo test --release testes::morph_sheets -- --ignored --nocapture
#[test]
#[ignore]
fn morph_sheets() {
    use crate::ui::theme::{ease_spring, Rgba};
    com();
    let cfg = empty_cfg();
    let kinds: Vec<(&str, Box<dyn Kind>, Value)> = vec![
        ("tempo", Box::new(Tempo::sample("inicio", 0, 0.0, Mode::M)), cfg.clone()),
        ("sistema", Box::new(System::sample()), cfg.clone()),
        ("midia", Box::new(Media::sample()), cfg.clone()),
        ("github", Box::new(GitHub::sample(&cfg, &json!({}))), cfg.clone()),
        ("pomodoro", Box::new(Pomodoro::new(&pomodoro_cfg("foco", true))), pomodoro_cfg("foco", true)),
        ("bateria", Box::new(Battery::sample()), cfg.clone()),
    ];
    let steps = [0.0, 0.15, 0.3, 0.45, 0.6, 0.8, 1.0];
    let trips = [(Mode::P, Mode::M), (Mode::M, Mode::G), (Mode::G, Mode::P)];
    let s = 1.0;
    let (cell_w, cell_h) = (widgets::WIDE + 16.0, widgets::LARGE_H + 16.0);
    for (name, kind, cfg) in kinds {
        let env = Env { cfg: &cfg, key: kind.key() };
        let mut sheet = Canvas::new((cell_w * steps.len() as f32) as u32, (cell_h * trips.len() as f32) as u32, s);
        sheet.fill_round_rect(0.0, 0.0, cell_w * steps.len() as f32, cell_h * trips.len() as f32, 0.0, Rgba::rgb(60, 52, 96));
        for (row, &(from, to)) in trips.iter().enumerate() {
            let (a, b) = (crate::card::morph::record(kind.as_ref(), from, s, 1.0, &env), crate::card::morph::record(kind.as_ref(), to, s, 1.0, &env));
            for (col, &p) in steps.iter().enumerate() {
                let e = ease_spring(p);
                let (la, lb) = (Layout::of(from), Layout::of(to));
                let (w, h) = (la.w + (lb.w - la.w) * e, la.h + (lb.h - la.h) * e);
                let mut c = Canvas::new(w.round().max(1.0) as u32, h.round().max(1.0) as u32, s);
                c.fill_round_rect(0.0, 0.0, w, h, la.radius + (lb.radius - la.radius) * e, Rgba(30, 30, 40, 235));
                crate::card::morph::draw(&mut c, &a, &b, p);
                sheet.image(&c.finish(), col as f32 * cell_w + 8.0, row as f32 * cell_h + 8.0);
            }
        }
        let path = std::env::temp_dir().join(format!("troca_{name}.png"));
        crate::gfx::save_png(&sheet.finish(), &path).unwrap();
        println!("{}", path.display());
    }
}

// TEMPORÁRIO (legibilidade): impressão digital de todo desenho, para comparar antes e depois.
#[test]
#[ignore]
fn zz_fingerprint() {
    use crate::widgets::{island_ui, IslandForm};
    com();
    fn hash(img: &Image) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in img.px.iter().chain(img.w.to_le_bytes().iter()).chain(img.h.to_le_bytes().iter()) {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
    let mut out = String::new();
    let all: Vec<(String, Box<dyn Kind>, Value)> = cases().into_iter().chain(crate::widgets::outside::samples()).collect();
    for (name, kind, cfg) in &all {
        let env = Env { cfg, key: kind.key() };
        for &mode in kind.sizes() {
            out += &format!("{name} {:?} {:x}\n", mode, hash(&draw(kind.as_ref(), mode, cfg)));
        }
        let sizes = kind.sizes();
        if sizes.len() > 1 {
            let (a, b) = (crate::card::morph::record(kind.as_ref(), sizes[0], S, 1.0, &env), crate::card::morph::record(kind.as_ref(), sizes[1], S, 1.0, &env));
            for p in [0.3, 0.6] {
                let l = Layout::of(sizes[1]);
                let mut c = Canvas::new((l.w * S).round() as u32, (l.h * S).round() as u32, S);
                crate::card::morph::draw(&mut c, &a, &b, p);
                out += &format!("{name} morph {p} {:x}\n", hash(&c.finish()));
            }
        }
        if kind.island_item(&env).is_some() {
            for form in [IslandForm::Bubble, IslandForm::Compact, IslandForm::Banner, IslandForm::Medium, IslandForm::Large] {
                let island_ui::FormSize { w, h, .. } = island_ui::size(form);
                let mut c = Canvas::new((w * S).round() as u32, (h * S).round() as u32, S);
                kind.island_draw(&mut c, form, (0.0, 0.0, w, h), None, &env);
                out += &format!("{name} ilha {form:?} {:x}\n", hash(&c.finish()));
            }
        }
    }
    // vidro sobre uma imagem qualquer
    let mut pic = Canvas::new(400, 300, 1.0);
    pic.fill_round_rect(0.0, 0.0, 400.0, 300.0, 0.0, crate::ui::theme::Rgba::rgb(40, 90, 160));
    pic.fill_round_rect(60.0, 40.0, 200.0, 150.0, 30.0, crate::ui::theme::Rgba::rgb(220, 120, 40));
    let pic = pic.finish();
    let bd = crate::card::backdrop::Backdrop::from_picture(&pic, 20.0, S);
    let content = draw(all[0].1.as_ref(), Mode::M, &all[0].2);
    out += &format!("vidro {:x}\n", hash(&bd.glass(&content, 22.0, 30, 20, 0.5)));
    let base = bd.glass_base(content.w, content.h, 30, 20, 0.5);
    out += &format!("vidro base {:x}\n", hash(&base));
    out += &format!("vidro fim {:x}\n", hash(&crate::card::backdrop::finish_glass(&base, &content, 22.0, S)));
    out += &format!("vidro partes {:x}\n", hash(&crate::card::backdrop::GlassParts::new(&base, 22.0, S).compose(&content)));
    // o menu de vidro
    {
        use crate::widgets::MenuItem;
        let items = vec![
            MenuItem::Title("GitHub".into(), "github"),
            MenuItem::Primary("Organizar widgets".into(), 1, "organizar"),
            MenuItem::Sep,
            MenuItem::Sub("Tamanho".into(), vec![MenuItem::Radio("Médio".into(), true, 2)]).icon("tamanho"),
            MenuItem::Cmd("Atualizar".into(), 3),
            MenuItem::Check("Travar posições".into(), true, 5).icon("lock"),
            MenuItem::Check("Sem ícone".into(), true, 6),
            MenuItem::Radio("Rádio".into(), true, 9),
            MenuItem::Hint("Com atalho".into(), 7, "Ctrl+K".into()),
        ];
        for (hot, s) in [(None, 1.0), (Some(1), 1.5), (Some(4), 1.25)] {
            out += &format!("menu {hot:?} {s} {:x}
", hash(&crate::ui::popup::menu_image(&items, hot, s)));
        }
    }
    let path = std::env::var("FINGERPRINT").unwrap_or_else(|_| "fingerprint.txt".into());
    std::fs::write(path, out).unwrap();
}
