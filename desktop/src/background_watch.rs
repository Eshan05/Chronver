//! Hourly, headless watch checks. The normal desktop launch never runs this worker.
use chrono::{DateTime, Utc};
use releasescout_engine::{
    Basis, Channel, Error, Result, Scheme, catalog, extension_updates,
    http::{CacheOptions, HttpClient},
    inventory::{Manager, ProcessRunner},
    matching,
    version::Version,
    watchlist,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_ITEMS: usize = 250;
const MAX_EVENTS: usize = 80;
const ON_DISK: u64 = 512 * 1024;
pub const CHRONVER_RELEASES: &str = "https://github.com/Eshan05/Chronver/releases";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    App {
        package: String,
        stream: String,
    },
    Extension {
        editor: Manager,
        id: String,
        profile: String,
    },
    Npm {
        name: String,
    },
}

impl Target {
    pub fn chronver() -> Self {
        Self::App {
            package: "chronver".into(),
            stream: "main".into(),
        }
    }

    pub fn is_chronver(&self) -> bool {
        matches!(self, Self::App { package, stream } if package == "chronver" && stream == "main")
    }

    pub fn key(&self) -> String {
        format!("{:x}", Sha256::digest(serde_json::to_vec(self).unwrap()))
    }
    fn event_label(&self) -> &'static str {
        match self {
            Self::Extension { .. } => "Update available",
            _ => "New release",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Follow {
    pub target: Target,
    pub title: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    pub enabled: bool,
    pub items: Vec<Follow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub item: Follow,
    pub version: String,
    pub found_at: DateTime<Utc>,
}
impl Event {
    pub fn label(&self) -> String {
        format!("{}: {}", self.item.target.event_label(), self.version)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Results {
    pub last_check: Option<DateTime<Utc>>,
    pub verified: usize,
    pub unverified: usize,
    pub recent: Vec<Event>,
    pub delivery_error: Option<String>,
    baselines: BTreeMap<String, String>,
}

pub fn directory() -> PathBuf {
    crate::live::cache_directory()
        .parent()
        .expect("cache has app data directory")
        .to_path_buf()
}

pub(crate) fn load<T: Default + DeserializeOwned>(file: &Path) -> Result<T> {
    let f = match File::open(file) {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(T::default()),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    f.take(ON_DISK + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > ON_DISK {
        return Err(Error::Config("Background Watch file is too large".into()));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub(crate) fn save<T: Serialize>(file: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() as u64 > ON_DISK {
        return Err(Error::Config("Background Watch file is too large".into()));
    }
    let parent = file
        .parent()
        .ok_or_else(|| Error::Config("Watch directory missing".into()))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(&bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(file).map_err(|e| e.error)?;
    Ok(())
}

pub fn settings(dir: &Path) -> Result<Settings> {
    let settings: Settings = load(&dir.join("background-watch.json"))?;
    if settings.items.len() > MAX_ITEMS
        || settings
            .items
            .iter()
            .any(|item| matches!(item.target, Target::Npm { .. }))
    {
        return Err(Error::Config("Invalid Background Watch settings".into()));
    }
    Ok(settings)
}

pub fn results(dir: &Path) -> Result<Results> {
    load(&dir.join("background-watch-results.json"))
}

impl Settings {
    pub fn store(&self, dir: &Path) -> Result<()> {
        if self.items.len() > MAX_ITEMS {
            return Err(Error::Config(
                "Background Watch is limited to 250 items".into(),
            ));
        }
        save(&dir.join("background-watch.json"), self)
    }
}

fn pending_items(dir: &Path, mut items: Vec<Follow>) -> Result<Vec<Follow>> {
    for entry in watchlist::load(&dir.join("watchlist.json"))? {
        if entry.ecosystem == watchlist::Ecosystem::Npm {
            items.push(Follow {
                title: entry.name.clone(),
                target: Target::Npm { name: entry.name },
            });
        }
    }
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.target.key()));
    if items.len() > MAX_ITEMS {
        return Err(Error::Config(
            "Background Watch is limited to 250 followed items".into(),
        ));
    }
    Ok(items)
}

impl Results {
    pub fn last_verified(&self, target: &Target) -> Option<&str> {
        self.baselines.get(&target.key()).map(String::as_str)
    }

    fn observe(
        &mut self,
        item: &Follow,
        version: &str,
        scheme: Scheme,
        reportable: bool,
        now: DateTime<Utc>,
    ) -> Option<Event> {
        let next = Version::parse(version, scheme).ok()?;
        let key = item.target.key();
        let Some(previous) = self
            .baselines
            .get(&key)
            .and_then(|v| Version::parse(v, scheme).ok())
        else {
            self.baselines.insert(key, version.to_owned());
            return None;
        };
        if !next.compare(&previous).is_gt() {
            return None;
        }
        self.baselines.insert(key, version.to_owned());
        reportable.then(|| Event {
            item: item.clone(),
            version: version.to_owned(),
            found_at: now,
        })
    }
}

fn app_release(
    engine: &releasescout_engine::Engine<HttpClient>,
    package: &str,
    stream: &str,
) -> Option<(String, Scheme)> {
    let profile = engine.catalog.packages.iter().find(|p| p.id == package)?;
    let stream = profile
        .streams
        .iter()
        .find(|s| s.id == stream && s.basis == Basis::Version && s.channel == Channel::Stable)?;
    let report = engine.version_snapshot(package, &stream.id).ok()?;
    if report.evidence.is_empty()
        || !report.evidence.iter().all(|e| {
            matches!(
                e.cache.as_str(),
                "network" | "revalidated_cache" | "fresh_cache"
            )
        })
        || report
            .warnings
            .iter()
            .any(|w| w.contains("official public Atom snapshot"))
    {
        return None;
    }
    let newest = report
        .notes
        .iter()
        .filter(|n| !n.preview)
        .filter_map(|n| {
            let text = n.version.as_deref()?;
            let version = Version::parse(text, stream.scheme).ok()?;
            (!version.preview()).then_some((version, text))
        })
        .max_by(|(a, _), (b, _)| a.compare(b))?
        .1;
    Some((newest.to_owned(), stream.scheme))
}

fn resolve(
    dir: &Path,
    items: &[Follow],
    deadline: Instant,
    refresh: bool,
) -> Result<BTreeMap<String, (String, Scheme, bool)>> {
    let cache = dir.join("cache");
    let public_registry = HttpClient::new(CacheOptions {
        directory: cache.clone(),
        public_only: true,
        refresh,
        ..Default::default()
    })?;
    let gallery = HttpClient::new(CacheOptions {
        directory: cache,
        refresh,
        github_token: std::env::var("GITHUB_TOKEN").ok(),
        ..Default::default()
    })?;
    let engine = releasescout_engine::Engine {
        catalog: catalog::bundled()?,
        fetcher: gallery,
    };
    let mut found = BTreeMap::new();
    let runner = ProcessRunner {
        timeout: Duration::from_secs(10),
        ..Default::default()
    };
    if items
        .iter()
        .any(|item| matches!(item.target, Target::Extension { .. }))
    {
        let scans = matching::load_snapshot(&dir.join("inventory.json")).unwrap_or_default();
        for editor in Manager::EDITORS {
            if Instant::now() >= deadline {
                break;
            }
            let Some(scan) = scans.iter().find(|scan| scan.manager == editor) else {
                continue;
            };
            let (keys, installs): (Vec<_>, Vec<_>) = items
                .iter()
                .filter_map(|item| {
                    let Target::Extension {
                        editor: manager,
                        id,
                        profile,
                    } = &item.target
                    else {
                        return None;
                    };
                    if *manager != editor {
                        return None;
                    }
                    let mut records = scan.packages.iter().filter(|p| {
                        p.extension.as_ref().is_some_and(|e| {
                            e.id.eq_ignore_ascii_case(id)
                                && e.profile_id.as_deref() == Some(profile)
                        })
                    });
                    let record = records.next()?;
                    if records.next().is_some() {
                        return None;
                    }
                    Some((
                        item.target.key(),
                        extension_updates::Installation {
                            id: id.clone(),
                            version: record.installed.clone()?,
                            profile: profile.clone(),
                            root: record.extension.as_ref()?.root.clone(),
                        },
                    ))
                })
                .unzip();
            if installs.is_empty() {
                continue;
            }
            if let Ok(statuses) =
                extension_updates::check(editor, &installs, &runner, &engine.fetcher)
            {
                for ((key, install), status) in keys.into_iter().zip(installs).zip(statuses) {
                    let (version, reportable) = match status {
                        extension_updates::Status::Available(version) => (version, true),
                        extension_updates::Status::NoNewerCompatibleRelease => {
                            (install.version, false)
                        }
                        extension_updates::Status::Unverified(_) => continue,
                    };
                    found.insert(key, (version, Scheme::Semver, reportable));
                }
            }
        }
    }
    for item in items {
        if Instant::now() >= deadline {
            break;
        }
        let version = match &item.target {
            Target::App { package, stream } => app_release(&engine, package, stream),
            Target::Npm { name } => watchlist::latest_stable(
                &public_registry,
                &watchlist::Entry {
                    name: name.clone(),
                    ecosystem: watchlist::Ecosystem::Npm,
                    group: Default::default(),
                },
            )
            .ok()
            .map(|v| (v, Scheme::Semver)),
            Target::Extension { .. } => continue,
        };
        if let Some((version, scheme)) = version {
            found.insert(item.target.key(), (version, scheme, true));
        }
    }
    Ok(found)
}

pub fn run(dir: &Path, manual: bool) -> Result<Results> {
    fs::create_dir_all(dir)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("background-watch.lock"))?;
    lock.try_lock()
        .map_err(|_| Error::Source("A background check is already running".into()))?;
    let cfg = settings(dir)?;
    let mut result = results(dir)?;
    if !manual && !cfg.enabled {
        return Ok(result);
    }
    let items = pending_items(dir, cfg.items)?;
    let now = Utc::now();
    let versions = if items.is_empty() {
        BTreeMap::new()
    } else {
        resolve(
            dir,
            &items,
            Instant::now() + Duration::from_secs(480),
            manual,
        )?
    };
    let active: BTreeSet<_> = items.iter().map(|item| item.target.key()).collect();
    result.baselines.retain(|key, _| active.contains(key));
    result
        .recent
        .retain(|event| active.contains(&event.item.target.key()));
    result.last_check = Some(now);
    result.verified = versions.len();
    result.unverified = items.len() - versions.len();
    let mut events: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let (version, scheme, reportable) = versions.get(&item.target.key())?;
            result.observe(item, version, *scheme, *reportable, now)
        })
        .collect();
    let new_count = events.len();
    let first = events.first().cloned();
    events.append(&mut result.recent);
    events.truncate(MAX_EVENTS);
    result.recent = events;
    let file = dir.join("background-watch-results.json");
    save(&file, &result)?;
    if cfg.enabled
        && settings(dir)?.enabled
        && let Some(event) = first
    {
        result.delivery_error = crate::background_windows::notify(&event, new_count)
            .err()
            .map(|e| e.to_string());
        save(&file, &result)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn baseline_and_rollbacks_do_not_alert() {
        let mut result = Results::default();
        let item = Follow {
            target: Target::Npm {
                name: "fixture".into(),
            },
            title: "Fixture".into(),
        };
        let mut observe = |v| result.observe(&item, v, Scheme::Semver, true, Utc::now());
        assert!(observe("2.0.0").is_none());
        assert!(observe("1.0.0").is_none());
        assert_eq!(observe("2.1.0").unwrap().version, "2.1.0");
        assert!(observe("2.1.0").is_none());
        assert!(observe("2.0.0").is_none());
        assert!(observe("2.1.0").is_none());
    }
    #[test]
    fn a_new_installed_version_is_not_a_new_update() {
        let mut result = Results::default();
        let item = Follow {
            target: Target::Extension {
                editor: Manager::Vscode,
                id: "test.example".into(),
                profile: "default".into(),
            },
            title: "Fixture".into(),
        };
        let mut observe =
            |v, reportable| result.observe(&item, v, Scheme::Semver, reportable, Utc::now());
        assert!(observe("1.0.0", false).is_none());
        assert!(observe("1.1.0", false).is_none());
        assert!(observe("1.1.0", true).is_none());
        assert!(observe("1.2.0", true).is_some());
    }
    #[test]
    fn disabled_worker_does_not_read_inventory_or_registries() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("watchlist.json"), "invalid").unwrap();
        assert!(run(tmp.path(), false).unwrap().last_check.is_none());
        assert!(!tmp.path().join("background-watch-results.json").exists());
        assert!(run(tmp.path(), true).is_err());
    }
}
