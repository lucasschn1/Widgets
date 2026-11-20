//! O mundo lá fora na ilha: o pôr do sol, a lua cheia e os eclipses da Lua, a Estação Espacial
//! passando, a cotação do dólar e do bitcoin e o fim do expediente.
//!
//! Não são widgets da área de trabalho: são fontes que só aparecem na ilha, cada uma na hora
//! dela, e somem sozinhas quando a hora passa (ou quando a pessoa aperta OK). O céu usa a cidade
//! do widget Tempo. Cada uma liga e desliga em Configurações → Geral → "O mundo lá fora".

mod greeting;
mod quote;
mod skywatch;
mod workday;

use serde_json::Value;

use super::island_ui as iu;
use super::{IslandForm, Kind, Rect4};
use crate::config::Get;
use crate::gfx::Canvas;
use crate::win;

pub use greeting::Greeting;
#[cfg(test)]
pub use greeting::Period;
pub use quote::Quote;
pub use skywatch::{Moon, Station, Sunset};
pub use workday::{Workday, DEFAULT_END};

/// Os horários do fim do expediente que dá para escolher.
pub fn workday_end_choices() -> &'static [&'static str] {
    &workday::END_CHOICES
}

/// As fontes, na ordem das Configurações.
pub const KEYS: [&str; 6] = ["saudacao", "sol", "lua", "iss", "cotacao", "expediente"];

/// Nome de cada fonte (no menu da ilha e nas Configurações).
pub fn title(key: &str) -> &'static str {
    match key {
        "saudacao" => "Saudações",
        "sol" => "Pôr do sol",
        "lua" => "Lua cheia",
        "iss" => "Estação Espacial",
        "cotacao" => "Cotação",
        "expediente" => "Fim do expediente",
        _ => "",
    }
}

/// O que cada fonte mostra e quando (nas Configurações).
pub fn description(key: &str) -> &'static str {
    match key {
        "saudacao" => "Bom dia, boa tarde e boa noite na primeira vez que você usa o PC em cada período, com o tempo e as cotações",
        "sol" => "15 minutos antes, com o céu previsto para a hora (na cidade do Tempo)",
        "lua" => "Na noite de lua cheia, com a hora em que ela nasce, e nos eclipses da Lua que dá para ver daqui",
        "iss" => "5 minutos antes de ela passar visível no seu céu (de noite e sem nuvens), e enquanto passa",
        "cotacao" => "Quando o dólar varia mais de 1,5% no dia, ou o bitcoin mais de 5%",
        "expediente" => "Uma contagem na última hora, de segunda a sexta, e um \"bom descanso\" no fim",
        _ => "",
    }
}

/// Cria as fontes.
pub fn create_all(cache: &Value) -> Vec<Box<dyn Kind>> {
    vec![Box::new(Greeting::new(cache)), Box::new(Sunset::new()), Box::new(Moon::new()), Box::new(Station::new(cache)), Box::new(Quote::new(cache)), Box::new(Workday::new())]
}

/// Ações dos botões na ilha.
pub const OK: u32 = 1;
pub const OPEN: u32 = 2;

/// O relógio das fontes: agora e o fuso do PC (fixos nos testes).
#[derive(Clone, Copy, Default, Debug)]
pub struct Clock {
    fixed: Option<(f64, f64)>,
}

impl Clock {
    /// Parado em `unix`, no fuso `offset` (segundos).
    #[cfg(test)]
    pub fn at(unix: f64, offset: f64) -> Clock {
        Clock { fixed: Some((unix, offset)) }
    }
    pub fn now(&self) -> f64 {
        self.fixed.map_or_else(win::now, |f| f.0)
    }
    pub fn offset(&self) -> f64 {
        self.fixed.map_or_else(win::utc_offset_now, |f| f.1)
    }
    /// "18:47"
    pub fn hhmm(&self, t: f64) -> String {
        let s = ((t + self.offset()).floor() as i64).rem_euclid(86400);
        format!("{:02}:{:02}", s / 3600, s / 60 % 60)
    }
    /// O dia (local) de `t`, em dias desde 1970.
    pub fn day(&self, t: f64) -> i64 {
        ((t + self.offset()) / 86400.0).floor() as i64
    }
    /// A meia-noite (local) que começa o dia `day`.
    pub fn midnight(&self, day: i64) -> f64 {
        day as f64 * 86400.0 - self.offset()
    }
    /// 0 = segunda ... 6 = domingo
    pub fn weekday(&self, t: f64) -> i64 {
        (self.day(t) + 3).rem_euclid(7)
    }
    /// "24 de dezembro"
    pub fn date_text(&self, t: f64) -> String {
        const MONTHS: [&str; 12] = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
        let (_, m, d) = crate::util::civil_from_days(self.day(t));
        format!("{d} de {}", MONTHS[(m as usize).clamp(1, 12) - 1])
    }
}

/// A cidade do widget Tempo: (latitude, longitude, nome curto).
pub fn location(cfg: &Value) -> Option<(f64, f64, String)> {
    let t = &cfg["tempo"];
    let (lat, lon) = (t.f64_of("lat")?, t.f64_of("lon")?);
    let place = t.str_of("place").or(t.str_of("city")).unwrap_or("").split(',').next().unwrap_or("").trim().to_string();
    Some((lat, lon, place))
}

/// O clima de agora guardado pelo Tempo: (código da Open-Meteo, ventania).
pub fn cached_weather(cache: &Value) -> Option<(i64, bool)> {
    let w = &cache["weather"];
    Some((w.i64_of("code")?, w.bool_of("windy").unwrap_or(false)))
}

/// A previsão hora a hora guardada pelo Tempo: (dia a partir de hoje, hora, código do tempo).
pub fn cached_hours(cache: &Value) -> Vec<(i64, i64, i64)> {
    let list = cache["weather"]["hours"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    list.iter().filter(|h| h[2].is_i64()).filter_map(|h| Some((h[5].as_i64()?, h[6].as_i64()?, h[3].as_i64()?))).collect()
}

/// Os botões da ilha: OK na média (à direita) e na aberta (embaixo, à direita), com um segundo
/// botão ao lado dele na aberta.
pub fn buttons(form: IslandForm, w: f32, h: f32, second: bool) -> Vec<(u32, Rect4)> {
    match form {
        IslandForm::Medium => vec![(OK, (w - 74.0, h / 2.0 - 16.0, w - 18.0, h / 2.0 + 16.0))],
        IslandForm::Large => {
            let mut v = vec![(OK, (w - 82.0, h - 46.0, w - 22.0, h - 14.0))];
            if second {
                v.push((OPEN, (w - 186.0, h - 46.0, w - 92.0, h - 14.0)));
            }
            v
        }
        _ => Vec::new(),
    }
}

pub fn hit(form: IslandForm, x: f32, y: f32, w: f32, h: f32, second: bool) -> Option<u32> {
    buttons(form, w, h, second).into_iter().find(|(_, b)| iu::inside(*b, x, y)).map(|(a, _)| a)
}

pub fn draw_buttons(c: &mut Canvas, form: IslandForm, r: Rect4, hot: Option<u32>, second: Option<&str>) {
    let (x, y, w, h) = r;
    for (act, b) in buttons(form, w, h, second.is_some()) {
        let label = if act == OK { "OK" } else { second.unwrap_or("") };
        iu::pill_button(c, (x + b.0, y + b.1, x + b.2, y + b.3), label, act == OK && form == IslandForm::Large, hot == Some(act));
    }
}

/// As fontes com dados de exemplo, em cada estado que aparece na ilha (para os testes), e a
/// configuração de cada uma.
#[cfg(test)]
pub fn samples() -> Vec<(String, Box<dyn Kind>, Value)> {
    let cfg = serde_json::json!({"tempo": {"lat": -23.55, "lon": -46.63, "place": "São Paulo, Brasil"}, "app": {}});
    vec![
        ("bom dia".into(), Box::new(Greeting::sample(greeting::Period::Morning)) as Box<dyn Kind>, cfg.clone()),
        ("boa tarde".into(), Box::new(Greeting::sample(greeting::Period::Afternoon)), cfg.clone()),
        ("boa noite".into(), Box::new(Greeting::sample(greeting::Period::Night)), cfg.clone()),
        ("pôr do sol".into(), Box::new(Sunset::sample()), cfg.clone()),
        ("lua cheia".into(), Box::new(Moon::sample_full()), cfg.clone()),
        ("eclipse chegando".into(), Box::new(Moon::sample_eclipse(false)), cfg.clone()),
        ("eclipse agora".into(), Box::new(Moon::sample_eclipse(true)), cfg.clone()),
        ("iss chegando".into(), Box::new(Station::sample(false)), cfg.clone()),
        ("iss passando".into(), Box::new(Station::sample(true)), cfg.clone()),
        ("dólar subiu".into(), Box::new(Quote::sample(2.1, 0.8)), cfg.clone()),
        ("bitcoin caiu".into(), Box::new(Quote::sample(0.3, -6.4)), cfg.clone()),
        ("expediente acabando".into(), Box::new(Workday::sample(-25.0 * 60.0)), cfg.clone()),
        ("expediente no fim".into(), Box::new(Workday::sample(4.0)), cfg.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Env;

    /// Cada fonte em cada forma da ilha, num PNG por fonte (para conferir a olho):
    /// cargo test --release outside_png -- --ignored --nocapture
    #[test]
    #[ignore]
    fn outside_png() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let s = 1.5;
        let forms = [IslandForm::Bubble, IslandForm::Compact, IslandForm::Banner, IslandForm::Medium, IslandForm::Large];
        for (name, kind, cfg) in samples() {
            let env = Env { cfg: &cfg, key: kind.key() };
            let total_h: f32 = forms.iter().map(|f| iu::size(*f).1 + 14.0).sum::<f32>() + 14.0;
            let mut c = Canvas::new((400.0 * s) as u32, (total_h * s) as u32, s);
            c.fill_round_rect(0.0, 0.0, 400.0, total_h, 0.0, crate::theme::Rgba::rgb(70, 80, 100));
            let mut y = 14.0;
            for form in forms {
                let (w, h, r) = iu::size(form);
                c.fill_round_rect(14.0, y, 14.0 + w, y + h, r, crate::theme::Rgba::rgb(0, 0, 0));
                c.push_round_clip(14.0, y, 14.0 + w, y + h, r);
                kind.island_draw(&mut c, form, (14.0, y, w, h), None, &env);
                c.pop_clip();
                y += h + 14.0;
            }
            let path = std::env::temp_dir().join(format!("ilha_fora_{}.png", name.replace(' ', "_")));
            crate::gfx::save_png(&c.finish(), &path).unwrap();
            println!("{}", path.display());
        }
    }
}
