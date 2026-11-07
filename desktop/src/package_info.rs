use crate::{
    model::{LibrarySection, Package, PackageRow, State},
    sidebar_filters, theme, update_status,
};
use rune::{El, Icon, TextStyle, col, row, text};
use rune_kit::{Button, ButtonVariant, IconButton, IconButtonVariant, Kit, glyphs};

static PACKAGE: Icon = Icon {
    name: "info-package",
    view: 24.,
    body: include_str!("../assets/symbols/package.svg"),
};
static TERMINAL: Icon = Icon {
    name: "info-diagnostics",
    view: 24.,
    body: include_str!("../assets/symbols/terminal.svg"),
};
static UNSUPPORTED: Icon = Icon {
    name: "info-unsupported",
    view: 24.,
    body: include_str!("../assets/status/ban.svg"),
};
static UNCHECKED: Icon = Icon {
    name: "info-unchecked",
    view: 24.,
    body: include_str!("../assets/status/history.svg"),
};

pub(crate) fn value(value: impl Into<String>, k: &Kit, mono: bool) -> El {
    text(value)
        .font(if mono {
            k.ty.text_xsmall.mono()
        } else {
            k.ty.text_xsmall
        })
        .color(k.c.text_base)
        .min_w(0.)
        .wrap_anywhere()
        .selectable(true)
}
pub(crate) fn field(key: &str, label: &str, content: El, k: &Kit) -> El {
    row()
        .key(key)
        .w_full()
        .min_w(0.)
        .gap(10.)
        .pl(18.)
        .py(1.)
        .child(
            text(label)
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted)
                .w(92.),
        )
        .child(content.grow(1.).min_w(0.))
}
pub(crate) fn copied_field(key: &str, label: &str, raw: &str, k: &Kit) -> El {
    let copy = raw.to_owned();
    let name = label.to_owned();
    field(
        key,
        label,
        row()
            .w_full()
            .min_w(0.)
            .gap(4.)
            .child(value(raw, k, true).grow(1.).min_w(0.))
            .child(
                IconButton::new(&glyphs::COPY)
                    .variant(IconButtonVariant::Ghost)
                    .label(format!("Copy {label}"))
                    .help(format!("Copy {label}"))
                    .on_press_tx(move |s: &mut State, tx| {
                        tx.copy(copy.clone());
                        s.status = format!("{name} copied");
                    })
                    .build(k)
                    .size(22., 22.)
                    .key(format!("{key}-copy")),
            ),
        k,
    )
}
fn heading(label: &str, k: &Kit) -> El {
    text(label)
        .font(TextStyle::new(12., 500.).lh(18.))
        .color(k.c.text_muted)
}
fn section(label: &str, glyph: &'static Icon, k: &Kit) -> El {
    row()
        .w_full()
        .gap(6.)
        .center_y()
        .pb(2.)
        .child(rune::icon(glyph, 12.).color(k.c.text_muted))
        .child(
            text(label)
                .font(TextStyle::new(12., 550.).lh(18.))
                .color(k.c.text_base),
        )
}
pub(crate) fn time(raw: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|t| {
            t.with_timezone(&chrono::Utc)
                .format("%b %-d, %Y / %H:%M UTC")
                .to_string()
        })
        .unwrap_or_else(|_| raw.into())
}
pub(crate) fn public_url(raw: &str) -> Option<url::Url> {
    url::Url::parse(raw).ok().filter(|u| {
        matches!(u.scheme(), "https" | "http")
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}
pub(crate) fn source(s: &State, p: &Package) -> Option<String> {
    if let Some(url) = s
        .live
        .sessions
        .get(&p.id)
        .and_then(|q| q.report.as_ref())
        .and_then(|report| {
            report
                .notes
                .first()
                .map(|n| n.url.clone())
                .or_else(|| report.evidence.first().map(|e| e.url.clone()))
        })
    {
        return Some(url);
    }
    if p.extension.is_some()
        && s.live
            .sessions
            .get(&p.id)
            .is_some_and(|q| !q.extension_published)
    {
        return p
            .extension
            .as_ref()
            .and_then(|e| e.changelog.as_ref())
            .and_then(|p| url::Url::from_file_path(p).ok())
            .map(Into::into);
    }
    s.live
        .catalog
        .packages
        .iter()
        .find(|profile| profile.id == p.id)
        .and_then(|profile| {
            profile
                .streams
                .get(s.live.sessions.get(&p.id).map_or(0, |q| q.component))
        })
        .and_then(|stream| stream.sources.first())
        .and_then(|source| match source {
            releasescout_engine::Source::InstalledDocument { root, filename, .. } => {
                url::Url::from_file_path(std::path::Path::new(root).join(filename))
                    .ok()
                    .map(Into::into)
            }
            releasescout_engine::Source::Github { owner, repo, .. } => {
                Some(format!("https://github.com/{owner}/{repo}"))
            }
            releasescout_engine::Source::Document { url, .. }
            | releasescout_engine::Source::Feed { url }
            | releasescout_engine::Source::LinkedDocuments { url, .. } => Some(url.clone()),
        })
        .or_else(|| p.repository.clone())
}
fn source_value(raw: &str, k: &Kit) -> El {
    let Some(url) = public_url(raw) else {
        return value(
            if raw.starts_with("file:") {
                "Installed changelog"
            } else {
                "Unavailable"
            },
            k,
            false,
        );
    };
    let name = if url.host_str() == Some("github.com") {
        url.path_segments()
            .map(|s| s.take(2).collect::<Vec<_>>().join("/"))
            .unwrap_or_else(|| "GitHub".into())
    } else {
        url.host_str().unwrap().trim_start_matches("www.").into()
    };
    let target = url.to_string();
    row()
        .key("package-info-source-link")
        .role("link")
        .label(format!("Open {name}"))
        .help(raw)
        .focusable(true)
        .min_w(0.)
        .gap(5.)
        .center_y()
        .radius(3.)
        .focus_ring(1., 2., k.c.border_focus)
        .child(
            text(name)
                .font(k.ty.text_xsmall)
                .color(k.c.text_base)
                .min_w(0.)
                .truncate(),
        )
        .child(rune::icon(&glyphs::ARROW_RIGHT, 12.).color(k.c.text_muted))
        .on_press_tx(move |s: &mut State, tx| {
            s.status = match tx.open_url(&target) {
                Ok(()) => "Opened release source".into(),
                Err(e) => format!("Could not open source: {e}"),
            };
        })
}
fn notice(key: &str, message: &str, k: &Kit) -> El {
    row()
        .key(key)
        .w_full()
        .min_w(0.)
        .gap(7.)
        .child(rune::icon(&glyphs::CIRCLE_X, 13.).color(k.c.danger.fg))
        .child(
            text(message)
                .font(k.ty.text_xsmall)
                .color(k.c.text_base)
                .grow(1.)
                .min_w(0.)
                .max_lines(2),
        )
}

fn candidate_status(p: &Package, k: &Kit) -> El {
    let o = p.observation.as_ref().expect("inventory observation");
    let supported = p
        .provider
        .is_some_and(releasescout_engine::inventory_refresh::supports_candidates);
    let (glyph, color, key, message) = if o.confirmed_update {
        (
            &glyphs::CHECK,
            k.c.success.fg,
            "inventory-check-ok",
            "Update available",
        )
    } else if !supported {
        (
            &UNSUPPORTED,
            k.c.text_muted,
            "inventory-check-unsupported",
            o.candidate_status(p),
        )
    } else if !o.requested {
        (
            &UNCHECKED,
            k.c.text_muted,
            "inventory-check-unchecked",
            "Candidates not checked",
        )
    } else if !o.succeeded {
        (
            &glyphs::CIRCLE_X,
            k.c.danger.fg,
            "inventory-check-failed",
            "Check failed or incomplete",
        )
    } else {
        (
            &glyphs::CHECK,
            k.c.success.fg,
            "inventory-check-ok",
            if p.target.is_none() {
                "No confirmed newer candidate"
            } else {
                o.candidate_status(p)
            },
        )
    };
    row()
        .w_full()
        .min_w(0.)
        .gap(5.)
        .center_y()
        .child(rune::icon(glyph, 13.).key(key).color(color).a11y_hidden())
        .child(value(message, k, false).grow(1.))
}

pub(crate) fn observation_detail(o: &crate::sidebar_info::Observation) -> Option<&str> {
    o.detail.as_deref().filter(|detail| {
        !matches!(
            *detail,
            "Scoop candidate from local buckets"
                | "No newer version in local buckets"
                | "Local registration only; no update source is configured"
        )
    })
}

pub fn view(s: &State, r: &PackageRow, p: &Package, technical: bool, k: &Kit) -> El {
    let mut facts = col().key("package-info-facts").w_full().gap(2.);
    if s.section == LibrarySection::Software {
        facts = facts
            .child(section("Installation", &PACKAGE, k))
            .child(field(
                "package-info-update",
                if r.members.len() > 1 {
                    "Group updates"
                } else {
                    "Updates"
                },
                row().child(update_status::badge(sidebar_filters::assessment(s, r), k)),
                k,
            ));
    } else {
        facts = facts.child(section(
            if p.extension.is_some() {
                "Extension"
            } else {
                "Package"
            },
            &PACKAGE,
            k,
        ));
    }
    if p.watchlist.is_none() {
        facts = facts.child(field(
            "package-info-installed",
            "Installed",
            value(p.installed.as_deref().unwrap_or("Unrecorded"), k, true),
            k,
        ));
    }
    if let Some(target) = &p.target {
        facts = facts.child(field(
            "package-info-candidate",
            "Candidate",
            value(target, k, true).help(
                if p.observation.as_ref().is_some_and(|o| o.confirmed_update) {
                    "Provider-confirmed update candidate"
                } else {
                    "Provider candidate; newer version not confirmed"
                },
            ),
            k,
        ));
    }
    if p.extension.is_some() {
        facts = facts.child(field(
            "package-info-profile",
            "Profile",
            row()
                .gap(6.)
                .center_y()
                .child(crate::profile_icons::view(
                    p.extension.as_ref().and_then(|e| e.profile_icon.as_deref()),
                    k,
                    14.,
                ))
                .child(value(crate::extension_ui::profile_name(p), k, false)),
            k,
        ));
    } else if let Some(o) = &p.observation {
        facts = facts.child(field(
            "package-info-scope",
            "Scope",
            value(&o.scope, k, false),
            k,
        ));
    }
    if let Some(w) = &p.watchlist {
        facts = facts.child(field(
            "package-info-category",
            "Category",
            row().child(crate::watchlist_ui::category_badge(w.group, k)),
            k,
        ));
    }
    if let Some(d) = &p.dependency {
        facts = facts
            .child(field(
                "package-info-requirement",
                "Declared",
                value(&d.requirement, k, true),
                k,
            ))
            .child(field(
                "package-info-project",
                "Project",
                value(&d.project, k, false),
                k,
            ));
    }
    let publisher = update_status::publisher(s, p);
    let mut notes = col()
        .key("package-info-notes")
        .w_full()
        .gap(2.)
        .pt(8.)
        .border_sides([1., 0., 0., 0.], theme::separator(k))
        .child(section("Release notes", &glyphs::FILE_TEXT, k))
        .child(field(
            "package-info-publisher",
            "Publisher",
            value(&publisher.value, k, false).help(&publisher.detail),
            k,
        ));
    if let Some(q) = s.live.sessions.get(&p.id) {
        if let Some(report) = &q.report {
            let count = report
                .notes
                .iter()
                .filter(|n| n.authored_notes && !n.body.trim().is_empty())
                .count();
            notes = notes.child(field(
                "package-info-coverage",
                "Notes",
                value(
                    format!(
                        "{count} loaded / {}",
                        if report.complete {
                            "complete selection"
                        } else {
                            "partial snapshot"
                        }
                    ),
                    k,
                    false,
                )
                .help(&report.scope),
                k,
            ));
        }
        if let Some(error) = &q.error {
            notes = notes.child(notice("package-info-note-error", error, k));
        }
    }
    if let Some(url) = source(s, p) {
        notes = notes.child(field(
            "package-info-source",
            "Source",
            source_value(&url, k),
            k,
        ));
    } else if s.live.unavailable(&p.id).is_some()
        && p.watchlist.is_none()
        && p.dependency.is_none()
        && p.extension.is_none()
    {
        notes = notes.child(field(
            "package-info-discovery",
            "Source",
            value(s.source_status_for(p), k, false),
            k,
        ));
    }
    if let Some(o) = &p.observation
        && o.requested
        && !o.succeeded
        && p.provider
            .is_some_and(releasescout_engine::inventory_refresh::supports_candidates)
    {
        facts = facts.child(notice(
            "package-info-candidate-error",
            o.detail
                .as_deref()
                .unwrap_or("Candidate check failed or incomplete"),
            k,
        ));
    }
    let mut body = col().w_full().gap(8.).child(facts).child(notes).child(
        row().w_full().child(
            Button::new("Technical details")
                .variant(ButtonVariant::Ghost)
                .icon(if technical {
                    &glyphs::CHEVRON_UP
                } else {
                    &glyphs::CHEVRON_DOWN
                })
                .expanded(technical)
                .on_press(|s: &mut State| {
                    if let Some(p) = &mut s.sidebar_actions.panel {
                        p.technical = !p.technical;
                    }
                })
                .build(k)
                .h(24.)
                .key("package-info-technical-toggle"),
        ),
    );
    if technical {
        let diagnostics = crate::sidebar_info::summary(s, r, crate::sidebar_info::today()).tooltip;
        let mut details = col()
            .key("package-info-technical")
            .w_full()
            .gap(0.)
            .pt(6.)
            .border_sides([1., 0., 0., 0.], theme::separator(k))
            .child(
                row()
                    .w_full()
                    .center_y()
                    .child(section("Diagnostics", &TERMINAL, k).grow(1.))
                    .child(
                        IconButton::new(&glyphs::COPY)
                            .variant(IconButtonVariant::Ghost)
                            .label("Copy diagnostics")
                            .help(if r.members.len() > 1 {
                                "Copy group diagnostics"
                            } else {
                                "Copy diagnostics"
                            })
                            .on_press_tx(move |s: &mut State, tx| {
                                tx.copy(diagnostics.clone());
                                s.status = "Diagnostics copied".into();
                            })
                            .build(k)
                            .size(22., 22.)
                            .key("package-info-copy-diagnostics"),
                    ),
            );
        if let Some(o) = &p.observation {
            details = details
                .child(copied_field("package-info-id", "Package ID", &o.id, k))
                .child(field(
                    "package-info-provider-source",
                    "Provider source",
                    value(&o.source, k, false),
                    k,
                ))
                .child(field(
                    "package-info-observed",
                    "Last scan",
                    value(time(&o.checked_at), k, false).help(&o.checked_at),
                    k,
                ));
            if let Some(at) = s.activity.data.candidate_checks.get(&o.check_key) {
                details = details.child(field(
                    "package-info-checked",
                    "Provider check",
                    value(time(at), k, false).help(at),
                    k,
                ));
            }
            if let Some(id) = o.registration_id.as_deref().filter(|id| *id != o.id) {
                details = details.child(copied_field(
                    "package-info-registration",
                    "Registration",
                    id,
                    k,
                ));
            }
            if let Some(path) = &o.root {
                details = details.child(copied_field("package-info-location", "Location", path, k));
            }
            if let Some(detail) = observation_detail(o) {
                details = details.child(value(detail, k, false));
            }
            if !o.succeeded
                && let Some(error) = p
                    .provider
                    .and_then(|m| s.inventory.candidate_errors.get(m.id()))
            {
                details = details
                    .child(heading("Provider notice", k))
                    .child(value(error, k, false));
            }
        } else if let Some(w) = &p.watchlist {
            details = details.child(copied_field("package-info-id", "Package ID", &w.name, k));
        } else if let Some(reference) = &p.reference_text {
            details = details.child(value(reference, k, false));
        }
        if let Some(q) = s.live.sessions.get(&p.id) {
            if let Some(at) = q
                .report_feed
                .as_ref()
                .and_then(|feed| s.activity.data.source_checks.get(feed))
            {
                details = details.child(field(
                    "package-info-source-checked",
                    "Source check",
                    value(time(at), k, false).help(at),
                    k,
                ));
            }
            if let Some(report) = &q.report {
                details = details.child(field(
                    "package-info-report-scope",
                    "Coverage",
                    value(&report.scope, k, false),
                    k,
                ));
                for warning in &report.warnings {
                    details = details.child(value(warning, k, false));
                }
                for attempt in report.attempts.iter().filter(|a| a.outcome == "failed") {
                    details = details.child(value(&attempt.detail, k, false));
                }
                for evidence in &report.evidence {
                    details = details.child(value(
                        format!("{} / {}", evidence.cache, time(&evidence.fetched_at)),
                        k,
                        false,
                    ));
                }
            }
            if let Some(error) = &q.error {
                details = details.child(value(error, k, false));
            }
        }
        body = body.child(details);
    }
    body
}

pub fn inventory_view(s: &State, p: &Package, technical: bool, k: &Kit) -> El {
    let mut facts = col()
        .key("inventory-info-facts")
        .w_full()
        .gap(2.)
        .child(section("Local inventory", &PACKAGE, k))
        .child(field(
            "inventory-info-installed",
            "Installed",
            value(p.installed.as_deref().unwrap_or("Unrecorded"), k, true),
            k,
        ));
    if let Some(target) = &p.target {
        facts = facts.child(field(
            "inventory-info-candidate",
            "Candidate",
            value(target, k, true).help(
                if p.observation.as_ref().is_some_and(|o| o.confirmed_update) {
                    "Provider-confirmed update candidate"
                } else {
                    "Provider candidate; newer version not confirmed"
                },
            ),
            k,
        ));
    }
    if let Some(o) = &p.observation {
        facts = facts
            .child(field(
                "inventory-info-check",
                "Update check",
                candidate_status(p, k),
                k,
            ))
            .child(field(
                "inventory-info-scope",
                "Scope",
                value(&o.scope, k, false),
                k,
            ))
            .child(field(
                "inventory-info-observed",
                "Last scan",
                value(time(&o.checked_at), k, false).help(&o.checked_at),
                k,
            ));
        if let Some(at) = s.activity.data.candidate_checks.get(&o.check_key) {
            facts = facts.child(field(
                "inventory-info-checked",
                "Provider check",
                value(time(at), k, false).help(at),
                k,
            ));
        }
    } else {
        let status = if p
            .provider
            .is_some_and(|m| !releasescout_engine::inventory_refresh::supports_candidates(m))
        {
            "Candidate checks not supported by this provider"
        } else {
            p.update_label()
        };
        facts = facts.child(field(
            "inventory-info-check",
            "Update check",
            value(status, k, false),
            k,
        ));
    }
    if let Some(reason) = &p.component_reason {
        facts = facts.child(value(reason, k, false));
    }
    let mut body = col().w_full().gap(8.).child(facts).child(
        row().w_full().child(
            Button::new("Technical details")
                .variant(ButtonVariant::Ghost)
                .icon(if technical {
                    &glyphs::CHEVRON_UP
                } else {
                    &glyphs::CHEVRON_DOWN
                })
                .expanded(technical)
                .on_press(|s: &mut State| {
                    s.reader_controls.inventory_technical = !s.reader_controls.inventory_technical
                })
                .build(k)
                .h(24.)
                .key("inventory-info-technical-toggle"),
        ),
    );
    if technical {
        let mut details = col()
            .key("inventory-info-technical")
            .w_full()
            .gap(0.)
            .pt(6.)
            .border_sides([1., 0., 0., 0.], theme::separator(k))
            .child(section("Diagnostics", &TERMINAL, k));
        if let Some(o) = &p.observation {
            details = details
                .child(copied_field("inventory-info-id", "Package ID", &o.id, k))
                .child(field(
                    "inventory-info-source",
                    "Provider source",
                    value(&o.source, k, false),
                    k,
                ));
            if let Some(id) = o.registration_id.as_deref().filter(|id| *id != o.id) {
                details = details.child(copied_field(
                    "inventory-info-registration",
                    "Registration",
                    id,
                    k,
                ));
            }
            if let Some(path) = &o.root {
                details =
                    details.child(copied_field("inventory-info-location", "Location", path, k));
            }
            if let Some(detail) = observation_detail(o) {
                details = details.child(value(detail, k, false));
            }
            if o.requested
                && !o.succeeded
                && p.provider
                    .is_some_and(releasescout_engine::inventory_refresh::supports_candidates)
                && let Some(error) = p
                    .provider
                    .and_then(|m| s.inventory.candidate_errors.get(m.id()))
            {
                details = details
                    .child(heading("Provider notice", k))
                    .child(value(error, k, false));
            }
        } else if let Some(reference) = &p.reference_text {
            details = details.child(value(reference, k, false));
        }
        body = body.child(details);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_provenance_is_hidden_without_discarding_actionable_notes() {
        let mut s = State::new();
        crate::sidebar_actions::scene(&mut s, crate::sidebar_actions::Mode::Details);
        let mut o = s.selected().unwrap().observation.clone().unwrap();
        for detail in [
            "Scoop candidate from local buckets",
            "No newer version in local buckets",
            "Local registration only; no update source is configured",
        ] {
            o.detail = Some(detail.into());
            assert!(observation_detail(&o).is_none());
            assert_eq!(o.detail.as_deref(), Some(detail));
        }
        for detail in [
            "Missing dependencies: runtime",
            "ambiguous or stale update identity: package",
            "Provider check timed out",
        ] {
            o.detail = Some(detail.into());
            assert_eq!(observation_detail(&o), Some(detail));
        }
    }

    #[test]
    fn source_links_are_safe_and_belong_to_the_selected_observation() {
        for raw in [
            "javascript:alert(1)",
            "file:///C:/private.txt",
            "https://user:secret@example.com/notes",
        ] {
            assert!(public_url(raw).is_none());
        }
        assert!(public_url("https://github.com/cli/cli/releases").is_some());
        let mut s = State::new();
        crate::sidebar_actions::scene(&mut s, crate::sidebar_actions::Mode::Details);
        let selected = &s.sidebar_actions.panel.as_ref().unwrap().package;
        let p = s
            .catalog
            .packages
            .iter()
            .find(|p| &p.id == selected)
            .unwrap();
        assert_eq!(
            source(&s, p).as_deref(),
            Some("https://github.com/cli/cli/releases")
        );
        let other = s
            .catalog
            .packages
            .iter()
            .find(|p| p.detected && p.name == "pnpm")
            .unwrap();
        assert_ne!(source(&s, p), source(&s, other));
    }
}
