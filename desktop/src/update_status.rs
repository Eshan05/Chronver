use crate::{
    model::{Package, PackageRow, State},
    sidebar_filters::{self, Assessment, Update},
};
use releasescout_engine::{Basis, version::Version};
use rune::{El, TextStyle, col, row, text};
use rune_kit::Kit;

pub fn badge(status: Assessment, k: &Kit) -> El {
    let color = if status.update == Update::Available {
        k.c.success.fg
    } else {
        k.c.text_muted
    };
    row()
        .key("manager-status-badge")
        .min_w(0.)
        .max_w(150.)
        .px(5.)
        .py(1.)
        .radius(3.)
        .fill(if status.update == Update::Available {
            color.alpha(0.08)
        } else {
            k.c.layer_02
        })
        .border(0.5, k.c.border_muted)
        .label(status.label())
        .description(status.detail())
        .help(status.detail())
        .child(
            text(if status.update == Update::Current {
                "Current"
            } else {
                status.label()
            })
            .font(TextStyle::new(11., 400.).lh(15.))
            .color(color)
            .min_w(0.)
            .truncate()
            .a11y_hidden(),
        )
}

#[derive(Debug)]
pub struct Publisher {
    pub value: String,
    pub detail: String,
    pub url: Option<String>,
}

fn plain(value: &str, detail: &str) -> Publisher {
    Publisher {
        value: value.into(),
        detail: detail.into(),
        url: None,
    }
}

fn public_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}

pub fn publisher(s: &State, p: &Package) -> Publisher {
    if s.live.active.as_deref() == Some(&p.id) {
        return plain(
            "Checking",
            "A publisher release-note request is running. Manager candidate checks are separate.",
        );
    }
    if p.sample_notes {
        return plain(
            "Sample history",
            "These are bundled samples, not a live publisher check.",
        );
    }
    let Some(query) = s.live.sessions.get(&p.id) else {
        return plain(
            "Not checked",
            "No publisher release result has been retrieved for this observation.",
        );
    };
    let Some(report) = query.report.as_ref().filter(|r| r.package == p.id) else {
        return if query.error.is_some() {
            plain(
                "Check failed",
                "The publisher request failed. This does not establish release or update availability.",
            )
        } else {
            plain(
                "Not checked",
                "No publisher release result has been retrieved for this observation.",
            )
        };
    };
    let notes: Vec<_> = report
        .notes
        .iter()
        .filter(|note| public_url(&note.url))
        .collect();
    if notes.is_empty() {
        return if query.error.is_some() || report.status == "source_failed" {
            plain(
                "Check failed",
                "The publisher request failed. No release availability was inferred.",
            )
        } else if !report.notes.is_empty() {
            plain(
                "Local notes only",
                "Only installed or local changelogs were loaded; no public publisher release is established.",
            )
        } else {
            plain(
                "None retrieved",
                "No public releases were retrieved in this selection. This does not mean the publisher has no newer release.",
            )
        };
    }
    let stream = s
        .live
        .catalog
        .packages
        .iter()
        .find(|profile| profile.id == p.id)
        .and_then(|profile| {
            profile
                .streams
                .iter()
                .find(|stream| stream.id == report.stream)
        });
    let ordered = (report.basis == Basis::Version)
        .then_some(stream)
        .flatten()
        .and_then(|stream| {
            notes
                .iter()
                .filter_map(|note| {
                    let version = Version::parse(note.version.as_deref()?, stream.scheme).ok()?;
                    Some((*note, version))
                })
                .max_by(|a, b| a.1.compare(&b.1))
                .map(|(note, _)| note)
        });
    let note = ordered
        .or_else(|| notes.iter().copied().max_by_key(|note| note.date))
        .unwrap();
    let name = if report.basis == Basis::Date {
        note.date
            .map(|date| date.format("%b %-d, %Y").to_string())
            .unwrap_or_else(|| "Notes".into())
    } else {
        note.version
            .clone()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| {
                if note.title.trim().is_empty() {
                    "Notes".into()
                } else {
                    note.title.clone()
                }
            })
    };
    let provenance = if query.error.is_some() {
        "prior result"
    } else if query.restored || report
        .evidence
        .iter()
        .any(|e| e.cache == "offline_cache" || e.cache.starts_with("stale_cache") || e.cache.starts_with("parsed_cache"))
    {
        "cached"
    } else {
        "retrieved"
    };
    let preview = if note.preview { " / preview" } else { "" };
    Publisher {
        value: format!("{name}{preview} / {provenance}"),
        detail: format!(
            "Publisher result: {name}{preview}\n{} / {} / {}\nLimited to loaded notes and their fetched selection, not necessarily the latest upstream release. This is not a manager update candidate or a compatibility check.{}",
            report.scope,
            report.stream,
            report.status.replace('_', " "),
            if query.error.is_some() {
                " The latest request failed; this is the retained prior result."
            } else {
                ""
            }
        ),
        url: Some(note.url.clone()),
    }
}

pub fn overview(s: &State, r: &PackageRow, p: &Package, k: &Kit) -> El {
    let status = sidebar_filters::assessment(s, r);
    let publisher = publisher(s, p);
    let mut source = row()
        .key("publisher-release-state")
        .w_full()
        .min_w(0.)
        .gap(6.)
        .center_y()
        .justify_end()
        .help(&publisher.detail)
        .description(&publisher.detail)
        .child(
            text("Publisher")
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted),
        )
        .child(
            text(&publisher.value)
                .font(k.ty.text_xsmall)
                .color(k.c.text_base)
                .max_w(240.)
                .min_w(0.)
                .truncate()
                .key("publisher-release-value"),
        );
    if let Some(url) = publisher.url {
        source = source
            .role("link")
            .label(format!("Open publisher result: {}", publisher.value))
            .focusable(true)
            .radius(3.)
            .focus_ring(1., 2., k.c.border_focus)
            .on_press_tx(move |s: &mut State, tx| {
                s.status = match tx.open_url(&url) {
                    Ok(()) => "Opened publisher release".into(),
                    Err(error) => format!("Could not open publisher release: {error}"),
                };
            });
    }
    col()
        .key("update-status-overview")
        .w_full()
        .gap(4.)
        .child(
            row()
                .key("manager-update-state")
                .w_full()
                .gap(6.)
                .center_y()
                .justify_end()
                .child(text("Manager").font(k.ty.text_xsmall).color(k.c.text_muted))
                .child(badge(status, k)),
        )
        .child(source)
}

pub fn scene(s: &mut State) {
    use releasescout_engine::{
        Basis, Note, Report,
        inventory::{self, Manager},
    };
    let data = serde_json::json!({
        "installed":{"apps":[
            {"Name":"gh","Version":"2.0.0","Source":"main"},
            {"Name":"fixture-unchecked","Version":"1.0.0","Source":"main"},
            {"Name":"fixture-failed","Version":"1.0.0","Source":"main"},
            {"Name":"fixture-identity","Version":"1.0.0","Source":"main"}
        ]},
        "metadata":{"gh":{"description":"GitHub's command-line interface.","hasCommands":true}}
    });
    s.accept_scan(inventory::parse(Manager::Scoop, false, &data.to_string()).unwrap());
    s.accept_scan(inventory::parse(Manager::Local, false, &serde_json::json!({"installed":[
        {"Id":"ARP\\User\\X64\\fixture-desktop","Name":"Standalone Desktop","Version":"1.0.0","Scope":"User"}
    ]}).to_string()).unwrap());
    let mut selected = String::new();
    for p in &mut s.catalog.packages {
        if let Some(o) = &mut p.observation {
            match o.id.as_str() {
                "gh" => {
                    o.requested = true;
                    o.succeeded = true;
                    selected = p.id.clone();
                }
                "fixture-failed" => {
                    o.requested = true;
                    o.succeeded = false;
                }
                "fixture-identity" => {
                    o.requested = true;
                    o.succeeded = false;
                    o.detail = Some("ambiguous or stale update identity: fixture-identity".into());
                }
                _ => {}
            }
        }
    }
    s.activity.data.sidebar_filters[0] = Default::default();
    s.refilter();
    s.select_package(&selected);
    let stream = s
        .live
        .catalog
        .packages
        .iter()
        .find(|p| p.id == selected)
        .unwrap()
        .streams[0]
        .id
        .clone();
    s.apply_engine_result(selected.clone(), Ok(Report {
        package: selected, stream, scope: "GitHub CLI".into(), basis: Basis::Version,
        status: "partial".into(), complete: false,
        notes: vec![Note {
            title: "2.2.0".into(), version: Some("2.2.0".into()), date: Some("2026-11-14".parse().unwrap()),
            url: "https://example.com/releases/2.2.0".into(),
            body: "## Improvements\n\n- **Fixed** configuration loading.\n- Improved error reporting.\n\n### Compatibility\n\nReview changes before upgrading.".into(),
            authored_notes: true, preview: false,
        }], warnings: vec![], evidence: vec![], attempts: vec![],
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::LibrarySection, sidebar_filters::UnknownReason};
    use rune::{Env, Runtime};

    fn state() -> State {
        let mut s = State::new();
        scene(&mut s);
        s
    }

    #[test]
    fn unknown_reasons_are_specific_and_keep_the_existing_unknown_filter() {
        let mut s = state();
        for (id, reason) in [
            ("fixture-unchecked", UnknownReason::NotChecked),
            ("fixture-failed", UnknownReason::CheckFailed),
            ("fixture-identity", UnknownReason::IdentityUnresolved),
            (
                "ARP\\User\\X64\\fixture-desktop",
                UnknownReason::Unsupported,
            ),
        ] {
            let row = s
                .rows
                .iter()
                .find(|r| {
                    s.catalog.packages[r.package]
                        .observation
                        .as_ref()
                        .is_some_and(|o| o.id == id)
                })
                .unwrap();
            let status = sidebar_filters::assessment(&s, row);
            assert_eq!(status.update, Update::Unknown);
            assert_eq!(status.reason, Some(reason));
            assert_ne!(status.label(), "Unknown");
        }
        let index = s.selected().unwrap().id.clone();
        let p = s
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.id == index)
            .unwrap();
        p.installed = None;
        assert_eq!(
            sidebar_filters::assessment(
                &s,
                s.rows
                    .iter()
                    .find(|r| s.catalog.packages[r.package].id == index)
                    .unwrap()
            )
            .reason,
            Some(UnknownReason::VersionUnknown)
        );
        let p = s
            .catalog
            .packages
            .iter_mut()
            .find(|p| p.id == index)
            .unwrap();
        p.installed = Some("rolling".into());
        p.target = Some("nightly".into());
        assert_eq!(
            sidebar_filters::assessment(
                &s,
                s.rows
                    .iter()
                    .find(|r| s.catalog.packages[r.package].id == index)
                    .unwrap()
            )
            .reason,
            Some(UnknownReason::VersionUnclear)
        );
    }

    #[test]
    fn retrieved_publisher_releases_do_not_change_manager_status_or_start_requests() {
        let mut s = state();
        let p = s.selected().unwrap();
        let id = p.id.clone();
        let row = s
            .rows
            .iter()
            .find(|r| s.catalog.packages[r.package].id == id)
            .unwrap();
        assert_eq!(
            sidebar_filters::assessment(&s, row).label(),
            "Current in manager"
        );
        assert_eq!(publisher(&s, p).value, "2.2.0 / retrieved");
        assert!(!crate::sidebar_actions::can_upgrade(&s, p));
        let query = s.live.sessions.get_mut(&id).unwrap();
        query.range = true;
        query.channel = 1;
        query.to = rune::TextEdit::new("1.0.0");
        assert_eq!(
            publisher(&s, s.selected().unwrap()).value,
            "2.2.0 / retrieved",
            "draft controls cannot relabel the fetched selection"
        );
        assert!(!s.live.work.is_pending());
        assert!(!s.live.history.is_pending());
        let row = s
            .rows
            .iter()
            .find(|r| s.catalog.packages[r.package].id == id)
            .unwrap();
        assert_eq!(sidebar_filters::update(&s, row), Update::Current);
    }

    #[test]
    fn publisher_results_distinguish_cache_prior_results_local_notes_and_missing_data() {
        let mut s = state();
        let id = s.selected().unwrap().id.clone();
        let q = s.live.sessions.get_mut(&id).unwrap();
        q.report
            .as_mut()
            .unwrap()
            .evidence
            .push(releasescout_engine::Evidence {
                url: "https://example.com/releases".into(),
                fetched_at: "2026-11-14T00:00:00Z".into(),
                cache: "offline_cache".into(),
                content_sha256: None,
            });
        assert_eq!(publisher(&s, s.selected().unwrap()).value, "2.2.0 / cached");
        s.live.sessions.get_mut(&id).unwrap().error = Some("Request failed".into());
        assert_eq!(
            publisher(&s, s.selected().unwrap()).value,
            "2.2.0 / prior result"
        );
        let q = s.live.sessions.get_mut(&id).unwrap();
        q.error = None;
        q.report.as_mut().unwrap().notes[0].url = "file:///installed/CHANGELOG.md".into();
        assert_eq!(
            publisher(&s, s.selected().unwrap()).value,
            "Local notes only"
        );
        s.live
            .sessions
            .get_mut(&id)
            .unwrap()
            .report
            .as_mut()
            .unwrap()
            .notes[0]
            .url = "https://secret@example.com/releases".into();
        assert!(publisher(&s, s.selected().unwrap()).url.is_none());
        s.live.sessions.get_mut(&id).unwrap().report = None;
        assert_eq!(publisher(&s, s.selected().unwrap()).value, "Not checked");
        s.live.sessions.get_mut(&id).unwrap().error = Some("Request failed".into());
        assert_eq!(publisher(&s, s.selected().unwrap()).value, "Check failed");
    }

    #[test]
    fn publisher_uses_the_report_scope_and_never_calls_loaded_versions_globally_latest() {
        let mut s = state();
        let id = s.selected().unwrap().id.clone();
        let report = s
            .live
            .sessions
            .get_mut(&id)
            .unwrap()
            .report
            .as_mut()
            .unwrap();
        let mut newer = report.notes[0].clone();
        newer.version = Some("10.0.0".into());
        newer.preview = true;
        newer.date = Some("2026-11-13".parse().unwrap());
        report.notes.push(newer);
        report.complete = true;
        let result = publisher(&s, s.selected().unwrap());
        assert_eq!(result.value, "10.0.0 / preview / retrieved");
        assert!(!result.value.to_lowercase().contains("latest"));
        let report = s
            .live
            .sessions
            .get_mut(&id)
            .unwrap()
            .report
            .as_mut()
            .unwrap();
        report.basis = Basis::Date;
        assert_eq!(
            publisher(&s, s.selected().unwrap()).value,
            "Nov 14, 2026 / retrieved"
        );
        s.live
            .sessions
            .get_mut(&id)
            .unwrap()
            .report
            .as_mut()
            .unwrap()
            .package = "another-product".into();
        assert_eq!(publisher(&s, s.selected().unwrap()).value, "Not checked");
    }

    #[test]
    fn publisher_notes_can_exist_while_the_detected_provider_has_no_candidate_support() {
        let mut s = state();
        let id = s.selected().unwrap().id.clone();
        let p = s.catalog.packages.iter_mut().find(|p| p.id == id).unwrap();
        p.provider = Some(releasescout_engine::inventory::Manager::Local);
        let row = s
            .rows
            .iter()
            .find(|r| s.catalog.packages[r.package].id == id)
            .unwrap();
        assert_eq!(
            sidebar_filters::assessment(&s, row).reason,
            Some(UnknownReason::Unsupported)
        );
        assert_eq!(
            publisher(&s, s.selected().unwrap()).value,
            "2.2.0 / retrieved"
        );
        assert!(!crate::sidebar_actions::can_upgrade(
            &s,
            s.selected().unwrap()
        ));
        assert!(!s.live.work.is_pending());
    }

    #[test]
    fn status_rows_and_provider_icons_fit_both_themes_without_changing_selection_or_fetching() {
        for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
            let mut s = state();
            s.dark = dark;
            s.pane.set_width(420.);
            let id = s.selected().unwrap().id.clone();
            let mut rt = Runtime::with_text(
                s,
                crate::ui::view,
                size,
                Env {
                    system_dark: dark,
                    scale: 1.,
                },
                rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES),
            );
            rt.frame(1.);
            for key in ["manager-update-state", "publisher-release-state"] {
                let e = rt
                    .elements()
                    .into_iter()
                    .find(|e| e.key.as_deref() == Some(key))
                    .unwrap();
                assert!(e.rect[0] >= 420. && e.rect[0] + e.rect[2] <= size.0);
            }
            assert!(
                rt.elements()
                    .iter()
                    .any(|e| e.key.as_deref() == Some("sidebar-provider-status"))
            );
            let elements = rt.elements();
            assert!(elements.iter().all(|e|e.key.as_deref()!=Some("sidebar-update-reason")));
            for badge in elements
                .iter()
                .filter(|e| e.key.as_deref() == Some("sidebar-provider-status"))
            {
                assert!(elements.iter().any(|row| row.key.as_deref().is_some_and(|key|key.starts_with("provider-badge-"))
                    && row.rect[3] == 18.
                    && badge.rect[0] >= row.rect[0]
                    && badge.rect[0] + badge.rect[2] <= row.rect[0] + row.rect[2]
                    && badge.rect[1] >= row.rect[1]
                    && badge.rect[1] + badge.rect[3] <= row.rect[1] + row.rect[3]));
            }
            assert!(rt.accessibility_json().to_string().contains("Manager updates: Unsupported"));
            assert!(
                rt.accessibility_json()
                    .to_string()
                    .contains("Current in manager")
            );
            assert!(
                rt.accessibility_json()
                    .to_string()
                    .contains("2.2.0 / retrieved")
            );
            assert!(rt.state.section == LibrarySection::Software);
            assert_eq!(rt.state.selected().unwrap().id, id);
            assert!(!rt.state.live.work.is_pending());
            assert!(!rt.state.live.history.is_pending());
        }
    }
}
