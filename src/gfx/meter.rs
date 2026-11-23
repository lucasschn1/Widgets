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

/// O que uma etapa de um widget gastou na janela: quantas vezes, o tempo de relógio (que conta
/// também a espera, pela placa de vídeo, pelo Windows) e os ciclos de CPU da thread (só o
/// trabalho).
#[derive(Default)]
struct StepTotal {
    count: u32,
    wall: Duration,
    cycles: u64,
}

/// O que foi medido desde o último resumo.
#[derive(Default)]
struct Totals {
    /// por (widget, etapa)
    steps: BTreeMap<(&'static str, Step), StepTotal>,
    since: Option<Instant>,
    /// a CPU do processo e os ciclos desta thread no começo da janela
    cpu: Option<Duration>,
    cycles: u64,
}

thread_local! {
    static TOTALS: RefCell<Totals> = RefCell::new(Totals::default());
}

/// O medidor está ligado (`WIDGETS_PERF=1`)?
pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("WIDGETS_PERF").is_ok_and(|value| value == "1"))
}

/// Mede `f` como a etapa `step` do widget `key`.
pub fn time<R>(key: &'static str, step: Step, f: impl FnOnce() -> R) -> R {
    if !on() {
        return f();
    }
    let (started, cycles_before) = (Instant::now(), thread_cycles());
    let result = f();
    let (spent, cycles) = (started.elapsed(), thread_cycles() - cycles_before);
    TOTALS.with(|totals| {
        let mut totals = totals.borrow_mut();
        let total = totals.steps.entry((key, step)).or_default();
        total.count += 1;
        total.wall += spent;
        total.cycles += cycles;
    });
    result
}

/// De vez em quando (chamado pelo laço principal): escreve o resumo dos últimos 10 segundos.
pub fn flush() {
    if !on() {
        return;
    }
    TOTALS.with(|totals| {
        let mut totals = totals.borrow_mut();
        if totals.since.is_none() {
            totals.since = Some(Instant::now());
            totals.cpu = process_cpu();
            totals.cycles = thread_cycles();
        }
        let since = totals.since.unwrap();
        let window = since.elapsed();
        if window < Duration::from_secs(10) {
            return;
        }
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let mut out = format!("janela de {:.1} s\n", window.as_secs_f64());
        let cpu_now = process_cpu();
        if let (Some(cpu_before), Some(cpu_after)) = (totals.cpu, cpu_now) {
            out += &format!("CPU do processo inteiro: {:.1}% de um núcleo\n", ms(cpu_after.saturating_sub(cpu_before)) / ms(window) * 100.0);
        }
        // ciclos por milissegundo (o contador de ciclos anda no ritmo fixo do relógio do processador)
        let hz = cycles_per_ms();
        let cyc_now = thread_cycles();
        let pct = |cycles: u64| cycles as f64 / hz / ms(window) * 100.0;
        out += &format!("CPU da thread da interface: {:.1}% de um núcleo\n", pct(cyc_now - totals.cycles));
        // (a camada animada e os detalhes estão dentro de Show: não somam de novo)
        let nested = |step: &Step| matches!(step, Step::Overlay | Step::Decorate);
        let total: u64 = totals.steps.iter().filter(|((_, step), _)| !nested(step)).map(|(_, spent)| spent.cycles).sum();
        out += &format!("medido no desenho: {:.1}% de um núcleo\n\n", pct(total));
        out += &format!("{:<10} {:<9} {:>6} {:>12} {:>12} {:>9}\n", "widget", "etapa", "vezes", "relógio ms", "CPU ms", "% núcleo");
        // as que mais gastaram primeiro
        let mut rows: Vec<_> = totals.steps.iter().collect();
        rows.sort_by(|a, b| b.1.cycles.cmp(&a.1.cycles));
        for ((key, step), spent) in rows {
            let n = spent.count as f64;
            out += &format!("{key:<10} {:<9} {n:>6} {:>12.3} {:>12.3} {:>8.2}%\n", format!("{step:?}"), ms(spent.wall) / n, spent.cycles as f64 / hz / n, pct(spent.cycles));
        }
        let _ = std::fs::write(std::env::temp_dir().join("widgets-perf.txt"), out);
        totals.steps.clear();
        totals.since = Some(Instant::now());
        totals.cpu = cpu_now;
        totals.cycles = cyc_now;
    });
}

/// Tempo de CPU (usuário + núcleo do Windows) gasto pelo processo até agora.
pub fn process_cpu() -> Option<Duration> {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user).ok()? };
    // em unidades de 100 ns
    let ticks = |time: FILETIME| ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64;
    Some(Duration::from_nanos((ticks(kernel) + ticks(user)) * 100))
}

/// Ciclos de CPU gastos por esta thread até agora (sem contar o tempo esperando).
pub fn thread_cycles() -> u64 {
    // (direto da kernel32: no crate windows, ela fica num módulo que o app não liga)
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn QueryThreadCycleTime(thread: windows::Win32::Foundation::HANDLE, cycles: *mut u64) -> i32;
    }
    let mut cycles = 0u64;
    unsafe {
        QueryThreadCycleTime(windows::Win32::System::Threading::GetCurrentThread(), &mut cycles);
    }
    cycles
}

/// Quantos ciclos o contador anda por milissegundo (medido uma vez, girando 20 ms).
pub fn cycles_per_ms() -> f64 {
    static HZ: OnceLock<f64> = OnceLock::new();
    *HZ.get_or_init(|| {
        let (started, cycles_before) = (Instant::now(), thread_cycles());
        while started.elapsed() < Duration::from_millis(20) {
            std::hint::spin_loop();
        }
        (thread_cycles() - cycles_before) as f64 / (started.elapsed().as_secs_f64() * 1000.0)
    })
}