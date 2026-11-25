//! A cor do papel de parede nos widgets (Aparência → "Cor do papel de parede"), como os widgets
//! tingidos do macOS: o matiz de cada cor desenhada vira o da cor que domina o papel de parede.
//!
//! As contas são no OKLab, o espaço de cor em que a claridade é a que os olhos veem: trocando só
//! o matiz, um laranja claro vira um azul tão claro quanto ele (no RGB, o azul sairia bem mais
//! escuro), e o texto e os anéis continuam legíveis sobre o vidro. Branco, cinza e preto não
//! mudam, e as imagens (a capa do álbum, a foto do GitHub) também não: só as cores que o `Canvas`
//! pinta passam por aqui.

use crate::ui::theme::Rgba;

/// Abaixo deste croma (a "força" da cor no OKLab) uma cor conta como cinza e fica como está:
/// o texto branco, os trilhos dos anéis, as linhas.
const GRAY_CHROMA: f32 = 0.03;
/// O croma de um papel de parede bem colorido: daí para cima, as cores dos widgets ficam com a
/// força de sempre; num papel de parede mais apagado, perdem força na mesma proporção (num em
/// preto e branco, os widgets ficam em tons de cinza).
const VIVID_CHROMA: f32 = 0.10;
/// Os pixels do papel de parede escuros ou claros demais quase não contam para a cor dele (num
/// céu à noite, as estrelas e o preto não são "a cor").
const DIM_LIGHTNESS: (f32, f32) = (0.22, 0.96);
/// Quantas fatias de matiz o papel de parede é dividido para achar a cor que domina.
const HUE_BINS: usize = 36;
/// No máximo quantos pixels do papel de parede são lidos (os outros são pulados por igual).
const MAX_SAMPLES: usize = 40_000;

/// A cor do papel de parede: o matiz (radianos, no OKLab) e quanta cor ele tem (croma).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallColor {
    pub hue: f32,
    pub chroma: f32,
}

/// Uma cor no OKLab: claridade (0 a 1) e os dois eixos de cor.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lab {
    pub(crate) l: f32,
    pub(crate) a: f32,
    pub(crate) b: f32,
}

/// Um canal sRGB (0 a 255) na luz de verdade (linear, 0 a 1).
fn to_linear(channel: u8) -> f32 {
    let v = channel as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// A luz de verdade (linear) de volta num canal sRGB (0 a 1, sem cortar).
fn from_linear(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.max(0.0).powf(1.0 / 2.4) - 0.055 }
}

/// A cor (sem o alfa) no OKLab.
pub(crate) fn lab_of(r: u8, g: u8, b: u8) -> Lab {
    let (r, g, b) = (to_linear(r), to_linear(g), to_linear(b));
    let l = (0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    Lab {
        l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    }
}

/// A cor do OKLab em sRGB (0 a 1 por canal); pode sair de 0 a 1 se a tela não mostra essa cor.
pub(crate) fn rgb_of(lab: Lab) -> [f32; 3] {
    let l = (lab.l + 0.396_337_78 * lab.a + 0.215_803_76 * lab.b).powi(3);
    let m = (lab.l - 0.105_561_346 * lab.a - 0.063_854_17 * lab.b).powi(3);
    let s = (lab.l - 0.089_484_18 * lab.a - 1.291_485_5 * lab.b).powi(3);
    [
        from_linear(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        from_linear(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
        from_linear(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    ]
}

impl WallColor {
    /// A cor `color` no matiz do papel de parede, com a mesma claridade e o mesmo alfa; cinzas
    /// ficam como estão. A força da cor é a dela, menos num papel de parede apagado.
    pub fn apply(&self, color: Rgba) -> Rgba {
        let lab = lab_of(color.0, color.1, color.2);
        let chroma = lab.a.hypot(lab.b);
        if chroma < GRAY_CHROMA {
            return color;
        }
        let mut target = chroma * (self.chroma / VIVID_CHROMA).min(1.0);
        // uma cor que a tela não mostra (um azul tão claro quanto um amarelo e tão forte quanto
        // ele): perde força até caber, sem mudar a claridade nem o matiz
        loop {
            let rgb = rgb_of(Lab { l: lab.l, a: target * self.hue.cos(), b: target * self.hue.sin() });
            let fits = rgb.iter().all(|v| (-0.001..=1.001).contains(v));
            if fits || target < 0.005 {
                let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                return Rgba(channel(rgb[0]), channel(rgb[1]), channel(rgb[2]), color.3);
            }
            target *= 0.94;
        }
    }

    /// Como `apply`, mas as cores com o matiz em `keep` (graus, de-até) ficam como são: os
    /// alertas de um widget (o vermelho da bateria fraca) continuam dizendo o que dizem.
    pub fn apply_except(&self, color: Rgba, keep: Option<(f32, f32)>) -> Rgba {
        if let Some((from, to)) = keep {
            let lab = lab_of(color.0, color.1, color.2);
            let hue = lab.b.atan2(lab.a).to_degrees().rem_euclid(360.0);
            if lab.a.hypot(lab.b) >= GRAY_CHROMA && (from..=to).contains(&hue) {
                return color;
            }
        }
        self.apply(color)
    }

    /// A cor que domina uma imagem (pixels RGB, três bytes cada): a fatia de matiz com mais cor
    /// (cada pixel conta pelo croma dele, e os muito escuros ou claros quase nada), junto com as
    /// duas vizinhas. Numa imagem cinza, o croma sai perto de zero.
    pub fn of_pixels(rgb: &[u8]) -> WallColor {
        let count = rgb.len() / 3;
        let step = count.div_ceil(MAX_SAMPLES).max(1);
        // por fatia de matiz: o peso, a soma dos eixos de cor e a do croma (ponderados)
        let mut weight = [0.0f32; HUE_BINS];
        let mut sum_a = [0.0f32; HUE_BINS];
        let mut sum_b = [0.0f32; HUE_BINS];
        let mut sum_chroma = [0.0f32; HUE_BINS];
        let (mut all_a, mut all_b, mut all) = (0.0f32, 0.0f32, 0usize);
        for pixel in rgb.chunks_exact(3).step_by(step) {
            let lab = lab_of(pixel[0], pixel[1], pixel[2]);
            all_a += lab.a;
            all_b += lab.b;
            all += 1;
            let chroma = lab.a.hypot(lab.b);
            if chroma < GRAY_CHROMA {
                continue;
            }
            let lit = if (DIM_LIGHTNESS.0..=DIM_LIGHTNESS.1).contains(&lab.l) { 1.0 } else { 0.2 };
            let w = chroma * lit;
            let bin = (((lab.b.atan2(lab.a) / std::f32::consts::TAU).rem_euclid(1.0) * HUE_BINS as f32) as usize).min(HUE_BINS - 1);
            weight[bin] += w;
            sum_a[bin] += lab.a * w;
            sum_b[bin] += lab.b * w;
            sum_chroma[bin] += chroma * w;
        }
        let around = |bin: usize| [(bin + HUE_BINS - 1) % HUE_BINS, bin, (bin + 1) % HUE_BINS];
        let peak = (0..HUE_BINS).max_by(|&x, &y| {
            let total = |bin: usize| around(bin).iter().map(|&i| weight[i]).sum::<f32>();
            total(x).total_cmp(&total(y))
        });
        let (mut w, mut a, mut b, mut chroma) = (0.0, 0.0, 0.0, 0.0);
        for i in peak.map(around).into_iter().flatten() {
            w += weight[i];
            a += sum_a[i];
            b += sum_b[i];
            chroma += sum_chroma[i];
        }
        // (quase nada colorido: o tom médio da imagem, que dá um croma perto de zero)
        if w <= f32::EPSILON || (w / all.max(1) as f32) < 0.002 {
            let n = all.max(1) as f32;
            return WallColor { hue: (all_b / n).atan2(all_a / n), chroma: (all_a / n).hypot(all_b / n) };
        }
        WallColor { hue: b.atan2(a), chroma: chroma / w }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hue_deg(color: Rgba) -> f32 {
        let lab = lab_of(color.0, color.1, color.2);
        lab.b.atan2(lab.a).to_degrees().rem_euclid(360.0)
    }

    fn lightness(color: Rgba) -> f32 {
        lab_of(color.0, color.1, color.2).l
    }

    /// Uma imagem toda de uma cor.
    fn flat(color: (u8, u8, u8), n: usize) -> Vec<u8> {
        [color.0, color.1, color.2].repeat(n)
    }

    #[test]
    fn oklab_round_trips() {
        for (r, g, b) in [(0, 0, 0), (255, 255, 255), (242, 140, 40), (30, 90, 200), (88, 204, 130), (128, 128, 128)] {
            let rgb = rgb_of(lab_of(r, g, b));
            for (got, want) in rgb.iter().zip([r, g, b]) {
                assert!((got * 255.0 - want as f32).abs() < 0.6, "({r}, {g}, {b}): {rgb:?}");
            }
        }
    }

    /// Branco, cinza, preto e o branco translúcido do texto e dos trilhos não mudam.
    #[test]
    fn grays_stay_as_they_are() {
        let blue = WallColor::of_pixels(&flat((40, 90, 220), 100));
        for color in [Rgba::rgb(255, 255, 255), Rgba::rgb(0, 0, 0), Rgba::rgb(128, 128, 130), Rgba(255, 255, 255, 30), Rgba(250, 250, 252, 255)] {
            assert_eq!(blue.apply(color), color);
        }
    }

    /// Uma cor viva ganha o matiz do papel de parede e fica com a mesma claridade e o mesmo alfa
    /// (o laranja do Claude vira um azul tão claro quanto ele).
    #[test]
    fn colors_take_the_wallpaper_hue_and_keep_their_lightness() {
        let blue = (40, 90, 220);
        let wall = WallColor::of_pixels(&flat(blue, 100));
        let target = hue_deg(Rgba::rgb(blue.0, blue.1, blue.2));
        for color in [Rgba::rgb(242, 140, 40), Rgba::rgb(88, 204, 130), Rgba(242, 102, 92, 120), Rgba::rgb(255, 226, 92)] {
            let tinted = wall.apply(color);
            let off = (hue_deg(tinted) - target + 540.0).rem_euclid(360.0) - 180.0;
            assert!(off.abs() < 3.0, "{color:?} → {tinted:?}: matiz {off:+.1}° do papel de parede");
            assert!((lightness(tinted) - lightness(color)).abs() < 0.02, "{color:?} → {tinted:?}: mudou a claridade");
            assert_eq!(tinted.3, color.3);
        }
    }

    /// Num papel de parede em preto e branco, as cores viram tons de cinza (com a mesma claridade).
    #[test]
    fn a_gray_wallpaper_makes_gray_widgets() {
        let mut gray = flat((20, 20, 20), 500);
        gray.extend(flat((200, 200, 200), 500));
        let wall = WallColor::of_pixels(&gray);
        assert!(wall.chroma < 0.01, "{wall:?}");
        let tinted = wall.apply(Rgba::rgb(242, 140, 40));
        let lab = lab_of(tinted.0, tinted.1, tinted.2);
        assert!(lab.a.hypot(lab.b) < 0.01, "{tinted:?}");
    }

    /// Os alertas ficam: com a faixa de matiz dos vermelhos, o vermelho não muda e o verde muda.
    #[test]
    fn alert_hues_keep_their_color() {
        let blue = WallColor::of_pixels(&flat((40, 90, 220), 64));
        let (red, green) = (Rgba::rgb(242, 102, 92), Rgba::rgb(88, 204, 130));
        assert_eq!(blue.apply_except(red, Some((10.0, 45.0))), red);
        assert_ne!(blue.apply_except(green, Some((10.0, 45.0))), green);
        assert_ne!(blue.apply_except(red, None), red);
    }

    /// O `Canvas` pinta as cores no matiz do papel de parede, mas as imagens (a capa do álbum)
    /// e o que ele desenha com `own_colors` (o Clawd) ficam como são.
    #[test]
    fn the_canvas_tints_colors_but_not_pictures() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        use crate::gfx::{Canvas, Image};
        let wall = WallColor::of_pixels(&flat((40, 90, 220), 64));
        let orange = Rgba::rgb(242, 140, 40);
        let mut picture = Image::new(4, 4);
        for p in picture.px.chunks_exact_mut(4) {
            p.copy_from_slice(&[40, 140, 242, 255]); // (BGRA) laranja
        }
        let mut c = Canvas::new(30, 10, 1.0);
        c.wall_color = Some(wall);
        c.fill_round_rect(0.0, 0.0, 10.0, 10.0, 0.0, orange);
        c.own_colors(|c| c.fill_round_rect(10.0, 0.0, 20.0, 10.0, 0.0, orange));
        c.image(&picture, 23.0, 3.0);
        let img = c.finish();
        let at = |x: u32| img.px[((5 * img.w + x) * 4) as usize..][..4].to_vec();
        assert_eq!(at(5)[..3], { let t = wall.apply(orange); [t.2, t.1, t.0] }, "a cor pintada não mudou");
        assert_eq!(at(15), vec![40, 140, 242, 255], "o own_colors mudou a cor");
        assert_eq!(at(24), vec![40, 140, 242, 255], "a imagem mudou de cor");
    }

    /// A cor que domina vence a que aparece pouco, e o preto e o branco não contam: um papel de
    /// parede quase todo azul-escuro, com um pouco de laranja e muito preto, é azul.
    #[test]
    fn the_dominant_color_wins() {
        let mut px = flat((0, 0, 0), 4000);
        px.extend(flat((255, 255, 255), 2000));
        px.extend(flat((30, 70, 170), 1500));
        px.extend(flat((40, 85, 190), 1500));
        px.extend(flat((240, 120, 30), 600));
        let wall = WallColor::of_pixels(&px);
        let blue = hue_deg(Rgba::rgb(35, 78, 180));
        let off = (wall.hue.to_degrees().rem_euclid(360.0) - blue + 540.0).rem_euclid(360.0) - 180.0;
        assert!(off.abs() < 8.0, "{wall:?}");
        assert!(wall.chroma > VIVID_CHROMA, "{wall:?}");
    }
}
