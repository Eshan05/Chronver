use releasescout_engine::inventory::Manager;
use rune::{El, Icon, Image, ImageCache, icon, row, text};
use rune_kit::Kit;
use std::sync::{Arc, LazyLock};

macro_rules! vector {
    ($name:ident, $asset:literal) => {
        static $name: Icon = Icon {
            name: concat!("provider-", $asset),
            view: 24.0,
            body: include_str!(concat!("../assets/providers/", $asset, ".svg")),
        };
    };
}
vector!(NPM, "npm");
vector!(PNPM, "pnpm");
vector!(BUN, "bun");
vector!(CHOCO, "chocolatey");
vector!(STORE, "store");
vector!(LOCAL, "monitor");

static UNSUPPORTED: Icon = Icon {
    name: "provider-status-unsupported", view: 24.,
    body: include_str!("../assets/status/ban.svg"),
};

// Bundled raster assets, decoded once and shared by badges and profile headers.
static IMAGES: LazyLock<[Image; 6]> = LazyLock::new(|| {
    let cache = ImageCache::new(Arc::new(|| {}));
    let images = [
        cache.load(&include_bytes!("../assets/providers/winget.png")[..]),
        cache.load(&include_bytes!("../assets/providers/scoop.ico")[..]),
        cache.load(&include_bytes!("../assets/providers/vsc.ico")[..]),
        cache.load(&include_bytes!("../assets/providers/vsc-ins.png")[..]),
        cache.load(&include_bytes!("../assets/providers/cursor-light.png")[..]),
        cache.load(&include_bytes!("../assets/providers/cursor-dark.png")[..]),
    ];
    for image in &images {
        image.wait().expect("valid bundled provider icon");
    }
    images
});

pub fn label(manager: Manager) -> &'static str {
    match manager {
        Manager::Npm => "NPM",
        Manager::Pnpm => "PNPM",
        Manager::Bun => "Bun",
        Manager::Winget => "WinGet",
        Manager::Scoop => "Scoop",
        Manager::Chocolatey => "Choco",
        Manager::Store => "Store",
        Manager::VscodeInsiders => editor_label(true),
        Manager::Local => "Local",
        Manager::Vscode => editor_label(false),
        Manager::Cursor => "Cursor",
    }
}

pub fn editor_label(insiders: bool) -> &'static str {
    if insiders { "VSC-Ins" } else { "VSC" }
}

fn ink(manager: Manager, k: &Kit) -> rune::Color {
    let (dark, light) = match manager {
        Manager::Npm => ("#d99090", "#a84a4a"),
        Manager::Pnpm => ("#dfbd73", "#946716"),
        Manager::Bun => ("#d6c5b3", "#907255"),
        Manager::Chocolatey => ("#85b0cd", "#477c9c"),
        Manager::Store => ("#91b1d0", "#4c78a1"),
        Manager::Winget => ("#d3b277", "#916e32"),
        Manager::Scoop => ("#b4a1cf", "#7b639e"),
        Manager::VscodeInsiders => ("#85bca5", "#397b62"),
        Manager::Local => return k.c.text_muted,
        Manager::Vscode => ("#87b6d4", "#39779f"),
        Manager::Cursor => return k.c.text_muted,
    };
    rune::color::hex(if k.dark { dark } else { light }).unwrap()
}

pub fn symbol(manager: Manager, k: &Kit) -> El {
    symbol_sized(manager, k, 12.)
}

pub fn symbol_sized(manager: Manager, k: &Kit, size: f32) -> El {
    let raster = match manager {
        Manager::Winget => Some(0),
        Manager::Scoop => Some(1),
        Manager::VscodeInsiders => Some(3),
        Manager::Vscode => Some(2),
        Manager::Cursor => Some(if k.dark {5} else {4}),
        _ => None,
    };
    if let Some(index) = raster {
        return rune::el().size(size, size).pixels(IMAGES[index].pixels());
    }
    let glyph = match manager {
        Manager::Npm => &NPM,
        Manager::Pnpm => &PNPM,
        Manager::Bun => &BUN,
        Manager::Chocolatey => &CHOCO,
        Manager::Store => &STORE,
        Manager::Local => &LOCAL,
        _ => unreachable!(),
    };
    icon(glyph, size).color(ink(manager, k))
}

pub fn badge(manager: Manager, k: &Kit) -> El {
    if matches!(manager, Manager::VscodeInsiders | Manager::Vscode) {
        return editor_badge(manager == Manager::VscodeInsiders, k);
    }
    build(
        label(manager),
        symbol(manager, k),
        manager.id(),
        ink(manager, k),
        k,
    )
}

pub fn editor_badge(insiders: bool, k: &Kit) -> El {
    build(
        editor_label(insiders),
        rune::el()
            .size(12.0, 12.0)
            .pixels(IMAGES[if insiders { 3 } else { 2 }].pixels()),
        if insiders {
            "vscode-insiders"
        } else {
            "vscode"
        },
        if insiders {
            ink(Manager::VscodeInsiders, k)
        } else {
            rune::color::hex(if k.dark { "#87b6d4" } else { "#39779f" }).unwrap()
        },
        k,
    )
}

fn build(label: &str, symbol: El, id: &str, tint: rune::Color, k: &Kit) -> El {
    row()
        .h(18.0)
        .px(4.0)
        .gap(4.0)
        .center_y()
        .radius(4.0)
        .fill(tint.alpha(if k.dark { 0.12 } else { 0.08 }))
        .border(0.5, tint.alpha(0.3))
        .role("group")
        .label(label)
        .key(format!("provider-badge-{id}"))
        .child(symbol.a11y_hidden())
        .child(
            text(label)
                .font(k.ty.small_medium)
                .color(k.c.text_muted.mix(tint, 0.65))
                .a11y_hidden(),
        )
}

pub fn scene(s: &mut crate::model::State) {
    for manager in Manager::ALL {
        let scan = serde_json::from_value(serde_json::json!({
            "manager":manager,"checked_at":"2026-11-12T00:00:00Z",
            "update_check_requested":false,"update_check_succeeded":false,"warnings":[],
            "packages":[{"manager":manager,"id":format!("provider-fixture-{}",manager.id()),
                "name":format!("{} tool",label(manager)),"installed":"1.0.0","available":null,
                "source":"fixture","scope":"user","root":null,"pinned":false,
                "install_origin_known":false,"evidence":{"has_commands":true,"has_visible_app":true}}]
        })).unwrap();
        s.accept_scan(scan);
    }
}

pub fn unique(managers: impl IntoIterator<Item = Manager>) -> Vec<Manager> {
    let mut result = Vec::new();
    for manager in managers {
        if !result.contains(&manager) {
            result.push(manager);
        }
    }
    result
}

pub fn strip(managers: &[Manager], observations: usize, k: &Kit) -> El {
    strip_with_status(managers, observations, None, k)
}

pub fn strip_with_status(
    managers: &[Manager], observations: usize,
    status: Option<crate::sidebar_filters::Assessment>, k: &Kit,
) -> El {
    let mut strip = row().h(18.0).gap(4.0).center_y().help(format!(
        "{} / {observations} observations",
        managers
            .iter()
            .map(|m| label(*m))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    for &manager in managers.iter().take(1) {
        let mut chip = badge(manager, k);
        if let Some(status) = status.filter(|s|s.reason.is_some()) {
            static INFO: Icon = Icon {
                name: "provider-status-info", view: 24.,
                body: include_str!("../assets/status/info.svg"),
            };
            let glyph = if status.reason == Some(crate::sidebar_filters::UnknownReason::Unsupported) {
                &UNSUPPORTED
            } else { &INFO };
            chip = chip
                .label(format!("{} / Manager updates: {}", label(manager), status.label()))
                .description(status.detail())
                .help(format!("Group update status: {}\n{}", status.label(), status.detail()))
                .child(icon(glyph, 12.).color(k.c.text_muted)
                    .key("sidebar-provider-status").a11y_hidden());
        }
        strip = strip.child(chip);
    }
    if managers.len() > 1 {
        strip = strip.child(
            text(format!("+{}", managers.len() - 1))
                .font(k.ty.small_medium)
                .color(k.c.text_faint),
        );
    }
    strip
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_labels_and_typed_deduplication() {
        assert_eq!(
            Manager::ALL.map(label),
            [
                "WinGet", "Scoop", "Choco", "NPM", "PNPM", "Bun", "Store", "VSC-Ins", "Local",
                "VSC", "Cursor"
            ]
        );
        assert_eq!(editor_label(false), "VSC");
        assert_eq!(
            unique([Manager::Scoop, Manager::Winget, Manager::Scoop]),
            [Manager::Scoop, Manager::Winget]
        );
    }

    #[test]
    fn vector_brand_tints_are_theme_aware_and_local_stays_neutral() {
        for dark in [false, true] {
            let k = crate::theme::kit(dark);
            assert_eq!(ink(Manager::Local, &k), k.c.text_muted);
            for manager in [
                Manager::Npm,
                Manager::Pnpm,
                Manager::Bun,
                Manager::Chocolatey,
                Manager::Store,
            ] {
                assert_ne!(ink(manager, &k), k.c.text_muted);
                assert_ne!(ink(manager, &k), ink(manager, &crate::theme::kit(!dark)));
            }
        }
    }

    #[test]
    fn bundled_raster_icons_decode_offline() {
        for image in IMAGES.iter() {
            let size = image.size().unwrap();
            assert!(size[0] >= 12 && size[1] >= 12, "{size:?}");
        }
    }

    #[test]
    fn vector_icons_have_visible_pixels_at_badge_size() {
        for glyph in [&NPM, &PNPM, &BUN, &CHOCO, &STORE, &LOCAL, &UNSUPPORTED] {
            let pixels = rune::text::icon_coverage(glyph, 12);
            assert!(
                pixels.iter().filter(|&&p| p > 32).count() > 10,
                "{} is blank",
                glyph.name
            );
            assert!(
                pixels.iter().filter(|&&p| p > 250).count() < 144,
                "{} is a solid square",
                glyph.name
            );
        }
    }

    #[test]
    fn compact_badges_and_overflow_fit_sidebar_without_live_regions() {
        use rune::{Env, Runtime, col};
        for dark in [false, true] {
            let mut rt = Runtime::new(
                dark,
                |dark, _| {
                    let k = crate::theme::kit(*dark);
                    col()
                        .w(168.0)
                        .gap(4.0)
                        .children(Manager::ALL.map(|m| badge(m, &k)))
                        .child(editor_badge(false, &k))
                        .child(strip(&Manager::ALL, 12, &k))
                        .child(strip(&[Manager::VscodeInsiders], 99, &k))
                },
                (168.0, 400.0),
                Env {
                    system_dark: dark,
                    scale: 1.0,
                },
            );
            rt.frame(0.0);
            for element in rt.elements() {
                assert_ne!(element.role, "status");
                if element
                    .key
                    .as_deref()
                    .is_some_and(|key| key.starts_with("provider-badge-"))
                {
                    assert_eq!(element.rect[3], 18.0);
                }
                let [x, y, w, h] = element.rect;
                assert!(
                    x >= 0.0 && y >= 0.0 && x + w <= 168.5 && y + h <= 400.5,
                    "{:?}: {:?}",
                    element.key,
                    element.rect
                );
            }
        }
    }
}
