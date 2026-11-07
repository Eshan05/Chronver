use rune::{Color, El, Icon, Shadow, TextStyle, col, el, icon, row, text};
use rune_kit::{Kit, Placement, popover, scrim, scroll_bar};

pub(crate) fn kit(base: &Kit) -> Kit {
    let mut k = base.clone();
    k.radius.md = 6.;
    k.radius.sm = 4.;
    // Rune's standard rows add 4pt per side; this line box gives a centered 32pt item.
    k.ty.text_small_medium = TextStyle::new(14., 400.).lh(24.);
    k.ty.text_xsmall_medium = TextStyle::new(14., 500.).lh(24.);
    k.c.std_icon = k.c.text_muted;
    // Standard menus replace border opacity with 50%, so use a neutral grey, not white.
    k.c.std_border = if k.dark {
        k.c.layer_04
    } else {
        k.c.border_base
    };
    k.c.surface_raised_hover = k.c.layer_02;
    k.elevation.md = vec![
        Shadow {
            y: 4.,
            blur: 6.,
            spread: -1.,
            color: Color::BLACK.alpha(0.1),
            ..Shadow::default()
        },
        Shadow {
            y: 2.,
            blur: 4.,
            spread: -2.,
            color: Color::BLACK.alpha(0.1),
            ..Shadow::default()
        },
    ];
    k
}

pub fn leading_icon(glyph: &'static Icon, k: &Kit) -> El {
    icon(glyph, 16.)
        .key("menu-action-icon")
        .color(k.c.text_muted)
        .a11y_hidden()
}

pub fn item(key: impl Into<String>, label: &str, glyph: &'static Icon, k: &Kit) -> El {
    row()
        .key(key)
        .role("menuitem")
        .label(label)
        .focusable(true)
        .radius(4.)
        .px(8.)
        .py(6.)
        .gap(8.)
        .center_y()
        .focus_highlight(k.c.layer_02)
        .child(leading_icon(glyph, k))
        .child(
            text(label)
                .font(TextStyle::new(14., 400.).lh(20.))
                .color(k.c.text_base)
                .truncate()
                .a11y_hidden(),
        )
}

pub fn separator(key: &str, k: &Kit) -> El {
    el().key(key)
        .h(1.)
        .my(4.)
        .mx(-4.)
        .fill(k.c.border_base)
        .a11y_hidden()
}

/// A composable header without treating the title as a disabled menu action.
pub fn with_header<S: 'static>(
    key: &str,
    width: f32,
    header: El,
    items: El,
    base: &Kit,
    layers: &mut Vec<El>,
    close: fn(&mut S),
) {
    let k = kit(base);
    let anchor = format!("{key}-trigger");
    let rows = col()
        .key("rows")
        .items_stretch()
        .padding(4.)
        .child(header)
        .child(items);
    let surface = popover::raised(
        &k,
        col()
            .key(format!("{key}-menu"))
            .role("menu")
            .focus_group()
            .focus_return(&anchor)
            .w(width)
            .min_h(0.)
            .items_stretch()
            .child(
                scroll_bar(
                    &k,
                    &format!("{key}-scroll-area"),
                    "scroll",
                    popover::fit_scroll(rows),
                )
                .w_full()
                .shrink()
                .min_h(0.),
            ),
        6.,
    )
    .modal(true, close);
    layers.push(scrim(format!("{key}-scrim"), close));
    layers.push(popover::anchored(
        &k,
        surface,
        &anchor,
        Placement::BottomStart,
        4.,
    ));
}
