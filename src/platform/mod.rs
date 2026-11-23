//! A conversa com o Windows: janelas e a fila de eventos, o resto da API do Windows (`win`),
//! iniciar com o Windows, os números do sistema, a bateria e as atualizações.

pub mod win;
pub mod events;
pub mod startup;
pub mod sysinfo;
pub mod battery;
pub mod updates;
pub mod notifications;
