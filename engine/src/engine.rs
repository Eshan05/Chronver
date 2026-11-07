use crate::{
    http::{Fetcher, https},
    model::*,
    parse,
    version::Version,
};
use chrono::DateTime;
use regex::Regex;
use scraper::Html;
use serde::Deserialize;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

pub struct Engine<F> {
    pub catalog: Catalog,
    pub fetcher: F,
}
impl<F: Fetcher> Engine<F> {
    pub fn history(
        &self,
        id: &str,
        component: Option<&str>,
        cursor: Option<HistoryCursor>,
        include_inactive: bool,
    ) -> Result<HistoryPage> {
        let package = self.package(id)?;
        if !include_inactive && !matches!(package.tracking.as_str(), "track" | "low_priority") {
            return Err(Error::Query("tracking is inactive".into()));
        }
        let stream = self.stream(package, component)?;
        let start = cursor.as_ref().map_or(0, |c| c.source_index);
        if let Some(c) = &cursor
            && (c.package != id
                || c.stream != stream.id
                || c.page == 0
                || c.page > 1000
                || c.offset > 20_000
                || stream.sources.get(start).is_none_or(|s| {
                    serde_json::to_string(s).ok().as_deref() != Some(&c.source_key)
                }))
        {
            return Err(Error::Query(
                "history cursor does not match this source/stream".into(),
            ));
        }
        let mut attempts = vec![];
        let mut sparse_fallback = None;
        for (index, source) in stream.sources.iter().enumerate().skip(start) {
            let page = cursor.as_ref().map_or(1, |c| c.page);
            let offset = cursor.as_ref().map_or(0, |c| c.offset);
            let fetched = match source {
                Source::Github {
                    owner,
                    repo,
                    tag_prefix,
                    tag_pattern,
                    ..
                } => self.github(
                    owner,
                    repo,
                    tag_prefix,
                    tag_pattern.as_deref(),
                    (page, 20, 1),
                ),
                _ => self
                    .retrieve(source, stream, &Selection::Inspect)
                    .map(|data| (data, None)),
            };
            match fetched {
                Ok((data, next_page)) => {
                    let mut report = select(package, stream, &Selection::Inspect, data)?;
                    let github = matches!(source, Source::Github { .. });
                    let more = if github {
                        next_page.is_some()
                    } else {
                        report.notes.len() > offset + 20
                    };
                    if !github {
                        report.notes = report.notes.into_iter().skip(offset).take(20).collect();
                        report.warnings.push("Single-document source: the document is fetched/cached whole; only this page is returned".into());
                    }
                    if more {
                        report.complete = false;
                        report
                            .warnings
                            .push("More history available; this is a paged snapshot".into());
                    }
                    report.warnings.extend(
                        [&package.caveats, &stream.caveats]
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .cloned(),
                    );
                    report.warnings.sort();
                    report.warnings.dedup();
                    if report.notes.is_empty() {
                        report.status = "no_matching_notes".into();
                    } else if !report.notes.iter().any(|n| n.authored_notes) {
                        report.status = "no_authored_notes".into();
                        report.complete = false;
                    } else if !report.complete && report.status == "available" {
                        report.status = "partial".into();
                    }
                    attempts.push(Attempt {
                        source: source.clone(),
                        outcome: report.status.clone(),
                        detail: report.warnings.join("; "),
                    });
                    report.attempts = attempts.clone();
                    let bounded_more =
                        more && next_page.is_none_or(|p| p <= 1000) && offset + 20 <= 20_000;
                    if more && !bounded_more {
                        report.warnings.push("History cursor budget reached".into());
                    }
                    let next = bounded_more.then(|| HistoryCursor {
                        package: id.into(),
                        stream: stream.id.clone(),
                        source_index: index,
                        source_key: serde_json::to_string(source).unwrap(),
                        page: next_page.unwrap_or(page),
                        offset: if github { 0 } else { offset + 20 },
                    });
                    let page = HistoryPage { report, next };
                    if cursor.is_none()
                        && !page.report.notes.iter().any(|n| n.authored_notes)
                        && index + 1 < stream.sources.len()
                    {
                        if sparse_fallback.as_ref().is_none_or(|old: &HistoryPage| {
                            old.report.notes.len() < page.report.notes.len()
                        }) {
                            sparse_fallback = Some(page);
                        }
                        continue;
                    }
                    if !page.report.notes.iter().any(|n| n.authored_notes)
                        && let Some(mut fallback) = sparse_fallback
                        && fallback.report.notes.len() >= page.report.notes.len()
                    {
                        fallback.report.attempts = attempts;
                        return Ok(fallback);
                    }
                    return Ok(page);
                }
                Err(error) => {
                    attempts.push(Attempt {
                        source: source.clone(),
                        outcome: "failed".into(),
                        detail: error.to_string(),
                    });
                    if cursor.is_some() {
                        break;
                    }
                }
            }
        }
        if let Some(mut fallback) = sparse_fallback {
            fallback.report.attempts = attempts;
            return Ok(fallback);
        }
        Err(Error::Source(
            attempts
                .iter()
                .map(|a| a.detail.as_str())
                .collect::<Vec<_>>()
                .join("; "),
        ))
    }
    pub fn package(&self, id: &str) -> Result<&Package> {
        self.catalog
            .packages
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::Query(format!("unknown package {id}")))
    }
    pub fn stream<'a>(&self, package: &'a Package, component: Option<&str>) -> Result<&'a Stream> {
        match component {
            Some(id) => package
                .streams
                .iter()
                .find(|s| s.id == id)
                .ok_or_else(|| Error::Query(format!("unknown stream {id}"))),
            None if package.streams.len() == 1 => Ok(&package.streams[0]),
            None if package.streams.is_empty() => Err(Error::Query(
                "source references exist, but an extraction profile is not configured yet".into(),
            )),
            None => Err(Error::Query(format!(
                "choose --component from: {}",
                package
                    .streams
                    .iter()
                    .map(|s| s.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }
    pub fn notes(
        &self,
        id: &str,
        component: Option<&str>,
        selection: Selection,
        include_inactive: bool,
    ) -> Result<Report> {
        let package = self.package(id)?;
        if !include_inactive && !matches!(package.tracking.as_str(), "track" | "low_priority") {
            return Err(Error::Query(format!(
                "{} is {}; explicit --include-inactive required",
                package.id, package.tracking
            )));
        }
        let stream = self.stream(package, component)?;
        validate_query(stream, &selection)?;
        let mut attempts = vec![];
        let mut best: Option<Report> = None;
        for source in &stream.sources {
            match self.retrieve(source, stream, &selection) {
                Ok(retrieved) => {
                    let report = select(package, stream, &selection, retrieved)?;
                    attempts.push(Attempt {
                        source: source.clone(),
                        outcome: report.status.clone(),
                        detail: report.warnings.join("; "),
                    });
                    let score = |r: &Report| {
                        (
                            !r.notes.is_empty(),
                            r.complete,
                            r.notes.iter().filter(|n| n.authored_notes).count(),
                            r.notes.len(),
                        )
                    };
                    if best.as_ref().is_none_or(|old| score(&report) > score(old)) {
                        best = Some(report);
                    }
                    if best
                        .as_ref()
                        .is_some_and(|r| r.complete && r.status == "available")
                    {
                        break;
                    }
                }
                Err(error) => attempts.push(Attempt {
                    source: source.clone(),
                    outcome: "failed".into(),
                    detail: error.to_string(),
                }),
            }
        }
        let mut report = best.unwrap_or_else(|| Report {
            package: id.into(),
            stream: stream.id.clone(),
            scope: stream.scope.clone(),
            basis: stream.basis,
            status: "source_failed".into(),
            complete: false,
            notes: vec![],
            warnings: vec!["all configured sources failed".into()],
            evidence: vec![],
            attempts: vec![],
        });
        report.attempts = attempts;
        report.warnings.extend(
            [&package.caveats, &stream.caveats]
                .into_iter()
                .filter(|s| !s.is_empty())
                .cloned(),
        );
        report.warnings.sort();
        report.warnings.dedup();
        Ok(report)
    }
    fn retrieve(
        &self,
        source: &Source,
        stream: &Stream,
        selection: &Selection,
    ) -> Result<Retrieved> {
        match source {
            Source::InstalledDocument {
                root,
                filename,
                package,
                version,
            } => {
                let (body, url) =
                    crate::source_discovery::installed_document(root, filename, package, version)?;
                let document = crate::source_discovery::document(&url, false);
                let notes = parse::document(&body, &document)?
                    .into_iter()
                    .filter(|n| {
                        n.version
                            .as_ref()
                            .and_then(|v| Version::parse(v, stream.scheme).ok())
                            .zip(Version::parse(version, stream.scheme).ok())
                            .is_some_and(|(v, current)| v.compare(&current) != Ordering::Greater)
                    })
                    .collect();
                Ok(Retrieved {
                    notes,
                    complete: false,
                    warnings: vec![
                        "Installed-file snapshot; newer releases are not included.".into(),
                    ],
                    evidence: vec![Evidence {
                        url,
                        fetched_at: chrono::Utc::now().to_rfc3339(),
                        cache: "installed_file".into(),
                    }],
                })
            }
            Source::Github {
                owner,
                repo,
                tag_prefix,
                tag_pattern,
                max_pages,
            } => self
                .github(
                    owner,
                    repo,
                    tag_prefix,
                    tag_pattern.as_deref(),
                    (1, 100, *max_pages),
                )
                .map(|(data, _)| data),
            Source::Document { url, format, .. } => {
                let doc = self.fetcher.get(url)?;
                let mut canonical = source.clone();
                if let Source::Document { url, .. } = &mut canonical {
                    *url = doc.evidence.url.clone();
                }
                let mut notes = parse::document(&doc.body, &canonical)?;
                if stream.channel == Channel::Preview {
                    for note in &mut notes {
                        note.preview = true;
                    }
                }
                Ok(Retrieved {
                    notes,
                    complete: !matches!(format, Format::HtmlEntries | Format::JsonMarkdown),
                    warnings: if matches!(format, Format::HtmlEntries | Format::JsonMarkdown) {
                        vec!["Selected entry snapshot, bounded to 200 entries; complete historical coverage is unverified.".into()]
                    } else {
                        vec![]
                    },
                    evidence: vec![doc.evidence],
                })
            }
            Source::Feed { url } => {
                let doc = self.fetcher.get(url)?;
                Ok(Retrieved {
                    notes: parse::feed(&doc.body, &doc.evidence.url)?,
                    complete: true,
                    warnings: vec![],
                    evidence: vec![doc.evidence],
                })
            }
            Source::LinkedDocuments {
                url,
                links,
                version_pattern,
                content,
                boundary,
                allowed_origins,
                max_documents,
            } => {
                let doc = self.fetcher.get(url)?;
                let html = Html::parse_document(&doc.body);
                let regex =
                    Regex::new(version_pattern).map_err(|e| Error::Config(e.to_string()))?;
                let base = https(&doc.evidence.url)?;
                let mut candidates = Vec::new();
                let mut seen = HashSet::new();
                for link in html.select(&parse::selector(links)?) {
                    let Some(href) = link.value().attr("href") else {
                        continue;
                    };
                    let target = base.join(href).map_err(|e| Error::Source(e.to_string()))?;
                    if target.origin() != base.origin()
                        && !allowed_origins.contains(&target.origin().ascii_serialization())
                    {
                        continue;
                    }
                    let Some(cap) = regex.captures(target.as_str()) else {
                        continue;
                    };
                    let version = cap
                        .name("version")
                        .ok_or_else(|| {
                            Error::Config("version_pattern needs version capture".into())
                        })?
                        .as_str()
                        .to_string();
                    if boundary.is_none()
                        && let Selection::Versions { from, to, .. } = selection
                        && (Version::parse(&version, stream.scheme)?
                            .compare(&Version::parse(from, stream.scheme)?)
                            == Ordering::Less
                            || Version::parse(&version, stream.scheme)?
                                .compare(&Version::parse(to, stream.scheme)?)
                                == Ordering::Greater)
                    {
                        continue;
                    }
                    if seen.insert(target.to_string()) {
                        candidates.push((version, target));
                    }
                }
                if candidates.is_empty() {
                    return Err(Error::Source("no matching versioned document links".into()));
                }
                let mut complete = boundary.is_none() && candidates.len() <= *max_documents;
                let mut warnings = if complete {
                    vec![]
                } else {
                    vec!["Bounded linked-document snapshot; complete historical coverage is unverified".into()]
                };
                let mut evidence = vec![doc.evidence];
                let mut notes = vec![];
                for (version, target) in candidates.into_iter().take(*max_documents) {
                    let page = match self.fetcher.get(target.as_str()) {
                        Ok(page) => page,
                        Err(error) if !notes.is_empty() => {
                            complete = false;
                            warnings.push(format!("later linked document failed: {error}"));
                            break;
                        }
                        Err(error) => return Err(error),
                    };
                    if let Some(boundary) = boundary {
                        let parsed = parse::document(
                            &page.body,
                            &Source::Document {
                                url: page.evidence.url.clone(),
                                format: Format::HtmlEntries,
                                selector: Some(content.clone()),
                                exclude: None,
                                boundary: boundary.clone(),
                                date_formats: vec![],
                                paragraphs: false,
                            },
                        );
                        evidence.push(page.evidence);
                        match parsed {
                            Ok(found) => notes.extend(found),
                            Err(error) if !notes.is_empty() => {
                                complete = false;
                                warnings
                                    .push(format!("later linked document parsing failed: {error}"));
                                break;
                            }
                            Err(error) => return Err(error),
                        }
                        continue;
                    }
                    let body = parse::html_markdown(&page.body, Some(content), &page.evidence.url)?;
                    notes.push(Note {
                        title: version.clone(),
                        version: Some(version),
                        date: None,
                        url: page.evidence.url.clone(),
                        authored_notes: parse::authored(&body),
                        body,
                        preview: false,
                    });
                    evidence.push(page.evidence);
                }
                Ok(Retrieved {
                    notes,
                    complete,
                    warnings,
                    evidence,
                })
            }
        }
    }
    fn github(
        &self,
        owner: &str,
        repo: &str,
        prefix: &str,
        pattern: Option<&str>,
        paging: (usize, usize, usize),
    ) -> Result<(Retrieved, Option<usize>)> {
        let (start, per_page, max_pages) = paging;
        let path = format!("/repos/{owner}/{repo}/releases");
        let canonical = Regex::new(r"^/repositories/[0-9]+/releases$").unwrap();
        let tags = pattern
            .map(|p| Regex::new(&format!("^(?:{p})$")).map_err(|e| Error::Config(e.to_string())))
            .transpose()?;
        let mut url = format!("https://api.github.com{path}?per_page={per_page}&page={start}");
        let mut continuation = None;
        let mut seen = HashSet::new();
        let mut result = Retrieved {
            notes: vec![],
            complete: true,
            warnings: vec![],
            evidence: vec![],
        };
        for page in 0..max_pages {
            if !seen.insert(url.clone()) {
                return Err(Error::Source("GitHub pagination loop".into()));
            }
            let doc = match self.fetcher.get(&url) {
                Ok(doc) => doc,
                Err(error) if page > 0 => {
                    result.complete = false;
                    result
                        .warnings
                        .push(format!("later GitHub page failed: {error}"));
                    break;
                }
                Err(error) => {
                    if start == 1 {
                        let feed_url = format!("https://github.com/{owner}/{repo}/releases.atom");
                        if let Ok(feed) = self.fetcher.get(&feed_url)
                            && let Ok(mut notes) = parse::feed(&feed.body, &feed_url)
                        {
                            let canonical = match https(&feed.evidence.url) {
                                Ok(url) if url.host_str() == Some("github.com") => url,
                                _ => return Err(error),
                            };
                            let parts = canonical.path_segments().unwrap().collect::<Vec<_>>();
                            if parts.len() != 3
                                || parts[2] != "releases.atom"
                                || !parts[..2].iter().all(|s| {
                                    !s.is_empty()
                                        && s.chars()
                                            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
                                })
                            {
                                return Err(error);
                            }
                            let expected = format!("/{}/{}/releases/tag/", parts[0], parts[1]);
                            notes.retain_mut(|note| {
                                let Ok(link) = https(&note.url) else {
                                    return false;
                                };
                                if link.host_str() != Some("github.com") {
                                    return false;
                                }
                                let Some(tag) = link.path().strip_prefix(&expected) else {
                                    return false;
                                };
                                let Ok(tag) =
                                    percent_encoding::percent_decode_str(tag).decode_utf8()
                                else {
                                    return false;
                                };
                                note.version = if let Some(tags) = &tags {
                                    tags.captures(&tag).and_then(|c| {
                                        c.name("version").map(|v| v.as_str().to_string())
                                    })
                                } else {
                                    tag.strip_prefix(prefix).map(str::to_string)
                                };
                                note.version.is_some()
                            });
                            return Ok((
                                Retrieved {
                                    notes,
                                    complete: false,
                                    warnings: vec![format!(
                                        "GitHub API unavailable: {error}; using official public Atom snapshot. Pagination, publication dates and GitHub prerelease flags may be unavailable."
                                    )],
                                    evidence: vec![feed.evidence],
                                },
                                None,
                            ));
                        }
                    }
                    return Err(error);
                }
            };
            result.evidence.push(doc.evidence);
            let releases: Vec<GithubRelease> = match serde_json::from_str(&doc.body) {
                Ok(releases) => releases,
                Err(error) if page > 0 => {
                    result.complete = false;
                    result
                        .warnings
                        .push(format!("later GitHub page was invalid: {error}"));
                    break;
                }
                Err(error) => return Err(error.into()),
            };
            for release in releases {
                if release.draft {
                    continue;
                }
                let capture = tags.as_ref().and_then(|r| r.captures(&release.tag_name));
                let version = match &tags {
                    Some(_) => capture
                        .as_ref()
                        .and_then(|c| c.name("version"))
                        .map(|v| v.as_str()),
                    None => release.tag_name.strip_prefix(prefix),
                };
                let Some(version) = version else {
                    continue;
                };
                let body =
                    parse::release_markdown(&release.body.unwrap_or_default(), &release.html_url)?;
                let date = release
                    .published_at
                    .as_deref()
                    .map(|s| DateTime::parse_from_rfc3339(s).map(|d| d.date_naive()))
                    .transpose();
                let date = match date {
                    Ok(date) => date,
                    Err(error) => {
                        result.complete = false;
                        result.warnings.push(format!(
                            "invalid publication date for {}: {error}",
                            release.tag_name
                        ));
                        None
                    }
                };
                result.notes.push(Note {
                    title: release.name.unwrap_or_else(|| release.tag_name.clone()),
                    version: Some(version.into()),
                    date,
                    url: release.html_url,
                    authored_notes: parse::authored(&body),
                    body,
                    preview: release.prerelease,
                });
            }
            let next = (|| -> Result<Option<String>> {
                let Some(next) = next_link(doc.headers.get("link").map(String::as_str))? else {
                    return Ok(None);
                };
                let parsed = https(&next)?;
                let params: Vec<_> = parsed.query_pairs().collect();
                let expected_page = (start + page + 1).to_string();
                if parsed.origin().ascii_serialization() != "https://api.github.com"
                    || !(parsed.path() == path || canonical.is_match(parsed.path()))
                    || parsed.fragment().is_some()
                    || params.len() != 2
                    || !params
                        .iter()
                        .any(|(k, v)| k == "page" && v == expected_page.as_str())
                    || !params
                        .iter()
                        .any(|(k, v)| k == "per_page" && v == per_page.to_string().as_str())
                {
                    return Err(Error::Source("unsafe GitHub pagination URL".into()));
                }
                // Rebuild our original repository endpoint; never follow repository drift.
                Ok(Some(format!(
                    "https://api.github.com{path}?per_page={per_page}&page={expected_page}"
                )))
            })();
            let next = match next {
                Ok(next) => next,
                Err(error) => {
                    result.complete = false;
                    result
                        .warnings
                        .push(format!("GitHub pagination stopped: {error}"));
                    break;
                }
            };
            match next {
                Some(next) => {
                    continuation = Some(start + page + 1);
                    if page + 1 == max_pages {
                        result.complete = false;
                        if per_page == 100 {
                            result.warnings.push(
                                "GitHub pagination cap reached; history may be incomplete".into(),
                            );
                        }
                    }
                    url = next;
                }
                None => {
                    continuation = None;
                    break;
                }
            }
        }
        if result.notes.is_empty() && per_page == 100 {
            return Err(Error::Source(
                "no published releases with configured tag prefix".into(),
            ));
        }
        Ok((result, continuation))
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
}
fn next_link(header: Option<&str>) -> Result<Option<String>> {
    let Some(header) = header else {
        return Ok(None);
    };
    for part in header.split(',') {
        let mut pieces = part.trim().split(';');
        let link = pieces.next().unwrap_or_default().trim();
        if pieces.any(|p| {
            p.trim()
                .strip_prefix("rel=")
                .is_some_and(|r| r.trim_matches('"').split_whitespace().any(|v| v == "next"))
        }) {
            return link
                .strip_prefix('<')
                .and_then(|s| s.strip_suffix('>'))
                .map(|s| Some(s.to_string()))
                .ok_or_else(|| Error::Source("malformed pagination Link".into()));
        }
    }
    Ok(None)
}

fn validate_query(stream: &Stream, selection: &Selection) -> Result<()> {
    match selection {
        Selection::Versions { from, to, .. } => {
            if stream.basis != Basis::Version {
                return Err(Error::Query(
                    "dated product feeds need --since/--until, not binary versions".into(),
                ));
            }
            if Version::parse(from, stream.scheme)?.compare(&Version::parse(to, stream.scheme)?)
                != Ordering::Less
            {
                return Err(Error::Query(
                    "expected from < to; interval is (from, to]".into(),
                ));
            }
        }
        Selection::Dates { since, until } => {
            if stream.basis != Basis::Date {
                return Err(Error::Query("versioned stream needs --from/--to".into()));
            }
            if since >= until || *until > chrono::Utc::now().date_naive() {
                return Err(Error::Query("expected since < until <= today".into()));
            }
        }
        Selection::Inspect => {}
    }
    Ok(())
}

fn select(
    package: &Package,
    stream: &Stream,
    query: &Selection,
    mut data: Retrieved,
) -> Result<Report> {
    if data
        .evidence
        .iter()
        .any(|e| e.url.ends_with("/releases.atom"))
        && matches!(
            query,
            Selection::Versions {
                channel: Channel::Stable | Channel::Preview,
                ..
            }
        )
    {
        data.notes.clear();
        data.warnings.push("Atom fallback cannot verify GitHub channel flags; use history or the All channel rather than a channel-specific range.".into());
    }
    let mut indexed = Vec::new();
    let bounds = match query {
        Selection::Versions { from, to, channel } => Some((
            Version::parse(from, stream.scheme)?,
            Version::parse(to, stream.scheme)?,
            *channel,
        )),
        _ => None,
    };
    let mut duplicates = HashMap::new();
    let mut exact = HashSet::new();
    for mut note in data.notes {
        let version = if stream.basis == Basis::Version {
            match note
                .version
                .as_deref()
                .map(|v| Version::parse(v, stream.scheme))
                .transpose()
            {
                Ok(Some(v)) => {
                    note.preview |= v.preview();
                    Some(v)
                }
                _ => {
                    data.complete = false;
                    data.warnings.push(format!(
                        "unsupported release tag: {}",
                        note.version.as_deref().unwrap_or("missing")
                    ));
                    continue;
                }
            }
        } else {
            None
        };
        if !exact.insert((
            note.version.clone(),
            note.date,
            note.url.clone(),
            note.body.clone(),
            note.preview,
        )) {
            continue;
        }
        if let Some(ref v) = version
            && bounds.as_ref().is_none_or(|(from, to, channel)| {
                v.compare(from) != Ordering::Less && v.compare(to) != Ordering::Greater
                    && channel_matches(&note, *channel)
            })
        {
            let key = format!("{v:?}");
            if duplicates.insert(key, note.url.clone()).is_some() {
                data.complete = false;
                data.warnings.push(format!(
                    "ambiguous duplicate version: {}",
                    note.version.as_deref().unwrap()
                ));
            }
        }
        indexed.push((version, note));
    }
    match query {
        Selection::Versions { from, to, channel } => {
            let from = Version::parse(from, stream.scheme)?;
            let to = Version::parse(to, stream.scheme)?;
            for (label, boundary) in [("installed", &from), ("target", &to)] {
                if !indexed.iter().any(|(v, _)| {
                    v.as_ref()
                        .is_some_and(|v| v.compare(boundary) == Ordering::Equal)
                }) {
                    data.complete = false;
                    data.warnings.push(format!(
                        "{label} version boundary not found in retrieved history"
                    ));
                } else if !indexed.iter().any(|(v, n)| {
                    v.as_ref()
                        .is_some_and(|v| v.compare(boundary) == Ordering::Equal)
                        && channel_matches(n, *channel)
                }) {
                    data.complete = false;
                    data.warnings.push(format!(
                        "{label} version boundary excluded by selected {channel:?} channel"
                    ));
                }
            }
            indexed.retain(|(v, n)| {
                v.as_ref().is_some_and(|v| {
                    v.compare(&from) == Ordering::Greater && v.compare(&to) != Ordering::Greater
                }) && channel_matches(n, *channel)
            });
            indexed.sort_by(|a, b| b.0.as_ref().unwrap().compare(a.0.as_ref().unwrap()));
        }
        Selection::Dates { since, until } => {
            if indexed.iter().any(|(_, n)| n.date.is_none()) {
                data.complete = false;
                data.warnings
                    .push("some entries lack publication dates".into());
            }
            if !indexed
                .iter()
                .any(|(_, n)| n.date.is_some_and(|date| date <= *since))
            {
                data.complete = false;
                data.warnings
                    .push("requested start predates retrieved dated history".into());
            }
            indexed.retain(|(_, n)| n.date.is_some_and(|date| date > *since && date <= *until));
            indexed.sort_by_key(|a| std::cmp::Reverse(a.1.date));
        }
        Selection::Inspect => {
            indexed.sort_by(|a, b| match (&a.0, &b.0) {
                (Some(a), Some(b)) => b.compare(a),
                _ => b.1.date.cmp(&a.1.date),
            });
        }
    }
    let notes: Vec<_> = indexed.into_iter().map(|(_, n)| n).collect();
    if stream.basis == Basis::Date && notes.iter().any(|n| n.date.is_none()) {
        data.complete = false;
        data.warnings
            .push("some entries lack publication dates".into());
    }
    let authored_count = notes.iter().filter(|n| n.authored_notes).count();
    if package.identity == "metadata_discovered" {
        data.complete = false;
        data.warnings.push("Automatically discovered source; product/component coverage and update applicability are unverified.".into());
    }
    if authored_count < notes.len() {
        data.complete = false;
        data.warnings.push(
            "some releases contain no recognized authored notes; sparse bodies are retained".into(),
        );
    }
    if data
        .evidence
        .iter()
        .any(|e| e.cache == "offline_cache" || e.cache.starts_with("stale_cache"))
    {
        data.complete = false;
        data.warnings
            .push("offline cached snapshot; upstream freshness is unknown".into());
    }
    if !stream.caveats.is_empty() {
        data.warnings.push(stream.caveats.clone());
    }
    data.warnings.sort();
    data.warnings.dedup();
    let status = if notes.is_empty() {
        "no_matching_notes"
    } else if authored_count == 0 {
        "no_authored_notes"
    } else if data.complete {
        "available"
    } else {
        "partial"
    };
    Ok(Report {
        package: package.id.clone(),
        stream: stream.id.clone(),
        scope: stream.scope.clone(),
        basis: stream.basis,
        status: status.into(),
        complete: data.complete,
        notes,
        warnings: data.warnings,
        evidence: data.evidence,
        attempts: vec![],
    })
}

fn channel_matches(note: &Note, channel: Channel) -> bool {
    match channel {
        Channel::Stable => !note.preview,
        Channel::Preview => note.preview,
        Channel::All => true,
    }
}
