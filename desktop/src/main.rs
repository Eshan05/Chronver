// SPDX-FileCopyrightText: 2026 Eshan05
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg_attr(
    all(target_os = "windows", not(feature = "console")),
    windows_subsystem = "windows"
)]

mod activity;
mod ai_menu;
mod ai_models;
mod ai_metadata;
mod ai_summary;
mod app_chrome;
mod command;
mod custom_icons;
mod description_preview;
mod description_ui;
mod descriptions;
mod discovery;
mod display_names;
mod dropdown;
mod extension_ui;
mod fonts;
mod icon_loader;
mod icon_picker;
mod inventory_sources;
mod inventory_menu;
mod inventory_tools;
mod inventory_ui;
mod inventory_view;
mod inventory_workspace;
mod live;
mod model;
#[cfg(target_os = "windows")]
mod native_acrylic;
mod native_backdrop;
mod native_frame;
mod note_cache;
mod note_prefetch;
mod package_icons;
mod package_info;
mod package_ui;
mod profile_icons;
mod provider_badge;
mod reader_controls;
mod release_index;
mod rich_select;
mod sidebar;
mod sidebar_actions;
mod sidebar_filters;
mod sidebar_groups;
mod sidebar_info;
mod sidebar_sort;
mod smooth_scroll;
#[cfg(test)]
mod source_audit;
mod source_loader;
#[cfg(test)]
mod tests;
mod theme;
mod ui;
mod update_status;
mod upgrade_preview;
mod watchlist_ui;

use model::{Dataset, LibrarySection, NoteMode, State};
use rune::AppSpec;

fn headless() -> bool {
    std::env::args().any(|arg| matches!(arg.as_str(), "--snap" | "--tree" | "--scene"))
}

fn main() {
    let native = !headless();
    let native_material = cfg!(target_os = "windows")
        && std::env::var_os("CHRONVER_NATIVE_BACKDROP").is_some_and(|value| value == "1");
    #[cfg(target_os = "windows")]
    if native_material && native && std::env::args_os().len() == 1 {
        // Transparent windows cannot use Rune's CPU-first launch.
        rune::warm_gpu(Default::default());
    }
    let app = rune::App::new(AppSpec {
        setup: Some(|s: &mut State, waker| {
            s.ai.work.set_waker(waker.clone());
            s.ai.metadata.set_waker(waker.clone());
            s.source_loader.work.set_waker(waker.clone());
            s.activity.save.set_waker(waker.clone());
            s.custom_icons.work.set_waker(waker.clone());
            for worker in &s.icon_loader.workers {worker.set_waker(waker.clone());}
            s.icon_loader.cache.set_waker(waker.clone());
            s.icons=rune::ImageCache::new(waker.clone());
            s.live.history.set_waker(waker.clone());
            s.live.work.set_waker(waker.clone());
            s.packages.work.set_waker(waker.clone());
            s.packages.watch.save.set_waker(waker.clone());
            s.inventory.work.set_waker(waker.clone());
            s.inventory.workspace.export.set_waker(waker.clone());
            s.package_icons.work.set_waker(waker.clone());
            s.inventory.snapshot.set_waker(waker.clone());
            s.note_cache.read.set_waker(waker.clone());
            s.note_cache.write.set_waker(waker.clone());
            s.note_prefetch.set_waker(waker.clone());
            for work in &s.inventory.discovery { work.set_waker(waker.clone()); }
            if !headless() {
                s.ai.model_file_enabled = true;
                s.icon_loader.enabled=true;
                s.icon_loader.network_enabled=true;
                s.custom_icons.enabled=true;
                s.source_loader.enabled=true;
                s.note_cache.enabled=true;
                if let Some(directory) = live::cache_directory().parent() {
                    s.live.load_local_settings(directory);
                }
                s.restore_project();
                s.restore_activity();
                s.restore_startup_inventory();
            }
        }),
        poll: Some(|s: &mut State, _, tx| {
            if s.backdrop.sync(s.dark) | s.poll_engine() | s.poll_history() | s.poll_inventory() | s.poll_inventory_export() | s.poll_discovery() | s.poll_icons() | s.poll_custom_icons() | s.poll_package_icons() | s.poll_remote_icons() | s.poll_source_discovery() | s.poll_packages() | s.poll_watchlist() | s.poll_activity() | s.poll_note_cache() | s.poll_note_prefetch() | s.ai.poll() {
                command::refresh(s);
                tx.instant();
                true
            } else {
                false
            }
        }),
        min_size: Some((1024.0, 640.0)),
        fonts: fonts::FACES,
        families: fonts::FAMILIES,
        on_key: Some(ui::on_key),
        scenes: &[
            ("saved-gcloud-notes", note_cache::saved_scene),
            ("saved-gcloud-notes-light", |s|{note_cache::saved_scene(s);s.dark=false;}),
            ("saved-note-range", note_cache::saved_range_scene),
            ("saved-note-range-light", |s|{note_cache::saved_range_scene(s);s.dark=false;}),
            ("local-inventory-popover", reader_controls::inventory_scene),
            ("local-inventory-popover-light", |s|{reader_controls::inventory_scene(s);s.dark=false;}),
            ("local-inventory-technical", |s|{reader_controls::inventory_scene(s);s.reader_controls.inventory_technical=true;}),
            ("gcloud-history", live::gcloud_scene),
            ("update-status", update_status::scene),
            ("update-status-light", |s|{update_status::scene(s);s.dark=false;}),
            ("update-status-unknown", |s|{update_status::scene(s);let id=s.catalog.packages.iter().find(|p|p.name=="Standalone Desktop").unwrap().id.clone();s.select_package(&id);}),
            ("mica-surfaces", |s| {reader_controls::scene(s,LibrarySection::Software);s.backdrop.active=true;}),
            ("mica-surfaces-light", |s| {reader_controls::scene(s,LibrarySection::Software);s.backdrop.active=true;s.dark=false;}),
            ("reader-software",|s|reader_controls::scene(s,LibrarySection::Software)),
            ("reader-software-light",|s|{reader_controls::scene(s,LibrarySection::Software);s.dark=false;}),
            ("reader-extension",|s|reader_controls::scene(s,LibrarySection::Extensions)),
            ("reader-extension-light",|s|{reader_controls::scene(s,LibrarySection::Extensions);s.dark=false;}),
            ("reader-packages",|s|reader_controls::scene(s,LibrarySection::Packages)),
            ("reader-packages-light",|s|{reader_controls::scene(s,LibrarySection::Packages);s.dark=false;}),
            ("command", command::scene),
            ("command-light", |s| { command::scene(s); s.dark = false; }),
            ("command-query", |s| { command::scene(s); command::query_scene(s); }),
            ("saved-inventory", saved_inventory_scene),
            ("sidebar-ai-menu", ai_menu::scene),
            ("sidebar-ai-menu-light", |s|{ai_menu::scene(s);s.dark=false;}),
            ("ai-settings", |s|ai_menu::settings_scene(s,ai_summary::Provider::OpenAi)),
            ("ai-settings-light", |s|{ai_menu::settings_scene(s,ai_summary::Provider::Claude);s.dark=false;}),
            ("ai-settings-custom", |s|ai_menu::settings_scene(s,ai_summary::Provider::Custom)),
            ("ai-reader-summary", ai_menu::reader_scene),
            ("ai-reader-summary-light", |s|{ai_menu::reader_scene(s);s.dark=false;}),
            ("ai-confirmation", ai_menu::preview_scene),
            ("ai-confirmation-light", |s|{ai_menu::preview_scene(s);s.dark=false;}),
            ("sidebar-groups-extensions", |s|sidebar_groups::scene(s,true)),
            ("sidebar-groups-extensions-light", |s|{sidebar_groups::scene(s,true);s.dark=false;}),
            ("sidebar-groups-packages", |s|sidebar_groups::scene(s,false)),
            ("sidebar-groups-packages-light", |s|{sidebar_groups::scene(s,false);s.dark=false;}),
            ("sidebar-composed", |s| {sidebar_actions::scene(s,sidebar_actions::Mode::Details);s.sidebar_actions.panel=None;}),
            ("sidebar-selection", |s| {sidebar_actions::scene(s,sidebar_actions::Mode::Details);s.sidebar_actions.panel=None;let id=s.table.cursor().unwrap().to_string();s.toggle_sidebar_selection(&id);}),
            ("sidebar-details", |s| sidebar_actions::scene(s,sidebar_actions::Mode::Details)),
            ("sidebar-observations",|s|sidebar_actions::observations_scene(s,false)),
            ("sidebar-observations-open",|s|sidebar_actions::observations_scene(s,true)),
            ("sidebar-details-light", |s| {sidebar_actions::scene(s,sidebar_actions::Mode::Details);s.dark=false;}),
            ("sidebar-details-technical", |s| {sidebar_actions::scene(s,sidebar_actions::Mode::Details);s.sidebar_actions.panel.as_mut().unwrap().technical=true;}),
            ("sidebar-versions-long-description", |s| {sidebar_actions::scene(s,sidebar_actions::Mode::Versions);let id=s.selected().unwrap().id.clone();s.catalog.packages.iter_mut().find(|p|p.id==id).unwrap().description=Some("# Package documentation\n\n**Install and configure** this package for development.\n\n- A supported workflow\n- Another workflow\n\n".repeat(30));}),
            ("sidebar-versions", |s| sidebar_actions::scene(s,sidebar_actions::Mode::Versions)),
            ("sidebar-versions-light", |s| {sidebar_actions::scene(s,sidebar_actions::Mode::Versions);s.dark=false;}),
            ("sidebar-upgrade", |s| {sidebar_actions::scene(s,sidebar_actions::Mode::Versions);let id=s.selected().unwrap().id.clone();s.prepare_upgrade(&id);}),
            ("custom-icon", custom_icons::scene),
            ("icon-picker", icon_picker::scene),
            ("icon-picker-symbols", |s| {icon_picker::scene(s);icon_picker::symbols_scene(s);}),
            ("icon-picker-light", |s| {icon_picker::scene(s);icon_picker::symbols_scene(s);s.dark=false;}),
            ("sidebar-status", sidebar_info::scene),
            ("sidebar-status-light", |s|{sidebar_info::scene(s);s.dark=false;}),
            ("sidebar-descriptions", sidebar_info::description_scene),
            ("sidebar-system-on", |s| { sidebar_info::description_scene(s); s.toggle_components(); }),
            ("sidebar-system-on-light", |s| { sidebar_info::description_scene(s); s.toggle_components(); s.dark=false; }),
            ("description-markdown", description_preview::scene),
            ("sidebar-sort", sidebar_sort::scene),
            ("software-provider-groups", sidebar_groups::provider_scene),
            ("software-provider-groups-light", |s| { sidebar_groups::provider_scene(s); s.dark=false; }),
            ("candidate-scope", sidebar_info::candidate_scope_scene),
            ("sidebar-sort-light", |s| {sidebar_sort::scene(s);s.dark=false;}),
            ("description-markdown-light", |s| {description_preview::scene(s);s.dark=false;}),
            ("sidebar-descriptions-light", |s| {sidebar_info::description_scene(s);s.dark=false;}),
            ("description-editor", description_ui::scene),
            ("description-editor-light", |s| {description_ui::scene(s);s.dark=false;}),
            ("software-names", display_names::scene),
            ("provider-badges", provider_badge::scene),
            ("inventory-provider-select",|s|{provider_badge::scene(s);s.inventory.open=true;s.inventory.picker=true;}),
            ("project-rich-select",|s|{package_ui::scene(s);s.packages.picker=true;}),
            ("component-rich-select",|s|{s.select_package("supabase");s.engine_query().component_open=true;}),
            ("component-rich-select-light",|s|{s.select_package("supabase");s.engine_query().component_open=true;s.dark=false;}),
            ("provider-badges-light", |s| {provider_badge::scene(s);s.dark=false;}),
            ("watchlist",watchlist_ui::scene),
            ("package-icons-cached",package_icons::cached_scene),
            ("watchlist-saved",watchlist_ui::audited_scene),
            ("watchlist-saved-light", |s| {watchlist_ui::audited_scene(s);s.dark=false;}),
            ("watchlist-notes",watchlist_ui::notes_scene),
            ("packages-empty", |s| s.set_section(LibrarySection::Packages)),
            ("packages", package_ui::scene),
            ("packages-notes", package_ui::notes_scene),
            ("packages-observations", |s| {package_ui::scene(s);s.observation_picker=true;}),
            ("recovered-notes", recovered_notes_scene),
            ("owner-notes", owner_notes_scene),
            ("source-discovery", |s| source_loader::scene(s, false)),
            ("source-discovered", |s| source_loader::scene(s, true)),
            ("extension-notes",extension_notes_scene),
            ("editor-profiles",extension_ui::scene),
            ("editor-rich-select",|s|extension_ui::menu_scene(s,true)),
            ("profile-rich-select",|s|extension_ui::menu_scene(s,false)),
            ("profile-rich-select-light",|s|{extension_ui::menu_scene(s,false);s.dark=false;}),
            ("editor-profiles-light", |s| {extension_ui::scene(s);s.dark=false;}),
            ("editor-profiles-filtered", |s| {extension_ui::scene(s);s.extensions.editor=Some(releasescout_engine::inventory::Manager::Vscode);s.refresh_extension_filter(&mut rune::Tx::default());}),
            ("local-apps", local_apps_scene),
            ("portable-filtering", portable_filter_scene),
            ("portable-support", |s|{portable_filter_scene(s);s.toggle_components();}),
            ("extensions", extension_scene),
            ("extensions-empty", |s|{automatic_filter_scene(s);s.set_section(LibrarySection::Extensions);}),
            ("profile-observations", |s|{extension_scene(s);s.observation_picker=true;}),
            ("reconciled", reconciled_scene),
            ("reconciled-observations", |s|{reconciled_scene(s);s.observation_picker=true;}),
            ("automatic-filtering", automatic_filter_scene),
            ("automatic-components", |s|{automatic_filter_scene(s);s.toggle_components();}),
            ("grouped", grouped_scene),
            ("components-visible", |s|{grouped_scene(s);s.toggle_components();}),
            ("observations-open", |s|{grouped_scene(s);s.observation_picker=true;}),
            ("detected", |s| {
                let scan=releasescout_engine::inventory::parse(releasescout_engine::inventory::Manager::Scoop,true,&serde_json::json!({"installed":{"apps":[{"Name":"gh","Version":"2.0.0","Source":"main"},{"Name":"unmapped-fixture-tool","Version":"1.0.0","Source":"extras"}]},"updates":[{"Name":"gh","Installed Version":"2.0.0","Latest Version":"3.0.0"}]}).to_string()).unwrap();
                s.accept_scan(scan);
                let id=s.catalog.packages.iter().find(|p|p.detected && p.name=="GitHub CLI").unwrap().id.clone();s.select_package(&id);
            }),
            ("inventory", |s| {s.inventory.open=true;}),
            ("inventory-loading", |s| {s.inventory.open=true;s.inventory.loading=true;}),
            ("inventory-error", |s| {s.inventory.open=true;s.inventory.error=Some("Fixture: manager unavailable or scan timed out".into());}),
            ("inventory-results", inventory_view::scene),
            ("inventory-workspace", inventory_view::workspace_scene),
            ("inventory-context-menu", inventory_menu::scene),
            ("inventory-context-menu-light", |s| { inventory_menu::scene(s); s.dark = false; }),
            ("inventory-workspace-light",|s|{inventory_view::workspace_scene(s);s.dark=false;}),
            ("inventory-workspace-columns",|s|{inventory_view::workspace_scene(s);s.inventory.workspace.columns_open=true;}),
            ("inventory-workspace-review",|s|{inventory_view::workspace_scene(s);s.inventory.workspace.selected=inventory_workspace::records(s).into_iter().map(|r|r.key).collect();s.review_inventory_selection();}),
            ("inventory-results-light", |s|{inventory_view::scene(s);s.dark=false;}),
            ("inventory-row-details", |s|{inventory_view::scene(s);let scan=s.inventory.selected_scan().unwrap();s.inventory.details=Some(sidebar_info::check_key(scan.packages.iter().find(|p|p.id=="gh").unwrap()));}),
            ("engine-partial", |s| {
                s.select_package("gh");
                let p = s.selected().unwrap();
                let notes = p
                    .releases
                    .iter()
                    .map(|r| releasescout_engine::Note {
                        title: r.version.clone(),
                        version: Some(r.version.clone()),
                        date: r.published.get(..10).and_then(|d| d.parse().ok()),
                        url: r.url.clone(),
                        body: r.markdown.clone(),
                        authored_notes: true,
                        preview: false,
                    })
                    .collect();
                s.engine_query().caption =
                    "Captured render snapshot / installed range unknown".into();
                s.apply_engine_result(
                    "gh".into(),
                    Ok(releasescout_engine::Report {
                        package: "gh".into(),
                        stream: "main".into(),
                        scope: "GitHub CLI".into(),
                        basis: releasescout_engine::Basis::Version,
                        status: "partial".into(),
                        complete: false,
                        notes,
                        warnings: vec!["Offline snapshot; upstream freshness is unknown".into()],
                        evidence: vec![],
                        attempts: vec![],
                    }),
                );
            }),
            ("engine-range", |s| {
                s.select_package("gh");
                s.engine_query().range = true;
            }),
            ("engine-components", |s| {
                s.select_package("supabase");
                s.engine_query().component_open = true;
            }),
            ("engine-loading", |s| {
                s.select_package("gh");
                let p = s
                    .catalog
                    .packages
                    .iter_mut()
                    .find(|p| p.id == "gh")
                    .unwrap();
                p.releases.clear();
                p.sample_notes = false;
                s.live.active = Some("gh".into());
            }),
            ("engine-error", |s| {
                s.select_package("gh");
                let p = s
                    .catalog
                    .packages
                    .iter_mut()
                    .find(|p| p.id == "gh")
                    .unwrap();
                p.releases.clear();
                p.sample_notes = false;
                s.engine_query().error =
                    Some("Offline cache miss / no cached response for this source".into());
            }),
            ("notes", |s| s.select_package("gh")),
            ("notes-light", |s| { s.select_package("gh"); s.dark = false; }),
            ("readme", readme_scene),
            ("unconfirmed", |s| {
                if let Some(p) = s.catalog.packages.iter_mut().find(|p| p.id == "agent-browser") {
                    p.unconfirmed = true;
                }
                s.select_package("agent-browser");
            }),
            ("long-reference", |s| {
                if let Some(p) = s.catalog.packages.iter_mut().find(|p| p.id == "yarn") {
                    p.reference_text = Some("Synthetic inventory provenance for a package with a long source label and more than one possible installation scope.".into());
                }
                s.select_package("yarn");
            }),
            ("app", |s| s.select_package("powertoys")),
            ("removed-app", |s| {
                if let Some(p) = s.catalog.packages.iter_mut().find(|p| p.id == "audacity") {
                    p.kind = model::EntryKind::App;
                    p.removed = true;
                }
                s.select_package("audacity");
            }),
            ("dark", |s| s.dark = true),
            ("light", |s| s.dark = false),
            ("global-chrome", app_chrome::scene),
            ("update-note-activity", |s| { note_prefetch::activity_scene(s); s.chrome.status_pinned=true; }),
            ("sidebar-filters", sidebar_filters::scene),
            ("sidebar-update-colors", sidebar_info::update_colors_scene),
            ("sidebar-update-colors-light", |s| {sidebar_info::update_colors_scene(s);s.dark=false;}),
            ("sidebar-filters-saved", |s| {saved_inventory_scene(s);s.sidebar_filters_open=true;}),
            ("sidebar-filters-light", |s| {saved_inventory_scene(s);s.sidebar_filters_open=true;s.dark=false;}),
            ("global-chrome-light", |s| { app_chrome::scene(s); s.dark=false; }),
            ("collapsed", |s| s.pane.set_open(false)),
            ("stress", |s| s.set_dataset(Dataset::Stress)),
            ("source", |s| {
                s.select_package("gh");
                s.mode = NoteMode::Source;
            }),
            ("empty", |s| {
                s.query = rune::TextEdit::new("no-such-package");
                s.refilter();
            }),
        ],
        ..AppSpec::new(
            releasescout_engine::APP_NAME,
            (1280.0, 800.0),
            if native { State::new } else { State::demo },
            ui::view,
        )
    })
    .titlebar(rune::Titlebar::new(rune_kit::shell::TOOLBAR_HEIGHT))
    .tab_visits_controls(true);
    let app = native_backdrop::configure(app, native_material);
    let app = if native {
        let handle = app.handle();
        app.on_ready(move |_| {
            handle.update(|s, tx| {
                s.note_prefetch.enabled = true;
                s.startup_discovery();
                tx.instant();
            });
        })
    } else {
        app
    };
    app.run();
}

fn readme_scene(s: &mut State) {
    use releasescout_engine::{Basis, Note, Report, inventory::{Manager, parse}};
    use serde_json::json;

    let versions = [
        ("7zip", "26.00", "26.06"),
        ("ffmpeg", "8.1.1", "9.0.2"),
        ("fzf", "0.74.0", "0.75.0"),
        ("gh", "2.103.1", "2.103.3"),
        ("nodejs", "22.0.0", "24.0.0"),
        ("pnpm", "10.0.0", "10.1.0"),
        ("ripgrep", "14.1.1", "15.0.0"),
        ("uv", "0.8.1", "0.8.2"),
    ];
    let apps: Vec<_> = versions.iter().map(|(name, installed, _)| {
        json!({"Name": name, "Version": installed, "Source": "main"})
    }).collect();
    let updates: Vec<_> = versions.iter().map(|(name, installed, latest)| {
        json!({"Name": name, "Installed Version": installed, "Latest Version": latest})
    }).collect();
    let inventory = json!({"installed": {"apps": apps}, "updates": updates});
    s.accept_scan(parse(Manager::Scoop, true, &inventory.to_string()).unwrap());
    let id = s.catalog.packages.iter().find(|p| {
        p.observation.as_ref().is_some_and(|o| o.id == "gh")
    }).unwrap().id.clone();
    s.select_package(&id);
    let query = s.engine_query();
    query.range = true;
    query.from = rune::TextEdit::new("2.103.1");
    query.to = rune::TextEdit::new("2.103.3");
    query.caption = "Demo release notes".into();

    let notes = [
        ("2.103.3", "2026-10-26", "## Authentication\n\n- Explain when a token has expired instead of showing a generic error.\n- Make it easier to tell which account `gh auth status` is checking."),
        ("2.103.2", "2026-10-23", "## Fixes\n\n- Keep the selected repository when switching accounts.\n- Preserve line breaks when copying a release description."),
    ].into_iter().map(|(version, date, body)| Note {
        title: format!("v{version}"),
        version: Some(version.into()),
        date: Some(date.parse().unwrap()),
        url: "https://github.com/cli/cli/releases".into(),
        body: body.into(),
        authored_notes: true,
        preview: false,
    }).collect();
    s.apply_engine_result(id.clone(), Ok(Report {
        package: id,
        stream: "main".into(),
        scope: "Demo release notes".into(),
        basis: Basis::Version,
        status: "available".into(),
        complete: true,
        notes,
        warnings: Vec::new(),
        evidence: Vec::new(),
        attempts: Vec::new(),
    }));
}

fn saved_inventory_scene(s: &mut State) {
    let path = live::cache_directory().with_file_name("inventory.json");
    for scan in releasescout_engine::matching::load_snapshot(&path).expect("saved inventory") {
        s.accept_scan(scan);
    }
    for p in &s.catalog.packages {
        if let Some(image) = &p.icon {
            let _ = image.wait();
        }
    }
}

fn local_apps_scene(s: &mut State) {
    use releasescout_engine::inventory::{Manager, parse};
    s.accept_scan(parse(Manager::Local,false,&serde_json::json!({"installed":[
        {"Id":"ARP\\User\\X64\\local-example","Name":"Local Desktop App","Version":"4.2","Scope":"User"},
        {"Id":"ARP\\Machine\\X86\\older-example","Name":"Older Desktop App","Scope":"Machine"}
    ]}).to_string()).unwrap());
    let image = s.icons.rgba(
        2,
        2,
        vec![
            46, 170, 99, 255, 46, 170, 99, 255, 46, 170, 99, 255, 46, 170, 99, 255,
        ],
    );
    image.wait().unwrap();
    s.catalog
        .packages
        .iter_mut()
        .find(|p| p.detected && p.name == "Local Desktop App")
        .unwrap()
        .icon = Some(image);
}

fn recovered_notes_scene(s: &mut State) {
    use releasescout_engine::{
        Engine,
        http::{CacheOptions, HttpClient},
    };
    s.select_package("chatgpt-desktop");
    s.engine_query().cache = 2;
    let result = HttpClient::new(CacheOptions {
        directory: live::cache_directory(),
        offline: true,
        public_only: true,
        ..Default::default()
    })
    .and_then(|fetcher| {
        Engine {
            catalog: s.live.catalog.clone(),
            fetcher,
        }
        .history("chatgpt-desktop", None, None, false)
    })
    .map(|page| page.report)
    .map_err(|e| e.to_string());
    s.apply_engine_result("chatgpt-desktop".into(), result);
}

fn owner_notes_scene(s: &mut State) {
    use releasescout_engine::{
        Engine,
        http::{CacheOptions, HttpClient},
    };
    let scans = releasescout_engine::matching::load_snapshot(std::path::Path::new(
        "artifacts/source-coverage-audit/owner-corrections/inventory-refresh.json",
    ))
    .expect("explicit owner-notes scene requires the saved audit inventory");
    for scan in scans {
        s.accept_scan(scan);
    }
    let product =
        std::env::var("RELEASESCOUT_SCENE_PRODUCT").unwrap_or_else(|_| "angular-cli".into());
    let id = s
        .catalog
        .packages
        .iter()
        .find(|p| p.detected && p.reference_id.as_deref() == Some(&product))
        .expect("requested audited product must be present")
        .id
        .clone();
    s.select_package(&id);
    s.engine_query().cache = 2;
    let result = HttpClient::new(CacheOptions {
        directory: live::cache_directory(),
        offline: true,
        public_only: true,
        ..Default::default()
    })
    .and_then(|fetcher| {
        Engine {
            catalog: s.live.catalog.clone(),
            fetcher,
        }
        .history(&id, None, None, false)
    })
    .map(|p| p.report)
    .map_err(|e| e.to_string());
    s.apply_engine_result(id, result);
}

fn extension_notes_scene(s: &mut State) {
    use releasescout_engine::{
        Channel,
        extensions::{self, Mode},
        http::{CacheOptions, HttpClient},
        inventory::{Manager, parse},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/extension-notes");
    s.accept_scan(parse(Manager::VscodeInsiders,false,&serde_json::json!({"profiles":[{
        "id":"fixture-profile","name":"Work","root":"Z:/fixture/profile","installed":"fixture.extension@1.2.0",
        "metadata":{"fixture.extension":{"extension":{"root":root,"changelog":root.join("CHANGELOG.md"),"pre_release":false}}}
    }]}).to_string()).unwrap());
    s.set_section(LibrarySection::Extensions);
    let p = s.selected().unwrap();
    let id = p.id.clone();
    let info = p.extension.as_ref().unwrap();
    let fetch = HttpClient::new(CacheOptions {
        offline: true,
        ..Default::default()
    })
    .unwrap();
    let r = extensions::notes(
        &fetch,
        &id,
        &info.id,
        "1.2.0",
        info,
        Mode::Installed,
        Channel::Stable,
    )
    .unwrap();
    s.live.sessions.get_mut(&id).unwrap().caption = "Installed extension notes".into();
    s.apply_engine_result(id, Ok(r));
}

fn portable_filter_scene(s: &mut State) {
    use releasescout_engine::inventory::{Manager, parse};
    s.accept_scan(parse(Manager::Store,false,&serde_json::json!([
        {"Name":"NewOS.Builtin","PackageFamilyName":"NewOS.Builtin_pub","Version":"1.0","FriendlyName":"System app","Publisher":"O=Microsoft Corporation","SignatureKind":"Store","HasVisibleApp":true,"HasCliAlias":false},
        {"Name":"NewVendor.App","PackageFamilyName":"NewVendor.App_pub","Version":"1.0","FriendlyName":"Independent app","Publisher":"O=New Vendor","SignatureKind":"Store","HasVisibleApp":true,"HasCliAlias":false},
        {"Name":"NewOS.Tool","PackageFamilyName":"NewOS.Tool_pub","Version":"1.0","FriendlyName":"Command-line tool","Publisher":"O=Microsoft Corporation","SignatureKind":"Store","HasCliAlias":true},
        {"Name":"NewOEM.Config","PackageFamilyName":"NewOEM.Config_pub","Version":"1.0","FriendlyName":"Device configuration helper","HasPreinstalledTask":true,"HasCliAlias":false}
    ]).to_string()).unwrap());
    s.accept_scan(
        parse(
            Manager::Scoop,
            false,
            &serde_json::json!({"installed":{"apps":[
                {"Name":"unfamiliar-runtime","Version":"1.0","Source":"unfamiliar-bucket"},
                {"Name":"unfamiliar-data","Version":"1.0","Source":"unfamiliar-bucket"},
                {"Name":"unfamiliar-sdk","Version":"1.0","Source":"unfamiliar-bucket"},
                {"Name":"unfamiliar-cli","Version":"1.0","Source":"unfamiliar-bucket"}
            ]},"metadata":{
                "unfamiliar-runtime":{"runtimeOnly":true},
                "unfamiliar-data":{"hasCommands":false,"hasShortcuts":false,"isDependency":true},
                "unfamiliar-sdk":{"hasCommands":true,"runtimeOnly":false},
                "unfamiliar-cli":{"hasCommands":true,"isDependency":true}
            }})
            .to_string(),
        )
        .unwrap(),
    );
    s.accept_scan(parse(Manager::Pnpm,false,&serde_json::json!({"root":"Z:/portable/global","installedExit":0,
        "installed":{"dependencies":{"unfamiliar-library":{"version":"1.0"},"unfamiliar-global-cli":{"version":"1.0"}}},
        "metadata":{"unfamiliar-library":{"hasCommands":false},"unfamiliar-global-cli":{"hasCommands":true}}
    }).to_string()).unwrap());
    s.accept_scan(parse(Manager::VscodeInsiders,false,&serde_json::json!({"profiles":[
        {"id":"default","name":"Default","root":"Z:/settings/User","installed":"vendor.shared@1.0"},
        {"id":"work","name":"Work With Spaces","root":"Z:/settings/User/profiles/work","installed":"vendor.shared@2.0-pre.1\nvendor.work@3.0"},
        {"id":"inherited","name":"Inherited","root":"Z:/settings/User/profiles/inherited","installed":"vendor.shared@1.0"}
    ]}).to_string()).unwrap());
    let id = s
        .catalog
        .packages
        .iter()
        .find(|p| p.detected && p.name == "Independent app")
        .unwrap()
        .id
        .clone();
    s.select_package(&id);
}

fn extension_scene(s: &mut State) {
    portable_filter_scene(s);
    s.set_section(LibrarySection::Extensions);
    let id = s
        .catalog
        .packages
        .iter()
        .find(|p| p.detected && p.name == "vendor.shared")
        .unwrap()
        .id
        .clone();
    s.select_package(&id);
}

fn reconciled_scene(s: &mut State) {
    use releasescout_engine::inventory::{Manager, parse};
    let choco = parse(
        Manager::Chocolatey,
        false,
        &serde_json::json!({"installed":
            "cursoride|3.10.20\npython311|3.11.9\npython314|3.14.2"
        })
        .to_string(),
    )
    .unwrap();
    let store = parse(
        Manager::Store,
        false,
        &serde_json::json!([
            {"Name":"Claude","PackageFamilyName":"Claude_pzs8sxrjxfjjc","Version":"3.6556.2.0"}
        ])
        .to_string(),
    )
    .unwrap();
    let winget = parse(
        Manager::Winget,
        false,
        &serde_json::json!({"installed":{"Sources":[
            {"SourceDetails":{"Name":"winget"},"Packages":[
                {"PackageIdentifier":"Anysphere.Cursor","Version":"3.10.20"},
                {"PackageIdentifier":"Python.Python.3.14","Version":"3.14.2"},
                {"PackageIdentifier":"Anthropic.Claude","Version":"3.6556.2.0"}
            ]}
        ]}})
        .to_string(),
    )
    .unwrap();
    for scan in [choco, store, winget] {
        s.accept_scan(scan);
    }
    let id = s
        .catalog
        .packages
        .iter()
        .find(|p| p.detected && p.reference_id.as_deref() == Some("claude-desktop"))
        .unwrap()
        .id
        .clone();
    s.select_package(&id);
}

fn grouped_scene(s: &mut State) {
    use releasescout_engine::inventory::{Manager, parse};
    let choco=parse(Manager::Chocolatey,false,&serde_json::json!({"installed":"audacity|3.7.4\nvlc|3.0.21\nvlc.install|3.0.21\nhelper.extension|1.0","metadata":{"vlc":{"wrapperTarget":"vlc.install"},"helper.extension":{"tags":"chocolatey extension"}}}).to_string()).unwrap();
    let store=parse(Manager::Store,false,&serde_json::json!([{"Name":"Fixture.System","PackageFamilyName":"Fixture.System_pub","Version":"1.0","NonRemovable":true,"SignatureKind":"System"},{"Name":"Fixture.OrdinaryApp","PackageFamilyName":"Fixture.OrdinaryApp_pub","Version":"1.0","NonRemovable":false,"SignatureKind":"Store"}]).to_string()).unwrap();
    let winget=parse(Manager::Winget,false,&serde_json::json!({"installed":{"Sources":[{"SourceDetails":{"Name":"winget"},"Packages":[{"PackageIdentifier":"Audacity.Audacity","Version":"3.7.5"}]}]}}).to_string()).unwrap();
    for scan in [choco, store, winget] {
        s.accept_scan(scan);
    }
    let id = s
        .catalog
        .packages
        .iter()
        .find(|p| p.detected && p.name == "Audacity" && p.installed.as_deref() == Some("3.7.4"))
        .unwrap()
        .id
        .clone();
    s.select_package(&id);
}

fn automatic_filter_scene(s: &mut State) {
    use releasescout_engine::inventory::{Manager, parse};
    s.accept_scan(parse(Manager::Store,false,&serde_json::json!([
        {"Name":"Vendor.Codec","PackageFamilyName":"Vendor.Codec_pub","Version":"1.0","NonRemovable":false,"HasVisibleApp":false,"HasExecutionAlias":false},
        {"Name":"Vendor.TechnicalID","FriendlyName":"PDF Reader by Xodo","PackageFamilyName":"Vendor.TechnicalID_pub","Version":"6.3.4","HasVisibleApp":true,"HasExecutionAlias":false},
        {"Name":"Vendor.Command","FriendlyName":"example.exe","PackageFamilyName":"Vendor.Command_pub","Version":"2.0","HasVisibleApp":false,"HasExecutionAlias":true}
    ]).to_string()).unwrap());
    s.accept_scan(parse(Manager::Winget,false,&serde_json::json!({"installed":{"Sources":[{"SourceDetails":{"Name":"winget"},"Packages":[{"PackageIdentifier":"Microsoft.UI.Xaml.2.8","Version":"8.0"},{"PackageIdentifier":"Python.Python.3.14","Version":"3.14.0"}]}]}}).to_string()).unwrap());
    let id = s
        .catalog
        .packages
        .iter()
        .find(|p| p.detected && p.name == "PDF Reader by Xodo")
        .unwrap()
        .id
        .clone();
    s.select_package(&id);
}
