use crate::{
    activity,
    model::{Package, PackageRow, State},
    provider_badge,
};
use chrono::{NaiveDate, Utc};
use releasescout_engine::{
    Scheme,
    inventory::{InstalledPackage, Manager, Scan},
    version::Version,
};
use std::collections::BTreeSet;

pub static UPDATE: rune::Icon = rune::Icon {
    name: "status-update",
    view: 24.0,
    body: include_str!("../assets/status/circle-arrow-up.svg"),
};
pub static DOWNLOAD: rune::Icon = rune::Icon {
    name: "status-download",
    view: 24.,
    body: include_str!("../assets/status/download.svg"),
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UpdateSize {
    Patch,
    Minor,
    Major,
}

impl UpdateSize {
    pub fn between(installed: &str, candidate: &str) -> Option<Self> {
        let Version::Semver(from) = Version::parse(installed, Scheme::Semver).ok()? else {
            return None;
        };
        let Version::Semver(to) = Version::parse(candidate, Scheme::Semver).ok()? else {
            return None;
        };
        if to <= from {
            return None;
        }
        if to.major > from.major {
            Some(Self::Major)
        } else if to.minor > from.minor {
            Some(Self::Minor)
        } else if to.patch > from.patch {
            Some(Self::Patch)
        } else {
            None
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Patch => "Patch update",
            Self::Minor => "Minor update",
            Self::Major => "Major update",
        }
    }

    pub fn color(self, dark: bool) -> rune::Color {
        let (dark_hex, light_hex) = match self {
            Self::Patch => ("#4ade80", "#15803d"),
            Self::Minor => ("#facc15", "#b45309"),
            Self::Major => ("#fb7185", "#be123c"),
        };
        rune::color::hex(if dark { dark_hex } else { light_hex }).unwrap()
    }
}

#[derive(Clone)]
pub struct Observation {
    pub check_key: String,
    pub id: String,
    pub source: String,
    pub scope: String,
    pub root: Option<String>,
    pub checked_at: String,
    pub requested: bool,
    pub succeeded: bool,
    pub confirmed_update: bool,
    pub detail: Option<String>,
    pub registration_id: Option<String>,
}

pub fn check_key(p: &InstalledPackage) -> String {
    activity::hash((p.manager.id(), &p.id, &p.source, &p.scope, &p.root))
}

fn newer(from: &str, to: &str) -> bool {
    releasescout_engine::version::candidate_order(from, to).is_some_and(|order| order.is_gt())
}

fn candidates_supported(manager: Option<Manager>) -> bool {
    !manager.is_some_and(|m| m.is_editor() || matches!(m, Manager::Local | Manager::Store))
}

impl Observation {
    pub fn new(p: &InstalledPackage, scan: &Scan) -> Self {
        Self {
            check_key: check_key(p),
            id: p.id.clone(),
            source: p.source.clone(),
            scope: p.scope.clone(),
            root: p.root.clone(),
            checked_at: scan.checked_at.clone(),
            requested: scan.update_check_requested,
            succeeded: p
                .candidate_check_succeeded
                .unwrap_or(scan.update_check_succeeded),
            confirmed_update: scan.update_check_requested
                && p.candidate_check_succeeded
                    .unwrap_or(scan.update_check_succeeded)
                && candidates_supported(Some(p.manager))
                && p.installed
                    .as_deref()
                    .zip(p.available.as_deref())
                    .is_some_and(|(from, to)| newer(from, to)),
            detail: p.candidate_note.clone(),
            registration_id: p.evidence.registration_id.clone(),
        }
    }
    pub(crate) fn candidate_status(&self, p: &Package) -> &str {
        if self.confirmed_update {
            "Provider-confirmed update available"
        } else if !candidates_supported(p.provider) {
            "Candidate checks not supported by this provider"
        } else if !self.requested {
            "Candidates not checked"
        } else if !self.succeeded {
            "Candidate check failed or incomplete"
        } else if p.target.is_some() {
            "Provider candidate available; newer version not confirmed"
        } else {
            "Checked; no confirmed newer candidate"
        }
    }
}

pub fn age(date: NaiveDate, today: NaiveDate) -> Option<String> {
    match (today - date).num_days() {
        d if d < 0 => None,
        0 => Some("Today".into()),
        d @ 1..=29 => Some(format!("{d}d ago")),
        d @ 30..=364 => Some(format!("{}mo ago", d / 30)),
        d => Some(format!("{}y ago", d / 365)),
    }
}

pub struct Summary {
    pub update: bool,
    pub unread: usize,
    pub age: Option<String>,
    pub failed: bool,
    pub tooltip: String,
}

pub fn clean_description(value: Option<&str>) -> Option<String> {
    let bounded: String = value?
        .chars()
        .take(4096)
        .filter(|c| !c.is_control() || c.is_whitespace())
        .collect();
    let text = bounded
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned();
    if text.is_empty() || (text.starts_with('%') && text.ends_with('%') && !text.contains(' ')) {
        None
    } else {
        Some(text)
    }
}

pub fn description<'a>(s: &'a State, row: &PackageRow) -> Option<&'a str> {
    if let Some(value) = std::iter::once(row.package)
        .chain(row.members.iter().copied().filter(|&i| i != row.package))
        .find_map(|i| {
            s.activity
                .data
                .description_overrides
                .get(&crate::custom_icons::identity(&s.catalog.packages[i]))
        })
    {
        return Some(value);
    }
    let provider = std::iter::once(row.package)
        .chain(row.members.iter().copied().filter(|&i| i != row.package))
        .filter_map(|i| s.catalog.packages[i].description.as_deref())
        .find(|value| !value.trim().is_empty() && !value.eq_ignore_ascii_case(&row.name));
    provider.or_else(|| {
        std::iter::once(row.package)
            .chain(row.members.iter().copied())
            .find_map(|i| {
                s.catalog.packages[i]
                    .description_fallback
                    .map(|entry| entry.description.as_str())
            })
    })
}

pub fn description_scene(s: &mut State) {
    let data = serde_json::json!({
        "installed":{"apps":[
            {"Name":"gh","Version":"2.0.0","Source":"main"},
            {"Name":"uv","Version":"0.13.5","Source":"main"},
            {"Name":"fixture-no-description","Version":"10.0.0","Source":"main"}
        ]},
        "metadata":{
            "gh":{"description":"GitHub's command-line interface.","hasCommands":true},
            "uv":{"description":"A Python package and project manager that manages dependencies, virtual environments, Python versions and project tools from one command-line interface. This deliberately long fixture exercises the two-line description limit.","hasCommands":true},
            "fixture-no-description":{"description":"  \n  ","hasCommands":true}
        }
    });
    s.accept_scan(
        releasescout_engine::inventory::parse(Manager::Scoop, false, &data.to_string()).unwrap(),
    );
}

fn concise(value: &str) -> String {
    let mut result: String = value.chars().take(180).collect();
    if value.chars().count() > 180 {
        result.push_str("...");
    }
    result
}

pub fn summary(s: &State, row: &PackageRow, today: NaiveDate) -> Summary {
    let mut lines = vec![row.name.clone()];
    let mut unread = BTreeSet::new();
    let mut latest = None;
    let mut update = false;
    let mut failed = false;
    let providers = provider_badge::unique(
        row.members
            .iter()
            .filter_map(|&i| s.catalog.packages[i].provider),
    );
    if !providers.is_empty() {
        lines.push(format!(
            "Providers: {}",
            providers
                .iter()
                .map(|m| provider_badge::label(*m))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let scopes: BTreeSet<_> = row
        .members
        .iter()
        .filter_map(|&i| {
            s.catalog.packages[i]
                .observation
                .as_ref()
                .map(|o| o.scope.as_str())
        })
        .collect();
    if !scopes.is_empty() {
        lines.push(format!(
            "Scopes: {}",
            scopes.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    let versions: BTreeSet<_> = row
        .members
        .iter()
        .map(|&i| {
            s.catalog.packages[i]
                .installed
                .as_deref()
                .unwrap_or("Unknown")
        })
        .collect();
    if versions.len() > 1 {
        lines.push(format!(
            "Observed versions: {}",
            versions.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    for (at, &i) in row.members.iter().enumerate() {
        let start = lines.len();
        let p = &s.catalog.packages[i];
        if let Some(observation) = &p.observation {
            failed |= observation.requested
                && !observation.succeeded
                && !observation.confirmed_update
                && candidates_supported(p.provider);
            if let Some(error) = p
                .provider
                .and_then(|m| s.inventory.candidate_errors.get(m.id()))
            {
                failed = true;
                lines.push(format!("Latest candidate check failed: {}", concise(error)));
            }
            update |= observation.confirmed_update;
            lines.push(format!(
                "\n{} / {} / {} / {}",
                provider_badge::label(p.provider.unwrap()),
                observation.id,
                observation.source,
                observation.scope
            ));
            lines.push(format!(
                "Installed: {}{}",
                p.installed.as_deref().unwrap_or("Unknown"),
                p.target
                    .as_ref()
                    .map(|v| format!(" / candidate: {v}"))
                    .unwrap_or_default()
            ));
            lines.push(observation.candidate_status(p).into());
            lines.push(format!("Inventory observed: {}", observation.checked_at));
            if let Some(time) = s.activity.data.candidate_checks.get(&observation.check_key) {
                lines.push(format!("Last successful provider check: {time}"));
            }
            if !observation.confirmed_update
                && let Some(detail) = &observation.detail
            {
                lines.push(detail.clone());
            }
            if let Some(root) = &observation.root {
                lines.push(format!("Location: {root}"));
            }
        }
        if let Some(q) = s.live.sessions.get(&p.id) {
            if let Some(error) = &q.error {
                failed = true;
                lines.push(format!("Release-note check failed: {}", concise(error)));
            } else if s.live.active.as_deref() == Some(&p.id) {
                lines.push("Loading release notes".into());
            } else if let Some(report) = &q.report {
                failed |= report.status == "source_failed";
                lines.push(format!(
                    "Release notes: {} authored notes loaded / {}",
                    report
                        .notes
                        .iter()
                        .filter(|n| n.authored_notes && !n.body.trim().is_empty())
                        .count(),
                    report.status.replace('_', " ")
                ));
                lines.extend(report.warnings.iter().take(1).map(|w| concise(w)));
            } else if s
                .live
                .catalog
                .packages
                .iter()
                .find(|profile| profile.id == p.id)
                .is_some_and(|profile| profile.streams.is_empty())
                && p.extension.is_none()
                && p.dependency.is_none()
                && p.watchlist.is_none()
            {
                lines.push(s.source_status_for(p));
            } else {
                lines.push("Release notes not checked".into());
            }
            if !p.sample_notes
                && let Some(report) = &q.report
            {
                if let Some(feed) = &q.report_feed {
                    unread.extend(
                        s.activity
                            .data
                            .unread(feed, report)
                            .into_iter()
                            .map(|key| (feed.clone(), key)),
                    );
                    if let Some(time) = s.activity.data.source_checks.get(feed) {
                        lines.push(format!("Last successful release-source fetch: {time}"));
                    }
                }
                for date in report
                    .notes
                    .iter()
                    .filter(|n| n.authored_notes && !n.body.trim().is_empty())
                    .filter_map(|n| n.date)
                    .filter(|d| *d <= today)
                {
                    latest = Some(latest.map_or(date, |old: NaiveDate| old.max(date)));
                }
            }
        }
        if at >= 2 {
            lines.truncate(start);
        }
    }
    if row.members.len() > 2 {
        lines.push(format!("+{} more observations", row.members.len() - 2));
    }
    let age = latest.and_then(|date| age(date, today));
    if let Some(date) = latest {
        lines.push(format!("Latest retrieved dated release: {date} (UTC)"));
    }
    if !unread.is_empty() {
        lines.push(format!("{} unread notes", unread.len()));
    }
    if let Some(error) = &s.activity.error {
        lines.push(error.clone());
    }
    Summary {
        update,
        unread: unread.len(),
        age,
        failed,
        tooltip: lines.join("\n"),
    }
}

pub fn today() -> NaiveDate {
    Utc::now().date_naive()
}

pub fn candidate_scope_scene(s: &mut State) {
    use releasescout_engine::inventory;
    let data = serde_json::json!({
        "root":"C:/global", "installedExit":0, "updateExit":1,
        "installed":[{"dependencies":{
            "convex":{"version":"1.49.0"}, "cross-env":{"version":"10.1.0"},
            "fixture-broken":{"version":"2.0.0"}
        }}],
        "updates":{
            "@scope/uninstalled":{"current":"1.0.0","latest":"2.0.0"},
            "fixture-broken":{"current":"1.0.0","latest":"3.0.0"}
        },
        "metadata":{
            "convex":{"hasCommands":true,"description":"Client for the Convex Cloud"},
            "cross-env":{"hasCommands":true,"description":"Run scripts with environment variables across platforms"},
            "fixture-broken":{"hasCommands":true}
        }
    });
    s.accept_scan(inventory::parse(Manager::Pnpm, true, &data.to_string()).unwrap());
    let row = s
        .rows
        .iter()
        .find(|r| {
            s.catalog.packages[r.package]
                .observation
                .as_ref()
                .is_some_and(|o| o.id == "convex")
        })
        .unwrap();
    let id = row.id.clone();
    s.table.select(id.clone());
    s.open_sidebar_panel(&id, crate::sidebar_actions::Mode::Details);
}

pub fn update_colors_scene(s: &mut State) {
    let data = serde_json::json!({
        "installed":{"apps":[
            {"Name":"gh","Version":"2.0.0","Source":"main"},
            {"Name":"pnpm","Version":"10.0.0","Source":"main"},
            {"Name":"uv","Version":"0.10.0","Source":"main"}
        ]},
        "updates":[
            {"Name":"gh","Installed Version":"2.0.0","Latest Version":"3.0.0"},
            {"Name":"pnpm","Installed Version":"10.0.0","Latest Version":"10.0.1"},
            {"Name":"uv","Installed Version":"0.10.0","Latest Version":"0.11.0"}
        ]
    });
    s.accept_scan(
        releasescout_engine::inventory::parse(Manager::Scoop, true, &data.to_string()).unwrap(),
    );
}

pub fn scene(s: &mut State) {
    use releasescout_engine::{Basis, Note, Report, inventory};
    let scan=inventory::parse(Manager::Scoop,true,&serde_json::json!({
        "installed":{"apps":[{"Name":"gh","Version":"2.0.0","Source":"main"},{"Name":"pnpm","Version":"10.0.0","Source":"main"},{"Name":"unmapped-fixture-tool","Version":"1.0.0","Source":"extras"}]},
        "updates":[{"Name":"gh","Installed Version":"2.0.0","Latest Version":"2.1.0"}]
    }).to_string()).unwrap();
    s.accept_scan(scan);
    let id_for = |s: &State, name: &str| {
        s.catalog
            .packages
            .iter()
            .find(|p| p.detected && p.name == name)
            .unwrap()
            .id
            .clone()
    };
    let selected = id_for(s, "Unmapped Fixture Tool");
    s.select_package(&selected);
    for (name, days, count) in [("GitHub CLI", 3, 2), ("pnpm", 14, 1)] {
        let id = id_for(s, name);
        let notes = (0..count)
            .map(|i| Note {
                title: format!("2.{}.0", i + 1),
                version: Some(format!("2.{}.0", i + 1)),
                url: format!("https://example.com/{name}/{i}"),
                date: Some(today() - chrono::Duration::days(days + i)),
                body: "Fixed configuration loading and improved error reporting.".into(),
                authored_notes: true,
                preview: false,
            })
            .collect();
        s.apply_engine_result(
            id.clone(),
            Ok(Report {
                package: id.clone(),
                stream: "main".into(),
                scope: name.into(),
                basis: Basis::Version,
                status: "partial".into(),
                complete: false,
                notes,
                warnings: vec![],
                evidence: vec![],
                attempts: vec![],
            }),
        );
        if name == "pnpm" {
            s.apply_engine_result(
                id,
                Err("Temporary release-source request failure; previous notes retained".into()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_size_requires_comparable_semver_and_a_newer_core_version() {
        for (from, to, expected) in [
            ("1.2.3", "1.2.4", Some(UpdateSize::Patch)),
            ("1.2.3", "1.3.0", Some(UpdateSize::Minor)),
            ("1.2.3", "2.0.0", Some(UpdateSize::Major)),
            ("0.1.0", "0.2.0", Some(UpdateSize::Minor)),
            ("v1.2.3", "v1.2.4", Some(UpdateSize::Patch)),
            ("1.2.3", "2.0.0-rc.1", Some(UpdateSize::Major)),
            ("1.2.3-beta.1", "1.2.3", None),
            ("1.2.3+one", "1.2.3+two", None),
            ("1.2.3", "1.2.3", None),
            ("2.0.0", "1.9.0", None),
            ("1.2", "1.3", None),
            ("1.2.3.4", "1.2.3.5", None),
            ("unknown", "2.0.0", None),
        ] {
            assert_eq!(UpdateSize::between(from, to), expected, "{from} -> {to}");
        }
    }

    #[test]
    fn descriptions_are_bounded_normalized_and_selected_from_group_members() {
        assert_eq!(clean_description(None), None);
        assert_eq!(clean_description(Some(" \n\t ")), None);
        assert_eq!(clean_description(Some("%extension.description%")), None);
        assert_eq!(
            clean_description(Some("A\n description\twith  spaces\0")),
            Some("A\n description\twith  spaces".into())
        );
        assert_eq!(
            clean_description(Some(&"x".repeat(5000))).unwrap().len(),
            4096
        );
        let mut s = State::demo();
        let row = PackageRow {
            id: "fixture".into(),
            name: "Fixture".into(),
            package: 0,
            members: vec![0, 1],
        };
        s.catalog.packages[0].description = None;
        s.catalog.packages[1].description = Some("A second provider's description".into());
        assert_eq!(
            description(&s, &row),
            Some("A second provider's description")
        );
        s.catalog.packages[0].description =
            Some("The representative provider's description".into());
        assert_eq!(
            description(&s, &row),
            Some("The representative provider's description")
        );
        s.catalog.packages[0].description = Some("Fixture".into());
        assert_eq!(
            description(&s, &row),
            Some("A second provider's description")
        );
        description_scene(&mut s);
        let gh = s.rows.iter().find(|r| r.name == "GitHub CLI").unwrap();
        assert_eq!(
            description(&s, gh),
            Some("GitHub's command-line interface.")
        );
        let pnpm = s
            .rows
            .iter()
            .find(|r| r.name == "Fixture No Description")
            .unwrap();
        assert!(description(&s, pnpm).is_none());
    }

    #[test]
    fn comparisons_and_age_labels_do_not_guess() {
        assert!(newer("1.2.0", "1.3.0"));
        assert!(newer("10.0.1.2", "10.0.1.3"));
        assert!(!newer("2.0.0", "1.9.9"));
        assert!(!newer("unknown", "new"));
        assert!(!newer("1.0", "1.0.0"));
        let today = "2025-02-10".parse().unwrap();
        assert_eq!(
            age("2025-02-10".parse().unwrap(), today).as_deref(),
            Some("Today")
        );
        assert_eq!(
            age("2025-02-07".parse().unwrap(), today).as_deref(),
            Some("3d ago")
        );
        assert!(age("2025-02-11".parse().unwrap(), today).is_none());
    }

    #[test]
    fn source_publication_is_not_an_update_and_candidate_failures_keep_last_success() {
        let mut s = State::new();
        scene(&mut s);
        let row = s.rows.iter().find(|r| r.name == "pnpm").unwrap();
        let info = summary(&s, row, today());
        assert!(!info.update);
        assert_eq!(info.unread, 1);
        assert_eq!(info.age.as_deref(), Some("14d ago"));
        assert!(info.failed);
        let key = s.catalog.packages[row.package]
            .observation
            .as_ref()
            .unwrap()
            .check_key
            .clone();
        let checked = s.activity.data.candidate_checks[&key].clone();
        s.inventory
            .candidate_errors
            .insert("scoop".into(), "Provider unavailable".into());
        let info = summary(&s, row, today());
        assert!(info.tooltip.contains("Latest candidate check failed"));
        assert_eq!(s.activity.data.candidate_checks[&key], checked);
        let p = &mut s.catalog.packages[row.package];
        p.sample_notes = true;
        assert!(summary(&s, row, today()).age.is_none());
        assert_eq!(summary(&s, row, today()).unread, 0);
    }

    #[test]
    fn provider_evidence_is_required_even_for_comparable_candidate_versions() {
        let mut s = State::new();
        scene(&mut s);
        let row = s.rows.iter().find(|r| r.name == "GitHub CLI").unwrap();
        assert!(summary(&s, row, today()).update);
        let p = &mut s.catalog.packages[row.package];
        p.observation = None;
        assert!(!summary(&s, row, today()).update);
    }

    #[test]
    fn grouping_deduplicates_shared_feed_notes_and_keeps_all_scopes() {
        let mut s = State::new();
        scene(&mut s);
        let original = s
            .rows
            .iter()
            .find(|r| r.name == "GitHub CLI")
            .unwrap()
            .package;
        let p = &s.catalog.packages[original];
        let q = &s.live.sessions[&p.id];
        let report = q.report.clone();
        let feed = q.report_feed.clone();
        let mut copy = p.clone();
        copy.id = "second-observation".into();
        copy.observation.as_mut().unwrap().scope = "machine".into();
        let index = s.catalog.packages.len();
        s.catalog.packages.push(copy);
        s.live.sessions.insert(
            "second-observation".into(),
            crate::live::Query {
                report,
                report_feed: feed,
                ..Default::default()
            },
        );
        let row = PackageRow {
            id: "shared".into(),
            name: "GitHub CLI".into(),
            package: original,
            members: vec![original, index],
        };
        let info = summary(&s, &row, today());
        assert_eq!(info.unread, 2);
        assert!(info.tooltip.contains("machine"));
        assert!(info.tooltip.contains("user"));
    }
}
