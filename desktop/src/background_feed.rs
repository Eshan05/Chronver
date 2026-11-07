use crate::background_watch::{self, Follow, Target};
use chrono::{DateTime, Utc};
use releasescout_engine::{
    Basis, Catalog, Channel, Engine, Package, Report,
    http::{CacheOptions, HttpClient},
    watchlist::{self, Ecosystem, Entry, PublishedRelease},
};
use rune::{Latest, Waker};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashSet},
};

const PAGE_SIZE: usize = 18;
const FRESH_SECONDS: i64 = 60 * 60;

#[derive(Clone, Serialize, Deserialize)]
struct PackageHistory {
    checked_at: DateTime<Utc>,
    description: Option<String>,
    releases: Vec<FeedRelease>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FeedRelease {
    pub version: String,
    pub published_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

impl From<PublishedRelease> for FeedRelease {
    fn from(release: PublishedRelease) -> Self {
        Self {
            version: release.version,
            published_at: Some(release.published_at),
            excerpt: None,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Subject<'a> {
    Npm(&'a Entry),
    App(&'a Follow),
}

impl<'a> Subject<'a> {
    pub fn key(self) -> Cow<'a, str> {
        match self {
            Self::Npm(entry) => Cow::Borrowed(&entry.name),
            Self::App(follow) => Cow::Owned(format!("app:{}", follow.target.key())),
        }
    }

    pub fn title(self) -> &'a str {
        match self {
            Self::Npm(entry) => &entry.name,
            Self::App(follow) => &follow.title,
        }
    }

    fn job(self, catalog: &Catalog) -> Option<Job> {
        match self {
            Self::Npm(entry) => Some(Job::Npm(entry.clone())),
            Self::App(Follow {
                target: Target::App { package, stream },
                ..
            }) => {
                let profile = catalog.packages.iter().find(|p| p.id == *package)?;
                profile.streams.iter().find(|s| {
                    s.id == *stream && s.basis == Basis::Version && s.channel == Channel::Stable
                })?;
                Some(Job::App(profile.clone(), stream.clone()))
            }
            Self::App(_) => None,
        }
    }
}

fn subjects<'a>(watched: &'a [Entry], follows: &'a [Follow]) -> impl Iterator<Item = Subject<'a>> {
    follows
        .iter()
        .filter(|f| matches!(f.target, Target::App { .. }))
        .map(Subject::App)
        .chain(
            watched
                .iter()
                .filter(|e| e.ecosystem == Ecosystem::Npm)
                .map(Subject::Npm),
        )
}

enum Job {
    Npm(Entry),
    App(Package, String),
}

impl Job {
    fn load(self, refresh: bool) -> Option<PackageHistory> {
        let (description, releases) = match self {
            Self::Npm(entry) => {
                let history = watchlist::npm_release_history(&entry).ok()?;
                (
                    history.description,
                    history.releases.into_iter().map(Into::into).collect(),
                )
            }
            Self::App(package, stream) => {
                let id = package.id.clone();
                let fetcher = HttpClient::new(CacheOptions {
                    directory: crate::live::cache_directory(),
                    public_only: true,
                    refresh,
                    github_token: std::env::var("GITHUB_TOKEN").ok(),
                    ..Default::default()
                })
                .ok()?;
                let engine = Engine {
                    catalog: Catalog {
                        schema_version: 1,
                        packages: vec![package],
                    },
                    fetcher,
                };
                let report = engine.history(&id, Some(&stream), None, false).ok()?.report;
                if report.evidence.is_empty()
                    || report
                        .evidence
                        .iter()
                        .all(|e| e.cache.starts_with("stale_cache"))
                {
                    return None;
                }
                (None, app_releases(report))
            }
        };
        Some(PackageHistory {
            checked_at: Utc::now(),
            description,
            releases,
        })
    }
}

fn app_releases(report: Report) -> Vec<FeedRelease> {
    let mut seen = HashSet::new();
    report
        .notes
        .into_iter()
        .filter_map(|note| {
            let version = note.version?;
            if note.preview || !seen.insert(version.clone()) {
                return None;
            }
            let excerpt = (seen.len() == 1 && note.authored_notes)
                .then(|| note_excerpt(&note.body))
                .filter(|excerpt| !excerpt.is_empty());
            Some(FeedRelease {
                version,
                published_at: note
                    .date
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
                    .map(|date| date.and_utc()),
                excerpt,
            })
        })
        .take(5)
        .collect()
}

pub fn note_excerpt(body: &str) -> String {
    let bounded = body
        .char_indices()
        .nth(1500)
        .map_or(body.len(), |(offset, _)| offset);
    crate::description_preview::plain(&body[..bounded])
        .chars()
        .take(400)
        .collect()
}

impl PackageHistory {
    fn fresh(&self, now: DateTime<Utc>) -> bool {
        (0..FRESH_SECONDS).contains(&(now - self.checked_at).num_seconds())
    }
}

type Cache = BTreeMap<String, PackageHistory>;
type Outcome = (String, Option<PackageHistory>);

#[derive(Default)]
pub struct Feed {
    cache: Option<Cache>,
    read: Latest<Cache>,
    write: Latest<()>,
    workers: [Latest<Outcome>; 2],
    attempted: HashSet<String>,
    failed: HashSet<String>,
    dirty: bool,
    refresh_sources: bool,
    shown: usize,
}

fn cache_file() -> std::path::PathBuf {
    background_watch::directory().join("background-watch-feed.json")
}

impl Feed {
    pub fn set_waker(&self, waker: Waker) {
        self.read.set_waker(waker.clone());
        self.write.set_waker(waker.clone());
        for worker in &self.workers {
            worker.set_waker(waker.clone());
        }
    }

    pub fn poll(
        &mut self,
        active: bool,
        watched: &[Entry],
        follows: &[Follow],
        catalog: &Catalog,
    ) -> bool {
        let mut changed = false;
        if let Some(cache) = self.read.take() {
            self.cache = Some(cache);
            changed = true;
        }
        if self.cache.is_none() {
            if active && !self.read.is_pending() && subjects(watched, follows).next().is_some() {
                self.read.request((), |_| {
                    background_watch::load(&cache_file()).unwrap_or_default()
                });
            }
            return changed;
        }
        let cache = self.cache.as_mut().unwrap();
        if !active && !self.workers.iter().any(Latest::is_pending) {
            self.attempted.clear();
            self.refresh_sources = false;
        }
        for worker in &self.workers {
            if let Some((name, result)) = worker.take() {
                if let Some(history) = result {
                    self.failed.remove(&name);
                    cache.insert(name, history);
                    self.dirty = true;
                } else {
                    self.failed.insert(name);
                }
                changed = true;
            }
            if active && !worker.is_pending() {
                let now = Utc::now();
                let next = subjects(watched, follows).find_map(|subject| {
                    let key = subject.key();
                    if self.attempted.contains(key.as_ref())
                        || cache.get(key.as_ref()).is_some_and(|h| h.fresh(now))
                    {
                        return None;
                    }
                    Some((key.into_owned(), subject.job(catalog)))
                });
                if let Some((key, job)) = next {
                    self.attempted.insert(key.clone());
                    let refresh = self.refresh_sources;
                    worker.request((key, job), move |(key, job)| {
                        (key, job.and_then(|job| job.load(refresh)))
                    });
                    changed = true;
                }
            }
        }
        if self.dirty && !self.write.is_pending() && !self.workers.iter().any(Latest::is_pending) {
            self.dirty = false;
            let keep: HashSet<_> = subjects(watched, follows).map(Subject::key).collect();
            cache.retain(|key, _| keep.contains(key.as_str()));
            self.write.request(cache.clone(), |cache| {
                let _ = background_watch::save(&cache_file(), &cache);
            });
        }
        changed
    }

    pub fn updating(&self) -> bool {
        self.read.is_pending() || self.workers.iter().any(Latest::is_pending)
    }

    pub fn unavailable(&self, watched: &[Entry], follows: &[Follow]) -> usize {
        subjects(watched, follows)
            .filter(|subject| self.failed.contains(subject.key().as_ref()))
            .count()
    }

    pub fn refresh(&mut self) {
        if let Some(cache) = &mut self.cache {
            for history in cache.values_mut() {
                history.checked_at = DateTime::UNIX_EPOCH;
            }
        }
        self.attempted.clear();
        self.failed.clear();
        self.refresh_sources = true;
    }

    pub fn releases_for(&self, name: &str) -> Option<Vec<PublishedRelease>> {
        let history = self.cache.as_ref()?.get(name)?;
        history.fresh(Utc::now()).then(|| {
            history
                .releases
                .iter()
                .filter_map(|release| {
                    Some(PublishedRelease {
                        version: release.version.clone(),
                        published_at: release.published_at?,
                    })
                })
                .collect()
        })
    }

    pub fn description(&self, name: &str) -> Option<&str> {
        self.cache.as_ref()?.get(name)?.description.as_deref()
    }

    pub fn limit(&self) -> usize {
        self.shown.max(PAGE_SIZE)
    }

    pub fn show_more(&mut self) {
        self.shown = self.limit() + PAGE_SIZE;
    }

    pub fn cards<'a>(
        &'a self,
        watched: &'a [Entry],
        follows: &'a [Follow],
    ) -> Vec<(Subject<'a>, &'a [FeedRelease])> {
        let Some(cache) = &self.cache else {
            return Vec::new();
        };
        let mut result = Vec::new();
        for subject in subjects(watched, follows) {
            if let Some(history) = cache.get(subject.key().as_ref()) {
                let releases = &history.releases;
                if !releases.is_empty() {
                    result.push((subject, &releases[..releases.len().min(5)]));
                }
            }
        }
        result.sort_unstable_by(|(a, releases_a), (b, releases_b)| {
            releases_b[0]
                .published_at
                .cmp(&releases_a[0].published_at)
                .then_with(|| a.title().cmp(b.title()))
                .then_with(|| a.key().cmp(&b.key()))
        });
        // Multiple followed components of an app are different version streams.
        // Feature the most recent one without mixing their release histories.
        let mut apps = HashSet::new();
        result.retain(|(subject, _)| match subject {
            Subject::App(Follow {
                target: Target::App { package, .. },
                ..
            }) => apps.insert(package.as_str()),
            _ => true,
        });
        result
    }

    #[cfg(test)]
    pub fn fixture(&mut self, name: &str, description: &str, releases: Vec<PublishedRelease>) {
        self.cache.get_or_insert_with(Cache::new).insert(
            name.into(),
            PackageHistory {
                checked_at: Utc::now(),
                description: Some(description.into()),
                releases: releases.into_iter().map(Into::into).collect(),
            },
        );
    }

    #[cfg(test)]
    pub fn fixture_app(&mut self, follow: &Follow, releases: Vec<FeedRelease>) {
        self.cache.get_or_insert_with(Cache::new).insert(
            Subject::App(follow).key().into_owned(),
            PackageHistory {
                checked_at: Utc::now(),
                description: None,
                releases,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_npm_cache_reads_without_migration() {
        let cache: Cache = serde_json::from_str(r#"{"react":{"checked_at":"2026-10-09T01:00:00Z","description":"UI","releases":[{"version":"19.0.1","published_at":"2026-10-09T01:00:00Z"}]}}"#).unwrap();
        let cached = &cache["react"];
        assert_eq!(cached.description.as_deref(), Some("UI"));
        assert!(cached.releases[0].published_at.is_some());
        assert!(cached.releases[0].excerpt.is_none());
    }

    #[test]
    fn software_history_keeps_unknown_dates_and_does_not_merge_components() {
        let mut feed = Feed::default();
        let follows = ["main", "compat"].map(|stream| Follow {
            target: Target::App {
                package: "app".into(),
                stream: stream.into(),
            },
            title: "App".into(),
        });
        let release = |version: &str, date: Option<&str>| FeedRelease {
            version: version.into(),
            published_at: date.map(|d| d.parse().unwrap()),
            excerpt: None,
        };
        feed.fixture_app(
            &follows[0],
            vec![
                release("2.0", Some("2026-11-21T00:00:00Z")),
                release("1.9", None),
            ],
        );
        feed.fixture_app(
            &follows[1],
            vec![release("10.0", Some("2026-11-20T00:00:00Z"))],
        );
        let cards = feed.cards(&[], &follows);
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0]
                .1
                .iter()
                .map(|r| r.version.as_str())
                .collect::<Vec<_>>(),
            ["2.0", "1.9"]
        );
        assert!(cards[0].1[1].published_at.is_none());
        assert!(
            feed.cards(&[], &follows[1..])
                .iter()
                .all(|(_, list)| list[0].version == "10.0")
        );
        assert!(feed.cards(&[], &[]).is_empty());
    }
}
