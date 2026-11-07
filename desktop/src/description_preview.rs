use crate::{
    model::{PackageRow, State},
    sidebar_info, theme,
};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use rune::{El, TextStyle, Tx, col, text};
use rune_kit::{Kit, Markdown, Placement, popover};
use std::{borrow::Cow, time::Duration};

fn parser(source: &str) -> Parser<'_> {
    Parser::new_ext(
        source,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    )
}

// Use parser source ranges, not Markdown-shaped regexes. Drop image-only wrappers too,
// so linked badges and image-only list items leave no empty links or bullets behind.
pub(crate) fn without_images(source: &str) -> Cow<'_, str> {
    let mut ranges = vec![];
    let mut images = vec![];
    let mut containers: Vec<(usize, bool, bool)> = vec![];
    for (event, range) in parser(source).into_offset_iter() {
        match event {
            Event::Start(Tag::Image { .. }) => {
                for (_, image, _) in &mut containers {
                    *image = true;
                }
                images.push(range.start);
            }
            Event::End(TagEnd::Image) => {
                if let Some(start) = images.pop() {
                    ranges.push(start..range.end);
                }
            }
            _ if !images.is_empty() => {}
            Event::Start(_) => containers.push((range.start, false, false)),
            Event::End(_) => {
                if let Some((start, image, content)) = containers.pop()
                    && image
                    && !content
                {
                    ranges.push(start..range.end);
                }
            }
            Event::Text(ref s) | Event::Code(ref s) if s.trim().is_empty() => {}
            Event::Text(_)
            | Event::Code(_)
            | Event::Rule
            | Event::FootnoteReference(_)
            | Event::Html(_)
            | Event::InlineHtml(_) => {
                for (_, _, content) in &mut containers {
                    *content = true;
                }
            }
            _ => {}
        }
    }
    if ranges.is_empty() {
        return Cow::Borrowed(source);
    }
    ranges.sort_unstable_by_key(|range| range.start);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for range in ranges {
        if range.end <= cursor {
            continue;
        }
        if range.start >= cursor {
            out.push_str(&source[cursor..range.start]);
            out.push(' ');
        }
        cursor = range.end;
    }
    out.push_str(&source[cursor..]);
    Cow::Owned(out.trim().to_owned())
}

// Only the compact label is flattened; stored Markdown retains its structure.
pub fn plain(source: &str) -> String {
    let source = without_images(source);
    let mut out = String::new();
    for event in parser(&source) {
        match event {
            Event::Text(s) | Event::Code(s) => out.push_str(&s),
            Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::CodeBlock
                | TagEnd::TableCell
                | TagEnd::TableRow,
            ) => out.push(' '),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn web_link(link: &str) -> bool {
    url::Url::parse(link).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

pub fn markdown(source: &str, k: &Kit, inverse: bool) -> El {
    let source = without_images(source);
    let mut style = theme::markdown(k);
    if inverse {
        style = style
            .text(k.c.bg_base)
            .muted(k.c.bg_base.alpha(0.75))
            .link(k.c.bg_base)
            .link_underline(k.c.bg_base.alpha(0.5))
            .link_hover(k.c.bg_base)
            .body_font(TextStyle::new(12., 400.).lh(17.))
            .heading_font(1, TextStyle::new(14., 600.).lh(20.))
            .heading_font(2, TextStyle::new(13., 600.).lh(19.))
            .heading_font(3, TextStyle::new(12., 600.).lh(18.))
            .heading_font(4, TextStyle::new(12., 600.).lh(18.))
            .inline_code_bg(k.c.bg_base.alpha(0.08))
            .code_block(k.c.bg_base.alpha(0.06), k.c.bg_base.alpha(0.15))
            .code_block_text(k.c.bg_base)
            .plain_code_text(k.c.bg_base)
            .code_block_font(TextStyle::new(11., 400.).mono().lh(16.))
            .code_block_padding(8., 6.)
            .syntax_highlight(false)
            .table(k.c.bg_base.alpha(0.2), k.c.bg_base.alpha(0.06))
            .table_font(TextStyle::new(12., 400.).lh(17.))
            .quote(k.c.bg_base.alpha(0.3), 2., 8., k.c.bg_base)
            .rule(k.c.bg_base.alpha(0.2))
            .list(14., 4.);
    }
    // No image resolver: description previews never download embedded remote images.
    Markdown::new(source.as_ref())
        .style(style)
        .selectable(true)
        .scroll_wide(true)
        .tasks_read_only()
        .on_link_tx(|s: &mut State, link: &str, tx: &mut Tx| {
            if !web_link(link) {
                s.status = "Only HTTP and HTTPS links without credentials can be opened".into();
                return;
            }
            s.status = match tx.open_url(link) {
                Ok(()) => "Opened description link in browser".into(),
                Err(err) => format!("Could not open description link: {err}"),
            };
        })
        .build(k)
        .w_full()
        .min_w(0.)
}

#[derive(Debug)]
pub struct Hover {
    row: String,
    anchor: String,
    sidebar: bool,
    trigger_inside: bool,
    popup_inside: bool,
    revision: u64,
    scroll: f32,
}

fn allowed(s: &State) -> bool {
    s.description_editor.is_none()
        && s.custom_icons.picker.is_none()
        && s.sidebar_actions.panel.is_none()
        && s.sidebar_actions.command.is_none()
        && s.sidebar_actions.brief.is_none()
        && s.ai.open.is_none()
        && s.ai.preview.is_none()
        && !s.sidebar_sort_open
        && !s.sidebar_filters_open
}

pub fn label(source: &str, row: &PackageRow, k: &Kit, sidebar: bool) -> Option<El> {
    let id = row.id.clone();
    let anchor = format!(
        "description-{}-{}",
        if sidebar { "sidebar" } else { "reader" },
        crate::activity::hash(&id)
    );
    let key = anchor.clone();
    let plain = plain(source);
    if plain.is_empty() {
        return None;
    }
    Some(
        col()
            .key(anchor)
            .w_full()
            .min_w(0.)
            .on_hover_change(move |s: &mut State, inside, tx| {
                if inside && allowed(s) && (!sidebar || tx.clock() >= s.sidebar_scroll.hover_after)
                {
                    s.description_hover = Some(Hover {
                        row: id.clone(),
                        anchor: key.clone(),
                        sidebar,
                        trigger_inside: true,
                        popup_inside: false,
                        revision: s.list_revision,
                        scroll: s.sidebar_scroll.position.get().offset[1],
                    });
                } else if let Some(hover) = &mut s.description_hover
                    && hover.anchor == key
                {
                    hover.trigger_inside = false;
                    if !hover.popup_inside {
                        s.description_hover = None;
                    }
                }
            })
            .hover_delays(Duration::from_millis(600), Duration::from_millis(150))
            .child(
                text(&plain)
                    .key(if sidebar {
                        "sidebar-description"
                    } else {
                        "selected-description"
                    })
                    .font(if sidebar {
                        TextStyle::new(12., 400.).lh(17.)
                    } else {
                        k.ty.ui
                    })
                    .color(k.c.text_muted)
                    .w_full()
                    .min_w(0.)
                    .max_lines(if sidebar { 1 } else { 2 })
                    .description(&plain),
            ),
    )
}

fn active(s: &State) -> Option<(&Hover, &PackageRow)> {
    let hover = s.description_hover.as_ref().filter(|_| allowed(s))?;
    if hover.revision != s.list_revision
        || (!hover.sidebar && s.inventory.open)
        || (hover.sidebar
            && (!s.pane.is_open()
                || (hover.scroll - s.sidebar_scroll.position.get().offset[1]).abs() > 1.))
    {
        return None;
    }
    let row = crate::sidebar_groups::shown(s)
        .iter()
        .filter_map(|&i| s.rows.get(i))
        .find(|r| r.id == hover.row)?;
    if !hover.sidebar && s.table.cursor() != Some(row.id.as_str()) {
        return None;
    }
    Some((hover, row))
}

pub fn dismiss(s: &mut State) -> bool {
    if active(s).is_some() {
        s.description_hover = None;
        true
    } else {
        false
    }
}

pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>) {
    let Some((hover, row)) = active(s) else {
        return;
    };
    let Some(source) = sidebar_info::description(s, row) else {
        return;
    };
    if plain(source).is_empty() {
        return;
    }
    let anchor = hover.anchor.clone();
    let surface = col()
        .key("description-preview")
        .role("region")
        .label(format!("Description for {}", row.name))
        .w(320.)
        .max_h(280.)
        .min_h(0.)
        .radius(6.)
        .fill(k.c.text_base)
        .px(12.)
        .py(6.)
        .on_hover_change(move |s: &mut State, inside, _| {
            if let Some(hover) = &mut s.description_hover
                && hover.anchor == anchor
            {
                hover.popup_inside = inside;
                if !inside && !hover.trigger_inside {
                    s.description_hover = None;
                }
            }
        })
        .hover_delays(Duration::ZERO, Duration::from_millis(150))
        .child(popover::fit_scroll(markdown(source, k, true)));
    layers.push(popover::anchored(
        k,
        surface,
        &hover.anchor,
        Placement::BottomStart,
        4.,
    ));
}

pub fn scene(s: &mut State) {
    sidebar_info::description_scene(s);
    let row = s.rows.iter().find(|r| r.name == "uv").unwrap();
    let id = row.id.clone();
    let package = row.package;
    s.catalog.packages[package].description = Some("[![Build badge](https://example.com/badge.svg)](https://example.com/build)\n\n**Fast Python tooling**, written in Rust.\n\n![Quickstart image](https://example.com/quickstart.png)\n\n- Manage projects and environments\n- Run tools with `uvx`\n\n[Documentation](https://docs.astral.sh/uv/)".into());
    s.description_hover = Some(Hover {
        anchor: format!("description-sidebar-{}", crate::activity::hash(&id)),
        row: id,
        sidebar: true,
        trigger_inside: true,
        popup_inside: false,
        revision: s.list_revision,
        scroll: 0.,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_text_uses_markdown_labels_not_syntax_or_link_targets() {
        assert_eq!(
            plain(
                "# **Fast** _tools_\n\n- Use `uvx`\n- [Docs](https://example.com)\n\n![Logo](https://example.com/logo.png)"
            ),
            "Fast tools Use uvx Docs"
        );
        assert_eq!(
            plain("**OpenSSL** and ~~old~~ &amp; new\n\nNext paragraph"),
            "OpenSSL and old & new Next paragraph"
        );
        assert_eq!(
            plain("| Name | Version |\n|---|---|\n| uv | 1 |"),
            "Name Version uv 1"
        );
    }
    #[test]
    fn only_absolute_credential_free_web_links_can_be_opened() {
        for bad in [
            "javascript:alert(1)",
            "file:///C:/secret",
            "https://user:secret@example.com",
            "mailto:a@example.com",
            "../local",
            "#section",
        ] {
            assert!(!web_link(bad), "{bad}");
        }
        assert!(web_link("https://docs.astral.sh/uv/"));
        assert!(web_link("http://example.com"));
    }

    #[test]
    fn images_badges_and_empty_wrappers_are_removed_without_rewriting_other_markdown() {
        let source = "# Atlas\n\n[![Build badge](https://example.com/badge.svg)](https://example.com/build)\n\n**Manage projects** with [Docs](https://example.com/docs).\n\n![Quickstart][shot]\n\n[shot]: https://example.com/shot.png";
        let cleaned = without_images(source);
        assert!(cleaned.contains("# Atlas"));
        assert!(cleaned.contains("**Manage projects** with [Docs](https://example.com/docs)."));
        assert!(!cleaned.contains("Build badge"));
        assert!(!cleaned.contains("Quickstart"));
        assert!(!cleaned.contains("https://example.com/build"));
        assert!(!parser(&cleaned).any(|e| matches!(e, Event::Start(Tag::Image { .. }))));
        assert_eq!(plain(source), "Atlas Manage projects with Docs.");
        assert!(source.contains("Build badge"));
        assert_eq!(
            without_images("- **![Badge](image.png)**\n\n- Kept item").trim(),
            "- Kept item"
        );
        assert_eq!(without_images("界![alt](image.png)面"), "界 面");
        assert_eq!(
            without_images("`![literal](image.png)`"),
            "`![literal](image.png)`"
        );
        assert_eq!(
            without_images("```md\n![literal](image.png)\n```"),
            "```md\n![literal](image.png)\n```"
        );
        assert!(without_images("![Only image](image.png)").is_empty());
    }
}
