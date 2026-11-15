//! Bancada: quanto custa desenhar, etapa por etapa (para otimizar com números, não no chute). Os
//! números do app aberto, no uso de verdade, vêm do medidor (`meter`, com `WIDGETS_PERF=1`).
//!
//!   cargo test --release perf::perf -- --ignored --nocapture --exact        cada widget e o vidro
//!   cargo test --release perf::gpu_frame -- --ignored --nocapture --exact   um quadro na placa de vídeo
//!   cargo test --release perf::clip_snapshot -- --ignored --exact           "foto" dos recortes
//!
//! Os tempos aqui são com o processador acelerado (um quadro atrás do outro); no app, com um
//! quadro a cada tantos milissegundos, o mesmo trabalho custa umas duas vezes mais (o
//! `gpu_frame` mede também nesse ritmo). Vale comparar um número com outro da mesma rodada.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::backdrop::{finish_glass, Backdrop, GlassParts};
use crate::gfx::{Canvas, Image, Kept};
use crate::meter::{cycles_per_ms, thread_cycles as cycles};
use crate::widgets::{Env, Kind, Layout, Mode};

const S: f32 = 1.25;
const RUNS: u32 = 8;

fn com() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
}

/// Um papel de parede de mentira, com cor e detalhe (o desfoque precisa de algo para desfocar).
fn wallpaper() -> Image {
    let (w, h) = (1920u32, 1080u32);
    let mut img = Image::new(w, h);
    for (i, p) in img.px.chunks_exact_mut(4).enumerate() {
        let (x, y) = (i as u32 % w, i as u32 / w);
        let stripe = if (x / 40 + y / 40) % 2 == 0 { 30 } else { 0 };
        p.copy_from_slice(&[(x * 255 / w) as u8, ((y * 255 / h) as u8).saturating_add(stripe), 120, 255]);
    }
    img
}

/// Média em milissegundos de `runs` vezes `f`.
fn avg(runs: u32, mut f: impl FnMut()) -> f64 {
    let t = Instant::now();
    for _ in 0..runs {
        f();
    }
    t.elapsed().as_secs_f64() * 1000.0 / runs as f64
}

/// Cada widget (em todos os estados de `testes::cases`, no tamanho padrão) e o vidro em volta.
#[test]
#[ignore]
fn perf() {
    com();
    let backdrop = Backdrop::from_picture(&wallpaper(), 8.0, S);
    let gpu = crate::gpu::available();
    println!("placa de vídeo: {}", if !gpu { "indisponível" } else if crate::gpu::hardware() { "de verdade" } else { "WARP (software)" });

    // por widget: (casos, conteúdo sem a lista de textos, conteúdo, conteúdo na placa de vídeo)
    let mut sums: BTreeMap<&'static str, (u32, f64, f64, f64)> = BTreeMap::new();
    let cached = |on: bool| crate::gfx::LAYOUT_CACHE.store(on, std::sync::atomic::Ordering::Relaxed);
    for (_, kind, cfg) in crate::testes::cases() {
        let mode = kind.default_mode();
        let l = Layout::of(mode);
        let (w, h) = ((l.w * S).round() as u32, (l.h * S).round() as u32);
        let env = Env { cfg: &cfg, key: kind.key() };
        let draw = |c: &mut Canvas| {
            kind.draw(c, mode, &env);
            kind.overlay(c, mode, 0.0, 0.0);
        };
        let on_cpu = || {
            let mut c = Canvas::new(w, h, S);
            draw(&mut c);
            let _ = c.finish();
        };
        on_cpu(); // aquece (formatos de texto, imagens guardadas)
        cached(false);
        let before = avg(RUNS, on_cpu);
        cached(true);
        let now = avg(RUNS, on_cpu);
        let on_gpu = if gpu { avg(RUNS, || drop(crate::gpu::draw_offscreen(w, h, S, draw))) } else { 0.0 };
        let e = sums.entry(kind.key()).or_default();
        *e = (e.0 + 1, e.1 + before, e.2 + now, e.3 + on_gpu);
    }
    println!("\nconteúdo (o widget desenhando), média por quadro:");
    println!("{:<10} {:>6} {:>18} {:>12} {:>16}", "widget", "casos", "sem lista textos", "agora", "na placa vídeo");
    for (key, (n, before, now, on_gpu)) in sums {
        let n = n as f64;
        println!("{key:<10} {:>6} {:>15.2} ms {:>9.2} ms {:>13.2} ms", n, before / n, now / n, on_gpu / n);
    }

    // o vidro, no tamanho médio, com o conteúdo de verdade do Sistema
    let l = Layout::of(Mode::M);
    let (w, h, r) = ((l.w * S).round() as u32, (l.h * S).round() as u32, l.radius * S);
    let mut c = Canvas::new(w, h, S);
    crate::widgets::system::System::sample().draw(&mut c, Mode::M, &Env { cfg: &serde_json::json!({}), key: "sistema" });
    let content = c.finish();
    let base = backdrop.glass_base(w, h, 300, 200, 0.4);
    let parts = GlassParts::new(&base, r, S);
    let k = RUNS * 4;
    println!("\nvidro {w} × {h}:");
    println!("  fundo (recorte, escurecer, granulado): passo a passo {:.2} ms, numa passada {:.2} ms (só quando o cartão anda)",
        avg(k, || drop(backdrop.glass_base_steps(w, h, 300, 200, 0.4))), avg(k, || drop(backdrop.glass_base(w, h, 300, 200, 0.4))));
    println!("  acabamento (reflexo, conteúdo, bordas, cantos): de uma vez {:.3} ms, por peças {:.3} ms (preparar as peças: {:.3} ms, só quando o fundo muda)",
        avg(k, || drop(finish_glass(&base, &content, r, S))), avg(k, || drop(parts.compose(&content))), avg(RUNS, || drop(GlassParts::new(&base, r, S))));
    println!("  tela em memória vazia (criar e ler os pixels): {:.3} ms", avg(k, || drop(Canvas::new(w, h, S).finish())));
}

/// Um quadro na placa de vídeo: do mais simples ao do Tempo chovendo, fora da tela, numa janela
/// de verdade e no ritmo do app.
#[test]
#[ignore]
fn gpu_frame() {
    com();
    if !crate::gpu::available() {
        return;
    }
    let (w, h) = (560u32, 340u32);
    let glass = {
        let mut c = Canvas::new(w, h, 1.0);
        c.fill_round_rect(0.0, 0.0, w as f32, h as f32, 20.0, crate::theme::Rgba(40, 50, 70, 230));
        c.finish()
    };
    let kept = Kept::new(glass.clone());
    let rain = crate::widgets::tempo::Tempo::sample_sky(63, 10.0, false, Mode::M);
    let cases: [(&str, &dyn Fn(&mut Canvas)); 5] = [
        ("só limpar", &|_c| {}),
        ("vidro guardado", &|c| c.image_kept(&kept, 0.0, 0.0)),
        ("vidro enviado de novo", &|c| c.image(&glass, 0.0, 0.0)),
        ("vidro + recorte arredondado", &|c| {
            c.image_kept(&kept, 0.0, 0.0);
            c.push_round_clip(0.0, 0.0, w as f32, h as f32, 20.0);
            c.fill_circle(10.0, 10.0, 3.0, crate::theme::WHITE);
            c.pop_clip();
        }),
        ("vidro + chuva", &|c| {
            c.image_kept(&kept, 0.0, 0.0);
            rain.overlay(c, Mode::M, 0.0, 0.0);
        }),
    ];
    for (name, draw) in &cases {
        let _ = crate::gpu::draw_offscreen(w, h, 1.0, draw);
        println!("fora da tela:            {name:<30} {:.3} ms", avg(200, || drop(crate::gpu::draw_offscreen(w, h, 1.0, draw))));
    }
    // numa janela de verdade (escondida), pela cadeia de quadros e a DirectComposition; a CPU
    // pelos ciclos da thread (sem contar a espera)
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_POPUP};
    let hwnd = unsafe { CreateWindowExW(WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW, w!("STATIC"), w!(""), WS_POPUP, 0, 0, w as i32, h as i32, None, None, None, None).unwrap() };
    let mut surface = crate::gpu::Surface::new(hwnd);
    let cpu_ms = |cycles: u64, n: u32| cycles as f64 / cycles_per_ms() / n as f64;
    for (name, draw) in &cases {
        let _ = surface.present(w, h, 1.0, draw);
        let c = cycles();
        for _ in 0..200 {
            let _ = surface.present(w, h, 1.0, draw);
        }
        println!("na janela, seguidos:     {name:<30} CPU {:.3} ms", cpu_ms(cycles() - c, 200));
    }
    // no ritmo do app: um quadro a cada 35 ms (o processador desacelera entre eles)
    for (name, draw) in &cases {
        let mut spent = 0;
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(35));
            let c = cycles();
            let _ = surface.present(w, h, 1.0, draw);
            spent += cycles() - c;
        }
        println!("na janela, a cada 35 ms: {name:<30} CPU {:.3} ms", cpu_ms(spent, 40));
    }
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

/// Uma "foto" dos recortes e camadas (recorte arredondado em posições quebradas, com a tela
/// encolhida, opacidade por cima, um dentro do outro), salva em target\recortes-<RECORTE>.png:
/// para comparar byte a byte antes e depois de mexer neles.
#[test]
#[ignore]
fn clip_snapshot() {
    use crate::theme::Rgba;
    com();
    let mut c = Canvas::new(420, 260, 1.25);
    let paint = |c: &mut Canvas| {
        for i in 0..12 {
            let f = i as f32;
            c.line(&[(f * 31.0 - 40.0, -10.0), (f * 31.0 + 30.0, 230.0)], Rgba(200, 220, 255, 200), 1.3);
            c.fill_circle(f * 29.0, 15.0 + f * 16.0, 9.0, Rgba(255, 120, 60, 180));
        }
    };
    c.push_round_clip(10.3, 8.7, 200.6, 150.2, 22.5);
    paint(&mut c);
    c.push_opacity(0.6);
    c.fill_round_rect(-20.0, 60.0, 400.0, 90.0, 6.0, Rgba(40, 200, 120, 255));
    c.pop_clip();
    c.pop_clip();
    c.scale_about(250.0, 120.0, 0.93);
    c.push_round_clip(170.0, 30.0, 330.0, 200.0, 40.0);
    paint(&mut c);
    c.push_round_clip(200.0, 60.0, 300.0, 170.0, 12.0);
    c.fill_round_rect(150.0, 0.0, 400.0, 300.0, 0.0, Rgba(255, 255, 255, 90));
    c.pop_clip();
    c.pop_clip();
    c.reset_transform();
    let name = std::env::var("RECORTE").unwrap_or_else(|_| "agora".into());
    crate::gfx::save_png(&c.finish(), std::path::Path::new(&format!("target/recortes-{name}.png"))).unwrap();
}
