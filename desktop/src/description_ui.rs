use crate::{custom_icons::identity, model::State, sidebar_info};
use rune::{El, Icon, TextEdit, col, el, row, text};
use rune_kit::{Button, Dialog, Kit, TextArea, glyphs};

pub const LIMIT: usize = 240;
pub static EDIT: Icon = Icon {
    name: "edit-description",
    view: 24.,
    body: include_str!("../assets/status/pencil-line.svg"),
};

pub struct Editor {
    pub target: String,
    name: String,
    pub text: TextEdit,
}

pub fn normalize(value: &str) -> Result<String, &'static str> {
    if value.chars().count() > LIMIT {
        return Err("Description must be 240 characters or fewer.");
    }
    if value.chars().any(|c| c.is_control() && !c.is_whitespace()) {
        return Err("Description contains unsupported control characters.");
    }
    let value = value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned();
    if value.is_empty() {
        Err("Enter a description.")
    } else {
        Ok(value)
    }
}

pub fn valid(value: &str) -> bool {
    normalize(value).is_ok_and(|normalized| normalized == value)
}

pub fn selected(s: &State) -> Option<&str> {
    s.rows
        .iter()
        .find(|r| Some(r.id.as_str()) == s.table.cursor())
        .and_then(|r| sidebar_info::description(s, r))
}

impl State {
    pub fn open_description_editor(&mut self) {
        if !self.activity.writable() {
            return;
        }
        if let Some(p) = self.selected() {
            let mut text = TextEdit::new(selected(self).unwrap_or(""));
            text.focused = true;
            self.description_editor = Some(Editor {
                target: identity(p),
                name: p.name.clone(),
                text,
            });
        }
    }

    pub fn save_description(&mut self) {
        if !self.activity.writable() {
            return;
        }
        let Some(editor) = &self.description_editor else {
            return;
        };
        let Ok(value) = normalize(&editor.text.text) else {
            return;
        };
        self.activity
            .data
            .description_overrides
            .insert(editor.target.clone(), value);
        self.activity.dirty = true;
        self.list_revision = self.list_revision.wrapping_add(1);
        self.description_editor = None;
        self.status = "Description updated".into();
    }

    pub fn reset_description(&mut self) {
        if !self.activity.writable() {
            return;
        }
        let Some(editor) = &self.description_editor else {
            return;
        };
        self.activity.dirty |= self
            .activity
            .data
            .description_overrides
            .remove(&editor.target)
            .is_some();
        self.list_revision = self.list_revision.wrapping_add(1);
        self.description_editor = None;
        self.status = "Original description restored".into();
    }
}

pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>) {
    let Some(editor) = &s.description_editor else {
        return;
    };
    let count = editor.text.text.chars().count();
    let result = normalize(&editor.text.text);
    let invalid = !editor.text.text.is_empty() && result.is_err();
    let overridden = s
        .activity
        .data
        .description_overrides
        .contains_key(&editor.target);
    let body = col()
        .w_full()
        .padding(16.)
        .gap(12.)
        .child(
            text(&editor.name)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .w_full()
                .truncate(),
        )
        .child(
            TextArea::new(&editor.text)
                .label("Description")
                .placeholder("Description")
                .width(None)
                .auto_grow(3, 5)
                .wrap_anywhere()
                .invalid(invalid)
                .on_edit(|s: &mut State| &mut s.description_editor.as_mut().unwrap().text)
                .on_submit(|s: &mut State, _| s.save_description())
                .build(k)
                .key("description-input"),
        )
        .child(
            row()
                .w_full()
                .gap(12.)
                .center_y()
                .child(
                    text(if invalid {
                        result.as_ref().unwrap_err().to_string()
                    } else {
                        String::new()
                    })
                    .font(k.ty.text_xsmall)
                    .color(k.c.danger.fg)
                    .grow(1.)
                    .min_w(0.)
                    .wrap(),
                )
                .child(
                    text(format!("{count} / {LIMIT}"))
                        .font(k.ty.text_xsmall)
                        .color(if count > LIMIT {
                            k.c.danger.fg
                        } else {
                            k.c.text_muted
                        })
                        .key("description-count"),
                ),
        )
        .child(
            row()
                .w_full()
                .gap(8.)
                .center_y()
                .child(
                    Button::new("Restore original")
                        .icon(&crate::custom_icons::RESET)
                        .disabled(!overridden || !s.activity.writable())
                        .on_press(|s: &mut State| s.reset_description())
                        .build(k)
                        .key("description-reset"),
                )
                .child(el().grow(1.))
                .child(
                    Button::new("Cancel")
                        .on_press(|s: &mut State| s.description_editor = None)
                        .build(k)
                        .key("description-cancel"),
                )
                .child(
                    Button::new("Save")
                        .icon(&glyphs::CHECK)
                        .disabled(result.is_err() || !s.activity.writable())
                        .on_press(|s: &mut State| s.save_description())
                        .build(k)
                        .key("description-save"),
                ),
        );
    Dialog::new("description-editor", "Edit description")
        .open(true)
        .fit()
        .width(500.)
        .click_outside(false)
        .on_close(|s: &mut State| s.description_editor = None)
        .child(body)
        .build(k, layers);
}

pub fn scene(s: &mut State) {
    sidebar_info::description_scene(s);
    s.open_description_editor();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_markdown_descriptions_are_bounded_unicode_not_bytes() {
        assert!(valid(&"界".repeat(LIMIT)));
        assert!(normalize(&"界".repeat(LIMIT + 1)).is_err());
        assert!(normalize(" \n ").is_err());
        assert!(normalize("hidden\0control").is_err());
        assert_eq!(
            normalize(" My\n  description ").unwrap(),
            "My\n  description"
        );
        let markdown = "**My workflow**\n\n- Use `gh`\n- [Docs](https://cli.github.com)";
        assert!(valid(markdown));
        assert_eq!(
            normalize(&markdown.replace('\n', "\r\n")).unwrap(),
            markdown
        );
        assert!(!valid(" My description "));
    }

    #[test]
    fn editor_targets_survive_selection_changes_and_scans_without_replacing_provider_text() {
        let mut s = State::demo();
        s.select_package("gh");
        s.open_description_editor();
        let target = s.description_editor.as_ref().unwrap().target.clone();
        s.description_editor.as_mut().unwrap().text = TextEdit::new("My GitHub workflow");
        s.select_package("bun");
        s.save_description();
        assert_eq!(
            s.activity.data.description_overrides[&target],
            "My GitHub workflow"
        );
        assert!(selected(&s).is_none());
        sidebar_info::description_scene(&mut s);
        let row = s.rows.iter().find(|r| r.name == "GitHub CLI").unwrap();
        assert_eq!(
            sidebar_info::description(&s, row),
            Some("My GitHub workflow")
        );
        let id = row.id.clone();
        assert_eq!(
            s.catalog.packages[row.package].description.as_deref(),
            Some("GitHub's command-line interface.")
        );
        s.select_package(&id);
        s.open_description_editor();
        assert_eq!(
            s.description_editor.as_ref().unwrap().text.text,
            "My GitHub workflow"
        );
        s.description_editor.as_mut().unwrap().text = TextEdit::new("x".repeat(LIMIT + 1));
        s.save_description();
        assert!(s.description_editor.is_some());
        assert_eq!(
            s.activity.data.description_overrides[&target],
            "My GitHub workflow"
        );
        s.reset_description();
        assert!(!s.activity.data.description_overrides.contains_key(&target));
        assert_eq!(selected(&s), Some("GitHub's command-line interface."));
    }
}
