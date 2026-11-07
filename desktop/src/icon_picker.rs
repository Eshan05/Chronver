use crate::{
    custom_icons::identity,
    model::{Package, State},
};
use rune::{El, Icon, Image, ScrollSpec, TextEdit, col, el, icon, row, text};
use rune_kit::{Button, Dialog, Kit, SegmentedControl, TextInput, glyphs};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Choice {
    App(String),
    Symbol(String),
}
impl Choice {
    pub fn valid(&self) -> bool {
        match self {
            Self::App(key) => key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
            Self::Symbol(id) => symbol(id).is_some(),
        }
    }
}
macro_rules! symbol_asset {
    ($name:ident, $asset:literal) => {
        static $name: Icon = Icon {
            name: concat!("symbol-", $asset),
            view: 24.,
            body: include_str!(concat!("../assets/symbols/", $asset, ".svg")),
        };
    };
}
symbol_asset!(TERMINAL, "terminal");
symbol_asset!(DATABASE, "database");
symbol_asset!(PACKAGE, "package");
symbol_asset!(GLOBE, "globe");
const SYMBOLS: &[(&str, &str, &Icon)] = &[
    ("terminal", "Terminal", &TERMINAL),
    ("code", "Code", &glyphs::FILE_CODE),
    ("database", "Database", &DATABASE),
    ("package", "Package", &PACKAGE),
    ("globe", "Web", &GLOBE),
    ("folder", "Folder", &glyphs::FOLDER),
    ("document", "Document", &glyphs::FILE_TEXT),
    ("image", "Image", &glyphs::FILE_IMAGE),
    ("audio", "Audio", &glyphs::MIC),
    ("security", "Security", &glyphs::LOCK),
    ("keyboard", "Keyboard", &glyphs::KEYBOARD),
    ("automation", "Automation", &glyphs::SPARKLE),
];
pub fn symbol(id: &str) -> Option<&'static Icon> {
    SYMBOLS.iter().find(|s| s.0 == id).map(|s| s.2)
}
pub struct Picker {
    pub target: String,
    name: String,
    query: TextEdit,
    mode: usize,
    shown: usize,
}
struct AppIcon {
    key: String,
    name: String,
    image: Image,
}
fn app_icons(s: &State) -> Vec<AppIcon> {
    let mut icons = BTreeMap::new();
    for p in &s.catalog.packages {
        let key = identity(p);
        // References never resolve through other references, avoiding cycles.
        if s.activity.data.icon_choices.contains_key(&key) {
            continue;
        }
        let image = s
            .custom_icons
            .loaded
            .get(&key)
            .and_then(|i| i.as_ref())
            .or(p.icon.as_ref());
        if let Some(image) = image.filter(|i| i.size().is_some()) {
            icons.entry(key.clone()).or_insert_with(|| AppIcon {
                key,
                name: p.name.clone(),
                image: image.clone(),
            });
        }
    }
    let mut icons: Vec<_> = icons.into_values().collect();
    icons.sort_by_cached_key(|i| i.name.to_lowercase());
    icons
}
pub fn app_pool(s: &State) -> HashMap<String, Image> {
    app_icons(s).into_iter().map(|i| (i.key, i.image)).collect()
}
impl State {
    pub fn open_icon_picker(&mut self) {
        if self.custom_icons.dialog || self.custom_icons.work.is_pending() {
            return;
        }
        if let Some(p) = self.selected() {
            self.custom_icons.picker = Some(Picker {
                target: identity(p),
                name: p.name.clone(),
                query: TextEdit::new(""),
                mode: 0,
                shown: 80,
            });
        }
    }
    pub fn set_existing_icon(&mut self, choice: Choice) {
        let Some(picker) = &self.custom_icons.picker else {
            return;
        };
        if !choice.valid() {
            return;
        }
        if let Choice::App(source) = &choice
            && (source == &picker.target || !app_pool(self).contains_key(source))
        {
            return;
        }
        let target = picker.target.clone();
        self.activity.data.icon_choices.insert(target, choice);
        self.activity.dirty = true;
        self.custom_icons.error = None;
        self.custom_icons.picker = None;
        self.poll_custom_icons();
        self.status = "Icon choice saved locally".into();
    }
    pub fn has_icon_override(&self, p: &Package) -> bool {
        p.custom_icon.is_some()
            || p.custom_symbol.is_some()
            || self.activity.data.icon_choices.contains_key(&identity(p))
    }
}
fn tile(name: &str, choice: Choice, visual: El, selected: bool, k: &Kit) -> El {
    let key = match &choice {
        Choice::App(id) => format!("icon-app-{id}"),
        Choice::Symbol(id) => format!("icon-symbol-{id}"),
    };
    col()
        .size(64., 64.)
        .gap(4.)
        .center()
        .radius(6.)
        .fill(if selected { k.c.layer_02 } else { k.c.layer_01 })
        .border(
            1.,
            if selected {
                k.c.text_base
            } else {
                k.c.border_muted
            },
        )
        .hover(k.c.layer_02)
        .role("button")
        .label(name)
        .help(name)
        .focusable(true)
        .on_press(move |s: &mut State| s.set_existing_icon(choice.clone()))
        .child(visual.size(28., 28.).a11y_hidden())
        .child(
            text(name)
                .font(k.ty.small_medium)
                .color(k.c.text_muted)
                .w(56.)
                .truncate()
                .a11y_hidden(),
        )
        .key(key)
}
pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>) {
    let Some(picker) = &s.custom_icons.picker else {
        return;
    };
    let query = picker.query.text.trim().to_lowercase();
    let selected = s.activity.data.icon_choices.get(&picker.target);
    let mut grid = row().w_full().flex_wrap().gap(8.);
    let mut count = 0;
    if picker.mode == 0 {
        for app in app_icons(s)
            .into_iter()
            .filter(|i| i.key != picker.target && i.name.to_lowercase().contains(&query))
        {
            count += 1;
            if count > picker.shown {
                continue;
            }
            let choice = Choice::App(app.key);
            grid = grid.child(tile(
                &app.name,
                choice.clone(),
                el().pixels_fit(app.image.pixels(), rune::SurfaceFit::Fit),
                selected == Some(&choice),
                k,
            ));
        }
    } else {
        for &(id, name, glyph) in SYMBOLS
            .iter()
            .filter(|s| s.1.to_lowercase().contains(&query))
        {
            count += 1;
            let choice = Choice::Symbol(id.into());
            grid = grid.child(tile(
                name,
                choice.clone(),
                icon(glyph, 28.).color(k.c.text_base),
                selected == Some(&choice),
                k,
            ));
        }
    }
    let mut options = col().w_full().gap(12.).child(grid);
    if count == 0 {
        options = options.child(
            text("No matching icons")
                .font(k.ty.ui)
                .color(k.c.text_muted),
        );
    }
    if picker.mode == 0 && count > picker.shown {
        options = options.child(
            Button::new("Show more")
                .on_press(|s: &mut State| {
                    if let Some(p) = &mut s.custom_icons.picker {
                        p.shown += 80;
                    }
                })
                .build(k)
                .key("icon-show-more"),
        );
    }
    let body = col()
        .w_full()
        .grow(1.)
        .min_h(0.)
        .padding(16.)
        .gap(12.)
        .child(
            row()
                .w_full()
                .center_y()
                .gap(8.)
                .child(
                    text(&picker.name)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .grow(1.)
                        .min_w(0.)
                        .truncate(),
                )
                .child(
                    Button::new("Automatic")
                        .icon(&crate::custom_icons::RESET)
                        .on_press(|s: &mut State| s.reset_icon())
                        .build(k)
                        .key("icon-automatic"),
                )
                .child(
                    Button::new("From file")
                        .icon(&crate::custom_icons::CHOOSE)
                        .on_press_tx(|s: &mut State, tx| s.choose_icon(tx))
                        .build(k)
                        .key("icon-from-file"),
                ),
        )
        .child(
            TextInput::new(&picker.query)
                .label("Find icons")
                .placeholder("Find icons...")
                .on_edit(|s: &mut State| &mut s.custom_icons.picker.as_mut().unwrap().query)
                .build(k)
                .w_full()
                .key("icon-filter"),
        )
        .child(
            SegmentedControl::new(&["App icons", "Symbols"], Some(picker.mode))
                .label("Icon collection")
                .on_pick(|s: &mut State, i, tx| {
                    if let Some(p) = &mut s.custom_icons.picker {
                        p.mode = i;
                        p.shown = 80;
                    }
                    tx.scroll_to_instant("icon-options", 0., 0.);
                })
                .build(k)
                .key("icon-collection"),
        )
        .child(
            col()
                .w_full()
                .grow(1.)
                .min_h(0.)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(options)
                .key("icon-options"),
        );
    Dialog::new("icon-picker", "Choose icon")
        .open(true)
        .width(600.)
        .on_close(|s: &mut State| s.custom_icons.picker = None)
        .child(body)
        .build(k, layers);
}
pub fn scene(s: &mut State) {
    crate::custom_icons::scene(s);
    let image = s.selected().unwrap().custom_icon.clone();
    for p in &mut s.catalog.packages {
        if p.name != "Audacity" {
            p.icon = image.clone();
        }
    }
    s.open_icon_picker();
}
pub fn symbols_scene(s: &mut State) {
    if let Some(p) = &mut s.custom_icons.picker {
        p.mode = 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sources_are_ready_and_deduplicated_and_references_cannot_chain() {
        let mut s = State::new();
        scene(&mut s);
        let pool = app_pool(&s);
        assert!(!pool.is_empty());
        let target = s.custom_icons.picker.as_ref().unwrap().target.clone();
        let source = pool.keys().find(|k| **k != target).unwrap().clone();
        s.set_existing_icon(Choice::App(source.clone()));
        let image = s.selected().unwrap().custom_icon.as_ref().unwrap();
        assert_eq!(image.id(), pool[&source].id());
        assert!(!app_pool(&s).contains_key(&target));
        s.activity
            .data
            .icon_choices
            .insert(source, Choice::Symbol("folder".into()));
        s.poll_custom_icons();
        assert!(s.selected().unwrap().custom_icon.is_none());
        assert!(s.has_icon_override(s.selected().unwrap()));
        assert!(!Choice::App("../../escape".into()).valid());
        assert!(!Choice::Symbol("unknown".into()).valid());
    }
    #[test]
    fn every_symbol_renders_nonblank_coverage() {
        for &(_, _, glyph) in SYMBOLS {
            assert!(
                rune::text::icon_coverage(glyph, 28)
                    .iter()
                    .filter(|&&p| p > 32)
                    .count()
                    > 20,
                "{}",
                glyph.name
            );
        }
    }
    #[test]
    fn search_filters_both_collections_without_mutating_preferences() {
        let mut s = State::new();
        scene(&mut s);
        let mut rt = rune::Runtime::new(
            s,
            crate::ui::view,
            (1024., 640.),
            rune::Env {
                system_dark: true,
                scale: 1.,
            },
        );
        for mode in [0, 1] {
            rt.update(|s, _| {
                let picker = s.custom_icons.picker.as_mut().unwrap();
                picker.mode = mode;
                picker.query = TextEdit::new("nothing-matches");
            });
            rt.frame(1.);
            assert!(!rt.elements().iter().any(|e| {
                e.key
                    .as_deref()
                    .is_some_and(|k| k.starts_with("icon-app-") || k.starts_with("icon-symbol-"))
            }));
            assert!(rt.state.activity.data.icon_choices.is_empty());
        }
        rt.update(|s, _| s.custom_icons.picker.as_mut().unwrap().query = TextEdit::new("TeRMiNaL"));
        rt.frame(2.);
        assert!(rt.find("icon-symbol-terminal").is_some());
        assert!(rt.find("icon-symbol-folder").is_none());
    }
}
