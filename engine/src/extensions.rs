//! Shared extension notes: installed artifact, Marketplace asset, scoped repository.
use crate::{
    http::{Document, Fetcher, HttpClient, Transport},
    *,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Installed {
    #[serde(default)]
    pub id: String,
    pub root: Option<PathBuf>,
    pub changelog: Option<PathBuf>,
    pub repository: Option<String>,
    pub directory: Option<String>,
    pub pre_release: Option<bool>,
    pub target_platform: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub profile_name: Option<String>,
    #[serde(default)]
    pub profile_icon: Option<String>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Installed,
    Published,
}
pub trait Gallery: Fetcher {
    fn query(&self, id: &str) -> Result<Document>;
}
impl<T: Transport> Gallery for HttpClient<T> {
    fn query(&self, id: &str) -> Result<Document> {
        self.marketplace_query(id)
    }
}
fn read(path: &Path) -> Result<String> {
    let file = fs::File::open(path)?;
    let mut body = String::new();
    file.take(1024 * 1024 + 1).read_to_string(&mut body)?;
    if body.len() > 1024 * 1024 {
        return Err(Error::Source("extension document exceeds 1 MiB".into()));
    }
    Ok(body)
}
fn identity(id: &str) -> Result<(&str, &str)> {
    let regex =
        regex::Regex::new(r"^([A-Za-z0-9][A-Za-z0-9_-]*)\.([A-Za-z0-9][A-Za-z0-9_.-]*)$").unwrap();
    if !regex.is_match(id) {
        return Err(Error::Query("invalid extension identity".into()));
    }
    Ok(id.split_once('.').unwrap())
}
fn local(id: &str, version: &str, info: &Installed) -> Result<(String, String)> {
    let root = info
        .root
        .as_ref()
        .ok_or_else(|| Error::Source("extension installation root unavailable".into()))?
        .canonicalize()?;
    let manifest: Value = serde_json::from_str(&read(&root.join("package.json"))?)?;
    let actual = format!(
        "{}.{}",
        manifest["publisher"].as_str().unwrap_or(""),
        manifest["name"].as_str().unwrap_or("")
    );
    if !actual.eq_ignore_ascii_case(id) || manifest["version"] != version {
        return Err(Error::Source(
            "installed extension changed; rescan inventory".into(),
        ));
    }
    let path = info
        .changelog
        .as_ref()
        .ok_or_else(|| Error::Source("extension has no bundled changelog".into()))?
        .canonicalize()?;
    if !path.starts_with(&root) {
        return Err(Error::Source("changelog escapes extension root".into()));
    }
    Ok((
        read(&path)?,
        url::Url::from_file_path(path)
            .map_err(|_| Error::Source("invalid changelog path".into()))?
            .into(),
    ))
}
fn artifact_url(input: &str) -> Result<String> {
    let url = crate::http::https(input)?;
    let host = url.host_str().unwrap();
    if url.port_or_known_default() != Some(443)
        || !(host.ends_with(".vsassets.io") || host == "marketplace.visualstudio.com")
    {
        return Err(Error::Source("unexpected Marketplace asset host".into()));
    }
    Ok(url.into())
}
pub fn marketplace_asset(
    data: &Value,
    id: &str,
    version: &str,
    info: &Installed,
    mode: Mode,
    channel: Channel,
) -> Result<(String, String)> {
    let (publisher, name) = identity(id)?;
    let ext = data["results"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|v| v["extensions"].as_array().into_iter().flatten())
        .filter(|v| {
            v["publisher"]["publisherName"]
                .as_str()
                .is_some_and(|p| p.eq_ignore_ascii_case(publisher))
                && v["extensionName"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .collect::<Vec<_>>();
    if ext.len() != 1 {
        return Err(Error::Source(
            "Marketplace extension identity missing or ambiguous".into(),
        ));
    }
    let mut versions =
        ext[0]["versions"]
            .as_array()
            .ok_or_else(|| Error::Source("Marketplace versions missing".into()))?
            .iter()
            .filter_map(|v| {
                let text = v["version"].as_str()?;
                let ver = semver::Version::parse(text).ok()?;
                let platform = v["targetPlatform"].as_str().unwrap_or("universal");
                let platform_ok = platform == "universal"
                    || info
                        .target_platform
                        .as_deref()
                        .filter(|p| !p.is_empty())
                        .is_some_and(|p| p == platform);
                let preview = v["properties"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|p| p["key"] == "Microsoft.VisualStudio.Code.PreRelease")
                    .and_then(|p| p["value"].as_str())
                    .map_or(!ver.pre.is_empty(), |v| v.eq_ignore_ascii_case("true"));
                let channel_ok = if mode == Mode::Installed {
                    info.pre_release.is_none_or(|p| p == preview)
                } else {
                    matches!(channel, Channel::All)
                        || preview == matches!(channel, Channel::Preview)
                };
                (platform_ok && channel_ok && (mode == Mode::Published || text == version))
                    .then_some((ver, platform != "universal", v))
            })
            .collect::<Vec<_>>();
    versions.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let (_, _, v) = versions.first().ok_or_else(|| {
        Error::Source("no matching publication for this extension channel/platform".into())
    })?;
    if versions
        .iter()
        .filter(|v| v.0 == versions[0].0 && v.1 == versions[0].1)
        .count()
        != 1
    {
        return Err(Error::Source(
            "ambiguous Marketplace version/platform/channel".into(),
        ));
    }
    let urls = v["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["assetType"] == "Microsoft.VisualStudio.Services.Content.Changelog")
        .filter_map(|f| f["source"].as_str())
        .collect::<Vec<_>>();
    if urls.len() != 1 {
        return Err(Error::Source(
            "publication has no unique changelog asset".into(),
        ));
    }
    Ok((
        artifact_url(urls[0])?,
        v["version"].as_str().unwrap().into(),
    ))
}
fn repository_root(info: &Installed) -> Option<String> {
    let repo = info.repository.as_deref()?.trim_start_matches("git+");
    let url = crate::http::https(repo).ok()?;
    if url.host_str() != Some("github.com") || url.port().is_some() {
        return None;
    }
    let pieces = url
        .path_segments()?
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    if pieces.len() != 2 {
        return None;
    }
    let owner = pieces[0];
    let repo = pieces[1].trim_end_matches(".git");
    let mut root = url::Url::parse("https://raw.githubusercontent.com/").ok()?;
    {
        let mut segments = root.path_segments_mut().ok()?;
        segments.pop_if_empty().push(owner).push(repo).push("HEAD");
        if let Some(dir) = &info.directory {
            for part in dir.split('/') {
                if part.is_empty() {
                    continue;
                }
                if part == ".." || part == "." || part.contains(['\\', '%']) {
                    return None;
                }
                segments.push(part);
            }
        }
        segments.push("");
    }
    Some(root.into())
}

fn link_only(body: &str) -> bool {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut heading = false;
    let mut link = false;
    let mut links = 0;
    let mut prose = String::new();
    for event in Parser::new(body) {
        match event {
            Event::Start(Tag::Heading { .. }) => heading = true,
            Event::End(TagEnd::Heading(_)) => heading = false,
            Event::Start(Tag::Link { .. }) => {
                link = true;
                links += 1;
            }
            Event::End(TagEnd::Link) => link = false,
            Event::Text(text) if !heading && !link => prose.push_str(&text),
            Event::Start(Tag::List(_) | Tag::CodeBlock(_) | Tag::Image { .. }) | Event::Code(_) => {
                return false;
            }
            _ => {}
        }
    }
    (1..=2).contains(&links)
        && body.len() < 1500
        && (prose.trim().is_empty()
            || (prose.len() < 180
                && ["release", "changelog", "click", "see"]
                    .iter()
                    .any(|word| prose.to_lowercase().contains(word))))
}

fn useful(report: &Report) -> bool {
    report.notes.iter().any(|note| note.authored_notes)
}

fn manifest_matches(data: &Value, id: &str) -> bool {
    format!(
        "{}.{}",
        data["publisher"].as_str().unwrap_or(""),
        data["name"].as_str().unwrap_or("")
    )
    .eq_ignore_ascii_case(id)
}

/// A repository release is only extension history when its tagged, scoped manifest
/// confirms both identity and version. Linked upstream/monorepo releases may differ.
fn repository_release(
    fetch: &impl Gallery,
    package: &str,
    id: &str,
    installed: &str,
    info: &Installed,
    selection: (Mode, Channel, Option<&str>),
) -> Result<Report> {
    let (mode, channel, ceiling) = selection;
    let root = repository_root(info)
        .ok_or_else(|| Error::Source("no supported extension repository".into()))?;
    let root = url::Url::parse(&root).unwrap();
    let segments: Vec<_> = root.path_segments().unwrap().collect();
    let base = format!(
        "https://api.github.com/repos/{}/{}/releases",
        segments[0], segments[1]
    );
    let version = if mode == Mode::Installed {
        Some(installed)
    } else {
        ceiling
    };
    let preview = if mode == Mode::Installed {
        info.pre_release
            .unwrap_or_else(|| semver::Version::parse(installed).is_ok_and(|v| !v.pre.is_empty()))
    } else {
        channel == Channel::Preview
    };
    let mut candidates = vec![];
    if let Some(version) = version {
        let tags = [version.into(), format!("v{version}")];
        for tag in tags {
            let mut url = url::Url::parse(&format!("{base}/tags/")).unwrap();
            url.path_segments_mut().unwrap().pop_if_empty().push(&tag);
            if let Ok(doc) = fetch.get(url.as_str()) {
                candidates.push((serde_json::from_str::<Value>(&doc.body)?, doc.evidence));
            }
        }
    }
    if candidates.is_empty() {
        let doc = fetch.get(&format!("{base}?per_page=20"))?;
        let data: Vec<Value> = serde_json::from_str(&doc.body)?;
        candidates.extend(data.into_iter().take(20).map(|v| (v, doc.evidence.clone())));
    }
    let current = semver::Version::parse(installed).map_err(|e| Error::Version(e.to_string()))?;
    let pattern = regex::Regex::new(
        r"^(?:[^\s]+[/@-])?v?(?P<version>\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)$",
    )
    .unwrap();
    for (release, evidence) in candidates {
        if release["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let tag = release["tag_name"].as_str().unwrap_or("");
        let Some(capture) = pattern.captures(tag) else {
            continue;
        };
        let ver = &capture["version"];
        let parsed = semver::Version::parse(ver).map_err(|e| Error::Version(e.to_string()))?;
        if version.is_some_and(|expected| expected != ver) {
            continue;
        }
        let prerelease = release["prerelease"].as_bool().unwrap_or(false) || !parsed.pre.is_empty();
        if (mode == Mode::Installed && (parsed != current || prerelease != preview))
            || (mode == Mode::Published
                && (parsed <= current || (channel != Channel::All && prerelease != preview)))
        {
            continue;
        }
        let mut manifest_url = url::Url::parse("https://raw.githubusercontent.com/").unwrap();
        {
            let mut path = manifest_url.path_segments_mut().unwrap();
            path.pop_if_empty()
                .push(segments[0])
                .push(segments[1])
                .push(tag);
            for segment in segments.iter().skip(3).filter(|s| !s.is_empty()) {
                path.push(segment);
            }
            path.push("package.json");
        }
        let Ok(manifest) = fetch.get(manifest_url.as_str()) else {
            continue;
        };
        let data: Value = serde_json::from_str(&manifest.body)?;
        if !manifest_matches(&data, id) || data["version"] != ver {
            continue;
        }
        let mut url = url::Url::parse(&format!(
            "https://github.com/{}/{}/releases/tag/",
            segments[0], segments[1]
        ))
        .unwrap();
        url.path_segments_mut().unwrap().pop_if_empty().push(tag);
        let url = url.to_string();
        let body = crate::parse::release_markdown(release["body"].as_str().unwrap_or(""), &url)?;
        let authored = crate::parse::authored(&body);
        return Ok(Report {
            package: package.into(), stream: "extension".into(),
            scope: format!("Extension repository release {ver} / tagged manifest verified"),
            basis: Basis::Version, status: if authored { "partial" } else { "no_authored_notes" }.into(), complete: false,
            notes: vec![Note {
                title: release["name"].as_str().filter(|s| !s.is_empty()).unwrap_or(tag).into(),
                version: Some(ver.into()),
                date: release["published_at"].as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.date_naive()),
                url, body, authored_notes: authored, preview: prerelease,
            }],
            warnings: vec!["One identity-verified repository release, not complete history. Editor compatibility and update availability were not inferred.".into()],
            evidence: vec![evidence, manifest.evidence], attempts: vec![],
        });
    }
    Err(Error::Source(
        "no identity/version-verified extension repository release".into(),
    ))
}
fn report(
    package: &str,
    id: &str,
    installed: &str,
    body: &str,
    url: &str,
    ceiling: Option<&str>,
    mode: Mode,
) -> Result<Report> {
    if body.len() > 1024 * 1024 {
        return Err(Error::Source("extension changelog too large".into()));
    }
    if body.trim().is_empty() {
        return Err(Error::Source("empty extension changelog".into()));
    }
    let source=Source::Document{url:url.into(),format:Format::Markdown,boundary:r"(?i)(?:version\s+|release\s+)?\[?v?(?P<version>\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)\]?(?:\s+.*)?".into(),selector:None,exclude:None,date_formats:vec![],paragraphs:false};
    let mut warnings=vec!["Changelog coverage is unverified; no extension update availability or editor compatibility was inferred.".into()];
    let current = semver::Version::parse(installed).map_err(|e| Error::Version(e.to_string()))?;
    let ceiling = ceiling.and_then(|v| semver::Version::parse(v).ok());
    let mut notes = match crate::parse::changelog(body, &source) {
        Ok(notes) => notes
            .into_iter()
            .filter(|n| {
                n.version
                    .as_deref()
                    .and_then(|v| semver::Version::parse(v).ok())
                    .is_some_and(|v| {
                        if mode == Mode::Installed {
                            v <= current
                        } else {
                            v > current && ceiling.as_ref().is_none_or(|c| &v <= c)
                        }
                    })
            })
            .collect::<Vec<_>>(),
        Err(error) => {
            let pointer = link_only(body);
            warnings.push(format!("No versioned sections: {error}. This document does not establish an installed-to-target range."));
            vec![Note {
                title: if pointer {
                    "Release-notes link"
                } else {
                    "Unversioned changelog"
                }
                .into(),
                version: None,
                date: None,
                url: url.into(),
                body: body.into(),
                authored_notes: !pointer && crate::parse::authored(body),
                preview: false,
            }]
        }
    };
    let dates = regex::Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").unwrap();
    for n in &mut notes {
        if let Some(v) = n
            .version
            .as_deref()
            .and_then(|v| semver::Version::parse(v).ok())
        {
            n.preview = !v.pre.is_empty();
        }
        n.date = dates
            .find(&n.title)
            .and_then(|d| chrono::NaiveDate::parse_from_str(d.as_str(), "%Y-%m-%d").ok());
    }
    notes.sort_by(|a, b| {
        b.version
            .as_deref()
            .and_then(|v| semver::Version::parse(v).ok())
            .cmp(
                &a.version
                    .as_deref()
                    .and_then(|v| semver::Version::parse(v).ok()),
            )
    });
    if notes.len() > 200 {
        notes.truncate(200);
        warnings.push("Only the newest 200 parsed sections are shown.".into());
    }
    Ok(Report {
        package: package.into(),
        stream: "extension".into(),
        scope: format!("{id} / installed {installed}"),
        basis: Basis::Version,
        status: if notes.is_empty() {
            "no_matching_notes"
        } else {
            "partial"
        }
        .into(),
        complete: false,
        notes,
        warnings,
        evidence: vec![],
        attempts: vec![],
    })
}
pub fn notes(
    fetch: &impl Gallery,
    package: &str,
    id: &str,
    installed: &str,
    info: &Installed,
    mode: Mode,
    channel: Channel,
) -> Result<Report> {
    identity(id)?;
    let mut failures = vec![];
    let mut fallback = None;
    let mut ceiling = None;
    if mode == Mode::Installed {
        match local(id, installed, info) {
            Ok((body, url)) => {
                let mut r = report(package, id, installed, &body, &url, Some(installed), mode)?;
                r.scope = "Bundled installed-version changelog".into();
                r.evidence.push(Evidence {
                    url,
                    fetched_at: chrono::Utc::now().to_rfc3339(),
                    cache: "installed_file".into(),
                    content_sha256: None,
                });
                if useful(&r) {
                    return Ok(r);
                }
                failures.push("Bundled changelog contains only links or no matching authored notes; trying published sources.".into());
                fallback = Some(r);
            }
            Err(e) => failures.push(e.to_string()),
        }
    }
    let published: Result<Report> = (|| {
        let query = fetch.query(id)?;
        let data: Value = serde_json::from_str(&query.body)?;
        let (url, version) = marketplace_asset(&data, id, installed, info, mode, channel)?;
        ceiling = Some(version.clone());
        let doc = fetch.get(&url)?;
        let mut r = report(
            package,
            id,
            installed,
            &doc.body,
            &url,
            Some(&version),
            mode,
        )?;
        r.scope = format!(
            "Marketplace publication {version} / {}",
            if mode == Mode::Published {
                "notes newer than installed"
            } else {
                "installed version"
            }
        );
        r.warnings.push("The publication channel selects the artifact, not necessarily every section within its changelog.".into());
        r.evidence.extend([query.evidence, doc.evidence]);
        Ok(r)
    })();
    match published {
        Ok(r) if useful(&r) => return Ok(r),
        Ok(r) => {
            failures.push(
                "Marketplace changelog contains only links or no matching authored notes.".into(),
            );
            fallback = Some(r);
        }
        Err(e) => failures.push(e.to_string()),
    }
    if let Some(root) = repository_root(info) {
        let repository = (|| {
            let manifest = fetch.get(&format!("{root}package.json"))?;
            let data: Value = serde_json::from_str(&manifest.body)?;
            if !manifest_matches(&data, id) {
                return Err(Error::Source(
                    "repository manifest is not this extension; refusing repository-wide notes"
                        .into(),
                ));
            }
            for name in ["CHANGELOG.md", "changelog.md", "Changelog.md"] {
                let url = format!("{root}{name}");
                if let Ok(doc) = fetch.get(&url) {
                    let mut r = report(
                        package,
                        id,
                        installed,
                        &doc.body,
                        &url,
                        data["version"].as_str(),
                        mode,
                    )?;
                    r.scope = "Repository HEAD changelog (publication unverified)".into();
                    r.warnings.extend(failures.clone());
                    r.warnings.push("Repository HEAD may contain unpublished notes. Marketplace channel and update applicability could not be established.".into());
                    r.evidence.extend([manifest.evidence, doc.evidence]);
                    if useful(&r) {
                        return Ok(r);
                    }
                    fallback = Some(r);
                    break;
                }
            }
            Err(Error::Source(
                "repository has no readable root changelog".into(),
            ))
        })();
        match repository {
            Ok(r) => return Ok(r),
            Err(e) => failures.push(e.to_string()),
        }
    }
    match repository_release(
        fetch,
        package,
        id,
        installed,
        info,
        (mode, channel, ceiling.as_deref()),
    ) {
        Ok(mut report) => {
            report.warnings.extend(failures);
            if let Some(previous) = fallback {
                report.evidence.extend(previous.evidence);
                report.warnings.push("A link-only changelog was bypassed. Upstream links were not assumed to share the extension's version numbering.".into());
            }
            return Ok(report);
        }
        Err(error) => failures.push(error.to_string()),
    }
    if let Some(mut report) = fallback {
        report.warnings.extend(failures);
        report.status = "no_authored_notes".into();
        return Ok(report);
    }
    Err(Error::Source(format!(
        "Extension notes unavailable: {}",
        failures.join("; ")
    )))
}
