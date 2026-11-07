//! Personal public-package subscriptions, independent of projects or installations.
use crate::{http::Fetcher, source_discovery::Hints, *};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Ecosystem {
    Npm,
    Cargo,
}
impl Ecosystem {
    pub fn id(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Cargo => "cargo",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    #[default]
    Core,
    Tooling,
}
impl Group {
    pub fn id(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Tooling => "tooling",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub ecosystem: Ecosystem,
    pub name: String,
    #[serde(default)]
    pub group: Group,
}
impl Entry {
    pub fn validate(&self) -> Result<()> {
        let valid = match self.ecosystem {
            Ecosystem::Npm => dependencies::package_name(&self.name),
            Ecosystem::Cargo => {
                !self.name.is_empty()
                    && self.name.len() <= 64
                    && self.name.as_bytes()[0].is_ascii_alphabetic()
                    && self
                        .name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(Error::Query(
                "Enter a package name, without a URL or version range".into(),
            ))
        }
    }
    pub fn id(&self) -> String {
        format!(
            "watch-{}-{:x}",
            self.ecosystem.id(),
            Sha256::digest(self.name.as_bytes())
        )
    }
    pub fn url(&self) -> String {
        match self.ecosystem {
            Ecosystem::Npm => format!("https://www.npmjs.com/package/{}", self.name),
            Ecosystem::Cargo => format!("https://crates.io/crates/{}", self.name),
        }
    }
}

fn validate(entries: &[Entry]) -> Result<()> {
    if entries.len() > 1000 {
        return Err(Error::Config("watchlist exceeds 1,000 entries".into()));
    }
    let mut ids = HashSet::new();
    for entry in entries {
        entry.validate()?;
        if !ids.insert(entry.id()) {
            return Err(Error::Config("duplicate watchlist package".into()));
        }
    }
    Ok(())
}
pub fn load(path: &Path) -> Result<Vec<Entry>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = vec![];
    file.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 256 * 1024 {
        return Err(Error::Config("watchlist file exceeds 256 KiB".into()));
    }
    let entries: Vec<Entry> = serde_json::from_slice(&bytes)?;
    validate(&entries)?;
    Ok(entries)
}
pub fn save(path: &Path, entries: &[Entry]) -> Result<()> {
    validate(entries)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::Config("watchlist parent missing".into()))?;
    let bytes = serde_json::to_vec_pretty(entries)?;
    if bytes.len() > 256 * 1024 {
        return Err(Error::Config("watchlist file exceeds 256 KiB".into()));
    }
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn registry_package(entry: &Entry) -> Result<url::Url> {
    entry.validate()?;
    let mut registry = url::Url::parse(match entry.ecosystem {
        Ecosystem::Npm => "https://registry.npmjs.org/",
        Ecosystem::Cargo => "https://crates.io/api/v1/crates/",
    })
    .unwrap();
    registry
        .path_segments_mut()
        .unwrap()
        .pop_if_empty()
        .push(&entry.name);
    Ok(registry)
}

pub(crate) fn npm_registry_url(name: &str) -> Result<url::Url> {
    registry_package(&Entry { ecosystem: Ecosystem::Npm, name: name.into(), group: Group::Core })
}

fn publication(fetch: &impl Fetcher, entry: &Entry) -> Result<(crate::http::Document, Value, String)> {
    let mut registry = registry_package(entry)?;
    if entry.ecosystem == Ecosystem::Npm {
        registry.path_segments_mut().unwrap().push("latest");
    }
    let doc = fetch.get(registry.as_str())?;
    let value: Value = serde_json::from_str(&doc.body)?;
    let data = if entry.ecosystem == Ecosystem::Cargo {
        &value["crate"]
    } else {
        &value
    };
    if data[if entry.ecosystem == Ecosystem::Cargo {
        "id"
    } else {
        "name"
    }] != entry.name
    {
        return Err(Error::Source("registry package identity mismatch".into()));
    }
    let latest = data[if entry.ecosystem == Ecosystem::Cargo {
        "max_stable_version"
    } else {
        "version"
    }]
    .as_str()
    .filter(|v| semver::Version::parse(v).is_ok())
    .ok_or_else(|| Error::Source("registry published version missing or invalid".into()))?;
    let latest = latest.to_owned();
    Ok((doc, value, latest))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishedRelease {
    pub version: String,
    pub published_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
pub(crate) struct NpmMetadata {
    name: String,
    description: Option<String>,
    versions: std::collections::BTreeMap<String, serde::de::IgnoredAny>,
    time: std::collections::BTreeMap<String, Value>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct NpmDates {
    pub name: String,
    pub dates: std::collections::BTreeMap<String, chrono::NaiveDate>,
}

impl NpmMetadata {
    pub(crate) fn into_dates(self) -> NpmDates {
        let dates = self.time.into_iter().filter_map(|(version, published)| {
            if !self.versions.contains_key(&version) || semver::Version::parse(&version).is_err() {
                return None;
            }
            let published = chrono::DateTime::parse_from_rfc3339(published.as_str()?).ok()?;
            Some((version, published.to_utc().date_naive()))
        }).collect();
        NpmDates { name: self.name, dates }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NpmReleaseHistory {
    pub description: Option<String>,
    pub releases: Vec<PublishedRelease>,
}

fn stable_history(name: &str, metadata: NpmMetadata) -> Result<Vec<PublishedRelease>> {
    if metadata.name != name {
        return Err(Error::Source("registry package identity mismatch".into()));
    }
    let mut releases: Vec<_> = metadata
        .time
        .into_iter()
        .filter_map(|(version, time)| {
            let parsed = semver::Version::parse(&version).ok()?;
            if !parsed.pre.is_empty() || !metadata.versions.contains_key(&version) {
                return None;
            }
            Some(PublishedRelease {
                version,
                published_at: chrono::DateTime::parse_from_rfc3339(time.as_str()?)
                    .ok()?
                    .to_utc(),
            })
        })
        .collect();
    releases.sort_unstable_by(|a, b| {
        b.published_at
            .cmp(&a.published_at)
            .then_with(|| b.version.cmp(&a.version))
    });
    releases.truncate(5);
    Ok(releases)
}

pub fn recent_npm_releases(entry: &Entry) -> Result<Vec<PublishedRelease>> {
    Ok(npm_release_history(entry)?.releases)
}

pub fn npm_release_history(entry: &Entry) -> Result<NpmReleaseHistory> {
    if entry.ecosystem != Ecosystem::Npm {
        return Err(Error::Query(
            "Publication history requires an npm package".into(),
        ));
    }
    let mut metadata: NpmMetadata = crate::http::Network::npm_metadata(registry_package(entry)?)?;
    let description = metadata.description.take()
        .map(|description| description.trim().chars().take(400).collect::<String>())
        .filter(|description| !description.is_empty());
    let releases = stable_history(&entry.name, metadata)?;
    Ok(NpmReleaseHistory { description, releases })
}

#[cfg(test)]
mod recent_release_tests {
    use super::*;

    #[test]
    fn latest_five_follow_publication_time_not_version_order() {
        let metadata = serde_json::from_value(serde_json::json!({
            "name": "@scope/fixture",
            "versions": {
                "1.0.0": {}, "1.0.1": {}, "2.0.0": {}, "3.0.0": {},
                "4.0.0": {}, "5.0.0": {}, "6.0.0-beta.0": {}, "7.0.0": {}
            },
            "time": {
                "created": "2026-01-01T00:00:00Z",
                "unpublished": {"versions": ["9.0.0"]},
                "1.0.0": "2026-01-01T00:00:00Z", "1.0.1": "2026-01-06T00:00:00Z",
                "2.0.0": "2026-01-02T00:00:00Z", "3.0.0": "2026-01-03T00:00:00Z",
                "4.0.0": "2026-01-04T00:00:00Z", "5.0.0": "2026-01-05T00:00:00Z",
                "6.0.0-beta.0": "2026-01-07T00:00:00Z", "7.0.0": "not a date",
                "9.0.0": "2026-01-08T00:00:00Z"
            }
        })).unwrap();
        let releases = stable_history("@scope/fixture", metadata).unwrap();
        assert_eq!(
            releases.iter().map(|r| r.version.as_str()).collect::<Vec<_>>(),
            ["1.0.1", "5.0.0", "4.0.0", "3.0.0", "2.0.0"]
        );
    }

    #[test]
    fn history_rejects_a_different_package() {
        let metadata = serde_json::from_str(r#"{"name":"different","versions":{},"time":{}}"#).unwrap();
        assert!(stable_history("fixture", metadata).is_err());
    }

    #[test]
    #[ignore = "uses the public npm registry"]
    fn live_npm_history_includes_large_and_scoped_packages() {
        for name in ["next", "react", "@types/node"] {
            let history = npm_release_history(&Entry {
                name: name.to_owned(),
                ecosystem: Ecosystem::Npm,
                group: Group::Core,
            }).unwrap_or_else(|error| panic!("{name}: {error}"));
            if name == "react" {
                assert!(history.description.as_ref().is_some_and(|description| !description.is_empty()));
            }
            let releases = history.releases;
            assert_eq!(releases.len(), 5, "{name}");
            assert!(releases.windows(2).all(|w| w[0].published_at >= w[1].published_at));
        }
    }
}

pub fn latest_stable(fetch: &impl Fetcher, entry: &Entry) -> Result<String> {
    let (doc, _, version) = publication(fetch, entry)?;
    if !matches!(doc.evidence.cache.as_str(), "network" | "revalidated_cache" | "fresh_cache")
        || !semver::Version::parse(&version).is_ok_and(|v| v.pre.is_empty())
    {
        return Err(Error::Source("Fresh stable registry version unavailable".into()));
    }
    Ok(version)
}

pub fn notes(fetch: impl Fetcher, entry: &Entry, channel: Channel) -> Result<Report> {
    let (doc, value, latest) = publication(&fetch, entry)?;
    let data = if entry.ecosystem == Ecosystem::Cargo { &value["crate"] } else { &value };
    let latest = latest.as_str();
    let preview = !semver::Version::parse(latest).unwrap().pre.is_empty();
    let key = if entry.ecosystem == Ecosystem::Cargo {
        format!("cargo:{}", entry.name)
    } else {
        entry.name.clone()
    };
    let mut attempts = vec![];
    let mut profile = dependencies::known_package(&key)?;
    if profile.is_none() && entry.ecosystem == Ecosystem::Npm {
        let hints = Hints {
            manager: "npm".into(),
            source: "public package watchlist".into(),
            id: entry.name.clone(),
            installed: None,
            repository: data["repository"]
                .as_str()
                .or_else(|| data["repository"]["url"].as_str())
                .map(String::from),
            directory: data["repository"]["directory"].as_str().map(String::from),
            homepage: data["homepage"].as_str().map(String::from),
            release_notes: None,
            downloads: vec![],
            root: None,
            references: vec![],
        };
        let found = source_discovery::discover(&fetch, &hints);
        attempts = found.attempts;
        if !found.streams.is_empty() {
            profile = Some(Package {
                id: key,
                name: entry.name.clone(),
                tracking: "track".into(),
                identity: "metadata_discovered".into(),
                alias_of: None,
                caveats: String::new(),
                reference_urls: vec![],
                streams: found.streams,
            });
        }
    }
    let fetched = profile.map(|profile| dependencies::package_history(fetch, profile, channel));
    let mut report = match fetched {
        Some(Ok(report)) if !report.notes.is_empty() => report,
        result => {
            if let Some(Err(error)) = result {
                attempts.push(error.to_string());
            } else if let Some(Ok(report)) = result {
                attempts.extend(report.warnings);
            }
            Report {package:entry.id(),stream:"published".into(),scope:String::new(),basis:Basis::Version,status:"no_authored_notes".into(),complete:false,
                notes:if channel!=Channel::All && preview!=(channel==Channel::Preview) {vec![]} else {vec![Note{title:format!("Published {latest}"),version:Some(latest.into()),date:None,url:entry.url(),body:String::new(),authored_notes:false,preview}]},
                warnings:vec!["Registry publication metadata is available, but no authored changelog was retrieved. This is not an installed-version update candidate.".into()],evidence:vec![],attempts:vec![]}
        }
    };
    report.package = entry.id();
    report.scope = format!(
        "{} / {} / public release history",
        entry.ecosystem.id(),
        entry.name
    );
    report.complete = false;
    report.evidence.push(doc.evidence);
    report.warnings.extend(attempts.into_iter().take(12));
    report
        .warnings
        .push("Personal package watchlist: no project or installed version was inferred.".into());
    Ok(report)
}
