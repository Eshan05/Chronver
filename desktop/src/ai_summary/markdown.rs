use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub(super) struct Part<'a> {
    pub notes: &'a str,
    pub headings: Vec<&'a str>,
    pub breaking: Vec<&'a str>,
}

struct Boundary<'a> {
    offset: usize,
    headings: Vec<&'a str>,
    split: bool,
    breaking: bool,
}

fn breaking_heading(heading: &str) -> bool {
    let heading = heading.trim_matches(|c: char| c.is_whitespace() || "#*_:`".contains(c));
    let mut words = heading
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty());
    words
        .next()
        .is_some_and(|s| s.eq_ignore_ascii_case("breaking"))
        && words
            .next()
            .is_none_or(|s| s.eq_ignore_ascii_case("change") || s.eq_ignore_ascii_case("changes"))
}

pub(super) fn parts(notes: &str, target: usize, max: usize) -> Result<Vec<Part<'_>>, &'static str> {
    let mut boundaries = Vec::<Boundary>::new();
    let mut ancestors = Vec::<TagEnd>::new();
    let mut headings = Vec::<(u8, &str)>::new();
    let mut after_heading = false;
    let mut after_paragraph = false;
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;

    for (event, range) in Parser::new_ext(notes, options).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                let top = ancestors.is_empty();
                let item =
                    matches!(tag, Tag::Item) && matches!(ancestors.as_slice(), [TagEnd::List(_)]);
                let heading = match &tag {
                    Tag::Heading { level, .. } if top => Some(*level as u8),
                    _ => None,
                };
                if let Some(level) = heading {
                    headings.retain(|(parent, _)| *parent < level);
                }
                let paired_code = top && after_paragraph && matches!(tag, Tag::CodeBlock(_));
                // Nested lists, fences, tables and quotes stay inside their parent block.
                if !paired_code && ((top && !matches!(tag, Tag::List(_))) || item) {
                    let offset = notes[..range.start].rfind('\n').map_or(0, |i| i + 1);
                    let offset = if notes[offset..range.start].trim().is_empty() {
                        offset
                    } else {
                        range.start
                    };
                    boundaries.push(Boundary {
                        offset,
                        headings: headings.iter().map(|(_, h)| *h).collect(),
                        split: !after_heading || heading.is_some(),
                        breaking: heading.is_none()
                            && headings.iter().any(|(_, h)| breaking_heading(h)),
                    });
                    after_heading = heading.is_some();
                    if let Some(level) = heading {
                        headings.push((level, notes[range].trim()));
                    }
                }
                if top {
                    after_paragraph = matches!(tag, Tag::Paragraph);
                }
                ancestors.push(tag.to_end());
            }
            Event::End(_) => {
                ancestors.pop();
            }
            _ => {}
        }
    }

    let mut starts = vec![0];
    let mut previous = 0;
    let mut start = 0;
    for end in boundaries
        .iter()
        .filter(|b| b.split && b.offset != 0)
        .map(|b| b.offset)
        .chain(std::iter::once(notes.len()))
    {
        if end == previous {
            continue;
        }
        if end - previous > max {
            return Err(
                "A Markdown block exceeds 64 KiB and cannot be reviewed intact. Open the original notes for that release.",
            );
        }
        if end - start > target && previous > start {
            starts.push(previous);
            start = previous;
        }
        previous = end;
    }
    starts.push(notes.len());

    let mut blocks = boundaries.iter().enumerate().peekable();
    Ok(starts
        .windows(2)
        .map(|range| {
            let (start, end) = (range[0], range[1]);
            let headings = blocks
                .peek()
                .filter(|(_, b)| b.offset == start)
                .map_or_else(Vec::new, |(_, b)| b.headings.clone());
            let mut breaking = Vec::new();
            while let Some((i, block)) = blocks.next_if(|(_, b)| b.offset < end) {
                if block.breaking {
                    let stop = boundaries.get(i + 1).map_or(notes.len(), |b| b.offset);
                    let body = notes[block.offset..stop].trim_end();
                    if !body.trim().is_empty() {
                        breaking.push(body);
                    }
                }
            }
            Part {
                notes: &notes[start..end],
                headings,
                breaking,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_markdown_splits_between_list_items_not_inside_nested_content() {
        let first = "- Removed default mode.\n\n  ```ini\n  Before=true\n\n  After=false\n  ```\n\n  - Only on Windows.\n";
        let second = "- Rename the configuration key before restarting.\n";
        let input = format!("## Breaking Changes\n\n{first}\n{second}");
        let chunks = parts(&input, 95, 155).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].breaking, vec![first.trim_end()]);
        assert!(chunks[0].notes.contains("  ```\n\n  - Only on Windows."));
        assert_eq!(chunks[1].notes, second);
        assert_eq!(chunks[1].headings, vec!["## Breaking Changes"]);
        assert_eq!(chunks[1].breaking, vec![second.trim_end()]);
        assert_eq!(chunks.iter().map(|p| p.notes).collect::<String>(), input);
    }

    #[test]
    fn ai_markdown_keeps_a_code_block_with_its_introduction_or_refuses_to_send_halves() {
        let full = "Run this migration first:\n\n```sh\nline-one\n\nline-two\n```\n";
        let input = format!("{full}\nStop on errors.\n");
        let chunks = parts(&input, 42, 128).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].notes.trim_end(), full.trim_end());
        assert!(matches!(parts(&input, 42, 45), Err(e) if e.contains("cannot be reviewed intact")));
    }
}
