use crate::{
    Basis, Error, Report, Source, Stream,
    http::{BODY_LIMIT_ERROR, Document, Fetcher, https},
};
use chrono::{DateTime, NaiveDate};
use serde::Deserialize;
use std::collections::HashMap;

fn document_repo(url: &str) -> Option<(String, String, bool)> {
    let url = https(url).ok()?;
    if !matches!(
        url.host_str(),
        Some("github.com" | "raw.githubusercontent.com")
    ) || url.port().is_some()
    {
        return None;
    }
    let mut parts = url.path_segments()?;
    let owner = parts.next()?;
    let repo = parts.next()?;
    for part in [owner, repo] {
        if part.is_empty()
            || part == "."
            || part == ".."
            || !part
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        {
            return None;
        }
    }
    if url.host_str() == Some("github.com") && !matches!(parts.next(), Some("blob" | "raw")) {
        return None;
    }
    let git_ref = parts.next()?;
    let first_path_part = parts.next()?;
    if git_ref.is_empty() || first_path_part.is_empty() {
        return None;
    }
    Some((
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase(),
        parts.any(|part| !part.is_empty()),
    ))
}

fn version(value: &str) -> &str {
    let value = value.trim();
    value
        .strip_prefix(['v', 'V'])
        .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
        .unwrap_or(value)
}

#[derive(Deserialize)]
struct Publication {
    tag_name: String,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
}

fn release_metadata(fetcher: &impl Fetcher, owner: &str, repo: &str) -> crate::Result<Document> {
    let page = |size| {
        fetcher.get(&format!(
            "https://api.github.com/repos/{owner}/{repo}/releases?per_page={size}&page=1"
        ))
    };
    match page(100) {
        // Large projects sometimes attach dozens of assets per release.
        Err(Error::Source(reason)) if reason == BODY_LIMIT_ERROR => page(30),
        result => result,
    }
}

pub(crate) fn fill(fetcher: &impl Fetcher, source: &Source, stream: &Stream, report: &mut Report) {
    let Source::Document { url, .. } = source else {
        return;
    };
    if report.basis != Basis::Version
        || !report
            .notes
            .iter()
            .any(|note| note.authored_notes && note.date.is_none() && note.version.is_some())
    {
        return;
    }
    let Some((owner, repo, scoped)) = document_repo(url) else {
        return;
    };
    let repo_scope = |url: &str| {
        let (o, r, scoped) = document_repo(url)?;
        (o == owner && r == repo).then_some(scoped)
    };
    let Some(canonical_scoped) = report
        .evidence
        .iter()
        .find_map(|source| repo_scope(&source.url))
    else {
        return;
    };

    let tag = stream.sources.iter().find_map(|source| match source {
        Source::Github {
            owner: o,
            repo: r,
            tag_prefix,
            tag_pattern,
            ..
        } if o.eq_ignore_ascii_case(&owner) && r.eq_ignore_ascii_case(&repo) => {
            Some((tag_prefix.as_str(), tag_pattern.as_deref()))
        }
        _ => None,
    });
    let (prefix, pattern) = tag.unwrap_or(("", None));
    // Tags for a repo may belong to a different component in a monorepo.
    if (scoped || canonical_scoped) && prefix.is_empty() && pattern.is_none() {
        return;
    }
    let matcher = match pattern {
        Some(regex) => match regex::Regex::new(&format!("^(?:{regex})$")) {
            Ok(matcher) => Some(matcher),
            Err(_) => return,
        },
        None => None,
    };

    // Read one page of metadata. No guessed commit dates or per-note requests.
    let Ok(doc) = release_metadata(fetcher, &owner, &repo) else {
        return;
    };
    let Ok(publications) = serde_json::from_str::<Vec<Publication>>(&doc.body) else {
        return;
    };
    let mut dates: HashMap<_, Option<NaiveDate>> = HashMap::new();
    for release in &publications {
        if release.draft {
            continue;
        }
        let date = release
            .published_at
            .as_deref()
            .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.with_timezone(&chrono::Utc).date_naive());
        let tag_version = match &matcher {
            Some(matcher) => matcher
                .captures(&release.tag_name)
                .and_then(|captures| captures.name("version"))
                .map(|matched| matched.as_str()),
            None => release.tag_name.strip_prefix(prefix),
        };
        if let Some(v) = tag_version {
            dates
                .entry((version(v), release.prerelease))
                .and_modify(|known| *known = None)
                .or_insert(date);
        }
    }
    let mut filled = false;
    for note in &mut report.notes {
        if note.authored_notes
            && note.date.is_none()
            && repo_scope(&note.url).is_some()
            && let Some(v) = note.version.as_deref()
            && let Some(Some(date)) = dates.get(&(version(v), note.preview))
        {
            note.date = Some(*date);
            filled = true;
        }
    }
    if filled {
        report.evidence.push(doc.evidence);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn oversized_release_page_retries_smaller_without_retrying_access_denial() {
        struct Pages(bool, RefCell<Vec<String>>);
        impl Fetcher for Pages {
            fn get(&self, url: &str) -> crate::Result<Document> {
                self.1.borrow_mut().push(url.into());
                if self.0 {
                    Err(Error::Source(BODY_LIMIT_ERROR.into()))
                } else {
                    Err(Error::Http {
                        status: 403,
                        url: url.into(),
                        detail: String::new(),
                    })
                }
            }
        }
        let oversized = Pages(true, RefCell::default());
        assert!(release_metadata(&oversized, "owner", "repo").is_err());
        let requested = oversized.1.borrow();
        assert_eq!(requested.len(), 2);
        assert!(requested[0].contains("per_page=100"));
        assert!(requested[1].contains("per_page=30"));
        let denied = Pages(false, RefCell::default());
        assert!(release_metadata(&denied, "owner", "repo").is_err());
        assert_eq!(denied.1.borrow().len(), 1);
    }
}
