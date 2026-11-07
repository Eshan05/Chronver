use rune::{Color, TextStyle, color::hex, oklch};
use rune_kit::{Elevation, Kit, MarkdownStyle};

pub fn kit(dark: bool) -> Kit {
    let mut k = Kit::neutral(dark);
    // shadcn's neutral scaffold: https://ui.shadcn.com/docs/theming
    let neutral = |dark_l, light_l| oklch(if dark { dark_l } else { light_l }, 0.0, 0.0);
    k.c.page = neutral(0.145, 1.0);
    k.c.bg_base = k.c.page;
    k.c.bg_deep = neutral(0.205, 0.985);
    k.c.layer_01 = neutral(0.205, 1.0);
    k.c.layer_02 = neutral(0.269, 0.97);
    k.c.layer_03 = k.c.layer_02;
    k.c.layer_04 = neutral(0.371, 0.922);
    k.c.bg_button_neutral = k.c.layer_01;
    k.c.text_base = neutral(0.985, 0.145);
    k.c.text_strong = k.c.text_base;
    k.c.text_muted = neutral(0.708, 0.556);
    k.c.text_weak = k.c.text_muted;
    k.c.text_faint = k.c.text_muted;
    k.c.text_accent = neutral(0.922, 0.205);
    k.c.success.fg = hex(if dark { "#90caaa" } else { "#27714d" }).unwrap();
    k.c.bg_accent = k.c.text_accent;
    k.c.bg_inverse = k.c.text_base;
    k.c.bg_contrast = k.c.text_accent;
    k.c.bg_icon_button_contrast = k.c.text_accent;
    k.c.text_inverse = k.c.bg_base;
    k.c.text_contrast = neutral(0.205, 0.985);
    k.c.submit_fg = k.c.text_contrast;
    k.c.submit_icon = k.c.text_contrast;
    k.c.icon_base = k.c.text_base;
    k.c.icon_muted = k.c.text_muted;
    k.c.icon_faint = k.c.text_faint;
    k.c.icon_accent = k.c.text_accent;
    k.c.icon_inverse = k.c.text_inverse;
    k.c.icon_contrast = k.c.text_contrast;
    k.c.border_muted = if dark {
        Color::WHITE.alpha(0.1)
    } else {
        neutral(0.922, 0.922)
    };
    k.c.border_base = k.c.border_muted;
    k.c.border_weak = k.c.border_muted;
    k.c.border_strong = if dark {
        Color::WHITE.alpha(0.15)
    } else {
        k.c.border_base
    };
    k.c.border_focus = neutral(0.556, 0.708);
    k.c.border_active = k.c.text_accent;
    k.c.surface_base = k.c.layer_01;
    k.c.surface_weak = k.c.layer_01;
    k.c.surface_raised = k.c.layer_01;
    k.c.surface_raised_base = k.c.layer_01;
    k.c.surface_float = k.c.layer_01;
    k.c.surface_disabled = k.c.layer_02;
    k.c.border_hover = k.c.border_strong;
    k.c.border_disabled = k.c.border_base;
    k.c.hover = if dark {
        Color::WHITE.alpha(0.04)
    } else {
        Color::BLACK.alpha(0.03)
    };
    k.c.pressed = if dark {
        Color::WHITE.alpha(0.08)
    } else {
        Color::BLACK.alpha(0.07)
    };
    k.c.surface_hover = k.c.hover;
    k.c.surface_raised_hover = k.c.hover;
    k.elevation = Elevation::default();
    k.radius.lg = 0.0;
    k.ty.ui = TextStyle::new(13.0, 400.0).lh(20.0);
    k.ty.ui_medium = TextStyle::new(13.0, 500.0).lh(20.0);
    k.ty.mono = TextStyle::new(12.0, 400.0).mono().lh(19.0);
    k
}

pub fn shell(k: &Kit, dark: bool, material: bool) -> (Kit, Color) {
    let mut shell = k.clone();
    if !material {
        return (shell, k.c.page);
    }
    // The compositor can fall back to the opposite theme. Keep contrast in our own
    // render layer, while leaving a small amount of the native material visible.
    let tint = k.c.bg_deep.alpha(0.84);
    shell.c.bg_deep = Color::TRANSPARENT;
    let muted = hex(if dark { "#d4d4d4" } else { "#444444" }).unwrap();
    shell.c.text_muted = muted;
    shell.c.text_weak = muted;
    shell.c.text_faint = muted;
    shell.c.icon_muted = muted;
    shell.c.icon_faint = muted;
    (shell, tint)
}

pub fn tooltip(dark: bool, k: &Kit) -> rune::HelpLook {
    rune::HelpLook::system(dark)
        .fill(k.c.text_base)
        .ink(k.c.bg_base)
        .ring(0., Color::TRANSPARENT)
        .radius(6.)
        .padding(12., 6.)
        .font(TextStyle::new(12., 400.).lh(16.))
        .max_width(Some(280.))
        .gap(4.)
        .margin(8.)
        .delay(0.)
        .skip_delay(0.3)
        .fade(0.15, 0.1, 0.95)
}

pub fn separator(k: &Kit) -> Color {
    // Color::alpha replaces alpha; preserve the token's existing translucency.
    k.c.border_muted.alpha(k.c.border_muted.a * 0.6)
}

pub fn changelog_markdown(k: &Kit) -> MarkdownStyle {
    markdown(k)
        .text(k.c.text_accent)
        .body_font(TextStyle::new(14., 400.).lh(24.))
        .list(24., 6.)
        .list_nested_indent(20., 24.)
        .code_block_font(TextStyle::new(12., 400.).mono().lh(20.))
        .code_block_padding(14., 12.)
        .quote(separator(k), 2., 14., k.c.text_muted)
        .rule(separator(k))
        .link(k.c.text_base)
}

pub fn markdown(k: &Kit) -> MarkdownStyle {
    MarkdownStyle::from_kit(k)
        .body_font(TextStyle::new(13.5, 400.0).lh(23.0))
        .heading_font(1, TextStyle::new(18.0, 600.0).lh(28.0))
        .heading_font(2, TextStyle::new(16.0, 600.0).lh(25.0))
        .heading_font(3, TextStyle::new(14.0, 600.0).lh(23.0))
        .heading_font(4, TextStyle::new(13.5, 600.0).lh(23.0))
        .strong_weight(600.0)
        .link(k.c.text_accent)
        .link_underline(k.c.text_accent.alpha(0.4))
        .inline_code_bg(k.c.layer_02)
        .code_block(k.c.layer_01, k.c.border_base)
        .table(k.c.border_base, k.c.layer_01)
        .table_font(k.ty.ui)
        .selection(k.selection())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_reduce_the_existing_border_opacity_instead_of_replacing_it() {
        for dark in [false, true] {
            let k = kit(dark);
            let line = separator(&k);
            assert!((line.a - k.c.border_muted.a * 0.6).abs() < 0.0001);
            assert_eq!([line.r, line.g, line.b], [k.c.border_muted.r, k.c.border_muted.g, k.c.border_muted.b]);
            if dark { assert!(line.a < 0.07); }
        }
    }

    fn luminance(rgb: [f32; 3]) -> f32 {
        let linear = rgb.map(|v| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
    }

    #[test]
    fn material_shell_text_keeps_contrast_even_over_the_opposite_themes_backdrop() {
        for dark in [false, true] {
            let notes = kit(dark);
            let (shell, tint) = shell(&notes, dark, true);
            let tint = tint.premul_srgb(1.);
            assert!(tint[3] > 0.8 && tint[3] < 0.9);
            assert_eq!(shell.c.bg_deep.a, 0.);
            assert_eq!(notes.c.bg_base.a, 1.);
            assert_eq!(shell.c.bg_base.a, 1.);
            for backdrop in [[0.; 3], [1.; 3], [1., 0., 0.], [0., 0., 1.]] {
                let background: [f32; 3] =
                    std::array::from_fn(|i| tint[i] + backdrop[i] * (1. - tint[3]));
                for (selection, hover) in [(0., 0.), (0.065, 0.), (0.065, 0.1)] {
                    let mut row = background;
                    for alpha in [selection, hover] {
                        let wash = shell.c.text_base.alpha(alpha).premul_srgb(1.);
                        row = std::array::from_fn(|i| wash[i] + row[i] * (1. - wash[3]));
                    }
                    let background = luminance(row);
                    for color in [shell.c.text_base, shell.c.text_muted, shell.c.text_faint] {
                        let foreground = color.luminance();
                        let contrast = (foreground.max(background) + 0.05)
                            / (foreground.min(background) + 0.05);
                        assert!(
                            contrast >= 4.5,
                            "dark={dark}, backdrop={backdrop:?}, contrast={contrast}"
                        );
                    }
                }
            }
            let (fallback, background) = super::shell(&notes, dark, false);
            assert_eq!(background.a, 1.);
            assert_eq!(fallback.c.bg_deep.a, 1.);
        }
    }
}
