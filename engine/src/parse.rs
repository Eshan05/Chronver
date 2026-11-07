use crate::model::*;
use chrono::NaiveDate;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use regex::Regex;
use scraper::{Html, Selector};
use url::Url;

pub fn selector(input: &str) -> Result<Selector> {
    Selector::parse(input).map_err(|e| Error::Config(format!("CSS selector: {e}")))
}

/// The output is untrusted Markdown, never safe HTML for a UI renderer.
pub fn html_markdown(input: &str, root: Option<&str>, base: &str) -> Result<String> {
    let mut html = Html::parse_document(input);
    let remove = selector("script,style,nav,header,footer,form,iframe,noscript")?;
    let mut ids: Vec<_> = html.select(&remove).map(|e| e.id()).collect();
    // Documentation heading permalinks often contain only an icon/zero-width
    // character; html2md otherwise splits their enclosing heading into paragraphs.
    ids.extend(
        html.select(&selector("a")?)
            .filter(|e| {
                e.text().all(|text| {
                    text.chars()
                        .all(|c| c.is_whitespace() || matches!(c, '\u{200b}' | '\u{feff}'))
                })
            })
            .map(|e| e.id()),
    );
    for id in ids {
        if let Some(mut node) = html.tree.get_mut(id) {
            node.detach();
        }
    }
    // Disclosure widgets are content here, not interactive HTML for the reader.
    let disclosures: Vec<_> = html
        .select(&selector("details,summary")?)
        .map(|e| e.id())
        .collect();
    for id in disclosures {
        if let Some(mut node) = html.tree.get_mut(id)
            && let scraper::Node::Element(el) = node.value()
        {
            el.name.local = "div".into();
        }
    }
    let root = selector(root.unwrap_or("body"))?;
    let elements: Vec<_> = html.select(&root).collect();
    if elements.len() != 1 {
        return Err(Error::Source(format!(
            "expected one content root, found {}",
            elements.len()
        )));
    }
    let base = Url::parse(base).map_err(|e| Error::Config(e.to_string()))?;
    let link_ids: Vec<_> = html
        .select(&selector("a[href],img[src]")?)
        .map(|e| e.id())
        .collect();
    // Resolve relative links before conversion; do not fetch embedded media or links.
    for id in link_ids {
        if let Some(mut node) = html.tree.get_mut(id)
            && let scraper::Node::Element(el) = node.value()
        {
            let key = if el.name() == "a" { "href" } else { "src" };
            if let Some(index) = el.attrs.iter().position(|(name, _)| &*name.local == key) {
                let value = base.join(&el.attrs[index].1).ok();
                if let Some(value) = value.filter(|v| matches!(v.scheme(), "https" | "http")) {
                    el.attrs[index].1 = value.to_string().into();
                } else {
                    el.attrs.remove(index);
                }
            }
        }
    }
    let element = html.select(&root).next().unwrap();
    Ok(html2md::parse_html(&element.inner_html()))
}

struct Block {
    start: usize,
    end: usize,
    level: u8,
    text: String,
    nested: bool,
}
fn blocks(markdown: &str, paragraphs: bool) -> Vec<Block> {
    let mut result = vec![];
    let mut current: Option<Block> = None;
    let mut containers = 0usize;
    for (event, range) in Parser::new(markdown).into_offset_iter() {
        match event {
            Event::Start(Tag::List(_) | Tag::BlockQuote(_)) => containers += 1,
            Event::End(TagEnd::List(_) | TagEnd::BlockQuote(_)) => containers = containers.saturating_sub(1),
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some(Block {
                    start: range.start,
                    end: range.end,
                    level: level as u8,
                    text: String::new(),
                    nested: containers > 0,
                })
            }
            Event::Start(Tag::Paragraph) if paragraphs && current.is_none() => {
                current = Some(Block {
                    start: range.start,
                    end: range.end,
                    level: 7,
                    text: String::new(),
                    nested: containers > 0,
                })
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(b) = current.as_mut() {
                    b.text.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(b) = current.as_mut() {
                    b.text.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) | Event::End(TagEnd::Paragraph) => {
                if let Some(mut b) = current.take() {
                    b.end = range.end;
                    result.push(b);
                }
            }
            _ => {}
        }
    }
    result
}

pub fn document(input: &str, source: &Source) -> Result<Vec<Note>> {
    document_inner(input, source, false)
}

/// Standalone changelogs sometimes contain H1 notices inside H2 release sections.
/// Unlike arbitrary documentation, their next release heading is the boundary.
pub fn changelog(input: &str, source: &Source) -> Result<Vec<Note>> {
    document_inner(input, source, true)
}

fn document_inner(input: &str, source: &Source, changelog: bool) -> Result<Vec<Note>> {
    let Source::Document {
        url,
        format,
        boundary,
        selector,
        date_formats,
        paragraphs,
        exclude,
    } = source
    else {
        return Err(Error::Config(
            "document adapter requires a document source".into(),
        ));
    };
    let regex =
        Regex::new(&format!("^(?:{boundary})$")).map_err(|e| Error::Config(e.to_string()))?;
    if !regex
        .capture_names()
        .flatten()
        .any(|n| n == "version" || n == "date")
    {
        return Err(Error::Config(
            "boundary needs named version or date capture".into(),
        ));
    }
    let cleaned;
    let input = if let Some(exclude) = exclude {
        if !matches!(format, Format::Html | Format::HtmlEntries | Format::Plain) {
            return Err(Error::Config("HTML exclusions require HTML content".into()));
        }
        let mut html = Html::parse_document(input);
        let query = self::selector(exclude)?;
        let ids = html.select(&query).map(|e| e.id()).collect::<Vec<_>>();
        for id in ids {
            if let Some(mut node) = html.tree.get_mut(id) {
                node.detach();
            }
        }
        cleaned = html.html();
        cleaned.as_str()
    } else {
        input
    };
    let markdown = match format {
        Format::Plain if selector.is_some() => {
            let html = Html::parse_document(input);
            let selected = self::selector(selector.as_deref().unwrap())?;
            let texts = html
                .select(&selected)
                .map(|e| e.text().collect::<String>())
                .collect::<Vec<_>>();
            if texts.is_empty() {
                return Err(Error::Source(
                    "plain-text HTML selector matched no content".into(),
                ));
            }
            texts.join("\n\n").replace('\r', "\n")
        }
        Format::JsonMarkdown => {
            let value: serde_json::Value = serde_json::from_str(input)?;
            let path = selector
                .as_deref()
                .ok_or_else(|| Error::Config("JSON Markdown needs a pointer".into()))?;
            let values: Vec<&serde_json::Value> =
                if let Some((array, field)) = path.split_once("/*") {
                    if field.contains("/*") {
                        return Err(Error::Config(
                            "only one JSON array wildcard is supported".into(),
                        ));
                    }
                    value
                        .pointer(array)
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| Error::Source("JSON notes array missing".into()))?
                        .iter()
                        .take(200)
                        .map(|item| {
                            item.pointer(field)
                                .ok_or_else(|| Error::Source("JSON note field missing".into()))
                        })
                        .collect::<Result<_>>()?
                } else {
                    vec![
                        value
                            .pointer(path)
                            .ok_or_else(|| Error::Source("JSON note pointer missing".into()))?,
                    ]
                };
            values
                .into_iter()
                .map(|v| {
                    v.as_str().map(str::to_owned).ok_or_else(|| {
                        Error::Source("JSON note content is not Markdown text".into())
                    })
                })
                .collect::<Result<Vec<_>>>()?
                .join("\n\n")
        }
        Format::Html => html_markdown(input, selector.as_deref(), url)?,
        Format::HtmlEntries => {
            let root = selector
                .as_deref()
                .ok_or_else(|| Error::Config("HTML entries need an entry selector".into()))?;
            let html = Html::parse_document(input);
            let query = self::selector(root)?;
            let entries = html
                .select(&query)
                .take(200)
                .map(|e| {
                    html_markdown(&format!("<article>{}</article>", e.inner_html()), None, url)
                })
                .collect::<Result<Vec<_>>>()?;
            if entries.is_empty() {
                return Err(Error::Source(
                    "no matching release entries; source layout/profile may have changed".into(),
                ));
            }
            entries.join("\n\n")
        }
        _ => input.to_string(),
    };
    let sections = if matches!(format, Format::Plain) {
        let mut offset = 0;
        markdown
            .split_inclusive('\n')
            .map(|line| {
                let block = Block {
                    start: offset,
                    end: offset + line.len(),
                    level: 1,
                    text: line.trim().to_string(),
                    nested: false,
                };
                offset += line.len();
                block
            })
            .collect::<Vec<_>>()
    } else {
        blocks(&markdown, *paragraphs)
    };
    let mut notes = vec![];
    for (i, block) in sections.iter().enumerate() {
        let Some(capture) = regex.captures(block.text.trim()) else {
            continue;
        };
        let version = capture.name("version").map(|m| {
            if let Some(pre) = capture.name("prerelease") {
                format!(
                    "{}-{}",
                    m.as_str(),
                    pre.as_str()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(".")
                        .to_ascii_lowercase()
                )
            } else {
                m.as_str().to_string()
            }
        });
        let date = capture
            .name("date")
            .map(|m| {
                date_formats
                    .iter()
                    .find_map(|f| NaiveDate::parse_from_str(m.as_str().trim(), f).ok())
                    .or_else(|| NaiveDate::parse_from_str(m.as_str().trim(), "%Y-%m-%d").ok())
                    .ok_or_else(|| {
                        Error::Source(format!("unparseable release date: {}", m.as_str()))
                    })
            })
            .transpose()?;
        if version.is_none() && date.is_none() {
            continue;
        }
        let end = sections
            .iter()
            .skip(i + 1)
            .find(|next| {
                if matches!(format, Format::Plain) {
                    regex.is_match(next.text.trim())
                        || next.text.to_lowercase().contains("unreleased")
                } else if block.level == 7 || matches!(format, Format::HtmlEntries) {
                    regex.is_match(next.text.trim())
                } else {
                    regex.is_match(next.text.trim())
                        || next
                            .text
                            .trim()
                            .trim_matches(['[', ']'])
                            .eq_ignore_ascii_case("unreleased")
                        // A component heading inside a list/quote is still release content.
                        || (!changelog && !next.nested && next.level < block.level)
                }
            })
            .map_or(markdown.len(), |b| b.start);
        if let Some(next) = sections.iter().find(|b| b.start == end)
            && !matches!(format, Format::Plain | Format::HtmlEntries)
            && block.level != 7
            && next.level < block.level
            && !next.nested
            && !regex.is_match(next.text.trim())
            && !next
                .text
                .trim()
                .trim_matches(['[', ']'])
                .eq_ignore_ascii_case("unreleased")
        {
            return Err(Error::Source(format!(
                "ambiguous enclosing heading after release: {}",
                next.text
            )));
        }
        let following = markdown[block.end..end].trim();
        let date = date.or_else(|| {
            let first = following
                .lines()
                .find(|line| !line.trim().is_empty())?
                .trim();
            date_formats
                .iter()
                .find_map(|format| NaiveDate::parse_from_str(first, format).ok())
        });
        let body = if let Some(inline) = capture
            .name("body")
            .filter(|m| !m.as_str().trim().is_empty())
        {
            format!("{}\n\n{following}", inline.as_str().trim())
                .trim()
                .to_string()
        } else {
            following.to_string()
        };
        notes.push(Note {
            title: capture.name("body").map_or_else(
                || block.text.clone(),
                |inline| block.text.trim()[..inline.start()].trim().to_string(),
            ),
            version,
            date,
            url: url.clone(),
            authored_notes: authored(&body),
            body,
            preview: false,
        });
    }
    if notes.is_empty() {
        return Err(Error::Source(
            "no release boundaries matched; source layout/profile may have changed".into(),
        ));
    }
    Ok(notes)
}

pub fn release_markdown(body: &str, url: &str) -> Result<String> {
    if !pulldown_cmark::Parser::new(body).any(|e| matches!(e, Event::Html(_))) {
        return Ok(body.into());
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(
        &mut html,
        pulldown_cmark::Parser::new_ext(body, pulldown_cmark::Options::ENABLE_TABLES),
    );
    html_markdown(&html, None, url)
}

pub fn authored(body: &str) -> bool {
    // Changelogs often put authored descriptions in tables next to commit badges.
    // Asset/checksum tables without a description column are not release prose.
    let mut in_head = false;
    let mut column = 0;
    let mut description = None;
    let mut cell = String::new();
    for event in pulldown_cmark::Parser::new_ext(body, pulldown_cmark::Options::ENABLE_TABLES) {
        match event {
            Event::Start(Tag::Table(_)) => description = None,
            Event::Start(Tag::TableHead) => {
                in_head = true;
                column = 0;
            }
            Event::End(pulldown_cmark::TagEnd::TableHead) => in_head = false,
            Event::Start(Tag::TableRow) => column = 0,
            Event::Start(Tag::TableCell) => cell.clear(),
            Event::Text(text) | Event::Code(text) => {
                cell.push_str(&text);
                cell.push(' ');
            }
            Event::End(pulldown_cmark::TagEnd::TableCell) => {
                if in_head
                    && matches!(
                        cell.trim().to_ascii_lowercase().as_str(),
                        "description" | "changes" | "summary"
                    )
                {
                    description = Some(column);
                } else if !in_head
                    && description == Some(column)
                    && cell.split_whitespace().count() >= 3
                {
                    return true;
                }
                column += 1;
            }
            _ => {}
        }
    }
    let meaningful: Vec<_> = body
        .lines()
        .map(str::trim)
        .filter(|line| {
            let lower = line.to_lowercase();
            !line.is_empty()
                && !lower.starts_with('#')
                && !lower.starts_with("**full changelog")
                && !lower.starts_with("full changelog")
                && !lower.starts_with("[full changelog")
                && !lower.starts_with('<')
                && !lower.starts_with("![")
                && !lower.starts_with("-->")
                && !lower.starts_with('|')
                && !lower.starts_with("```")
                && !lower.contains("sha256")
                && !lower.contains("sha512")
                && !lower.contains("checksum")
                && !lower.starts_with("https://github.com/")
                && !lower.starts_with("[download")
        })
        .collect();
    meaningful.iter().any(|line| {
        let words = line.split_whitespace().count();
        words >= 3
            && !line
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c.is_whitespace())
    })
}

pub fn feed(input: &str, url: &str) -> Result<Vec<Note>> {
    let feed = feed_rs::parser::parse(input.as_bytes())
        .map_err(|e| Error::Source(format!("feed: {e}")))?;
    let base = Url::parse(url).map_err(|e| Error::Config(e.to_string()))?;
    let mut notes = Vec::new();
    for entry in feed.entries {
        let date = entry.published.map(|d| d.date_naive());
        let link = entry
            .links
            .iter()
            .find(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
            .and_then(|l| base.join(&l.href).ok())
            .map(|u| u.to_string())
            .unwrap_or_else(|| url.to_string());
        let raw = entry
            .content
            .and_then(|c| c.body)
            .or_else(|| entry.summary.map(|s| s.content))
            .unwrap_or_default();
        let body = html_markdown(&raw, None, &link)?;
        notes.push(Note {
            title: entry.title.map(|s| s.content).unwrap_or_default(),
            version: None,
            date,
            url: link,
            authored_notes: authored(&body),
            body,
            preview: false,
        });
    }
    if notes.is_empty() {
        return Err(Error::Source("feed has no entries".into()));
    }
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(format: Format) -> Source {
        Source::Document {
            url: "https://example.com/changelog".into(),
            format,
            boundary: r"\[?(?P<version>\d+\.\d+\.\d+)\]?(?: - (?P<date>\d{4}-\d{2}-\d{2}))?".into(),
            selector: None,
            exclude: None,
            date_formats: vec![],
            paragraphs: false,
        }
    }
    #[test]
    fn sections_preserve_nested_notes_and_stop_at_unreleased() {
        let text = "# Changes\n## Unreleased\nFuture work\n## [1.2.0] - 2026-11-01\n### Fixed\n- Correct handling of windows.\n## 1.1.0\n- Previous changes are here.\n## Unreleased\nMust not leak";
        let notes = document(text, &source(Format::Markdown)).unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].body.contains("### Fixed"));
        assert_eq!(notes[1].body, "- Previous changes are here.");
        assert_eq!(notes[0].date.unwrap().to_string(), "2026-11-01");
    }
    #[test]
    fn html_sanitizes_and_resolves_links() {
        let html = "<main><script>bad()</script><h2>1.2.0</h2><p>Fixed a <a href='/issue'>real problem</a></p></main>";
        let mut src = source(Format::Html);
        if let Source::Document { selector, .. } = &mut src {
            *selector = Some("main".into());
        }
        let notes = document(html, &src).unwrap();
        assert!(!notes[0].body.contains("bad()"));
        assert!(notes[0].body.contains("https://example.com/issue"));
        assert!(document("<p>layout changed</p>", &src).is_err());
    }
    #[test]
    fn heading_permalinks_and_button_titles_remain_parseable() {
        let mut src = source(Format::Html);
        if let Source::Document { paragraphs, .. } = &mut src {
            *paragraphs = true;
        }
        let notes = document("<h2><a href='#v1'><svg></svg></a><button>1.2.0</button></h2><p>Fixed a real bug.</p><h2>1.1.0<a href='#v0'>&#8203;</a></h2><p>Older bug was fixed.</p>",&src).unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].body.contains("Fixed a real bug"));
    }
    #[test]
    fn sparse_is_not_authored() {
        assert!(!authored(
            "**Full Changelog**: https://github.com/a/b/compare/v1...v2"
        ));
        assert!(!authored(
            "## Downloads\n| file | sha256 |\n| foo.exe | abc |"
        ));
        assert!(authored("* Fix Windows crash by @name in #123"));
    }
    #[test]
    fn rss_uses_publication_date() {
        let notes = feed(r#"<rss version="2.0"><channel><title>News</title><link>https://example.com</link><description>Updates</description><item><title>New release</title><link>https://example.com/one</link><pubDate>Mon, 09 Nov 2026 10:00:00 GMT</pubDate><description>Fixed a real problem</description></item></channel></rss>"#,"https://example.com/feed").unwrap();
        assert_eq!(notes[0].date.unwrap().to_string(), "2026-11-09");
    }
    #[test]
    fn paragraph_dates_include_following_feature_headings() {
        let src = Source::Document {
            url: "https://example.com".into(),
            format: Format::Markdown,
            boundary: r"(?P<date>\d{4}-\d{2}-\d{2})".into(),
            selector: None,
            exclude: None,
            date_formats: vec![],
            paragraphs: true,
        };
        let notes = document("2026-11-09\n\n## New feature\n\nA useful feature shipped.\n\n2026-11-01\n\n## Older feature\n\nAnother feature shipped here.",&src).unwrap();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].body.contains("New feature"));
        assert!(!notes[0].body.contains("Older feature"));
    }
}
