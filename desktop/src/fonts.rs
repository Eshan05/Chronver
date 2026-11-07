pub const FAMILIES: (&str, &str) = ("Geist", "Geist Mono");

pub const FACES: &[&[u8]] = &[
    include_bytes!("../assets/codicons/codicon.ttf"),
    include_bytes!("../assets/fonts/Geist-Regular.ttf"),
    include_bytes!("../assets/fonts/Geist-Medium.ttf"),
    include_bytes!("../assets/fonts/Geist-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Geist-Bold.ttf"),
    include_bytes!("../assets/fonts/Geist-Italic.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Regular.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Medium.ttf"),
    include_bytes!("../assets/fonts/GeistMono-Bold.ttf"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{TextStyle, text::TextSystem};

    #[test]
    fn bundled_geist_shapes_ui_and_fixed_width_code_without_system_fonts() {
        let mut text = TextSystem::new(FACES, FAMILIES);
        assert_eq!(text.families(), FAMILIES);
        for weight in [400., 500., 600., 700.] {
            let style = TextStyle::new(14., weight);
            let (w, h) = text.measure(releasescout_engine::APP_NAME, style, 1.);
            assert!(w > 50. && h > 10.);
            let narrow = text.measure("iiiiii", style.mono(), 1.).0;
            let wide = text.measure("WWWWWW", style.mono(), 1.).0;
            assert!((narrow - wide).abs() < 0.01);
            assert!(text.measure("iiiiii", style, 1.).0 < text.measure("WWWWWW", style, 1.).0);
        }
    }
}
