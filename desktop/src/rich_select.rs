use rune::{El, Tx, col, el, icon, row, text};
use rune_kit::{Kit, Placement, popover, scrim, scroll_bar};

pub struct OptionRow {
    pub id: String,
    pub label: String,
    pub description: String,
    pub content: El,
}

pub fn count_label(count: usize, unit: &str) -> String {
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

pub fn text_content(title: &str, detail: &str, k: &Kit) -> El {
    col()
        .gap(3.)
        .min_w(0.)
        .child(
            text(title)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .truncate(),
        )
        .child(
            text(detail)
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted)
                .truncate(),
        )
}

/// Content is composable; Rune owns popup fitting, focus, list navigation and dismissal.
pub struct RichSelect<'a, S> {
    pub key: &'a str,
    pub label: &'a str,
    pub current: &'a str,
    pub open: bool,
    pub popup_width: f32,
    pub value: El,
    pub options: Vec<OptionRow>,
    pub on_toggle: fn(&mut S),
    pub on_close: fn(&mut S),
    pub on_choose: fn(&mut S, &str, &mut Tx),
}

impl<S: 'static> RichSelect<'_, S> {
    pub fn build(self, k: &Kit, layers: &mut Vec<El>) -> El {
        self.build_density(k, layers, false)
    }

    pub fn build_compact(self, k: &Kit, layers: &mut Vec<El>) -> El {
        self.build_density(k, layers, true)
    }

    fn build_density(self, k: &Kit, layers: &mut Vec<El>, compact: bool) -> El {
        let anchor = format!("{}-trigger", self.key);
        let label = self
            .options
            .iter()
            .find(|o| o.id == self.current)
            .map(|o| o.label.as_str())
            .unwrap_or(self.label);
        let trigger = row()
            .key(anchor.clone())
            .role("combobox")
            .label(self.label)
            .a11y_value(label)
            .expanded(self.open)
            .focusable(true)
            .w_full()
            .h(if compact { 28. } else { 32. })
            .px(if compact { 6. } else { 8. })
            .gap(if compact { 6. } else { 8. })
            .center_y()
            .radius(6.)
            .fill(k.c.bg_base)
            .border(1., k.c.border_base)
            .hover(k.c.hover)
            .focus_within_ring(1., -1., k.c.border_focus)
            .help(format!("{}: {label}", self.label))
            .on_press(self.on_toggle)
            .child(self.value.grow(1.).min_w(0.).a11y_hidden())
            .child(
                icon(
                    if self.open {
                        k.glyph.chevron_up
                    } else {
                        k.glyph.chevron_down
                    },
                    if compact { 14. } else { 16. },
                )
                .color(k.c.icon_muted)
                .a11y_hidden(),
            );
        if !self.open {
            return trigger;
        }

        let mut rows = col()
            .key("rows")
            .w_full()
            .padding(4.)
            .gap(if compact { 0. } else { 2. });
        let count = self.options.len();
        for (i, option) in self.options.into_iter().enumerate() {
            let selected = option.id == self.current;
            let choose = self.on_choose;
            let close = self.on_close;
            let id = option.id;
            let mut item = row()
                .key(format!("{}-option-{id}", self.key))
                .role("option")
                .label(option.label)
                .position_in_set(i + 1, count)
                .focusable(true)
                .focus_selected(selected)
                .w_full()
                .min_h(if compact { 32. } else { 48. })
                .padding(if compact { 6. } else { 8. })
                .gap(8.)
                .center_y()
                .radius(4.)
                .focus_highlight(k.c.hover)
                .on_press_tx(move |s: &mut S, tx| {
                    choose(s, &id, tx);
                    close(s);
                })
                .child(option.content.grow(1.).min_w(0.).a11y_hidden())
                .child(
                    el().size(16., 16.).center().a11y_hidden().child(
                        icon(k.glyph.check_small, 14.)
                            .color(k.c.icon_base)
                            .opacity(if selected { 1. } else { 0. }),
                    ),
                );
            if !option.description.is_empty() {
                item = item
                    .description(option.description.clone())
                    .help(option.description);
            }
            rows = rows.child(item);
        }
        let surface = popover::raised(
            k,
            col()
                .key(format!("{}-content", self.key))
                .role("listbox")
                .label(self.label)
                .focus_group()
                .focus_return(&anchor)
                .w(self.popup_width)
                .max_h(320.)
                .min_h(0.)
                .child(
                    scroll_bar(
                        k,
                        &format!("{}-scroll-area", self.key),
                        "scroll",
                        popover::fit_scroll(rows),
                    )
                    .w_full()
                    .shrink()
                    .min_h(0.),
                ),
            6.,
        )
        .fill(k.c.layer_01)
        .border(1., k.c.border_base)
        .modal(true, self.on_close);
        layers.push(scrim(format!("{}-scrim", self.key), self.on_close));
        layers.push(popover::anchored(
            k,
            surface,
            &anchor,
            Placement::BottomStart,
            4.,
        ));
        trigger
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Key, KeyEvent, Mods, Runtime, text};
    #[derive(Default)]
    struct State {
        open: bool,
        selected: usize,
        dark: bool,
    }
    fn view(s: &State, _: &mut rune::Cx<State>) -> El {
        let k = crate::theme::kit(s.dark);
        let mut layers = vec![];
        let selected = s.selected.to_string();
        let options = (0..20)
            .map(|i| OptionRow {
                id: i.to_string(),
                label: format!("Choice {i}"),
                description: format!("Details {i}"),
                content: row()
                    .gap(8.)
                    .center_y()
                    .child(icon(k.glyph.check_small, 16.))
                    .child(
                        col()
                            .child(
                                text(format!("Choice {i}"))
                                    .font(k.ty.ui)
                                    .color(k.c.text_base),
                            )
                            .child(
                                text(format!("Details {i}"))
                                    .font(k.ty.text_xsmall)
                                    .color(k.c.text_muted),
                            ),
                    ),
            })
            .collect();
        let field = RichSelect {
            key: "test-select",
            label: "Choice",
            current: &selected,
            open: s.open,
            popup_width: 320.,
            value: text(format!("Choice {}", s.selected)),
            options,
            on_toggle: |s: &mut State| s.open = !s.open,
            on_close: |s: &mut State| s.open = false,
            on_choose: |s: &mut State, id, _| s.selected = id.parse().unwrap(),
        }
        .build(&k, &mut layers);
        col()
            .w_full()
            .h_full()
            .padding(16.)
            .child(field)
            .children(layers)
    }
    fn key(rt: &mut Runtime<State>, key: Key, time: f64) {
        rt.key(KeyEvent {
            key,
            mods: Mods::default(),
        });
        rt.frame(time);
    }
    #[test]
    fn keyboard_selection_scrolling_dismissal_and_focus_return_work_in_both_themes() {
        for dark in [false, true] {
            let mut rt = Runtime::new(
                State {
                    dark,
                    ..Default::default()
                },
                view,
                (360., 240.),
                Env {
                    system_dark: dark,
                    scale: 1.,
                },
            );
            rt.frame(0.);
            rt.update(|_, tx| tx.focus("test-select-trigger", false));
            rt.frame(0.);
            key(&mut rt, Key::Down, 1.);
            assert!(rt.state.open);
            assert!(rt.elements().iter().any(|e| e.role == "listbox"));
            key(&mut rt, Key::End, 2.);
            let last = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some("test-select-option-19"))
                .unwrap();
            assert!(last.focused);
            let [x, y, w, h] = last.rect;
            assert!(
                x >= 0. && y >= 0. && x + w <= 360. && y + h <= 240.,
                "{:?}",
                last.rect
            );
            key(&mut rt, Key::Enter, 3.);
            assert_eq!(rt.state.selected, 19);
            assert!(!rt.state.open);
            key(&mut rt, Key::Down, 4.);
            assert!(rt.state.open, "focus must return to the trigger");
            key(&mut rt, Key::Home, 5.);
            key(&mut rt, Key::Down, 6.);
            key(&mut rt, Key::Enter, 7.);
            assert_eq!(rt.state.selected, 1);
            assert!(!rt.state.open);
            key(&mut rt, Key::Down, 8.);
            key(&mut rt, Key::Escape, 9.);
            assert!(!rt.state.open);
            assert_eq!(rt.state.selected, 1);
            key(&mut rt, Key::Down, 10.);
            rt.pointer_down(350., 8.);
            rt.pointer_up(350., 8.);
            rt.frame(11.);
            assert!(!rt.state.open);
            assert_eq!(rt.state.selected, 1);
        }
    }
}
