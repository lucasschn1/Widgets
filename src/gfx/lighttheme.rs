//! O tema claro dos widgets (Aparência → "Tema claro do Windows"), como os widgets do macOS no
//! modo claro: vidro claro e texto escuro.
//!
//! Os widgets são desenhados para o vidro escuro, com o texto branco e as cores claras. Em vez de
//! cada widget ter duas paletas, o `Canvas` passa cada cor pintada por `light_variant`: os cinzas
//! (o texto, os trilhos, as linhas) trocam de claridade, o branco vira quase preto e o quase
//! preto vira claro; as cores vivas ficam com o matiz e escurecem só o necessário para ler sobre
//! o vidro claro. As contas são no OKLab (a claridade que os olhos veem). As imagens (a capa do
//! álbum, os ícones dos apps) não passam por aqui.

use super::wallcolor::{lab_of, rgb_of, Lab};
use crate::ui::theme::Rgba;

/// Abaixo deste croma (a "força" da cor no OKLab), a cor conta como cinza e troca de claridade.
const GRAY_CHROMA: f32 = 0.04;
/// A claridade do branco no tema claro (o texto principal): um quase preto, como o do macOS.
const INK_LIGHTNESS: f32 = 0.22;
/// As cores vivas trocam de claridade (claridade nova = `.0` - `.1` × antiga): o que se destacava
/// mais do vidro escuro se destaca mais do claro. Um verde forte (claro, no vidro escuro) fica um
/// pouco mais escuro; um verde apagado (escuro, no vidro escuro) fica claro, e o mapa do GitHub
/// continua dizendo "mais" com a cor mais forte. A inclinação menor que 1 deixa as cores fortes
/// perto do tom delas (o laranja do Claude não vira marrom).
const COLOR_FLIP: (f32, f32) = (1.04, 0.6);
/// A faixa de claridade das cores vivas no tema claro: mais escura que isso, a cor vira marrom ou
/// preto; mais clara, some no vidro claro.
const COLOR_LIGHTNESS: (f32, f32) = (0.42, 0.82);

/// A cor `color`, desenhada para o vidro escuro, como fica no vidro claro (o mesmo alfa).
pub fn light_variant(color: Rgba) -> Rgba {
    let lab = lab_of(color.0, color.1, color.2);
    let chroma = lab.a.hypot(lab.b);
    if chroma < GRAY_CHROMA {
        // os cinzas trocam de claridade: o branco (1) vira INK_LIGHTNESS, e o preto (0), branco
        let lightness = 1.0 - lab.l * (1.0 - INK_LIGHTNESS);
        return fit(Lab { l: lightness, a: lab.a, b: lab.b }, color.3);
    }
    let lightness = (COLOR_FLIP.0 - COLOR_FLIP.1 * lab.l).clamp(COLOR_LIGHTNESS.0, COLOR_LIGHTNESS.1);
    fit(Lab { l: lightness, a: lab.a, b: lab.b }, color.3)
}

/// A cor do OKLab em sRGB, com o alfa `alpha`; se a tela não mostra essa cor, perde força (sem
/// mudar a claridade nem o matiz) até caber.
fn fit(lab: Lab, alpha: u8) -> Rgba {
    let mut lab = lab;
    loop {
        let rgb = rgb_of(lab);
        let fits = rgb.iter().all(|v| (-0.001..=1.001).contains(v));
        if fits || lab.a.hypot(lab.b) < 0.005 {
            let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            return Rgba(channel(rgb[0]), channel(rgb[1]), channel(rgb[2]), alpha);
        }
        lab.a *= 0.94;
        lab.b *= 0.94;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::{contrast, AMBER, BLUE, DARK_GLYPH, FG, GREEN, LIGHT_TINT, MUTED, RED, WHITE};

    /// O texto branco vira quase preto, o secundário continua translúcido (o mesmo alfa) e o
    /// glifo escuro dos botões brancos fica claro: a mesma hierarquia, ao contrário.
    #[test]
    fn grays_swap_lightness() {
        let ink = light_variant(WHITE);
        assert!(ink.0 < 40 && ink.0 == ink.1, "{ink:?}");
        assert_eq!(light_variant(MUTED).3, MUTED.3);
        let (button, glyph) = (light_variant(WHITE), light_variant(DARK_GLYPH));
        assert!(contrast(glyph, button) >= 7.0, "{glyph:?} sobre {button:?}: {:.2}", contrast(glyph, button));
    }

    /// As cores de estado ficam legíveis sobre o vidro claro (3:1, o mínimo da WCAG para
    /// gráficos) e com o mesmo matiz.
    #[test]
    fn colors_stay_readable_on_light_glass() {
        let glass = Rgba::rgb(LIGHT_TINT.0, LIGHT_TINT.1, LIGHT_TINT.2);
        for color in [GREEN, AMBER, RED, BLUE, Rgba::rgb(242, 140, 40)] {
            let dark = light_variant(color);
            assert!(contrast(dark, glass) >= 3.0, "{color:?} → {dark:?}: {:.2}", contrast(dark, glass));
            let (before, after) = (lab_of(color.0, color.1, color.2), lab_of(dark.0, dark.1, dark.2));
            let turn = (after.b.atan2(after.a) - before.b.atan2(before.a)).to_degrees().abs();
            assert!(turn < 4.0 || turn > 356.0, "{color:?} → {dark:?}: o matiz andou {turn:.1}°");
        }
        // e o texto principal, bem legível (4,5:1)
        assert!(contrast(light_variant(FG), glass) >= 4.5);
    }

    /// O que se destacava mais no vidro escuro continua se destacando mais no claro: o verde
    /// forte do GitHub (muitas contribuições) fica mais escuro, e mais forte no vidro claro, que
    /// o verde apagado (poucas).
    #[test]
    fn strong_colors_stay_stronger() {
        let glass = Rgba::rgb(LIGHT_TINT.0, LIGHT_TINT.1, LIGHT_TINT.2);
        let (strong, faint) = (Rgba::rgb(88, 204, 130), Rgba::rgb(22, 92, 52));
        assert!(contrast(light_variant(strong), glass) > contrast(light_variant(faint), glass), "{:?} {:?}", light_variant(strong), light_variant(faint));
    }
}
