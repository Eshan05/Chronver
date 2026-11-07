use crate::{
    model::State,
    rich_select::{OptionRow, RichSelect},
};
use rune::{El, Icon, ScrollPos, Tx, WheelEvent, col, icon, row, text};
use rune_kit::{
    Button, ButtonVariant, IconButton, IconButtonVariant, Kit, Placement, glyphs, popover, scrim,
    scroll_bar,
};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, PartialEq)]
pub enum Menu {
    Channel,
    Cache,
    Category,
}
#[derive(Default)]
pub struct Controls {
    pub menu: Option<Menu>,
    pub inventory_open: Option<String>,
    pub inventory_technical: bool,
    pub release_index_open: bool,
    pub notes_position: Rc<Cell<ScrollPos>>,
    scroll: crate::smooth_scroll::Motion,
    scroll_owner: Option<String>,
}

pub fn local_inventory(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    const TRIGGER: &str = "local-inventory-trigger";
    let p = s.selected().expect("selected inventory observation");
    let open = s.reader_controls.inventory_open.as_deref() == Some(&p.id);
    let trigger = Button::new("Local inventory")
        .variant(ButtonVariant::Ghost)
        .icon(&glyphs::FILE_TEXT)
        .expanded(open)
        .help("Show installed observation")
        .on_press_tx(|s: &mut State, tx| {
            let id = s.selected().unwrap().id.clone();
            s.reader_controls.inventory_technical = false;
            s.reader_controls.inventory_open =
                if s.reader_controls.inventory_open.as_deref() == Some(&id) {
                    None
                } else {
                    Some(id)
                };
            tx.instant();
        })
        .build(k)
        .h(26.)
        .key(TRIGGER);
    if !open {
        return trigger;
    }
    let close = |s: &mut State| {
        s.reader_controls.inventory_open = None;
        s.reader_controls.inventory_technical = false;
    };
    let identity = col()
        .grow(1.)
        .min_w(0.)
        .gap(5.)
        .child(
            text(&p.name)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .wrap_anywhere(),
        )
        .children(
            p.provider
                .map(|manager| row().child(crate::provider_badge::badge(manager, k))),
        );
    let body = col()
        .w_full()
        .gap(10.)
        .padding(14.)
        .child(
            row()
                .w_full()
                .center_y()
                .gap(8.)
                .child(crate::ui::mark(p, k, 28.))
                .child(identity)
                .child(
                    IconButton::new(&glyphs::CLOSE)
                        .variant(IconButtonVariant::Ghost)
                        .label("Close inventory details")
                        .help("Close inventory details")
                        .on_press(close)
                        .build(k)
                        .size(24., 24.)
                        .key("local-inventory-close"),
                ),
        )
        .child(crate::package_info::inventory_view(
            s,
            p,
            s.reader_controls.inventory_technical,
            k,
        ));
    let surface = popover::raised(
        k,
        col()
            .key("local-inventory-popover")
            .role("dialog")
            .label("Local inventory")
            .focus_group()
            .focus_return(TRIGGER)
            .w(420.)
            .max_h(460.)
            .min_h(0.)
            .child(
                scroll_bar(
                    k,
                    "local-inventory-scroll-area",
                    "scroll",
                    popover::fit_scroll(body),
                )
                .w_full()
                .shrink()
                .min_h(0.),
            ),
        6.,
    )
    .fill(k.c.layer_01)
    .border(1., k.c.border_base)
    .modal(true, close);
    layers.push(scrim("local-inventory-scrim", close));
    layers.push(popover::anchored(
        k,
        surface,
        TRIGGER,
        Placement::BottomStart,
        4.,
    ));
    trigger
}

pub fn inventory_scene(s: &mut State) {
    crate::update_status::scene(s);
    let id = s.selected().unwrap().id.clone();
    s.live.sessions.get_mut(&id).unwrap().report = None;
    s.catalog
        .packages
        .iter_mut()
        .find(|p| p.id == id)
        .unwrap()
        .releases
        .clear();
    s.reader_controls.inventory_open = Some(id);
}

impl Controls {
    pub fn reset_scroll(&mut self) {
        self.scroll.reset();
        self.scroll_owner = None;
    }

    pub fn scroll_wheel(
        &mut self,
        owner: &str,
        event: WheelEvent,
        tx: &mut Tx,
        reduced: bool,
    ) -> bool {
        if self.scroll_owner.as_deref() != Some(owner) {
            self.reset_scroll();
            self.scroll_owner = Some(owner.into());
        }
        let handled = self.scroll.wheel(
            "release-notes",
            self.notes_position.get(),
            event,
            tx,
            reduced,
        );
        tx.live_only();
        handled
    }
}

static CHANNEL: Icon = Icon {
    name: "reader-channel",
    view: 24.,
    body: include_str!("../assets/status/git-branch.svg"),
};
static CACHE: Icon = Icon {
    name: "reader-cache",
    view: 24.,
    body: include_str!("../assets/symbols/database.svg"),
};
static REFRESH: Icon = Icon {
    name: "reader-refresh",
    view: 24.,
    body: include_str!("../assets/status/refresh-cw.svg"),
};
static OFFLINE: Icon = Icon {
    name: "reader-offline",
    view: 24.,
    body: include_str!("../assets/status/cloud-off.svg"),
};
static PACKAGE: Icon = Icon {
    name: "reader-package",
    view: 24.,
    body: include_str!("../assets/symbols/package.svg"),
};
static TOOLING: Icon = Icon {
    name: "reader-tooling",
    view: 24.,
    body: include_str!("../assets/symbols/terminal.svg"),
};

fn content(label: &str, glyph: &'static Icon, k: &Kit) -> El {
    row()
        .gap(6.)
        .center_y()
        .min_w(0.)
        .child(icon(glyph, 14.).color(k.c.text_muted))
        .child(
            text(label)
                .font(k.ty.ui_compact)
                .color(k.c.text_base)
                .grow(1.)
                .min_w(0.)
                .truncate(),
        )
}

fn select(s: &State, k: &Kit, layers: &mut Vec<El>, menu: Menu) -> El {
    let p = s.selected().unwrap();
    let q = &s.live.sessions[&p.id];
    let (key, label, current, choices, width): (_, _, _, Vec<(&str, &str, &'static Icon)>, _) =
        match menu {
            Menu::Channel => (
                "engine-channel",
                "Release channel",
                ["stable", "preview", "all"][q.channel],
                vec![
                    ("stable", "Stable", &CHANNEL),
                    ("preview", "Preview", &CHANNEL),
                    ("all", "All channels", &CHANNEL),
                ],
                116.,
            ),
            Menu::Cache => (
                "engine-cache",
                "Fetch cache policy",
                ["auto", "refresh", "offline"][q.cache],
                vec![
                    ("auto", "Auto cache", &CACHE),
                    ("refresh", "Refresh", &REFRESH),
                    ("offline", "Offline", &OFFLINE),
                ],
                122.,
            ),
            Menu::Category => {
                let tooling = p
                    .watchlist
                    .as_ref()
                    .is_some_and(|w| w.group == releasescout_engine::watchlist::Group::Tooling);
                (
                    "watchlist-group",
                    "Tracked package category",
                    if tooling { "tooling" } else { "core" },
                    vec![("core", "Core", &PACKAGE), ("tooling", "Tooling", &TOOLING)],
                    68.,
                )
            }
        };
    let selected = choices.iter().find(|c| c.0 == current).unwrap();
    let value = if menu == Menu::Category {
        row().center_y().child(crate::watchlist_ui::category_badge(
            p.watchlist.as_ref().unwrap().group,
            k,
        ))
    } else {
        content(
            if menu == Menu::Channel && current == "all" {
                "All"
            } else {
                selected.1
            },
            selected.2,
            k,
        )
    };
    let toggle: fn(&mut State) = match menu {
        Menu::Channel => |s| {
            s.reader_controls.menu =
                (s.reader_controls.menu != Some(Menu::Channel)).then_some(Menu::Channel)
        },
        Menu::Cache => |s| {
            s.reader_controls.menu =
                (s.reader_controls.menu != Some(Menu::Cache)).then_some(Menu::Cache)
        },
        Menu::Category => |s| {
            s.reader_controls.menu =
                (s.reader_controls.menu != Some(Menu::Category)).then_some(Menu::Category)
        },
    };
    let mut control = RichSelect {
        key,
        label,
        current,
        open: s.reader_controls.menu == Some(menu),
        popup_width: 188.,
        value,
        options: choices
            .into_iter()
            .map(|(id, label, glyph)| OptionRow {
                id: id.into(),
                label: label.into(),
                description: String::new(),
                content: content(label, glyph, k),
            })
            .collect(),
        on_toggle: toggle,
        on_close: |s: &mut State| s.reader_controls.menu = None,
        on_choose: |s: &mut State, value, tx| {
            match value {
                "stable" => s.engine_query().channel = 0,
                "preview" => s.engine_query().channel = 1,
                "all" => s.engine_query().channel = 2,
                "auto" => s.engine_query().cache = 0,
                "refresh" => s.engine_query().cache = 1,
                "offline" => s.engine_query().cache = 2,
                "core" => s.set_watchlist_group(0),
                "tooling" => s.set_watchlist_group(1),
                _ => return,
            }
            s.reader_controls.menu = None;
            tx.instant();
        },
    }
    .build_compact(k, layers);
    if menu == Menu::Category {
        control = control
            .fill(rune::Color::TRANSPARENT)
            .border(0., rune::Color::TRANSPARENT)
            .px(0.);
    }
    row().key(key).w(width).child(control)
}

pub fn category(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    select(s, k, layers, Menu::Category)
}

pub fn actions(s: &State, k: &Kit, layers: &mut Vec<El>, channel: bool) -> El {
    let p = s.selected().unwrap();
    let busy = s.live.active.as_deref() == Some(&p.id);
    row()
        .grow(1.)
        .min_w((if channel { 372. } else { 248. }) + if s.live.active.is_some() { 32. } else { 0. })
        .gap(8.)
        .center_y()
        .justify_end()
        .flex_wrap()
        .children(channel.then(|| select(s, k, layers, Menu::Channel)))
        .child(select(s, k, layers, Menu::Cache))
        .child(
            row()
                .role("button")
                .label(if busy { "Loading..." } else { "Load notes" })
                .focusable(true)
                .h(28.)
                .w(118.)
                .px(10.)
                .gap(6.)
                .center()
                .radius(6.)
                .fill(k.c.text_base)
                .hover(k.c.text_base.alpha(0.88))
                .focus_ring(2., 2., k.c.border_focus)
                .opacity(if busy { 0.5 } else { 1. })
                .disabled(busy)
                .on_press(|s: &mut State| s.fetch_notes())
                .child(
                    icon(&glyphs::ARROW_RIGHT, 14.)
                        .color(k.c.bg_base)
                        .a11y_hidden(),
                )
                .child(
                    text(if busy { "Loading..." } else { "Load notes" })
                        .font(k.ty.ui_compact)
                        .color(k.c.bg_base)
                        .a11y_hidden(),
                )
                .key("fetch-notes"),
        )
        .children(s.live.active.as_ref().map(|_| {
            IconButton::new(&glyphs::CLOSE)
                .variant(IconButtonVariant::Ghost)
                .label("Cancel notes fetch")
                .help("Cancel notes fetch")
                .on_press(|s: &mut State| {
                    s.live.work.cancel();
                    s.live.history.cancel();
                    if let Some(id) = s.live.active.take() {
                        s.live.sessions.get_mut(&id).unwrap().error = Some("Fetch canceled".into());
                    }
                })
                .build(k)
                .key("cancel-fetch")
        }))
}

pub fn area(key: &str) -> El {
    col().key(key).w_full().px(24.).pb(16.).gap(8.)
}

pub fn scene(s: &mut State, section: crate::model::LibrarySection) {
    match section {
        crate::model::LibrarySection::Software => {
            crate::sidebar_info::scene(s);
            let id = s
                .catalog
                .packages
                .iter()
                .find(|p| p.detected && p.name == "GitHub CLI")
                .unwrap()
                .id
                .clone();
            s.select_package(&id);
            let q = s.engine_query();
            q.range = true;
            q.from = rune::TextEdit::new("2.0.0");
            q.to = rune::TextEdit::new("3.0.0");
        }
        crate::model::LibrarySection::Extensions => crate::extension_ui::scene(s),
        crate::model::LibrarySection::Packages => crate::watchlist_ui::scene(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LibrarySection;
    use rune::{Env, Runtime};
    fn runtime(section: LibrarySection, dark: bool, width: f32) -> Runtime<State> {
        let mut state = State::demo();
        scene(&mut state, section);
        state.dark = dark;
        state.pane.set_width(width);
        let mut rt = Runtime::with_text(
            state,
            crate::ui::view,
            (1024., 640.),
            Env {
                system_dark: dark,
                scale: 1.,
            },
            rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES),
        );
        rt.on_key = Some(crate::ui::on_key);
        rt.frame(1.);
        rt
    }
    fn tap(rt: &mut Runtime<State>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.);
    }
    #[test]
    fn shared_channel_cache_and_category_menus_preserve_query_identity_without_fetching() {
        for dark in [true, false] {
            for section in [
                LibrarySection::Software,
                LibrarySection::Extensions,
                LibrarySection::Packages,
            ] {
                let mut rt = runtime(section, dark, 304.);
                if section == LibrarySection::Extensions {
                    tap(&mut rt, "extension-source/segment1");
                }
                let id = rt.state.selected().unwrap().id.clone();
                tap(&mut rt, "engine-cache-trigger");
                tap(&mut rt, "engine-cache-option-offline");
                assert_eq!(rt.state.live.sessions[&id].cache, 2);
                tap(&mut rt, "engine-channel-trigger");
                tap(&mut rt, "engine-channel-option-preview");
                assert_eq!(rt.state.live.sessions[&id].channel, 1);
                assert_eq!(rt.state.selected().unwrap().id, id);
                assert!(rt.state.reader_controls.menu.is_none());
                assert!(!rt.state.live.work.is_pending());
                if section == LibrarySection::Packages {
                    tap(&mut rt, "watchlist-group-trigger");
                    tap(&mut rt, "watchlist-group-option-tooling");
                    assert_eq!(
                        rt.state
                            .selected()
                            .unwrap()
                            .watchlist
                            .as_ref()
                            .unwrap()
                            .group,
                        releasescout_engine::watchlist::Group::Tooling
                    );
                    assert_eq!(rt.state.live.sessions[&id].cache, 2);
                }
            }
        }
    }
    #[test]
    fn multi_component_ranges_wrap_the_notes_actions_as_one_group() {
        let mut rt = runtime(LibrarySection::Software, true, 420.);
        rt.update(|s, _| {
            s.set_dataset(crate::model::Dataset::Reference);
            s.select_package("supabase");
            let component = s
                .live
                .catalog
                .packages
                .iter()
                .find(|p| p.id == "supabase")
                .unwrap()
                .streams
                .iter()
                .position(|stream| stream.basis == releasescout_engine::Basis::Version)
                .unwrap();
            let q = s.engine_query();
            q.component = component;
            q.range = true;
        });
        rt.frame(1.);
        let elements = rt.elements();
        let rect = |key: &str| {
            elements
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
                .rect
        };
        let mode = rect("engine-mode");
        let load = rect("fetch-notes");
        assert!(load[1] >= mode[1] + mode[3]);
        for key in ["engine-cache", "engine-channel"] {
            assert!((rect(key)[1] - load[1]).abs() < 1.);
        }
    }

    #[test]
    fn reader_controls_and_actions_fit_with_long_titles_and_the_widest_sidebar() {
        for dark in [true, false] {
            for section in [
                LibrarySection::Software,
                LibrarySection::Extensions,
                LibrarySection::Packages,
            ] {
                let mut rt = runtime(section, dark, 420.);
                rt.update(|s,_| {
                let id=s.selected().unwrap().id.clone();let p=s.catalog.packages.iter_mut().find(|p|p.id==id).unwrap();
                p.name="A deliberately long package title that should truncate rather than collide with actions".into();
                p.description=Some("A useful package description with a long sentence that stays within two lines. ".repeat(5));
            });
                rt.frame(1.);
                let elements = rt.elements();
                for key in [
                    "reader-header",
                    "reader-actions",
                    "selected-package",
                    "open-release",
                    "copy-notes",
                    "version-range",
                    "fetch-notes",
                    "engine-cache",
                    "note-mode",
                ] {
                    let e = elements
                        .iter()
                        .find(|e| e.key.as_deref() == Some(key))
                        .unwrap_or_else(|| panic!("missing {key}"));
                    let [x, y, w, h] = e.rect;
                    assert!(
                        x >= 420. && x + w <= 1024.5 && y >= 48. && y + h <= 640.5,
                        "{key}: {:?}",
                        e.rect
                    );
                }
                let title = elements
                    .iter()
                    .find(|e| e.key.as_deref() == Some("selected-package"))
                    .unwrap();
                let actions = elements
                    .iter()
                    .find(|e| e.key.as_deref() == Some("reader-actions"))
                    .unwrap();
                let metadata = elements
                    .iter()
                    .find(|e| e.key.as_deref() == Some("version-range"))
                    .unwrap();
                assert!(
                    metadata.rect[1] >= actions.rect[1] + actions.rect[3],
                    "metadata must sit below the reader actions"
                );
                assert!(
                    (metadata.rect[0] + metadata.rect[2] - actions.rect[0] - actions.rect[2]).abs()
                        < 1.,
                    "metadata and actions must share a right edge"
                );
                assert!(
                    title.rect[0] + title.rect[2] <= actions.rect[0]
                        || title.rect[1] + title.rect[3] <= actions.rect[1]
                );
                let tabs = elements
                    .iter()
                    .find(|e| e.key.as_deref() == Some("note-mode"))
                    .unwrap();
                assert!(
                    tabs.rect[1] + tabs.rect[3] < 470.,
                    "reader chrome leaves too little room for notes: {:?}",
                    tabs.rect
                );
            }
        }
    }
}
