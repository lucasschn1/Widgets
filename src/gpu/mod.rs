//! Desenhar na placa de vídeo: em vez de montar cada quadro do cartão em memória e copiar a
//! janela inteira para o Windows (`win::blit`), o quadro é desenhado direto numa superfície da
//! placa de vídeo da janela e mostrado pela DirectComposition, no ritmo da tela. O desenho em si
//! continua o mesmo (`Canvas`): só muda onde ele cai.
//!
//! - `device`: o Direct3D 11, o Direct2D ligado nele e a DirectComposition (um por thread);
//! - `surface`: a superfície de uma janela, que cresce quando o cartão cresce.
//!
//! Se a placa de vídeo sumir no meio (o PC dormiu, o driver reiniciou), o dispositivo é criado de
//! novo e as superfícies se refazem no próximo quadro.

mod device;
mod surface;

pub use device::available;
pub use surface::Surface;
