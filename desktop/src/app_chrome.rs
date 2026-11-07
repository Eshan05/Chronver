use crate::model::{Dataset, State};
use chrono::{DateTime, Utc};
use rune::{Color, El, Icon, Key, TextStyle, Transition, col, icon, row, text};
use rune_kit::{Button, Kit, Loader, glyphs};
use std::time::Duration;

pub const DOCK_CLEARANCE: f32 = 52.;
const NOTES_LOADING: &str = "Loading update notes";

#[derive(Default)]
pub struct Chrome {
    pub status_hover: bool,
    pub status_pinned: bool,
}

macro_rules! asset {
    ($name:ident, $file:literal) => {
        static $name: Icon = Icon {
            name: concat!("chrome-", $file),
            view: 24.,
            body: include_str!(concat!("../assets/status/", $file, ".svg")),
        };
    };
}
asset!(SUN, "sun");
asset!(MOON, "moon");
asset!(REFRESH, "refresh-cw");
asset!(WARNING, "triangle-alert");
asset!(HISTORY, "history");
static LIBRARY: Icon = Icon {
    name: "activity-library",
    view: 24.,
    body: include_str!("../assets/symbols/package.svg"),
};
static SYSTEM: Icon = Icon {
    name: "activity-system",
    view: 24.,
    body: include_str!("../assets/providers/monitor.svg"),
};

pub fn inventory_busy(s: &State) -> usize {
    s.inventory
        .discovery
        .iter()
        .filter(|w| w.is_pending())
        .count()
        + usize::from(s.inventory.loading)
}

pub fn notice_messages(s: &State) -> Vec<String> {
    let inventory = &s.inventory;
    let mut messages = inventory.discovery_errors.clone();
    messages.extend(
        inventory.candidate_errors.iter().map(|(manager, error)| {
            let provider = releasescout_engine::inventory::Manager::ALL
                .into_iter()
                .find(|m| m.id() == manager);
            let name = match provider {
                Some(manager) => crate::provider_badge::label(manager),
                None => manager.as_str(),
            };
            format!("{name}: {error}")
        }),
    );
    if let Some(error) = &inventory.error
        && !inventory.candidate_errors.values().any(|saved| saved == error)
    {
        messages.push(error.clone());
    }
    messages.extend(
        inventory
            .scans
            .iter()
            .filter(|scan| {
                scan.update_check_requested
                    && !scan.candidate_results_reused
                    && releasescout_engine::inventory_refresh::supports_candidates(scan.manager)
                    && !scan.update_check_succeeded
                    && !inventory.candidate_errors.contains_key(scan.manager.id())
            })
            .map(|scan| {
                format!("{}: Update check incomplete", crate::provider_badge::label(scan.manager))
            }),
    );
    if let Some(error) = &inventory.workspace.export_error {
        messages.push(format!("Export failed: {error}"));
    }

    let mut note_issues: Vec<_> = s
        .live
        .sessions
        .iter()
        .filter_map(|(id, query)| {
            let error = query.error.as_deref().or_else(|| {
                let report = query.report.as_ref().filter(|r| r.status == "source_failed")?;
                let reason = report
                    .attempts
                    .iter()
                    .map(|attempt| attempt.detail.trim())
                    .find(|detail| !detail.is_empty())
                    .unwrap_or("Release source unavailable");
                Some(reason)
            })?;
            let name = s
                .catalog
                .packages
                .iter()
                .find(|p| p.id == *id)
                .map_or(id.as_str(), |p| &p.name);
            Some(format!("{name}: {error}"))
        })
        .collect();
    note_issues.sort_unstable();
    messages.extend(note_issues);
    messages.extend(s.activity.error.iter().cloned());
    messages.extend(s.note_cache.error.iter().cloned());
    messages.extend(s.live.settings_errors.iter().cloned());
    let mut seen = std::collections::HashSet::new();
    messages.retain(|message| seen.insert(message.clone()));
    messages
}

fn busy(s: &State) -> bool {
    inventory_busy(s) > 0
        || s.live.work.is_pending()
        || s.live.history.is_pending()
        || s.source_loader.work.is_pending()
        || s.note_prefetch.progress().active > 0
}

fn activity(s: &State, notices: usize) -> String {
    let pending = inventory_busy(s);
    if pending > 0 {
        if s.inventory.loading {
            "Checking inventory".into()
        } else {
            "Scanning inventory".into()
        }
    } else if s.live.work.is_pending() || s.live.history.is_pending() {
        "Fetching notes".into()
    } else if s.source_loader.work.is_pending() {
        "Finding sources".into()
    } else if s.note_prefetch.progress().active > 0 {
        NOTES_LOADING.into()
    } else if notices > 0 {
        crate::rich_select::count_label(notices, "notice")
    } else if s.dataset != Dataset::Detected {
        "Read-only".into()
    } else if s.inventory.scans.is_empty() {
        "Not scanned".into()
    } else {
        last_scan(s).unwrap_or_else(|| "Scans complete".into())
    }
}

fn activity_icon(s: &State, notices: usize, k: &Kit) -> El {
    if busy(s) {
        Loader::new().size(13.).build(k).a11y_hidden()
    } else {
        icon(
            if notices > 0 {
                &WARNING
            } else if s.inventory.scans.is_empty() {
                &HISTORY
            } else {
                &glyphs::CHECK
            },
            13.,
        )
        .color(k.c.icon_muted)
        .a11y_hidden()
    }
}

fn open_inventory(s: &mut State) {
    s.inventory.workspace.context = None;
    s.inventory.open = true;
    if !notice_messages(s).is_empty() {
        s.inventory.notices_open = true;
    }
    s.chrome.status_pinned = false;
    s.chrome.status_hover = false;
    if let Some(scan) = s.inventory.scans.iter().find(|scan| {
        scan.update_check_requested
            && !scan.candidate_results_reused
            && !scan.update_check_succeeded
            && releasescout_engine::inventory_refresh::supports_candidates(scan.manager)
    }) {
        s.inventory.manager = releasescout_engine::inventory::Manager::ALL
            .iter()
            .position(|manager| *manager == scan.manager)
            .unwrap_or(s.inventory.manager);
    }
}

fn tool(key: &str, glyph: &'static Icon, label: &str, k: &Kit) -> El {
    crate::sidebar_actions::action(key, glyph, label, k)
        .size(28., 28.)
        .radius(6.)
}

pub fn header(s: &State, k: &Kit, compact: bool) -> El {
    let notices = notice_messages(s).len();
    let status = activity(s, notices);
    let notes_loading = status == NOTES_LOADING;
    row()
        .key("global-controls")
        .gap(8.)
        .center_y()
        .child(crate::command::trigger(k, compact))
        .child(
            row()
                .key("global-activity")
                .role("button")
                .label(if notes_loading {
                    "Show update note activity"
                } else if notices > 0 {
                    "View notices"
                } else {
                    "Open inventory activity"
                })
                .help(&status)
                .description(&status)
                .focusable(true)
                .h(28.)
                .max_w(if notes_loading { 164. } else { 140. })
                .px(8.)
                .gap(6.)
                .center_y()
                .radius(6.)
                .hover(k.c.hover)
                .focus_ring(1., -1., k.c.border_focus)
                .on_press(move |s: &mut State| {
                    if notes_loading {
                        s.chrome.status_pinned = true;
                    } else {
                        open_inventory(s);
                    }
                })
                .child(activity_icon(s, notices, k))
                .children((!compact).then(|| {
                    text(status)
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_muted)
                        .truncate()
                })),
        )
        .child(
            tool(
                "global-refresh",
                &REFRESH,
                "Scan now (automatic scans run on open, at most once every 24 hours)",
                k,
            )
            .disabled(inventory_busy(s) > 0)
            .on_press(|s: &mut State| s.start_discovery()),
        )
        .child(
            tool("global-ai-settings", &glyphs::SPARKLE, "AI settings", k)
                .on_press_tx(crate::ai_menu::open_settings_tx),
        )
        .child(
            tool(
                "global-theme",
                if s.dark { &SUN } else { &MOON },
                if s.dark {
                    "Switch to light mode"
                } else {
                    "Switch to dark mode"
                },
                k,
            )
            .on_press(|s: &mut State| s.dark = !s.dark),
        )
}

fn last_scan(s: &State) -> Option<String> {
    let now = Utc::now();
    let time = s
        .inventory
        .scans
        .iter()
        .filter_map(|scan| DateTime::parse_from_rfc3339(&scan.checked_at).ok())
        .filter(|time| *time <= now)
        .max()?;
    let minutes = (now.timestamp() - time.timestamp()) / 60;
    Some(if minutes == 0 {
        "Last scan just now".into()
    } else if minutes < 60 {
        format!("Last scan {minutes}m ago")
    } else if minutes < 1440 {
        format!("Last scan {}h ago", minutes / 60)
    } else {
        format!("Last scan {}d ago", minutes / 1440)
    })
}

fn section_label(s: &State) -> &'static str {
    if s.inventory.open {
        return "Inventory";
    }
    match s.section {
        crate::model::LibrarySection::Software => "Software",
        crate::model::LibrarySection::Extensions => "Extensions",
        crate::model::LibrarySection::Packages => "Packages",
    }
}

fn section_icon(s: &State) -> &'static Icon {
    if s.section == crate::model::LibrarySection::Extensions {
        &glyphs::FILE_CODE
    } else {
        &LIBRARY
    }
}

fn entry_count(s: &State, inventory_counts: Option<(usize, usize)>) -> String {
    if let Some((matched, total)) = inventory_counts {
        return format!("{matched} / {total}");
    }
    if s.visible.len() != s.rows.len() {
        format!("{} / {}", s.visible.len(), s.rows.len())
    } else {
        s.rows.len().to_string()
    }
}

fn compact_status(s: &State, inventory_counts: Option<(usize, usize)>) -> (String, Option<String>) {
    if busy(s) {
        let pending = inventory_busy(s);
        if pending > 0 && !s.inventory.loading {
            return (
                "Scanning providers".into(),
                Some(format!(
                    "{} / {}",
                    s.inventory.discovery.len().saturating_sub(pending),
                    s.inventory.discovery.len()
                )),
            );
        }
        return (activity(s, notice_messages(s).len()), None);
    }
    if s.status.starts_with(&s.inventory_summary()) {
        return (section_label(s).into(), Some(entry_count(s, inventory_counts)));
    }
    (s.status.clone(), None)
}

fn count_row(key: &str, glyph: &'static Icon, label: &str, count: String, k: &Kit) -> El {
    row()
        .key(key)
        .w_full()
        .gap(8.)
        .center_y()
        .child(icon(glyph, 14.).color(k.c.icon_muted).a11y_hidden())
        .child(
            text(label)
                .key(format!("{key}-label"))
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .grow(1.)
                .min_w(0.)
                .truncate(),
        )
        .child(
            text(count)
                .key(format!("{key}-count"))
                .font(k.ty.text_xsmall.mono())
                .color(k.c.text_base)
                .max_w(104.)
                .truncate(),
        )
}

pub fn footer(s: &State, k: &Kit, inventory_counts: Option<(usize, usize)>) -> El {
    let expanded = s.chrome.status_hover || s.chrome.status_pinned;
    let mut dock = col()
        .key("floating-status")
        .max_w(320.)
        .gap(6.)
        .items_end()
        .offset(0., if expanded { -4. } else { 0. })
        .motion(k.motion.control)
        .on_hover_change(|s: &mut State, inside, _| {
            s.chrome.status_hover = inside;
            if inside {
                s.description_hover = None;
            }
        })
        .hover_delays(Duration::from_millis(120), Duration::from_millis(160))
        .on_key(|s: &mut State, event: &rune::KeyEvent, tx: &mut rune::Tx| {
            if event.key == Key::Escape {
                s.chrome.status_pinned = false;
                s.chrome.status_hover = false;
                tx.focus("status-trigger", true);
                true
            } else {
                false
            }
        });
    if expanded {
        let mut details = col()
            .key("status-details")
            .role("region")
            .label("Activity details")
            .w(320.)
            .padding(12.)
            .gap(10.)
            .radius(8.)
            .fill(k.c.layer_01)
            .border(1., k.c.border_base)
            .shadow(4., 16., Color::BLACK.alpha(0.16))
            .transition(Transition::offset(0., 8.).combined(Transition::opacity()))
            .token_motion(k.motion.overlay)
            .child(
                row()
                    .w_full()
                    .center_y()
                    .gap(8.)
                    .child(
                        text("Activity")
                            .font(k.ty.ui_medium)
                            .color(k.c.text_base)
                            .grow(1.),
                    )
                    .child(
                        tool("status-close", &glyphs::CLOSE, "Close activity details", k).on_press(
                            |s: &mut State| {
                                s.chrome.status_pinned = false;
                                s.chrome.status_hover = false;
                            },
                        ),
                    ),
            );
        if !s.status.starts_with(&s.inventory_summary()) {
            details = details.child(
                text(&s.status)
                    .key("status")
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .w_full()
                    .max_lines(4)
                    .description(&s.status),
            );
        }
        let mut metrics = col()
            .key("activity-counts")
            .w_full()
            .gap(8.)
            .child(count_row(
                "activity-entries",
                section_icon(s),
                section_label(s),
                entry_count(s, inventory_counts),
                k,
            ));
        if s.section == crate::model::LibrarySection::Software && s.dataset == Dataset::Detected {
            let support = s
                .catalog
                .packages
                .iter()
                .filter(|p| p.detected && p.component_reason.is_some())
                .count();
            metrics = metrics.child(count_row(
                "activity-support",
                &SYSTEM,
                if s.show_components {
                    "System & support shown"
                } else {
                    "System & support hidden"
                },
                support.to_string(),
                k,
            ));
        }
        metrics = metrics
            .child(count_row(
                "activity-pending",
                &REFRESH,
                "Providers pending",
                inventory_busy(s).to_string(),
                k,
            ))
            .child(count_row(
                "activity-inventory-issues",
                &WARNING,
                "Inventory issues",
                s.inventory.discovery_errors.len().to_string(),
                k,
            ))
            .child(count_row(
                "activity-notices",
                &WARNING,
                "Notices",
                notice_messages(s).len().to_string(),
                k,
            ));
        let progress = s.note_prefetch.progress();
        let ready = progress.loaded + progress.saved;
        if progress.active > 0 {
            metrics = metrics.child(count_row(
                "activity-notes-loading",
                &REFRESH,
                "Update notes loading",
                progress.active.to_string(),
                k,
            ));
        }
        if progress.active + ready + progress.unavailable > 0 {
            metrics = metrics.child(
                count_row(
                    "activity-notes-ready",
                    &glyphs::FILE_TEXT,
                    "Update notes ready",
                    ready.to_string(),
                    k,
                )
                .help(format!("{} loaded, {} already saved", progress.loaded, progress.saved)),
            );
        }
        if progress.unavailable > 0 {
            metrics = metrics.child(count_row(
                "activity-notes-unavailable",
                &glyphs::FILE_TEXT,
                "No update notes returned",
                progress.unavailable.to_string(),
                k,
            ));
        }
        details = details.child(metrics);
        if let Some(error) = &s.note_cache.error {
            details = details.child(
                text(error)
                    .font(k.ty.text_xsmall)
                    .color(k.c.text_muted)
                    .wrap_anywhere(),
            );
        }
        for error in &s.live.settings_errors {
            details = details.child(
                text(error)
                    .font(k.ty.text_xsmall)
                    .color(k.c.text_muted)
                    .wrap_anywhere(),
            );
        }
        if let Some(time) = last_scan(s) {
            details = details.child(text(time).font(k.ty.text_xsmall).color(k.c.text_muted));
        }
        if s.dataset == Dataset::Reference {
            details = details.child(
                text(format!(
                    "Demo data / {}",
                    s.catalog.captured.get(..10).unwrap_or(&s.catalog.captured)
                ))
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted)
                .help(&s.catalog.source),
            );
        }
        details = details.child(
            row().w_full().center_y().justify_end().child(
                Button::new("Open inventory")
                    .on_press(open_inventory)
                    .build(k)
                    .key("status-inventory"),
            ),
        );
        dock = dock.child(details);
    }
    let (status_label, status_count) = compact_status(s, inventory_counts);
    let counted = status_count.is_some();
    dock = dock.child(
        row()
            .key("status-trigger")
            .role("button")
            .label("Show activity details")
            .description(if let Some(count) = &status_count {
                format!("{status_label}: {count}")
            } else {
                status_label.clone()
            })
            .expanded(expanded)
            .focusable(true)
            .h(30.)
            .w(280.)
            .max_w(280.)
            .px(12.)
            .gap(7.)
            .center_y()
            .radius(6.)
            .fill(k.c.layer_01)
            .border(1., k.c.border_base)
            .shadow(2., 8., Color::BLACK.alpha(0.12))
            .hover(k.c.hover)
            .focus_ring(1., -1., k.c.border_focus)
            .on_press(|s: &mut State| s.chrome.status_pinned = !s.chrome.status_pinned)
            .child(if counted && !busy(s) {
                icon(section_icon(s), 13.)
                    .color(k.c.icon_muted)
                    .a11y_hidden()
            } else {
                activity_icon(s, notice_messages(s).len(), k)
            })
            .child(
                text(status_label)
                    .key("status-label")
                    .font(TextStyle::new(12., 400.).lh(16.))
                    .color(k.c.text_muted)
                    .grow(1.)
                    .min_w(0.)
                    .truncate(),
            )
            .children(status_count.map(|count| {
                text(count)
                    .key("status-count")
                    .font(k.ty.text_xsmall.mono())
                    .color(k.c.text_base)
                    .max_w(96.)
                    .truncate()
            }))
            .children((!counted).then(|| {
                icon(
                    if expanded {
                        &glyphs::CHEVRON_DOWN
                    } else {
                        &glyphs::CHEVRON_UP
                    },
                    12.,
                )
                .color(k.c.icon_muted)
                .a11y_hidden()
            })),
    );
    // This is an overlay, not a layout footer. Only scroll content gets end clearance.
    col()
        .key("floating-status-layer")
        .absolute(0., 0.)
        .w_full()
        .h_full()
        .padding(12.)
        .justify_end()
        .items_end()
        .child(dock)
}

pub fn scene(s: &mut State) {
    crate::sidebar_info::scene(s);
    s.chrome.status_pinned = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use releasescout_engine::inventory::{Manager, Scan};

    fn scan(manager: Manager, checked_at: String, complete: bool) -> Scan {
        Scan {
            manager,
            checked_at,
            candidate_checked_at: None,
            candidate_results_reused: false,
            update_check_requested: true,
            update_check_succeeded: complete,
            packages: vec![],
            warnings: vec!["Expected provider scope limitation".into()],
        }
    }

    #[test]
    fn indicators_use_actual_work_and_do_not_treat_scope_warnings_as_failures() {
        let mut s = State::new();
        s.set_dataset(Dataset::Detected);
        assert_eq!(activity(&s, 0), "Not scanned");
        s.inventory
            .scans
            .push(scan(Manager::Scoop, Utc::now().to_rfc3339(), true));
        assert!(notice_messages(&s).is_empty());
        assert!(activity(&s, 0).starts_with("Last scan"));
        s.inventory.scans[0].update_check_succeeded = false;
        assert_eq!(notice_messages(&s), ["Scoop: Update check incomplete"]);
        s.inventory
            .candidate_errors
            .insert("scoop".into(), "Candidate request failed".into());
        s.inventory.error = Some("Candidate request failed".into());
        assert_eq!(
            notice_messages(&s),
            ["Scoop: Candidate request failed"],
            "One failed provider is not three separate notices"
        );
        s.inventory.loading = true;
        assert!(busy(&s));
        assert_eq!(activity(&s, 1), "Checking inventory");
        s.inventory.loading = false;
        s.inventory.candidate_errors.clear();
        s.inventory.error = None;
        s.inventory.scans.clear();
        s.set_dataset(Dataset::Stress);
        assert_eq!(activity(&s, 0), "Read-only");
    }

    #[test]
    fn scan_age_ignores_invalid_and_future_timestamps() {
        let mut s = State::new();
        assert!(last_scan(&s).is_none());
        s.inventory
            .scans
            .push(scan(Manager::Scoop, "not a date".into(), true));
        s.inventory.scans.push(scan(
            Manager::Scoop,
            (Utc::now() + chrono::Duration::days(1)).to_rfc3339(),
            true,
        ));
        assert!(last_scan(&s).is_none());
        s.inventory.scans.push(scan(
            Manager::Scoop,
            (Utc::now() - chrono::Duration::minutes(12)).to_rfc3339(),
            true,
        ));
        assert_eq!(last_scan(&s).as_deref(), Some("Last scan 12m ago"));
    }
}
