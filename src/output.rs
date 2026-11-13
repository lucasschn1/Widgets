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
use crate::gpu;
use crate::win;

pub enum Output {
    Layered,
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
    pub fn show_image(&mut self, hwnd: HWND, f: Frame, img: &crate::gfx::Image) {
        match self {
            Output::Layered => win::blit(hwnd, img, f.pos, f.alpha),
            Output::Gpu { .. } => self.show(hwnd, f, |c| c.image(img, 0.0, 0.0)),
        }
    }

    /// Desenha o quadro com `draw` e mostra na janela.
    pub fn show(&mut self, hwnd: HWND, f: Frame, draw: impl FnOnce(&mut Canvas)) {
        match self {
            Output::Layered => {
                let mut c = Canvas::new(f.w, f.h, f.s);
                draw(&mut c);
                win::blit(hwnd, &c.finish(), f.pos, f.alpha);
            }
            Output::Gpu { surface, size, hit } => {
                if f.pos.is_some() || *size != (f.w, f.h) {
                    win::place(hwnd, f.pos, (f.w as i32, f.h as i32));
                    *size = (f.w, f.h);
                }
                if *hit != Some(f.hit) {
                    win::set_hit_area(hwnd, f.hit);
                    *hit = Some(f.hit);
                }
                let alpha = f.alpha as f32 / 255.0;
                let drawn = surface.present(f.w, f.h, f.s, |c| {
                    if alpha < 1.0 {
                        c.push_opacity(alpha);
                    }
                    draw(c);
                });
                if let Err(e) = drawn {
                    debug_log!("placa de vídeo: {e:?}");
                }
            }
        }
    }
}
