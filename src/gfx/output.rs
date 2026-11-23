//! Como o quadro pronto de um cartão chega à tela:
//!
//! - `Layered` (o normal): o quadro é montado em memória e copiado para uma janela transparente
//!   (`win::blit`); os cliques nos pixels transparentes passam para a área de trabalho;
//! - `Gpu` (a opção "Desenhar pela placa de vídeo"): o quadro é desenhado direto na placa de
//!   vídeo (`gpu::Surface`), sem copiar a janela inteira a cada quadro; a área que recebe cliques
//!   é o retângulo do cartão (`hit`), recortado na janela.
//!
//! Os dois desenham com a mesma `Canvas`, então o cartão não precisa saber qual está em uso.

use windows::Win32::Foundation::HWND;

use crate::gfx::Canvas;
use crate::gfx::gpu;
use crate::platform::win;

/// Para onde vai o quadro de uma janela.
pub enum Output {
    Layered,
    /// a superfície, o tamanho da janela agora e a área de cliques já mandada ao Windows (None:
    /// ainda nenhuma)
    Gpu { surface: gpu::Surface, size: (u32, u32), hit: Option<Option<win::Rect>> },
}

/// Um quadro para mostrar: o tamanho da janela (pixels), a escala, para onde levar a janela (None:
/// fica onde está), a opacidade (0 a 255) e a área que recebe cliques, em pixels da janela (None:
/// ela toda).
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub s: f32,
    pub pos: Option<(i32, i32)>,
    pub alpha: u8,
    pub hit: Option<win::Rect>,
}

impl Output {
    /// Pela placa de vídeo (a janela precisa ter sido criada para isso: `events::create_composed`).
    pub fn gpu(hwnd: HWND) -> Output {
        Output::Gpu { surface: gpu::Surface::new(hwnd), size: (0, 0), hit: None }
    }

    /// Mostra uma imagem pronta do tamanho do quadro (a troca de tamanho, que monta cada passo em
    /// memória).
    pub fn show_image(&mut self, hwnd: HWND, frame: Frame, img: &crate::gfx::Image) {
        match self {
            Output::Layered => win::blit(hwnd, img, frame.pos, frame.alpha),
            Output::Gpu { .. } => self.show(hwnd, frame, |c| c.image(img, 0.0, 0.0)),
        }
    }

    /// Desenha o quadro com `draw` e mostra na janela.
    pub fn show(&mut self, hwnd: HWND, frame: Frame, draw: impl FnOnce(&mut Canvas)) {
        match self {
            Output::Layered => {
                let mut c = Canvas::new(frame.w, frame.h, frame.s);
                draw(&mut c);
                win::blit(hwnd, &c.finish(), frame.pos, frame.alpha);
            }
            Output::Gpu { surface, size, hit } => {
                if frame.pos.is_some() || *size != (frame.w, frame.h) {
                    win::place(hwnd, frame.pos, (frame.w as i32, frame.h as i32));
                    *size = (frame.w, frame.h);
                }
                if *hit != Some(frame.hit) {
                    win::set_hit_area(hwnd, frame.hit);
                    *hit = Some(frame.hit);
                }
                let alpha = frame.alpha as f32 / 255.0;
                let drawn = surface.present(frame.w, frame.h, frame.s, |c| {
                    if alpha < 1.0 {
                        c.push_opacity(alpha);
                    }
                    draw(c);
                });
                if let Err(error) = drawn {
                    debug_log!("placa de vídeo: {error:?}");
                }
            }
        }
    }
}
