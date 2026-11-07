use crate::{
    model::{Package, Release, State},
    rich_select::{OptionRow, RichSelect},
};
use rune::{Cx, El, Tx, col, el, row, text};
use rune_kit::{Kit, scroll_bar};

pub const WIDE_READER: f32 = 960.;

fn date(release: &Release) -> String {
    let (day, year) = crate::ui::date_parts(&release.published);
    if day.is_empty() {
        "Date unknown".into()
    } else if year.is_empty() {
        day
    } else {
        format!("{day}, {year}")
    }
}

fn installed(p: &Package, release: &Release) -> bool {
    !p.sample_notes
        && p.installed.as_deref().is_some_and(|version| {
            version.strip_prefix('v').unwrap_or(version)
                == release
                    .version
                    .strip_prefix('v')
                    .unwrap_or(&release.version)
        })
}

pub fn active(cx: &Cx<State>, count: usize) -> usize {
    let Some([_, top, _, height]) = cx.layout("release-notes") else {
        return 0;
    };
    let bottom = top + height;
    if cx
        .window_layout_with("release-document", move |r| {
            r[3] > height + 1. && r[1] + r[3] <= bottom + 1.
        })
        .unwrap_or(false)
    {
        return count.saturating_sub(1);
    }
    let cutoff = top + 25.;
    let mut active = 0;
    // Read only threshold crossings, not every pixel of a scrolling frame.
    for i in 1..count {
        if cx
            .window_layout_with(&format!("timeline-item-{i}"), move |r| r[1] <= cutoff)
            .unwrap_or(false)
        {
            active = i;
        }
    }
    active
}

fn jump(s: &mut State, index: usize, tx: &mut Tx) {
    if !s.selected().is_some_and(|p| index < p.releases.len()) {
        return;
    }
    let query = s.engine_query();
    query.shown = query.shown.max(index + 1);
    s.reader_controls.reset_scroll();
    s.reader_controls.release_index_open = false;
    tx.scroll_key_to(format!("timeline-item-{index}"), 24.);
    tx.instant();
}

fn entry(p: &Package, release: &Release, k: &Kit, active: bool) -> El {
    col()
        .w_full()
        .min_w(0.)
        .gap(1.)
        .child(
            row()
                .w_full()
                .gap(4.)
                .center_y()
                .child(
                    text(&release.version)
                        .font(rune::TextStyle::new(12., 500.).lh(18.))
                        .color(if active {
                            k.c.text_base
                        } else {
                            k.c.text_muted
                        })
                        .grow(1.)
                        .min_w(0.)
                        .truncate(),
                )
                .children(installed(p, release).then(|| {
                    text("Installed")
                        .font(rune::TextStyle::new(10., 500.).lh(14.))
                        .color(k.c.text_muted)
                        .px(4.)
                        .py(1.)
                        .radius(3.)
                        .fill(k.c.layer_02)
                })),
        )
        .child(
            text(date(release))
                .font(rune::TextStyle::new(11., 400.).lh(15.))
                .color(k.c.text_muted)
                .truncate(),
        )
}

pub fn rail(s: &State, k: &Kit, active: usize) -> El {
    let p = s.selected().unwrap();
    let mut entries = col().w_full();
    for (i, release) in p.releases.iter().enumerate() {
        let current = i == active;
        entries = entries.child(
            row()
                .key(format!("release-index-item-{i}"))
                .role("button")
                .label(format!("Go to release {}", release.version))
                .a11y_value(if current { "Current release" } else { "" })
                .position_in_set(i + 1, p.releases.len())
                .focusable(true)
                .focus_selected(current)
                .w_full()
                .min_w(0.)
                .px(8.)
                .py(5.)
                .radius(4.)
                .hover(k.c.hover)
                .focus_ring(1., -1., k.c.border_focus)
                .child(entry(p, release, k, current))
                .children(current.then(|| {
                    el().key("release-index-active")
                        .w(2.)
                        .pin(Some(0.), Some(7.), None, Some(7.))
                        .fill(k.c.text_base)
                        .a11y_hidden()
                }))
                .on_press_tx(move |s: &mut State, tx| jump(s, i, tx)),
        );
    }
    col()
        .key("release-index")
        .role("navigation")
        .label("On this page")
        .on_visible(|s: &mut State, visible, _| {
            if visible {
                s.reader_controls.release_index_open = false;
            }
        })
        .w(168.)
        .h_full()
        .min_h(0.)
        .pt(20.)
        .pr(12.)
        .pb(crate::app_chrome::DOCK_CLEARANCE)
        .gap(6.)
        .child(
            text("On this page")
                .font(rune::TextStyle::new(12., 500.).lh(18.))
                .color(k.c.text_base)
                .px(8.),
        )
        .child(
            scroll_bar(
                k,
                "release-index-area",
                "release-index-scroll",
                col()
                    .key("release-index-scroll")
                    .w_full()
                    .grow(1.)
                    .min_h(0.)
                    .focus_group()
                    .scroll(rune::ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(entries),
            )
            .grow(1.)
            .min_h(0.),
        )
}

pub fn compact(s: &State, k: &Kit, active: usize, layers: &mut Vec<El>) -> El {
    let p = s.selected().unwrap();
    let current = active.to_string();
    let options = p
        .releases
        .iter()
        .enumerate()
        .map(|(i, release)| OptionRow {
            id: i.to_string(),
            label: release.version.clone(),
            description: if installed(p, release) {
                format!("{} / Installed", date(release))
            } else {
                date(release)
            },
            content: entry(p, release, k, i == active),
        })
        .collect();
    RichSelect {
        key: "release-jump",
        label: "Jump to release",
        current: &current,
        open: s.reader_controls.release_index_open,
        popup_width: 240.,
        options,
        value: row()
            .gap(6.)
            .center_y()
            .child(text("Jump to").font(k.ty.text_xsmall).color(k.c.text_muted))
            .child(
                text(&p.releases[active].version)
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base)
                    .grow(1.)
                    .min_w(0.)
                    .truncate()
                    .key("release-jump-version"),
            ),
        on_toggle: |s: &mut State| {
            s.reader_controls.release_index_open = !s.reader_controls.release_index_open
        },
        on_close: |s: &mut State| s.reader_controls.release_index_open = false,
        on_choose: |s: &mut State, id, tx| {
            if let Ok(index) = id.parse() {
                jump(s, index, tx);
            }
        },
    }
    .build_compact(k, layers)
    .w(180.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NoteMode;
    use rune::{Env, Runtime};

    fn runtime(size: (f32, f32), dark: bool, count: usize) -> Runtime<State> {
        let mut s = State::demo();
        s.select_package("gh");
        s.dark = dark;
        let p = s
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.id == "gh")
            .unwrap();
        p.sample_notes = false;
        p.installed = Some("3.0.0".into());
        p.releases = (0..count).map(|i| Release {
            version: format!("{}.0.0", count - i),
            published: "2026-11-14T00:00:00Z".into(),
            url: format!("https://example.com/releases/{i}"),
            markdown: "## Improvements\n\nFixed configuration loading and improved error reporting.\n\n".repeat(if count > 10 { 1 } else { 12 }),
        }).collect();
        let mut rt = Runtime::with_text(
            s,
            crate::ui::view,
            size,
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
        rt.frame(1.);
    }

    fn rect(rt: &Runtime<State>, key: &str) -> [f32; 4] {
        rt.elements()
            .iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap()
            .rect
    }

    fn current(rt: &Runtime<State>) -> usize {
        let marker = rect(rt, "release-index-active");
        (0..rt.state.selected().unwrap().releases.len())
            .find(|i| {
                let row = rect(rt, &format!("release-index-item-{i}"));
                marker[1] >= row[1] && marker[1] < row[1] + row[3]
            })
            .unwrap()
    }

    #[test]
    fn rail_jumps_and_follows_scroll_without_fetching_or_moving_itself() {
        for dark in [false, true] {
            let mut rt = runtime((1440., 900.), dark, 5);
            assert!(rt.find("release-jump-trigger").is_none());
            assert_eq!(current(&rt), 0);
            assert!(rt.accessibility_json().to_string().contains("Installed"));
            let rail = rect(&rt, "release-index");
            assert_eq!(rail[2], 168.);
            assert!(rect(&rt, "release-index-item-0")[3] <= 44.);
            tap(&mut rt, "release-index-item-3");
            assert_eq!(current(&rt), 3);
            assert_eq!(rect(&rt, "release-index"), rail);
            rt.update(|_, tx| tx.scroll_key_to("timeline-item-1", 24.));
            rt.frame(1.);
            rt.frame(1.);
            assert_eq!(current(&rt), 1);
            rt.update(|_, tx| tx.scroll_to_bottom_instant("release-notes"));
            rt.frame(1.);
            rt.frame(1.);
            assert_eq!(current(&rt), 4);
            rt.update(|_, tx| tx.scroll_to_instant("release-notes", 0., 0.));
            rt.frame(1.);
            rt.frame(1.);
            assert_eq!(current(&rt), 0);
            assert!(!rt.state.live.work.is_pending());
            assert!(!rt.state.live.history.is_pending());
        }
    }

    #[test]
    fn compact_picker_resizes_dismisses_and_disappears_for_raw_or_single_release() {
        for dark in [false, true] {
            let mut rt = runtime((1024., 640.), dark, 5);
            assert!(rt.find("release-index").is_none());
            assert_eq!(rect(&rt, "release-jump-trigger")[2], 180.);
            tap(&mut rt, "release-jump-trigger");
            tap(&mut rt, "release-jump-option-2");
            assert!(!rt.state.reader_controls.release_index_open);
            assert_eq!(
                rt.elements()
                    .iter()
                    .find(|e| e.key.as_deref() == Some("release-jump-version"))
                    .unwrap()
                    .text
                    .as_deref(),
                Some("3.0.0")
            );
            tap(&mut rt, "release-jump-trigger");
            rt.key(rune::KeyEvent {
                key: rune::Key::Escape,
                mods: Default::default(),
            });
            rt.frame(1.);
            assert!(!rt.state.reader_controls.release_index_open);
            tap(&mut rt, "release-jump-trigger");
            rt.set_viewport((1440., 900.), 1.);
            rt.frame(1.);
            rt.frame(1.);
            assert!(!rt.state.reader_controls.release_index_open);
            assert!(rt.find("release-index").is_some());
            assert!(rt.find("release-jump-trigger").is_none());
            rt.update(|s, _| s.mode = NoteMode::Source);
            rt.frame(1.);
            assert!(rt.find("release-index").is_none());
            assert!(rt.find("release-jump-trigger").is_none());
            rt.update(|s, _| {
                s.mode = NoteMode::Rendered;
                s.catalog
                    .packages
                    .iter_mut()
                    .find(|p| p.id == "gh")
                    .unwrap()
                    .releases
                    .truncate(1);
            });
            rt.frame(1.);
            assert!(rt.find("release-index").is_none());
            assert!(rt.find("release-jump-trigger").is_none());
        }
    }

    #[test]
    fn jumping_to_loaded_but_unshown_notes_reveals_them_without_network() {
        let mut rt = runtime((1440., 900.), true, 55);
        rt.update(|s, _| s.engine_query().shown = 2);
        rt.frame(1.);
        assert!(rt.find("timeline-item-50").is_none());
        rt.update(|s, tx| jump(s, 50, tx));
        rt.frame(1.);
        assert_eq!(rt.state.live.sessions["gh"].shown, 51);
        assert!(rt.find("timeline-item-50").is_some());
        assert_eq!(current(&rt), 50);
        assert!(!rt.state.live.work.is_pending());
        assert!(!rt.state.live.history.is_pending());
    }
}
