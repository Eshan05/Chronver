use crate::model::State;
use rune::text;
use rune::{
    El, GrowthSpec, Key, KeyEvent, ScrollPos, ScrollSpec, Tx, WheelEvent, col, icon, row,
};
use rune_kit::{Kit, glyphs, scroll_bar};
use std::{
    borrow::Cow,
    cell::Cell,
    rc::Rc,
};

pub const ROW_PADDING_Y: f32 = 8.0;

pub fn system_switch(s: &State, k: &Kit) -> El {
    let on = s.show_components;
    // shadcn's default: 32 x 18.4 track, 16 px thumb and a 14 px checked translation.
    let track = if on {
        k.c.text_accent
    } else if k.dark {
        rune::Color::WHITE.alpha(0.12)
    } else {
        k.c.border_base
    };
    rune::el()
        .key("show-components")
        .role("switch")
        .checked(on)
        .label("Show system & support")
        .focusable(true)
        .size(32., 18.4)
        .radius(9.2)
        .focus_ring(3., 0., k.c.border_focus.alpha(0.5))
        .on_press(|s: &mut State| s.toggle_components())
        .help("Show system & support")
        .center()
        .child(
            rune::el()
                .key("control")
                .size(32., 18.4)
                .radius(9.2)
                .fill(track)
                .shadow(1., 2., rune::Color::BLACK.alpha(0.05))
                .motion(k.motion.control)
                .a11y_hidden()
                .child(
                    rune::el()
                        .key("thumb")
                        .size(16., 16.)
                        .radius(8.)
                        .absolute(if on { 15. } else { 1. }, 1.2)
                        .fill(if on && k.dark {
                            k.c.layer_01
                        } else if !k.dark {
                            k.c.bg_base
                        } else {
                            k.c.text_base
                        })
                        .motion(k.motion.control)
                        .center()
                        .child(
                            icon(&SYSTEM, 12.)
                                .key("system-switch-icon")
                                .color(if on || !k.dark {
                                    k.c.text_base
                                } else {
                                    k.c.bg_base
                                })
                                .a11y_hidden(),
                        ),
                ),
        )
}

static SYSTEM: rune::Icon = rune::Icon {
    name: "system-switch",
    view: 24.,
    body: include_str!("../assets/providers/monitor.svg"),
};
// Only a starting estimate for unvisited virtual rows; layout measures each row's content.
const ROW_ESTIMATE: f32 = 80.0;

#[derive(Default)]
pub struct WheelScroll {
    pub position: Rc<Cell<ScrollPos>>,
    pub visible_rows: std::ops::Range<usize>,
    pub hover_after: f64,
    motion: crate::smooth_scroll::Motion,
}

impl WheelScroll {
    pub fn reset(&mut self) {
        self.motion.reset();
    }

    fn wheel(&mut self, event: WheelEvent, tx: &mut Tx, reduced: bool, repaint: bool) -> bool {
        let handled = self.motion.wheel("viewport", self.position.get(), event, tx, reduced);
        if !repaint {
            tx.live_only();
        }
        handled
    }
}

fn navigate(s: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    let shown = crate::sidebar_groups::shown(s);
    if shown.is_empty() || event.mods.alt || event.mods.ctrl || event.mods.logo || event.mods.cmd {
        return false;
    }
    let count = shown.len();
    let at = s
        .table
        .cursor()
        .and_then(|id| shown.iter().position(|&i| s.rows[i].id == id));
    let page = s
        .sidebar_scroll
        .visible_rows
        .clone()
        .filter(|&i| crate::sidebar_groups::row_index(s, i).is_some())
        .count()
        .saturating_sub(1)
        .max(1);
    let index = match event.key {
        Key::Up => at.map_or(count - 1, |i| i.saturating_sub(1)),
        Key::Down => at.map_or(0, |i| (i + 1).min(count - 1)),
        Key::Home => 0,
        Key::End => count - 1,
        Key::PageUp => at.unwrap_or(0).saturating_sub(page),
        Key::PageDown => (at.unwrap_or(0) + page).min(count - 1),
        _ => return false,
    };
    let id = s.rows[shown[index]].id.clone();
    s.sidebar_scroll.reset();
    s.table.reveal(id, tx);
    s.mark_current_notes_read();
    tx.scroll_to_instant("release-notes", 0.0, 0.0);
    true
}

pub fn list(
    s: &State,
    k: &Kit,
    reduced: bool,
    build: fn(&State, usize, &Kit) -> El,
    layers: &mut Vec<El>,
) -> El {
    let row_kit = k.clone();
    let rows = col()
        .key("rows")
        .w_full()
        .growth(GrowthSpec::default())
        .virtual_rows(
            crate::sidebar_groups::len(s),
            ROW_ESTIMATE,
            |s: &State, i| Cow::Borrowed(crate::sidebar_groups::item_key(s, i)),
            move |s: &State, i| {
                let Some(i) = crate::sidebar_groups::row_index(s, i) else {
                    return crate::sidebar_groups::header(s, i, &row_kit)
                        .key(crate::sidebar_groups::item_key(s, i));
                };
                let id = s.rows[s.visible[i]].id.clone();
                let selected = s.table.cursor() == Some(id.as_str());
                build(s, i, &row_kit)
                    .key(id.clone())
                    .role("row")
                    .focus_selected(selected)
                    .w_full()
                    .py(ROW_PADDING_Y)
                    .center_y()
                    .hover_group_focus()
                    .radius(6.0)
                    .border(
                        1.0,
                        row_kit.c.text_base.alpha(if selected { 0.04 } else { 0.0 }),
                    )
                    .fill(
                        row_kit
                            .c
                            .text_base
                            .alpha(if selected { 0.065 } else { 0.0 }),
                    )
                    .hover(
                        row_kit
                            .c
                            .text_base
                            .alpha(if selected { 0.1 } else { 0.035 }),
                    )
                    .on_press_tx(move |s: &mut State, tx| {
                        s.table.select(id.clone());
                        s.mark_current_notes_read();
                        tx.focus("packages", false);
                        tx.scroll_to_instant("release-notes", 0.0, 0.0);
                    })
            },
        )
        .on_visible_rows(|s: &mut State, range| {
            s.sidebar_scroll.visible_rows = range;
            false
        })
        .virtual_rows_version(s.list_revision);
    let position = s.sidebar_scroll.position.clone();
    let viewport = col()
        .key("viewport")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .on_scroll_offset(move |at| position.set(at))
        .on_wheel(move |s: &mut State, event, tx| {
            let repaint = s.description_hover.take().is_some();
            s.sidebar_scroll.hover_after = tx.clock() + 0.6;
            s.sidebar_scroll.wheel(event, tx, reduced, repaint)
        })
        .child(if s.visible.is_empty() {
            col().w_full().py(32.).center().child(
                text(match s.section {
                    crate::model::LibrarySection::Software
                        if s.rows.is_empty() && s.dataset == crate::model::Dataset::Detected =>
                    {
                        if crate::app_chrome::inventory_busy(s) > 0 {
                            "Scanning for installed software..."
                        } else {
                            "No software detected"
                        }
                    }
                    crate::model::LibrarySection::Software => "No matching software",
                    crate::model::LibrarySection::Extensions => "No matching extensions",
                    crate::model::LibrarySection::Packages => "No matching packages",
                })
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted),
            )
        } else {
            rows
        });
    let name = col()
        .grow(1.0)
        .min_w(120.)
        .role("columnheader")
        .label(if s.section == crate::model::LibrarySection::Packages {
            "Packages"
        } else if s.section == crate::model::LibrarySection::Extensions {
            "Extensions"
        } else {
            "Software"
        })
        .sorted(
            s.table
                .sort()
                .map(|(_, dir)| dir == rune_kit::SortDir::Ascending),
        )
        .child(crate::sidebar_sort::controls(s, k, layers));
    let header = row()
        .key("header")
        .role("row")
        .w_full()
        .flex_wrap()
        .min_h(32.0)
        .py(2.)
        .gap(4.)
        .center_y()
        .child(name)
        .child(crate::sidebar_actions::selection_controls(s, k))
        .children(s.activity.error.as_ref().map(|error| {
            icon(&glyphs::CIRCLE_X, 14.0)
                .color(k.c.text_muted)
                .key("read-state-error")
                .label("Preferences unavailable or not saved")
                .help(error)
        }));
    let mut root = col()
        .key("packages")
        .role("grid")
        .label(if s.section == crate::model::LibrarySection::Packages {
            "Packages"
        } else if s.section == crate::model::LibrarySection::Extensions {
            "Extensions"
        } else {
            "Software"
        })
        .focusable(true)
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .clip_soft(0.0)
        .on_key(navigate)
        .child(header)
        .child(
            scroll_bar(k, "package-scroll", "viewport", viewport)
                .grow(1.0)
                .min_h(0.0),
        );
    if let Some(id) = s.table.cursor().filter(|id| {
        crate::sidebar_groups::shown(s)
            .iter()
            .any(|&i| s.rows[i].id == *id)
    }) {
        root = root.active_descendant(id.to_owned());
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_rows_measure_content_and_resize_without_a_height_constraint() {
        fn entry(s: &State, i: usize, k: &Kit) -> El {
            let lines = if i == 0 {
                1
            } else if s.dark {
                5
            } else {
                3
            };
            col().children((0..lines).map(|_| text("Content").font(k.ty.ui.lh(20.))))
        }
        fn view(s: &State, _: &mut rune::Cx<State>) -> El {
            let mut layers = vec![];
            let list = list(s, &crate::theme::kit(s.dark), true, entry, &mut layers);
            col().w_full().h_full().child(list).children(layers)
        }
        fn heights(rt: &rune::Runtime<State>) -> Vec<f32> {
            rt.elements()
                .iter()
                .filter(|e| e.role == "row" && e.key.as_deref() != Some("header"))
                .map(|e| e.rect[3])
                .collect()
        }
        let mut s = State::demo();
        s.visible.truncate(2);
        let mut rt = rune::Runtime::new(
            s,
            view,
            (280., 320.),
            rune::Env {
                system_dark: true,
                scale: 1.,
            },
        );
        for _ in 0..6 {
            rt.frame(1. / 60.);
        }
        let before = heights(&rt);
        assert_eq!(before.len(), 2);
        assert!(before[1] > before[0] + 60., "{before:?}");
        assert!(
            before[1] > ROW_ESTIMATE,
            "tall content must not be clipped to the estimate"
        );
        assert_eq!(rt.state.sidebar_scroll.visible_rows, 0..2);
        rt.update(|s, _| s.dark = false);
        rt.frame(1.);
        rt.frame(2.);
        let after = heights(&rt);
        assert!(after[1] < before[1], "{before:?} -> {after:?}");
        assert!(after[1] > after[0]);
    }

    #[test]
    fn reduced_motion_and_modified_wheel_do_not_add_smoothing() {
        let mut state = WheelScroll::default();
        let mut at = ScrollPos::default();
        at.max[1] = 1000.0;
        state.position.set(at);
        let mut tx = Tx::default();
        assert!(state.wheel(WheelEvent::new(0.0, -80.0), &mut tx, true, false));
        let event = WheelEvent::new(0.0, -80.0).mods(rune::Mods {
            shift: true,
            ..Default::default()
        });
        assert!(!state.wheel(event, &mut tx, false, false));
    }
}
