//! Bounded source discovery from installed metadata, never package-name web searches.
use crate::{
    http::{CacheOptions, Fetcher, HttpClient},
    inventory::InstalledPackage,
    *,
};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use url::Url;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hints {
    pub manager: String,
    pub source: String,
    pub id: String,
    pub installed: Option<String>,
    pub repository: Option<String>,
    pub directory: Option<String>,
    pub homepage: Option<String>,
    pub release_notes: Option<String>,
    pub downloads: Vec<String>,
    pub root: Option<String>,
    pub references: Vec<String>,
}
impl Hints {
    pub fn from_record(p: &InstalledPackage) -> Self {
        Self {
            manager: p.manager.id().into(),
            source: p.source.clone(),
            id: p.id.clone(),
            installed: p.installed.clone(),
            repository: p.repository.clone(),
            directory: p.repository_directory.clone(),
            homepage: p.homepage.clone(),
            release_notes: p.release_notes.clone(),
            downloads: p.download_urls.clone(),
            root: p.package_root.clone(),
            references: vec![],
        }
    }
    pub fn has_hints(&self) -> bool {
        self.repository.is_some()
            || self.homepage.is_some()
            || self.release_notes.is_some()
            || !self.downloads.is_empty()
            || !self.references.is_empty()
            || self.root.is_some()
            || winget_manifest(self).is_some()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Discovery {
    pub streams: Vec<Stream>,
    pub attempts: Vec<String>,
}
#[derive(Serialize, Deserialize)]
struct Cached {
    #[serde(default)]
    version: u8,
    hints: Hints,
    at: i64,
    result: Discovery,
}
const CACHE_REVISION: u8 = 4;

pub fn cached(mut options: CacheOptions, hints: &Hints) -> Discovery {
    let directory = options.directory.with_file_name("sources");
    let key = format!("{:x}", Sha256::digest(serde_json::to_vec(hints).unwrap()));
    let path = directory.join(format!("{key}.json"));
    let now = chrono::Utc::now().timestamp();
    if !options.refresh
        && let Some(entry) = fs::File::open(&path)
            .ok()
            .and_then(|f| serde_json::from_reader::<_, Cached>(f.take(256 * 1024)).ok())
        && entry.version == CACHE_REVISION
        && entry.hints == *hints
        && (options.offline
            || (0..if entry.result.streams.is_empty() {
                3600
            } else {
                86400
            })
                .contains(&(now - entry.at)))
    {
        return entry.result;
    }
    options.public_only = true;
    let result = HttpClient::new(options.clone())
        .map(|fetch| discover(&fetch, hints))
        .unwrap_or_else(|e| Discovery {
            attempts: vec![e.to_string()],
            ..Default::default()
        });
    if !options.offline
        && let Ok(()) = fs::create_dir_all(&directory)
        && let Ok(mut file) = tempfile::NamedTempFile::new_in(&directory)
        && let Ok(bytes) = serde_json::to_vec(&Cached {
            version: CACHE_REVISION,
            hints: hints.clone(),
            at: now,
            result: result.clone(),
        })
        && file.write_all(&bytes).is_ok()
    {
        let _ = file.persist(path);
    }
    result
}
struct Budget<'a, F> {
    fetch: &'a F,
    calls: Cell<usize>,
    started: Instant,
}
impl<F: Fetcher> Fetcher for Budget<'_, F> {
    fn get(&self, url: &str) -> Result<http::Document> {
        if self.calls.get() >= 12 || self.started.elapsed() > Duration::from_secs(30) {
            return Err(Error::Source("source discovery budget reached".into()));
        }
        crate::icons::safe_url(url)
            .ok_or_else(|| Error::Source("unsafe source metadata URL".into()))?;
        self.calls.set(self.calls.get() + 1);
        self.fetch.get(url)
    }
}
pub fn document(url: &str, dated: bool) -> Source {
    Source::Document{url:url.into(),format:if Url::parse(url).ok().is_some_and(|u|u.host_str()==Some("raw.githubusercontent.com") || u.path().ends_with(".md") || u.path().ends_with(".txt")) {Format::Markdown}else{Format::Html},
        boundary:if dated {r"(?:.*?\s)?(?P<date>\d{4}-\d{2}-\d{2}|[A-Za-z]+\s+\d{1,2},?\s+\d{4})(?:\s+.*)?"}else{r"(?i)(?:(?:version|release)\s+|[A-Za-z][A-Za-z0-9_+.-]{0,40}\s+)?\[?(?:[A-Za-z][A-Za-z_+-]{0,20})?(?P<version>\d+\.\d+(?:\.\d+){0,2}(?:-[0-9A-Za-z.-]+)?)\]?(?:\s+.*)?"}.into(),selector:None,exclude:None,date_formats:if dated {vec!["%Y-%m-%d".into(),"%B %d, %Y".into(),"%b %d, %Y".into(),"%B %d %Y".into(),"%b %d %Y".into()]}else{vec![]},paragraphs:dated || Url::parse(url).ok().is_some_and(|u|!u.path().ends_with(".md") && u.path().to_ascii_lowercase().contains("changelog"))}
}
fn https(input: &str) -> Option<Url> {
    let input = input.trim().trim_start_matches("git+");
    let expanded = input
        .strip_prefix("git@github.com:")
        .map(|path| format!("https://github.com/{path}"));
    let mut url = Url::parse(expanded.as_deref().unwrap_or(input)).ok()?;
    if url.host_str() == Some("github.com") && matches!(url.scheme(), "git" | "ssh") {
        if !matches!(url.username(), "" | "git") || url.password().is_some() || url.port().is_some()
        {
            return None;
        }
        let mut clean = Url::parse("https://github.com").ok()?;
        clean.set_path(url.path());
        url = clean;
    }
    if url.scheme() == "http" {
        url.set_scheme("https").ok()?;
    }
    crate::icons::safe_url(url.as_str())
}
fn notes_url(input: &str) -> Option<String> {
    if let Some(url) = https(input) {
        return Some(url.into());
    }
    for event in pulldown_cmark::Parser::new(input) {
        if let pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) = event
            && let Some(url) = https(&dest_url)
        {
            return Some(url.into());
        }
    }
    let html = Html::parse_fragment(input);
    let selector = Selector::parse("a[href]").unwrap();
    html.select(&selector)
        .find_map(|e| https(e.value().attr("href")?).map(Into::into))
}
fn reference_document(input: &str) -> Option<String> {
    let url = https(input)?;
    if url.host_str() == Some("github.com") {
        let parts = url.path_segments()?.collect::<Vec<_>>();
        if parts.len() < 5 || parts[2] != "blob" {
            return None;
        }
        let mut raw = Url::parse("https://raw.githubusercontent.com/").ok()?;
        {
            let mut path = raw.path_segments_mut().ok()?;
            path.pop_if_empty();
            for (i, part) in parts.into_iter().enumerate() {
                if i != 2 {
                    path.push(part);
                }
            }
        }
        return Some(raw.into());
    }
    Some(url.into())
}
fn github(input: &str) -> Option<(String, String, Option<String>)> {
    let url = https(input)?;
    if url.host_str() != Some("github.com") {
        return None;
    }
    let parts = url
        .path_segments()?
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2
        || !parts[..2].iter().all(|s| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        })
    {
        return None;
    }
    let tag = (parts.get(2) == Some(&"releases")
        && matches!(parts.get(3), Some(&"download" | &"tag")))
    .then(|| parts.get(4).map(|s| s.to_string()))
    .flatten();
    Some((
        parts[0].into(),
        parts[1].trim_end_matches(".git").into(),
        tag,
    ))
}
fn release_tag_parts<'a>(tag: &'a str, installed: &str) -> Option<(&'a str, &'a str)> {
    let part = |version: &str| {
        if version.is_empty() {
            return None;
        }
        let mut matches = tag.match_indices(version).filter_map(|(offset, _)| {
            let prefix = &tag[..offset];
            let suffix = &tag[offset + version.len()..];
            if prefix.as_bytes().last().is_some_and(u8::is_ascii_digit)
                || suffix.as_bytes().first().is_some_and(u8::is_ascii_digit)
                || (suffix.starts_with('.') && suffix.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
                || (prefix.ends_with('.') && prefix.as_bytes().iter().rev().nth(1).is_some_and(u8::is_ascii_digit))
            {
                return None;
            }
            Some((prefix, suffix))
        });
        let first = matches.next()?;
        matches.next().is_none().then_some(first)
    };
    part(installed).or_else(|| {
        installed.strip_suffix(".0")
            .filter(|_| installed.split('.').count() == 4)
            .and_then(part)
    })
}

fn repo_source(
    owner: &str,
    repo: &str,
    tag_parts: Option<(&str, &str)>,
) -> Source {
    let version = r"[0-9]+\.[0-9]+(?:\.[0-9]+){0,2}(?:-[0-9A-Za-z.-]+)?";
    let pattern = if let Some((prefix, suffix)) = tag_parts {
        format!(
            "^{}(?P<version>{version}){}$",
            regex::escape(prefix),
            regex::escape(suffix)
        )
    } else {
        let literal = regex::escape(repo);
        format!("^(?:(?i:{literal})[-_])?[vV]?(?P<version>{version})$")
    };
    Source::Github {
        owner: owner.into(),
        repo: repo.into(),
        tag_prefix: String::new(),
        tag_pattern: Some(pattern),
        max_pages: 1,
    }
}
fn repo_root(owner: &str, repo: &str, directory: Option<&str>) -> Option<String> {
    let mut url = Url::parse("https://raw.githubusercontent.com/").ok()?;
    {
        let mut path = url.path_segments_mut().ok()?;
        path.pop_if_empty().push(owner).push(repo).push("HEAD");
        for part in directory.unwrap_or("").split('/').filter(|p| !p.is_empty()) {
            if part == "." || part == ".." || part.contains(['\\', '%']) {
                return None;
            }
            path.push(part);
        }
        path.push("");
    }
    Some(url.into())
}
pub fn installed_document(
    root: &str,
    name: &str,
    id: &str,
    installed: &str,
) -> Result<(String, String)> {
    fn read(path: &Path) -> Result<String> {
        let mut body = String::new();
        fs::File::open(path)?
            .take(1024 * 1024 + 1)
            .read_to_string(&mut body)?;
        if body.len() > 1024 * 1024 {
            return Err(Error::Source("installed document too large".into()));
        }
        Ok(body)
    }
    if !["CHANGELOG.md", "changelog.md", "CHANGES.md", "NEWS.md"].contains(&name) {
        return Err(Error::Source(
            "unsupported installed changelog filename".into(),
        ));
    }
    let root = PathBuf::from(root).canonicalize()?;
    let manifest_path = root.join("package.json").canonicalize()?;
    if !manifest_path.starts_with(&root) {
        return Err(Error::Source("manifest escapes installed root".into()));
    }
    let manifest: serde_json::Value = serde_json::from_str(&read(&manifest_path)?)?;
    if manifest["name"] != id || manifest["version"] != installed {
        return Err(Error::Source(
            "installed package changed; rescan inventory".into(),
        ));
    }
    let path = root.join(name).canonicalize()?;
    if !path.starts_with(root) {
        return Err(Error::Source("changelog escapes installed root".into()));
    }
    Ok((
        read(&path)?,
        Url::from_file_path(path)
            .map_err(|_| Error::Source("invalid installed path".into()))?
            .into(),
    ))
}
fn stream(source: Source, basis: Basis, _hints: &Hints) -> Stream {
    Stream{id:if basis==Basis::Version {"version-notes"}else{"dated-updates"}.into(),scope:if basis==Basis::Version {"Metadata-discovered version notes"}else{"Official-site dated updates; installed-version applicability unverified"}.into(),basis,
        scheme:Scheme::Numeric,channel:Channel::Stable,caveats:"Automatically discovered from installed metadata, not a reviewed product/component binding. Two-part headings describe a release line, not each patch in that line.".into(),sources:vec![source]}
}
fn usable(fetch: &impl Fetcher, s: &Stream, hints: &Hints) -> Result<()> {
    let package = Package {
        id: "probe".into(),
        name: hints.id.clone(),
        tracking: "track".into(),
        identity: "metadata_discovered".into(),
        alias_of: None,
        caveats: String::new(),
        reference_urls: vec![],
        streams: vec![s.clone()],
    };
    let engine = Engine {
        catalog: Catalog {
            schema_version: 1,
            packages: vec![package],
        },
        fetcher: Borrowed(fetch),
    };
    let page = engine.history("probe", None, None, false)?;
    if page.report.notes.iter().any(|n| n.authored_notes) {
        Ok(())
    } else {
        Err(Error::Source(
            "source returned no authored release notes".into(),
        ))
    }
}
struct Borrowed<'a, F>(&'a F);
impl<F: Fetcher> Fetcher for Borrowed<'_, F> {
    fn get(&self, u: &str) -> Result<http::Document> {
        self.0.get(u)
    }
}
fn winget_manifest(hints: &Hints) -> Option<String> {
    if hints.manager != "winget" || !hints.source.eq_ignore_ascii_case("winget") {
        return None;
    }
    let version = hints.installed.as_deref()?;
    let valid = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c))
    };
    if !hints.id.contains('.') || !hints.id.split('.').all(valid) || !valid(version) {
        return None;
    }
    let mut url =
        Url::parse("https://raw.githubusercontent.com/microsoft/winget-pkgs/master/manifests/")
            .ok()?;
    {
        let mut path = url.path_segments_mut().ok()?;
        path.pop_if_empty()
            .push(&hints.id[..1].to_ascii_lowercase());
        for part in hints.id.split('.') {
            path.push(part);
        }
        path.push(version)
            .push(&format!("{}.locale.en-US.yaml", hints.id));
    }
    Some(url.into())
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WingetLocale {
    package_identifier: String,
    package_version: String,
    package_url: Option<String>,
    release_notes_url: Option<String>,
    default_locale: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WingetInstaller {
    package_identifier: String,
    package_version: String,
    #[serde(default)]
    installers: Vec<WingetDownload>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct WingetDownload {
    installer_url: Option<String>,
}
fn winget_locale(fetch: &impl Fetcher, hints: &Hints, url: &str) -> Result<WingetLocale> {
    let parse = |body: &str| {
        serde_yaml_ng::from_str::<WingetLocale>(body).map_err(|e| Error::Source(e.to_string()))
    };
    let read = |url: &str| fetch.get(url).and_then(|d| parse(&d.body));
    let matches = |m: &WingetLocale| {
        m.package_identifier == hints.id
            && hints.installed.as_deref() == Some(m.package_version.as_str())
    };
    if let Ok(locale) = read(url) {
        if matches(&locale) {
            return Ok(locale);
        }
        return Err(Error::Source(
            "WinGet manifest identity/version mismatch; ignored".into(),
        ));
    }
    let mut base = Url::parse(url).map_err(|e| Error::Source(e.to_string()))?;
    base.path_segments_mut()
        .unwrap()
        .pop()
        .push(&format!("{}.yaml", hints.id));
    let manifest = read(base.as_str())?;
    if !matches(&manifest) {
        return Err(Error::Source(
            "WinGet manifest identity/version mismatch; ignored".into(),
        ));
    }
    if manifest.release_notes_url.is_some() || manifest.package_url.is_some() {
        return Ok(manifest);
    }
    let locale = manifest
        .default_locale
        .as_deref()
        .filter(|s| {
            s.len() <= 35
                && !s.is_empty()
                && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        .ok_or_else(|| Error::Source("WinGet default locale missing/invalid".into()))?;
    base.path_segments_mut()
        .unwrap()
        .pop()
        .push(&format!("{}.locale.{locale}.yaml", hints.id));
    let localized = read(base.as_str())?;
    if !matches(&localized) {
        return Err(Error::Source(
            "WinGet locale identity/version mismatch; ignored".into(),
        ));
    }
    Ok(localized)
}
pub fn discover(fetch: &impl Fetcher, hints: &Hints) -> Discovery {
    let fetch = Budget {
        fetch,
        calls: Cell::new(0),
        started: Instant::now(),
    };
    let mut out = Discovery::default();
    let mut hints = hints.clone();
    hints.release_notes = hints.release_notes.as_deref().and_then(notes_url);
    if hints.release_notes.is_none()
        && let Some(url) = winget_manifest(&hints)
    {
        match winget_locale(&fetch, &hints, &url) {
            Ok(locale)
                if locale.package_identifier == hints.id
                    && hints.installed.as_deref() == Some(locale.package_version.as_str()) =>
            {
                hints.release_notes = locale.release_notes_url;
                if hints.homepage.is_none() {
                    hints.homepage = locale.package_url;
                }
                if hints.repository.is_none()
                    && hints.release_notes.is_none()
                    && hints.downloads.is_empty()
                    && hints.homepage.as_deref().and_then(github).is_none()
                {
                    let mut installer = Url::parse(&url).unwrap();
                    installer
                        .path_segments_mut()
                        .unwrap()
                        .pop()
                        .push(&format!("{}.installer.yaml", hints.id));
                    if let Ok(doc) = fetch.get(installer.as_str())
                        && let Ok(manifest) = serde_yaml_ng::from_str::<WingetInstaller>(&doc.body)
                        && manifest.package_identifier == hints.id
                        && hints.installed.as_deref() == Some(manifest.package_version.as_str())
                    {
                        hints.downloads = manifest
                            .installers
                            .into_iter()
                            .filter_map(|i| i.installer_url)
                            .take(8)
                            .collect();
                    }
                }
            }
            Ok(_) => out
                .attempts
                .push("WinGet manifest identity/version mismatch; ignored".into()),
            Err(e) => out.attempts.push(format!("WinGet manifest: {e}")),
        }
    }
    hints.release_notes = hints.release_notes.as_deref().and_then(notes_url);
    let hints = &hints;
    let mut candidates = vec![];
    for url in hints
        .references
        .iter()
        .take(3)
        .filter_map(|u| reference_document(u))
    {
        candidates.push(stream(document(&url, false), Basis::Version, hints));
        candidates.push(stream(document(&url, true), Basis::Date, hints));
    }
    if let Some(url) = hints.release_notes.as_deref().and_then(reference_document) {
        candidates.push(stream(document(&url, false), Basis::Version, hints));
        candidates.push(stream(document(&url, true), Basis::Date, hints));
    }
    // Download release identities have stronger component evidence than a project homepage.
    let mut scoped_repos = std::collections::HashSet::new();
    for url in hints
        .downloads
        .iter()
        .map(String::as_str)
        .chain(hints.release_notes.as_deref())
    {
        if let Some((o, r, Some(tag))) = github(url)
            && let Some((prefix, suffix)) = hints.installed.as_deref()
                .and_then(|installed| release_tag_parts(&tag, installed))
        {
            if !matches!(prefix, "" | "v" | "V") || !suffix.is_empty() {
                scoped_repos.insert((o.to_ascii_lowercase(), r.to_ascii_lowercase()));
            }
            candidates.push(stream(repo_source(&o, &r, Some((prefix, suffix))), Basis::Version, hints));
        }
    }
    // A declared raw GitHub download is repository evidence too, not only releases/download.
    let mut download_repos = vec![];
    for download in &hints.downloads {
        if let Some(url) = https(download)
            && url.host_str() == Some("raw.githubusercontent.com")
            && let Some(mut parts) = url.path_segments()
            && let (Some(owner), Some(repo)) = (parts.next(), parts.next())
        {
            download_repos.push(format!("https://github.com/{owner}/{repo}"));
        }
    }
    let mut repos = vec![];
    for url in [
        hints.repository.as_deref(),
        hints.homepage.as_deref(),
        hints.release_notes.as_deref(),
    ]
    .into_iter()
    .flatten()
    .chain(hints.references.iter().map(String::as_str))
    .chain(download_repos.iter().map(String::as_str))
    {
        if let Some((owner, repo, _)) = github(url)
            && !repos.contains(&(owner.clone(), repo.clone()))
        {
            repos.push((owner, repo));
        }
    }
    for (owner, repo) in repos.into_iter().take(2) {
        let Some(root) = repo_root(&owner, &repo, hints.directory.as_deref()) else {
            continue;
        };
        let node = matches!(hints.manager.as_str(), "npm" | "pnpm" | "bun");
        let verified = !node
            || fetch
                .get(&format!("{root}package.json"))
                .ok()
                .and_then(|d| serde_json::from_str::<serde_json::Value>(&d.body).ok())
                .is_some_and(|v| v["name"] == hints.id);
        if !verified {
            out.attempts.push(format!("{owner}/{repo}: repository package identity did not match; refusing repository-wide notes"));
            continue;
        }
        let root_scope = hints.directory.as_deref().is_none_or(|s| s.is_empty());
        if root_scope {
            if scoped_repos.contains(&(owner.to_ascii_lowercase(), repo.to_ascii_lowercase())) {
                continue;
            }
            candidates.push(stream(repo_source(&owner, &repo, None), Basis::Version, hints));
        }
        for name in ["CHANGELOG.md", "CHANGES.md", "NEWS.md"] {
            candidates.push(stream(
                document(&format!("{root}{name}"), false),
                Basis::Version,
                hints,
            ));
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut candidates = candidates.into_iter();
    while let Some(mut candidate) = candidates.next() {
        let key = serde_json::to_string(&candidate.sources).unwrap();
        if !seen.insert(key.clone()) {
            continue;
        }
        match usable(&fetch, &candidate, hints) {
            Ok(()) => {
                // Keep declared same-basis alternatives for subsequent reader failures.
                candidate.sources.extend(
                    candidates
                        .filter(|s| s.basis == candidate.basis)
                        .flat_map(|s| s.sources)
                        .take(3),
                );
                out.streams.push(candidate);
                return out;
            }
            Err(e) => out.attempts.push(format!("{key}: {e}")),
        }
        if fetch.calls.get() >= 9 {
            break;
        }
    }
    if let (Some(root), Some(version)) = (&hints.root, &hints.installed) {
        for name in ["CHANGELOG.md", "changelog.md", "CHANGES.md", "NEWS.md"] {
            let source = Source::InstalledDocument {
                root: root.clone(),
                filename: name.into(),
                package: hints.id.clone(),
                version: version.clone(),
            };
            let candidate = stream(source, Basis::Version, hints);
            if usable(&fetch, &candidate, hints).is_ok() {
                out.streams.push(candidate);
                return out;
            }
        }
    }
    if let Some(home) = hints
        .homepage
        .as_deref()
        .and_then(https)
        .filter(|u| github(u.as_str()).is_none())
    {
        // Some metadata already points directly at a changelog rather than a landing page.
        let page = fetch.get(home.as_str());
        if let Ok(page) = page {
            let home = Url::parse(&page.evidence.url)
                .ok()
                .filter(|u| crate::icons::safe_url(u.as_str()).is_some())
                .unwrap_or(home);
            let html = Html::parse_document(&page.body);
            let selector = Selector::parse("a[href],link[href]").unwrap();
            let mut links = vec![];
            for el in html.select(&selector) {
                let value = el.value();
                let text = el.text().collect::<String>().to_lowercase();
                let href = value.attr("href").unwrap_or("");
                let feed = value
                    .attr("rel")
                    .is_some_and(|s| s.split_whitespace().any(|v| v == "alternate"))
                    && value.attr("type").is_some_and(|t| {
                        matches!(t, "application/rss+xml" | "application/atom+xml")
                    });
                let explicit = text.contains("changelog")
                    || text.contains("release notes")
                    || matches!(
                        text.trim(),
                        "releases" | "release history" | "what's new" | "what’s new"
                    )
                    || href.to_lowercase().contains("changelog")
                    || href.to_lowercase().contains("release-notes");
                if !(feed || explicit) {
                    continue;
                }
                if let Ok(url) = home.join(href)
                    && url.origin() == home.origin()
                    && crate::icons::safe_url(url.as_str()).is_some()
                    && !links.contains(&(url.clone(), feed))
                {
                    links.push((url, feed));
                }
                if links.len() == 3 {
                    break;
                }
            }
            // Try explicitly declared links first, not guessed /changelog or repository URLs.
            for (url, feed) in links {
                let choices = if feed {
                    vec![stream(Source::Feed { url: url.into() }, Basis::Date, hints)]
                } else {
                    vec![
                        stream(document(url.as_str(), false), Basis::Version, hints),
                        stream(document(url.as_str(), true), Basis::Date, hints),
                    ]
                };
                for candidate in choices {
                    match usable(&fetch, &candidate, hints) {
                        Ok(()) => {
                            out.streams.push(candidate);
                            return out;
                        }
                        Err(e) => out.attempts.push(e.to_string()),
                    }
                }
            }
            let declared_notes = home.path().split('/').any(|part| {
                matches!(
                    part.to_ascii_lowercase().as_str(),
                    "changelog" | "release-notes" | "releases" | "whats-new" | "whatsnew"
                )
            }) || html.select(&Selector::parse("title,h1,h2").unwrap()).any(
                |el| {
                    let title = el.text().collect::<String>().to_lowercase();
                    title.contains("changelog")
                        || title.contains("release notes")
                        || title.contains("what's new")
                },
            );
            for dated in [false, true].into_iter().filter(|_| declared_notes) {
                let candidate = stream(
                    document(home.as_str(), dated),
                    if dated { Basis::Date } else { Basis::Version },
                    hints,
                );
                if usable(&fetch, &candidate, hints).is_ok() {
                    out.streams.push(candidate);
                    return out;
                }
            }
        } else if let Err(e) = page {
            out.attempts.push(e.to_string());
        }
    }
    if out.attempts.is_empty() {
        out.attempts
            .push("Installed metadata contained no usable release-note source hints.".into());
    }
    out
}
