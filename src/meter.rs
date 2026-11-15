//! Medidor de desempenho do app aberto, para otimizar com números do uso de verdade (e não só os
//! da bancada, `perf`). Desligado, não custa nada; com a variável de ambiente `WIDGETS_PERF=1`,
//! soma quanto tempo cada widget gasta em cada etapa e, a cada 10 segundos, escreve o resumo em
//! %TEMP%\widgets-perf.txt (sobrescrevendo).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// As etapas medidas.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Step {
    /// o widget desenhando o conteúdo (`Card::render`)
    Content,
    /// o fundo do vidro (papel de parede desfocado, escurecido, granulado)
    Base,
    /// o vidro em volta do conteúdo (brilho, borda, cantos)
    Glass,
    /// o quadro chegando à tela (camada animada, detalhes e a cópia ou o envio para a placa)
    Show,
    /// um quadro da troca de tamanho
    Resize,
    /// a camada animada do widget (dentro de `Show`)
    Overlay,
    /// os detalhes por cima do vidro: luz da borda, destaque, botões (dentro de `Show`)
    Decorate,
    /// na placa de vídeo (dentro de `Show`): preparar o quadro de desenho, terminar o desenho
    /// (o Direct2D manda tudo para a placa) e mostrar
    Target,
    EndDraw,
    Present,
    /// uma imagem enviada para a placa de vídeo (no meio do desenho; `image_kept` só envia uma vez)
    Upload,
}

#[derive(Default)]
struct Totals {
    /// (vezes, tempo de relógio, ciclos de CPU da thread) por (widget, etapa): o tempo de relógio
    /// conta também a espera (pela placa de vídeo, pelo Windows); os ciclos, só o trabalho
    steps: BTreeMap<(&'static str, Step), (u32, Duration, u64)>,
    since: Option<Instant>,
    /// a CPU do processo e os ciclos desta thread no começo da janela
    cpu: Option<Duration>,
    cycles: u64,
}

thread_local! {
    static TOTALS: RefCell<Totals> = RefCell::new(Totals::default());
}

pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("WIDGETS_PERF").is_ok_and(|v| v == "1"))
}

/// Mede `f` como a etapa `step` do widget `key`.
pub fn time<R>(key: &'static str, step: Step, f: impl FnOnce() -> R) -> R {
    if !on() {
        return f();
    }
    let (t, c) = (Instant::now(), thread_cycles());
    let r = f();
    let (spent, cycles) = (t.elapsed(), thread_cycles() - c);
    TOTALS.with(|t| {
        let mut t = t.borrow_mut();
        let e = t.steps.entry((key, step)).or_default();
        e.0 += 1;
        e.1 += spent;
        e.2 += cycles;
    });
    r
}

/// De vez em quando (chamado pelo laço principal): escreve o resumo dos últimos 10 segundos.
pub fn flush() {
    if !on() {
        return;
    }
    TOTALS.with(|t| {
        let mut t = t.borrow_mut();
        if t.since.is_none() {
            t.since = Some(Instant::now());
            t.cpu = process_cpu();
            t.cycles = thread_cycles();
        }
        let since = t.since.unwrap();
        let window = since.elapsed();
        if window < Duration::from_secs(10) {
            return;
        }
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let mut out = format!("janela de {:.1} s\n", window.as_secs_f64());
        let cpu_now = process_cpu();
        if let (Some(a), Some(b)) = (t.cpu, cpu_now) {
            out += &format!("CPU do processo inteiro: {:.1}% de um núcleo\n", ms(b.saturating_sub(a)) / ms(window) * 100.0);
        }
        // ciclos por milissegundo (o contador de ciclos anda no ritmo fixo do relógio do processador)
        let hz = cycles_per_ms();
        let cyc_now = thread_cycles();
        let pct = |cycles: u64| cycles as f64 / hz / ms(window) * 100.0;
        out += &format!("CPU da thread da interface: {:.1}% de um núcleo\n", pct(cyc_now - t.cycles));
        // (a camada animada e os detalhes estão dentro de Show: não somam de novo)
        let nested = |s: &Step| matches!(s, Step::Overlay | Step::Decorate);
        let total: u64 = t.steps.iter().filter(|(k, _)| !nested(&k.1)).map(|(_, v)| v.2).sum();
        out += &format!("medido no desenho: {:.1}% de um núcleo\n\n", pct(total));
        out += &format!("{:<10} {:<9} {:>6} {:>12} {:>12} {:>9}\n", "widget", "etapa", "vezes", "relógio ms", "CPU ms", "% núcleo");
        let mut rows: Vec<_> = t.steps.iter().collect();
        rows.sort_by(|a, b| b.1.2.cmp(&a.1.2));
        for ((key, step), (n, d, cycles)) in rows {
            let n = *n as f64;
            out += &format!("{key:<10} {:<9} {n:>6} {:>12.3} {:>12.3} {:>8.2}%\n", format!("{step:?}"), ms(*d) / n, *cycles as f64 / hz / n, pct(*cycles));
        }
        let _ = std::fs::write(std::env::temp_dir().join("widgets-perf.txt"), out);
        t.steps.clear();
        t.since = Some(Instant::now());
        t.cpu = cpu_now;
        t.cycles = cyc_now;
    });
}

/// Tempo de CPU (usuário + núcleo do Windows) gasto pelo processo até agora.
pub fn process_cpu() -> Option<Duration> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut a, mut b, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut kernel, &mut user).ok()? };
    let ticks = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    Some(Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
}

/// Ciclos de CPU gastos por esta thread até agora (sem contar o tempo esperando).
pub fn thread_cycles() -> u64 {
    // (direto da kernel32: no crate windows, ela fica num módulo que o app não liga)
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryThreadCycleTime(thread: windows::Win32::Foundation::HANDLE, cycles: *mut u64) -> i32;
    }
    let mut c = 0u64;
    unsafe {
        QueryThreadCycleTime(windows::Win32::System::Threading::GetCurrentThread(), &mut c);
    }
    c
}

/// Quantos ciclos o contador anda por milissegundo (medido uma vez, girando 20 ms).
pub fn cycles_per_ms() -> f64 {
    static HZ: OnceLock<f64> = OnceLock::new();
    *HZ.get_or_init(|| {
        let (t, c) = (Instant::now(), thread_cycles());
        while t.elapsed() < Duration::from_millis(20) {
            std::hint::spin_loop();
        }
        (thread_cycles() - c) as f64 / (t.elapsed().as_secs_f64() * 1000.0)
    })
}