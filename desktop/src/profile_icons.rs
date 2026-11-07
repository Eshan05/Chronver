use rune::{El, Family, TextStyle, text};
use rune_kit::Kit;
use std::{collections::HashMap, sync::LazyLock};

static GLYPHS: LazyLock<HashMap<&'static str, char>> = LazyLock::new(|| {
    let mapping: HashMap<&str, Vec<&str>> =
        serde_json::from_str(include_str!("../assets/codicons/mapping.json"))
            .expect("valid bundled Codicons mapping");
    mapping
        .into_iter()
        .flat_map(|(code, names)| {
            let glyph = char::from_u32(code.parse().expect("Codicons codepoint"))
                .expect("Codicons Unicode scalar");
            names.into_iter().map(move |name| (name, glyph))
        })
        .collect()
});

fn glyph(name: Option<&str>) -> char {
    name.and_then(|name| GLYPHS.get(name))
        .copied()
        .unwrap_or(GLYPHS["settings"])
}

pub fn view(name: Option<&str>, k: &Kit, size: f32) -> El {
    text(glyph(name).to_string())
        .font(
            TextStyle::new(size, 400.)
                .family(Family::Named("codicon"))
                .lh(size),
        )
        .size(size, size)
        .center()
        .color(k.c.icon_muted)
        .a11y_hidden()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_icons_use_the_full_bundled_mapping_and_a_safe_default() {
        assert!(GLYPHS.len() > 500);
        for name in [
            "vm",
            "star-empty",
            "beaker",
            "remote-explorer",
            "compass",
            "briefcase",
            "terminal",
            "code",
        ] {
            assert_ne!(glyph(Some(name)), glyph(None), "{name}");
        }
        assert_eq!(glyph(Some("does-not-exist")), glyph(None));
        assert_eq!(glyph(Some("https://example.com/icon.svg")), glyph(None));
        let mut fonts = rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES);
        let style = TextStyle::new(14., 400.)
            .family(Family::Named("codicon"))
            .lh(14.);
        for glyph in GLYPHS.values() {
            let (w, h) = fonts.measure(&glyph.to_string(), style, 1.);
            assert!(
                w > 0. && w <= 20. && h > 0. && h <= 20.,
                "glyph {glyph:?}: {w}x{h}"
            );
        }
    }
}
