use super::{
    model::{Dataset, EntryKind, LibrarySection, State},
    ui,
};
use rune::{Env, Key, KeyEvent, Mods, Runtime, TextEdit, WheelEvent};
use rune_kit::SortDir;

fn runtime(size: (f32, f32)) -> Runtime<State> {
    runtime_with(State::demo(), size)
}

fn runtime_with(state: State, size: (f32, f32)) -> Runtime<State> {
    let mut rt = Runtime::with_text(
        state,
        ui::view,
        size,
        Env {
            system_dark: false,
            scale: 1.0,
        },
        rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES),
    );
    rt.on_key = Some(ui::on_key);
    rt.frame(0.0);
    rt
}

fn tap(rt: &mut Runtime<State>, key: &str) {
    let (x, y) = rt
        .find(key)
        .unwrap_or_else(|| panic!("missing element: {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
}

fn inventory_key(s: &State, id: &str) -> String {
    crate::inventory_workspace::records(s)
        .into_iter()
        .find(|r| r.package.id == id)
        .unwrap()
        .key
}

fn choose_observation(rt: &mut Runtime<State>, index: usize) {
    let id = rt.state.selected_observations()[index].id.clone();
    tap(rt, &format!("selected-observation-option-{id}"));
}

fn toggle_system(rt: &mut Runtime<State>) {
    let was_open = rt.state.sidebar_filters_open;
    if !was_open {
        tap(rt, "sidebar-filters-trigger");
        rt.frame(1.);
    }
    tap(rt, "show-components");
    if !was_open {
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(1.);
    }
}

fn tap_menu_item(rt: &mut Runtime<State>, label: &str) {
    let key = rt
        .elements()
        .iter()
        .find(|e| e.role == "menuitem" && e.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("Missing menu item: {label}"))
        .key
        .clone()
        .unwrap();
    tap(rt, &key);
}

#[test]
fn chronver_branding_is_visible_with_either_sidebar_state() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        for inventory in [false, true] {
            for collapsed in [false, true] {
                let mut rt = runtime(size);
                rt.update(|s, _| {
                    s.dark = dark;
                    s.inventory.open = inventory;
                    s.pane.set_open(!collapsed);
                    s.inventory.workspace.pane.set_open(!collapsed);
                });
                rt.frame(1.);
                let elements = rt.elements();
                let brand = elements
                    .iter()
                    .find(|e| e.text.as_deref() == Some("Chronver"))
                    .expect("Chronver is visible in the app header");
                assert!(brand.rect[2] > 0. && brand.rect[3] > 0.);
                assert!(brand.rect[0] >= 0. && brand.rect[0] + brand.rect[2] <= size.0);
                assert!(!elements.iter().any(|e| {
                    matches!(e.text.as_deref(), Some("ReleaseScout" | "Versionaut"))
                }));
            }
        }
    }
}

#[test]
fn dropdowns_share_comfortable_rows_and_ai_icons_in_both_themes() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, _| {
            crate::ai_menu::scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        let elements = rt.elements();
        let items: Vec<_> = elements.iter().filter(|e| e.role == "menuitem").collect();
        let icons: Vec<_> = elements
            .iter()
            .filter(|e| e.key.as_deref() == Some("menu-action-icon"))
            .collect();
        assert_eq!(items.len(), 7);
        assert_eq!(icons.len(), items.len());
        for (item, icon) in items.iter().zip(&icons) {
            assert!((item.rect[3] - 32.).abs() < 0.5, "{:?}", item.rect);
            assert_eq!([icon.rect[2], icon.rect[3]], [16., 16.]);
            assert!((icon.rect[0] - icons[0].rect[0]).abs() < 0.5);
            assert!((icon.rect[1] + 8. - item.rect[1] - item.rect[3] / 2.).abs() < 0.5);
        }
        let menu = elements.iter().find(|e| e.role == "menu").unwrap();
        assert!(
            menu.rect[2] >= 256. && menu.rect[2] <= 260.,
            "AI labels must not be squeezed: {:?}",
            menu.rect
        );
        assert_eq!(menu.radius, 6.);
        let title = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("ai-menu-title"))
            .unwrap();
        let title_icon = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("ai-menu-title-icon"))
            .unwrap();
        let title_label = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("ai-menu-title-label"))
            .unwrap();
        assert!(!title.focusable, "the title must not be a menu action");
        assert_eq!([title_icon.rect[2], title_icon.rect[3]], [16., 16.]);
        assert!(title_label.rect[0] >= title_icon.rect[0] + 24.);
        assert!(title_label.rect[0] + title_label.rect[2] <= menu.rect[0] + menu.rect[2]);
        assert!(menu.rect[0] >= 0. && menu.rect[1] >= 0.);
        assert!(menu.rect[0] + menu.rect[2] <= size.0);
        assert!(menu.rect[1] + menu.rect[3] <= size.1);
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(1.);
    }
}

#[test]
fn ai_actions_are_always_visible_and_open_without_sending_requests() {
    for dark in [false, true] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s, _| {
            crate::sidebar_info::scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        let names: Vec<_> = rt
            .state
            .catalog
            .packages
            .iter()
            .map(|p| (p.name.clone(), p.description.clone()))
            .collect();
        let prefs = rt.state.activity.data.description_overrides.clone();
        let row = rt.state.rows[rt.state.visible[0]].id.clone();
        let trigger = {
            let r = rt.state.rows.iter().find(|r| r.id == row).unwrap();
            format!("{}-trigger", crate::ai_menu::key(r))
        };
        let selected = rt.state.selected().unwrap().id.clone();
        for intent in crate::ai_menu::Intent::ALL {
            let el = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(&trigger))
                .unwrap();
            assert_eq!(el.opacity, 1., "AI must not require hover or selection");
            tap(&mut rt, &trigger);
            let elements = rt.elements();
            let menu = elements.iter().find(|e| e.role == "menu").unwrap();
            assert!(
                menu.rect[0] >= 0.
                    && menu.rect[1] >= 0.
                    && menu.rect[0] + menu.rect[2] <= 1024.
                    && menu.rect[1] + menu.rect[3] <= 640.
            );
            for action in crate::ai_menu::Intent::ALL {
                assert!(elements.iter().any(|e|e.role=="menuitem" && e.label.as_deref()==Some(action.label())));
            }
            tap_menu_item(&mut rt, intent.label());
            rt.frame(1.);
            assert_eq!(rt.state.ai.preview.as_ref().unwrap().intent, intent);
            assert!(rt.find("ai-action-preview-dialog").is_some());
            tap(&mut rt, "ai-action-close");
            rt.frame(1.);
        }
        tap(&mut rt, &trigger);
        rt.key(KeyEvent {
            key: Key::End,
            mods: Mods::default(),
        });
        rt.frame(1.);
        assert!(rt.elements().iter().any(|e| e.role == "menuitem"
            && e.label.as_deref() == Some("Copy AI context")
            && e.focused));
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(1.);
        assert!(rt.state.ai.open.is_none());
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some(&trigger) && e.focused)
        );
        assert_eq!(rt.state.selected().unwrap().id, selected);
        assert_eq!(
            rt.state
                .catalog
                .packages
                .iter()
                .map(|p| (p.name.clone(), p.description.clone()))
                .collect::<Vec<_>>(),
            names
        );
        assert_eq!(rt.state.activity.data.description_overrides, prefs);
        assert!(!rt.state.live.work.is_pending() && !rt.state.live.history.is_pending());
        assert!(!rt.state.inventory.work.is_pending());
        assert!(!rt.state.ai.work.is_pending());
    }
}

#[test]
fn ai_key_entry_stays_masked_and_saving_does_not_submit() {
    use crate::ai_menu::{Intent, Preview};
    let mut rt = runtime((1024., 640.));
    rt.update(|s, _| {
        let p = s.selected().unwrap();
        let (id, name) = (p.id.clone(), p.name.clone());
        s.engine_query().report = Some(crate::ai_summary::tests::report());
        s.ai.preview = Some(Preview { package: id, name, intent: Intent::Latest });
    });
    rt.frame(1.);
    tap(&mut rt, "ai-configure");
    rt.frame(1.);
    tap(&mut rt, "ai-key-field");
    rt.paste("sk-fixture-not-a-real-key");
    rt.frame(1.);
    assert!(!rt.accessibility_json().to_string().contains("sk-fixture-not-a-real-key"));
    tap(&mut rt, "ai-save-key");
    rt.frame(1.);
    assert!(rt.find("ai-generate").is_some());
    assert!(!rt.state.ai.work.is_pending());
    assert!(rt.accessibility_json().to_string().contains("GPT-6 Luna"));
    tap(&mut rt, "ai-configure");
    rt.frame(1.);
    tap(&mut rt, "ai-provider-option-claude");
    rt.frame(1.);
    assert!(rt.accessibility_json().to_string().contains("claude-haiku-5-5"));
    tap(&mut rt, "ai-save-key");
    rt.frame(1.);
    assert!(rt.find("ai-settings-error").is_some(), "a different provider needs its own key");
    tap(&mut rt, "ai-key-field");
    rt.paste("fixture-claude-key");
    rt.frame(1.);
    tap(&mut rt, "ai-save-key");
    rt.frame(1.);
    assert!(rt.accessibility_json().to_string().contains("api.anthropic.com"));
    assert!(!rt.state.ai.work.is_pending());
    tap(&mut rt, "ai-configure");
    rt.frame(1.);
    tap(&mut rt, "ai-forget-key");
    rt.frame(1.);
    assert!(rt.find("ai-configure").is_some());
    assert!(rt.find("ai-generate").is_none());
}

#[test]
fn ai_settings_are_available_from_the_header_without_loaded_notes() {
    for dark in [true, false] {
        let mut rt = runtime((760., 500.));
        rt.update(|s, _| s.dark = dark);
        rt.frame(1.);
        tap(&mut rt, "global-ai-settings");
        rt.frame(1.);
        assert!(rt.find("ai-settings-dialog").is_some());
        tap(&mut rt, "ai-provider-option-custom");
        rt.frame(1.);
        assert!(rt.find("ai-endpoint-field").is_some());
        assert!(rt.find("ai-save-key").is_some());
        rt.key(KeyEvent { key: Key::Escape, mods: Mods::default() });
        rt.frame(1.);
        assert!(rt.find("ai-settings-dialog").is_none());
        assert!(!rt.state.ai.work.is_pending());
    }
}

#[test]
fn ai_settings_searches_models_inline_without_sending_a_request() {
    for dark in [true, false] {
        let mut rt = runtime((760., 500.));
        rt.update(|s, _| s.dark = dark);
        rt.frame(1.);
        tap(&mut rt, "global-ai-settings");
        rt.frame(1.);
        let layout = rt.elements();
        let dialog = layout.iter().find(|e| e.key.as_deref() == Some("ai-settings-dialog")).unwrap();
        assert!((dialog.rect[2] - 480.).abs() < 1. && dialog.rect[3] < 275.);
        assert!(rt.find("ai-provider-rail").is_some());
        tap(&mut rt, "ai-model-search");
        rt.paste("6.2 sol");
        rt.frame(1.);
        assert!(rt.accessibility_json().to_string().contains("gpt-6.2-sol"));
        assert!(rt.find("ai-model-option-0").is_none());
        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        rt.frame(1.);
        tap(&mut rt, "ai-manual-model");
        rt.frame(1.);
        assert!(rt.find("ai-model-field").is_some());
        assert!(rt.accessibility_json().to_string().contains("gpt-6.2-sol"));
        tap(&mut rt, "ai-provider-option-gemini");
        rt.frame(1.);
        assert!(rt.find("ai-model-search").is_some());
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains("gemini-4-hydrogen"));
        assert!(!tree.contains("gpt-6.2-sol"));
        assert!(!rt.state.ai.work.is_pending());
    }
}

#[test]
fn ai_key_field_tracks_the_provider_being_configured() {
    let mut rt = runtime((760., 500.));
    tap(&mut rt, "global-ai-settings");
    rt.frame(1.);
    for (provider, label) in [
        ("openai", "OpenAI API key"),
        ("claude", "Anthropic API key"),
        ("gemini", "Google AI API key"),
        ("custom", "Custom endpoint API key"),
    ] {
        tap(&mut rt, &format!("ai-provider-option-{provider}"));
        rt.frame(1.);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains(label), "{provider}: missing {label}");
        assert!(rt.find("ai-key-field").is_some());
    }
    assert!(!rt.state.ai.work.is_pending());
}

#[test]
fn ai_explicit_submit_displays_cited_summary_from_custom_endpoint() {
    use crate::model::NoteMode;
    use crate::ai_summary::tests::read_mock_request;
    use std::{
        io::Write,
        net::TcpListener,
        time::{Duration, Instant},
    };

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let mut rt = runtime((850., 560.));
    rt.update(|s, _| {
        let package = s.selected().unwrap().id.clone();
        let mut report = crate::ai_summary::tests::report();
        report.package = package.clone();
        report.notes[1].body.push_str("\nThe legacy export flag was removed.");
        s.apply_engine_result(package, Ok(report));
    });
    rt.frame(1.);
    assert!(rt.find("release-document").is_some());
    tap(&mut rt, "note-mode/segment2");
    rt.frame(1.);
    assert!(rt.state.mode == NoteMode::Ai);
    assert!(rt.find("timeline-item-0").is_none());
    ai_reader_action(&mut rt, 0);
    assert!(rt.find("ai-input-estimate").is_some());
    tap(&mut rt, "ai-configure");
    rt.frame(1.);
    tap(&mut rt, "ai-provider-option-custom");
    rt.frame(1.);
    tap(&mut rt, "ai-model-field");
    rt.paste("mock-model");
    rt.frame(1.);
    tap(&mut rt, "ai-endpoint-field");
    rt.paste(url.clone());
    rt.frame(1.);
    tap(&mut rt, "ai-save-key");
    rt.frame(1.);
    assert!(rt.find("ai-generate").is_some());
    assert!(!rt.state.ai.work.is_pending());
    let tree = rt.accessibility_json().to_string();
    assert!(tree.contains(&format!("{url}/chat/completions")));
    assert!(tree.contains("Send & summarize"));
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (headers, request) = read_mock_request(&mut stream);
        assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        assert_eq!(request["model"], "mock-model");
        let summary = serde_json::json!({
            "highlights": [{"release": 0, "text": "**Offline export** improved.", "evidence": "The exporter now works offline."}],
            "breaking_changes": [{"release": 0, "text": "The legacy export flag was removed.", "evidence": "The legacy export flag was removed."}]
        });
        let response = serde_json::json!({
            "choices": [{"finish_reason": "stop", "message": {"content": summary.to_string()}}]
        }).to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
    });
    tap(&mut rt, "ai-generate");
    let deadline = Instant::now() + Duration::from_secs(5);
    while rt.state.ai.work.is_pending() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    rt.update(|s, _| { s.ai.poll(); });
    rt.frame(1.);
    assert!(rt.find("ai-summary").is_some());
    let tree = rt.accessibility_json().to_string();
    assert!(tree.contains("Offline export improved."));
    assert!(tree.contains("1 release reviewed"));
    assert!(tree.contains("mock-model"));
    let layout = rt.elements();
    let item = |key| layout.iter().find(|e| e.key.as_deref() == Some(key)).unwrap().rect;
    let text_rect = item("ai-text-breaking-0");
    let source = item("ai-source-breaking-0");
    assert!((text_rect[1] - source[1]).abs() < 1.);
    assert!(text_rect[0] + text_rect[2] <= source[0]);
    assert!(source[2] <= 126.);
    assert!(item("ai-section-breaking")[1] < item("ai-section-highlights")[1]);
    assert!(layout.iter().any(|e| e.role == "button" && e.label.as_deref() == Some("Open release 2.0.0")));
    tap(&mut rt, "note-mode/segment0");
    rt.frame(1.);
    assert!(rt.find("timeline-item-0").is_some());
    assert!(rt.find("ai-summary").is_none());
    tap(&mut rt, "note-mode/segment2");
    assert!(rt.find("ai-summary").is_some());
    server.join().unwrap();
}

fn ai_reader_action(rt: &mut Runtime<State>, index: usize) {
    tap(rt, "ai-reader-actions-trigger");
    rt.frame(1.);
    assert!(rt.find("ai-reader-actions-menu").is_some());
    tap(rt, &format!("ai-reader-actions-item{index}"));
    rt.frame(1.);
    assert!(rt.find("ai-reader-actions-menu").is_none());
}

#[test]
fn ai_confirmation_layout_keeps_review_details_above_the_send_button() {
    for dark in [true, false] {
        let mut rt = runtime((680., 500.));
        rt.update(|s, _| {
            crate::ai_menu::preview_scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        let layout = rt.elements();
        let item = |key| layout.iter().find(|e| e.key.as_deref() == Some(key)).unwrap().rect;
        let count = item("ai-preview-release-count");
        let tokens = item("ai-input-estimate");
        let destination = item("ai-preview-destination");
        let send = item("ai-generate");
        assert!(count[1] + count[3] <= tokens[1]);
        assert!(tokens[1] + tokens[3] <= destination[1]);
        assert!(destination[1] + destination[3] <= send[1]);
        let tree = rt.accessibility_json().to_string();
        for expected in ["1 release", "GPT-6 Luna", "api.openai.com", "Send & summarize"] {
            assert!(tree.contains(expected), "missing {expected}");
        }
        assert!(!rt.state.ai.work.is_pending());
    }
}

#[test]
fn ai_summary_keeps_the_confirmation_open_when_notes_become_available() {
    let mut rt = runtime((1024., 640.));
    rt.update(|s, _| {
        s.mode = crate::model::NoteMode::Ai;
        let id = s.selected().unwrap().id.clone();
        s.engine_query().report = None;
        s.ai.preview = None;
        assert!(s.live.sessions[&id].report.is_none());
    });
    rt.frame(1.);
    ai_reader_action(&mut rt, 0);
    assert!(rt.find("ai-load-range").is_some());
    assert!(rt.find("ai-input-estimate").is_none());
    rt.update(|s, _| {
        let package = s.selected().unwrap().id.clone();
        let mut report = crate::ai_summary::tests::report();
        report.package = package.clone();
        s.apply_engine_result(package, Ok(report));
    });
    rt.frame(1.);
    assert!(rt.find("ai-action-preview-dialog").is_some());
    assert!(rt.find("ai-input-estimate").is_some());
    assert!(!rt.state.ai.work.is_pending());
}

#[test]
fn ai_breaking_changes_previews_all_batches_before_sending_anything() {
    let mut rt = runtime((1024., 640.));
    rt.update(|s, _| {
        s.mode = crate::model::NoteMode::Ai;
        let package = s.selected().unwrap().id.clone();
        let mut report = crate::ai_summary::tests::report();
        report.package = package.clone();
        report.notes.remove(0);
        report.notes[0].body = "A long published note with many changes.\n\n".repeat(900);
        s.apply_engine_result(package, Ok(report));
    });
    rt.frame(1.);
    ai_reader_action(&mut rt, 3);
    assert!(rt.find("ai-input-estimate").is_some());
    assert!(rt.accessibility_json().to_string().contains("2 requests"));
    assert!(!rt.state.ai.work.is_pending());
}

#[test]
fn sidebar_collapsible_groups_support_keyboard_selection_and_filters_in_both_themes() {
    for dark in [false, true] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s, _| {
            crate::watchlist_ui::scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        let selected = rt.state.selected().unwrap().id.clone();
        let core = "sidebar-group-packages-core-toggle";
        assert!(rt.find(core).is_some());
        tap(&mut rt, core);
        assert_eq!(crate::sidebar_groups::shown(&rt.state).len(), 1);
        assert_eq!(
            rt.state.selected().unwrap().id,
            selected,
            "Collapsing must not navigate the reader"
        );
        rt.update(|_, tx| tx.focus(core, false));
        rt.frame(1.);
        rt.key(KeyEvent {
            key: Key::Right,
            mods: Mods::default(),
        });
        rt.frame(1.);
        assert_eq!(crate::sidebar_groups::shown(&rt.state).len(), 3);
        rt.key(KeyEvent {
            key: Key::Left,
            mods: Mods::default(),
        });
        rt.frame(1.);
        assert_eq!(crate::sidebar_groups::shown(&rt.state).len(), 1);
        let row = crate::sidebar_groups::shown(&rt.state)[0];
        let id = rt.state.rows[row].id.clone();
        let key = format!("sidebar-check-{}", crate::activity::hash(&id));
        let (x, y) = rt.find(&key).unwrap();
        rt.pointer_move(x, y);
        rt.frame(1.);
        tap(&mut rt, &key);
        rt.frame(1.);
        assert_eq!(
            rt.state.sidebar_actions.selected.len(),
            1,
            "Select visible must exclude collapsed entries"
        );
        rt.update(|_, tx| tx.focus("packages", false));
        rt.frame(1.);
        rt.key(KeyEvent {
            key: Key::End,
            mods: Mods::default(),
        });
        rt.frame(1.);
        assert_eq!(rt.state.selected().unwrap().name, "typescript");
        rt.update(|s, _| {
            s.query = TextEdit::new("next");
            s.refilter();
        });
        rt.frame(1.);
        assert_eq!(crate::sidebar_groups::shown(&rt.state).len(), 1);
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some(core) && e.expanded == Some(true))
        );
        rt.update(|s, _| {
            s.query = TextEdit::new("");
            s.refilter();
        });
        rt.frame(1.);
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some(core) && e.expanded == Some(false))
        );
        rt.update(|s, _| {
            crate::extension_ui::scene(s);
        });
        rt.frame(1.);
        let headers: Vec<_> = rt
            .state
            .sidebar_groups
            .items
            .iter()
            .filter_map(|i| {
                if let crate::sidebar_groups::Item::Header { id, .. } = i {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(headers.len(), 6);
        let first = rt
            .elements()
            .iter()
            .find(|e| {
                e.key
                    .as_deref()
                    .is_some_and(|k| headers.iter().any(|id| k == format!("{id}-toggle")))
            })
            .unwrap()
            .key
            .clone()
            .unwrap();
        let count = crate::sidebar_groups::shown(&rt.state).len();
        tap(&mut rt, &first);
        assert!(crate::sidebar_groups::shown(&rt.state).len() < count);
        assert!(!rt.state.inventory.work.is_pending());
    }
}

#[test]
fn extension_group_headers_compose_editor_and_real_profile_icons_without_losing_accessibility() {
    for dark in [true, false] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s, tx| {
            crate::extension_ui::scene(s);
            s.dark = dark;
            s.extensions.editor = Some(releasescout_engine::inventory::Manager::VscodeInsiders);
            for p in &mut s.catalog.packages {
                if crate::extension_ui::profile_name(p) == "Work" {
                    p.extension.as_mut().unwrap().profile_name =
                        Some(format!("Work {}", "a long profile name ".repeat(12)));
                }
            }
            s.refresh_extension_filter(tx);
        });
        rt.frame(1.);
        let headers: Vec<_> = rt
            .state
            .sidebar_groups
            .items
            .iter()
            .filter_map(|item| {
                if let crate::sidebar_groups::Item::Header { id, .. } = item {
                    Some(format!("{id}-toggle"))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(headers.len(), 2);
        let elements = rt.elements();
        let mut glyphs = std::collections::BTreeSet::new();
        for id in &headers {
            let header = elements
                .iter()
                .find(|e| e.key.as_ref() == Some(id))
                .unwrap();
            assert!(
                header
                    .label
                    .as_deref()
                    .unwrap()
                    .contains("VS Code Insiders / ")
            );
            let child = |key: &str| {
                elements
                    .iter()
                    .find(|e| {
                        e.key.as_deref() == Some(key)
                            && e.rect[1] >= header.rect[1]
                            && e.rect[1] + e.rect[3] <= header.rect[1] + header.rect[3]
                    })
                    .unwrap()
            };
            let provider = child("extension-group-editor");
            let profile = child("extension-group-profile-icon");
            let name = child("extension-group-profile-name");
            assert_eq!(provider.rect[2], 14.);
            assert_eq!(profile.rect[2], 14.);
            assert!(provider.rect[0] + provider.rect[2] < profile.rect[0]);
            assert!(profile.rect[0] + profile.rect[2] < name.rect[0]);
            assert!(name.rect[0] + name.rect[2] < header.rect[0] + header.rect[2] - 20.);
            assert!(!name.text.as_deref().unwrap().starts_with("VS Code"));
            glyphs.insert(profile.text.clone().unwrap());
        }
        assert_eq!(
            glyphs.len(),
            2,
            "the assigned profile icon must differ from the default"
        );
        let before = crate::sidebar_groups::shown(&rt.state).len();
        tap(&mut rt, &headers[0]);
        assert!(crate::sidebar_groups::shown(&rt.state).len() < before);
        rt.key(KeyEvent {
            key: Key::Right,
            mods: Mods::default(),
        });
        rt.frame(1.);
        assert_eq!(crate::sidebar_groups::shown(&rt.state).len(), before);
    }
}

#[test]
fn every_sidebar_entry_has_an_info_button_without_hover_or_selection() {
    for scene in [
        crate::sidebar_info::scene as fn(&mut State),
        crate::extension_ui::scene,
        crate::watchlist_ui::scene,
    ] {
        for dark in [true, false] {
            let mut rt = runtime((1024., 640.));
            rt.update(|s, _| {
                scene(s);
                s.dark = dark;
            });
            rt.frame(1.);
            let elements = rt.elements();
            let mut buttons = Vec::new();
            for r in &rt.state.rows {
                if let Some(row) = elements.iter().find(|e| e.key.as_deref() == Some(&r.id)) {
                    let key = format!("sidebar-info-{}", crate::activity::hash(&r.id));
                    let info = elements
                        .iter()
                        .find(|e| e.key.as_deref() == Some(&key))
                        .expect("every mounted row needs an info button");
                    assert_eq!(info.role, "button");
                    assert!(
                        info.rect[0] >= row.rect[0]
                            && info.rect[0] + info.rect[2] <= row.rect[0] + row.rect[2] + 0.5
                    );
                    buttons.push((r.id.clone(), key));
                }
            }
            assert!(!buttons.is_empty());
            let selected = rt.state.table.cursor().map(str::to_owned);
            let viewport = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("viewport"))
                .unwrap()
                .rect;
            let (row, key) = buttons
                .iter()
                .find(|(_, key)| {
                    let info = elements
                        .iter()
                        .find(|e| e.key.as_deref() == Some(key))
                        .unwrap();
                    info.rect[1] >= viewport[1]
                        && info.rect[1] + info.rect[3] <= viewport[1] + viewport[3]
                })
                .unwrap();
            tap(&mut rt, key);
            let panel = rt.state.sidebar_actions.panel.as_ref().unwrap();
            assert_eq!(&panel.row, row);
            assert_eq!(panel.mode, crate::sidebar_actions::Mode::Details);
            assert_eq!(
                rt.state.table.cursor(),
                selected.as_deref(),
                "Info must not navigate the reader"
            );
            rt.key(KeyEvent {
                key: Key::Escape,
                mods: Mods::default(),
            });
            rt.frame(2.);
            assert!(rt.state.sidebar_actions.panel.is_none());
            assert!(
                rt.elements()
                    .iter()
                    .any(|e| e.key.as_deref() == Some(key) && e.focused),
                "Focus should return to {key}; focused: {:?}",
                rt.elements()
                    .iter()
                    .filter(|e| e.focused)
                    .map(|e| &e.key)
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn info_and_version_popovers_bound_long_descriptions_until_explicitly_expanded() {
    for mode in [
        crate::sidebar_actions::Mode::Details,
        crate::sidebar_actions::Mode::Versions,
    ] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s,_| {
            crate::sidebar_actions::scene(s,mode);
            let id=s.sidebar_actions.panel.as_ref().unwrap().package.clone();
            s.catalog.packages.iter_mut().find(|p|p.id==id).unwrap().description=
                Some("# Package documentation\n\n**Install and configure** this package for development.\n\n- A supported workflow\n- Another workflow\n\n".repeat(30));
        });
        rt.frame(1.);
        let description = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("sidebar-panel-description"))
            .unwrap();
        assert!(
            description.rect[3] <= 36.,
            "Collapsed description too tall: {:?}",
            description.rect
        );
        assert!(rt.find("sidebar-description-toggle").is_some());
        tap(&mut rt, "sidebar-description-toggle");
        let description = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("sidebar-description-scroll"))
            .unwrap();
        assert!(
            description.rect[3] <= 120.,
            "Expanded description must not dominate the popover"
        );
        tap(&mut rt, "sidebar-description-toggle");
        assert!(rt.find("sidebar-description-scroll").is_none());
        assert!(!rt.state.live.work.is_pending());
    }
}

#[test]
fn sidebar_info_is_structured_and_keeps_technical_metadata_on_demand() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|s, _| {
            crate::sidebar_actions::scene(s, crate::sidebar_actions::Mode::Details);
            s.dark = dark;
            let id = s.sidebar_actions.panel.as_ref().unwrap().package.clone();
            let p = s.catalog.packages.iter_mut().find(|p| p.id == id).unwrap();
            p.observation.as_mut().unwrap().root = Some("C:/packages/gh".into());
            p.description = Some("**Command line tools** for working with repositories, issues, pull requests and automation. Read the complete description for all of the supported workflows and configuration options.".into());
        });
        rt.frame(1.);
        assert!(rt.find("package-info-installed").is_some());
        assert!(rt.find("package-info-candidate").is_some());
        assert!(rt.find("package-info-notes").is_some());
        assert!(rt.find("package-info-technical").is_none());
        assert!(rt.find("package-info-location").is_none());
        tap(&mut rt, "sidebar-description-toggle");
        assert!(rt.find("sidebar-panel-description").is_some());
        tap(&mut rt, "sidebar-description-toggle");
        tap(&mut rt, "package-info-technical-toggle");
        assert!(rt.state.sidebar_actions.panel.as_ref().unwrap().technical);
        assert!(rt.find("package-info-id-copy").is_some());
        let json = rt.accessibility_json().to_string();
        assert!(json.contains("C:/packages/gh"));
        let [x, y, w, h] = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("sidebar-panel"))
            .unwrap()
            .rect;
        assert!(x >= 0. && y >= 0. && x + w <= size.0 && y + h <= size.1);
        // Copy remains an ordinary user action; expanding metadata starts no work.
        assert!(clipboard.text().is_none());
        assert!(!rt.state.inventory.work.is_pending());
        assert!(!rt.state.live.work.is_pending());
        assert!(!rt.state.live.history.is_pending());
        rt.wheel_event(
            WheelEvent::new(0., -500.)
                .at(x + w / 2., y + h / 2.)
                .precise(true),
        );
        rt.frame(2.);
        rt.frame(3.);
        tap(&mut rt, "package-info-id-copy");
        assert_eq!(clipboard.text().unwrap().as_str(), "gh");
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(4.);
        assert!(rt.state.sidebar_actions.panel.is_none());
    }
}

#[test]
fn sidebar_details_versions_selection_and_review_actions_are_real_and_read_only() {
    use crate::sidebar_actions::{self, Mode};
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|s, _| {
            sidebar_actions::scene(s, Mode::Versions);
            s.dark = dark;
        });
        rt.frame(1.);
        let id = rt.state.selected().unwrap().id.clone();
        let row_id = rt.state.table.cursor().unwrap().to_string();
        let versions_key = sidebar_actions::anchor(
            rt.state.rows.iter().find(|r| r.id == row_id).unwrap(),
            Mode::Versions,
        );
        let details_key = sidebar_actions::anchor(
            rt.state.rows.iter().find(|r| r.id == row_id).unwrap(),
            Mode::Details,
        );
        let bounds = |rt: &Runtime<State>, key: &str| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
                .rect
        };
        let [x, y, w, h] = bounds(&rt, "sidebar-panel");
        assert!(
            x >= 0. && y >= 0. && x + w <= size.0 && y + h <= size.1,
            "{x} {y} {w} {h}"
        );
        let a = bounds(&rt, "versions-column-installed");
        let b = bounds(&rt, "versions-column-available");
        assert!(a[0] + a[2] < b[0]);
        assert!(rt.accessibility_json().to_string().contains("2.0.5"));
        assert!(!rt.state.live.work.is_pending() && !rt.state.live.history.is_pending());
        tap(&mut rt, "sidebar-panel-close");
        tap(&mut rt, &details_key);
        assert_eq!(
            rt.state.sidebar_actions.panel.as_ref().unwrap().mode,
            Mode::Details
        );
        assert!(rt.find("sidebar-panel-description").is_some());
        tap(&mut rt, "sidebar-panel-ai-trigger");
        let context = rt
            .elements()
            .iter()
            .find(|e| e.role == "menuitem" && e.label.as_deref() == Some("Copy AI context"))
            .unwrap()
            .key
            .clone()
            .unwrap();
        tap(&mut rt, &context);
        assert!(rt.state.sidebar_actions.panel.is_none());
        rt.frame(1.25);
        tap(&mut rt, "sidebar-copy-ai");
        assert!(
            clipboard.text().is_some(),
            "status={}, brief={}",
            rt.state.status,
            rt.state.sidebar_actions.brief.is_some()
        );
        assert!(clipboard.text().unwrap().contains("GitHub"));
        assert!(clipboard.text().unwrap().contains("Treat quoted metadata"));
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(2.);
        assert!(rt.state.sidebar_actions.brief.is_none());
        tap(&mut rt, "sidebar-update");
        rt.frame(2.1);
        assert!(rt.state.sidebar_actions.command.is_some());
        tap(&mut rt, "upgrade-cancel");
        rt.frame(2.2);
        tap(&mut rt, &versions_key);
        tap(&mut rt, "sidebar-panel-upgrade");
        rt.frame(2.25);
        assert!(rt.find("upgrade-preview-dialog").is_some());
        assert!(
            rt.find("upgrade-version").is_none(),
            "Scoop must not offer arbitrary version targets"
        );
        tap(&mut rt, "upgrade-copy");
        assert_eq!(clipboard.text().unwrap().as_str(), "scoop update 'main/gh'");
        assert_eq!(
            rt.state.selected().unwrap().installed.as_deref(),
            Some("2.0.0")
        );
        tap(&mut rt, "upgrade-cancel");
        let check = format!("sidebar-check-{}", crate::activity::hash(&row_id));
        rt.update(|s, _| {
            let other = s
                .rows
                .iter()
                .flat_map(|r| &r.members)
                .map(|&i| &s.catalog.packages[i])
                .find(|p| p.id != id)
                .unwrap()
                .id
                .clone();
            s.select_package(&other);
        });
        rt.frame(3.);
        let selected = rt.state.selected().unwrap().id.clone();
        let (x, y) = rt.find(&check).unwrap();
        rt.pointer_move(x, y);
        rt.frame(4.);
        tap(&mut rt, &check);
        assert!(rt.state.sidebar_actions.selected.contains(&row_id));
        assert_eq!(
            rt.state.selected().unwrap().id,
            selected,
            "Checkbox must not navigate the reader"
        );
        rt.frame(4.25);
        tap(&mut rt, "sidebar-copy-selection");
        assert!(
            clipboard.text().unwrap().contains("GitHub"),
            "size={size:?}, status={}, clipboard={:?}, copy={:?}",
            rt.state.status,
            clipboard.text(),
            bounds(&rt, "sidebar-copy-selection")
        );
        tap(&mut rt, "sidebar-clear-selection");
        assert!(rt.state.sidebar_actions.selected.is_empty());
        assert!(!rt.state.inventory.work.is_pending());
        assert!(!rt.state.live.history.is_pending());
    }
}

#[test]
fn sidebar_upgrade_target_is_editable_validated_and_only_copied() {
    for dark in [true, false] {
        let mut rt = runtime((1024., 640.));
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|s,_|{
            s.dark=dark;
            let record=serde_json::from_value(serde_json::json!({"manager":"pnpm","id":"@vendor/fixture","name":"Fixture",
                "installed":"1.0.0","available":"2.0.0","source":"configured registry/source","scope":"global","root":null,
                "pinned":false,"install_origin_known":false})).unwrap();
            s.sidebar_actions.command=Some(crate::sidebar_actions::Command{name:"Fixture".into(),plan:crate::upgrade_preview::Plan::new(&record).unwrap(),version:TextEdit::new("2.0.0")});
        });
        rt.frame(1.);
        tap(&mut rt, "upgrade-version");
        rt.key(KeyEvent {
            key: Key::Command('a'),
            mods: Mods {
                ctrl: true,
                ..Mods::default()
            },
        });
        rt.key(KeyEvent {
            key: Key::Text("2.5.0".into()),
            mods: Mods::default(),
        });
        rt.frame(2.);
        tap(&mut rt, "upgrade-copy");
        assert_eq!(
            clipboard.text().unwrap().as_str(),
            "pnpm add --global '@vendor/fixture@2.5.0'"
        );
        rt.update(|s, _| {
            s.sidebar_actions.command.as_mut().unwrap().version = TextEdit::new("0.9.0")
        });
        rt.frame(3.);
        tap(&mut rt, "upgrade-copy");
        assert_eq!(
            clipboard.text().unwrap().as_str(),
            "pnpm add --global '@vendor/fixture@2.5.0'"
        );
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("newer than the installed version")
        );
        assert!(!rt.state.inventory.work.is_pending());
    }
}

#[test]
fn all_rich_selects_keep_provider_project_and_component_selection_read_only() {
    use releasescout_engine::{Channel, inventory::Manager};
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, _| {
            s.dark = dark;
            s.inventory.open = true;
        });
        rt.frame(1.);
        tap(&mut rt, "scan-manager-trigger");
        rt.frame(2.);
        tap(&mut rt, "scan-manager-option-npm");
        assert_eq!(Manager::ALL[rt.state.inventory.manager], Manager::Npm);
        assert!(!rt.state.inventory.picker);
        assert!(!rt.state.inventory.work.is_pending());
        assert!(rt.state.inventory.last_request.is_none());
        rt.update(|s, _| {
            s.inventory.open = false;
            crate::package_ui::scene(s);
        });
        rt.frame(3.);
        tap(&mut rt, "project-filter-trigger");
        rt.frame(4.);
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("1 declaration /")
        );
        tap(&mut rt, "project-filter-option-docs");
        assert_eq!(rt.state.packages.project, "docs");
        assert!(!rt.state.packages.picker);
        assert_eq!(rt.state.rows.len(), 1);
        assert_eq!(rt.state.selected_observations().len(), 1);
        assert!(!rt.state.packages.work.is_pending());
        let mut expected = 0;
        rt.update(|s, _| {
            s.set_section(LibrarySection::Software);
            s.set_dataset(Dataset::Reference);
            s.select_package("supabase");
            let profile = s
                .live
                .catalog
                .packages
                .iter()
                .find(|p| p.id == "supabase")
                .unwrap();
            expected = match profile.streams[1].channel {
                Channel::Stable => 0,
                Channel::Preview => 1,
                Channel::All => 2,
            };
            let q = s.engine_query();
            q.range = true;
            q.from = TextEdit::new("old from");
            q.to = TextEdit::new("old to");
            q.inventory_range = true;
        });
        rt.frame(5.);
        tap(&mut rt, "engine-component-trigger");
        rt.frame(6.);
        tap(&mut rt, "engine-component-option-hosted");
        let q = &rt.state.live.sessions["supabase"];
        assert_eq!(q.component, 1);
        assert_eq!(q.channel, expected);
        assert!(q.from.text.is_empty() && q.to.text.is_empty());
        assert!(!q.inventory_range && !q.component_open);
        assert!(!rt.state.live.work.is_pending());
        for key in ["engine-component-trigger", "fetch-notes"] {
            let [x, y, w, h] = rt
                .elements()
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
                .rect;
            assert!(x >= 0. && y >= 0. && x + w <= size.0 && y + h <= size.1);
        }
    }
}

#[test]
fn editor_and_profile_filters_preserve_memberships_versions_and_queries() {
    use releasescout_engine::inventory::Manager;
    fn choose(rt: &mut Runtime<State>, label: &str) {
        let [x, y, w, h] = rt
            .elements()
            .iter()
            .find(|e| e.role == "option" && e.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing option {label}"))
            .rect;
        rt.pointer_down(x + w / 2., y + h / 2.);
        rt.pointer_up(x + w / 2., y + h / 2.);
        rt.frame(0.);
    }
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, _| {
            crate::extension_ui::scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        assert_eq!(rt.state.rows.len(), 9);
        assert_eq!(rt.state.selected_observations().len(), 1);
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("VS Code / Default")
        );
        let original = rt.state.selected().unwrap().id.clone();
        rt.state.live.sessions.get_mut(&original).unwrap().from = TextEdit::new("saved query");
        tap(&mut rt, "extension-editor-trigger");
        rt.frame(2.);
        choose(&mut rt, "VS Code Insiders");
        assert_eq!(rt.state.extensions.editor, Some(Manager::VscodeInsiders));
        assert_eq!(rt.state.selected_observations().len(), 1);
        tap(&mut rt, "extension-profile-trigger");
        rt.frame(3.);
        choose(&mut rt, "Work");
        assert_eq!(rt.state.selected_observations().len(), 1);
        assert_eq!(
            rt.state.selected().unwrap().installed.as_deref(),
            Some("2.1.0")
        );
        tap(&mut rt, "extension-editor-trigger");
        rt.frame(4.);
        choose(&mut rt, "All editors");
        assert!(rt.state.extensions.profile.is_none());
        assert_eq!(rt.state.selected_observations().len(), 1);
        assert_eq!(rt.state.live.sessions[&original].from.text, "saved query");
        tap(&mut rt, "extension-profile-trigger");
        rt.frame(5.);
        choose(&mut rt, "Cursor / Work");
        assert_eq!(rt.state.selected_observations().len(), 1);
        assert_eq!(rt.state.selected().unwrap().provider, Some(Manager::Cursor));
        assert_eq!(
            rt.state.selected().unwrap().installed.as_deref(),
            Some("3.1.0")
        );
        let current = rt.state.selected().unwrap().id.clone();
        rt.update(|s, _| s.rebuild_detected());
        rt.frame(5.);
        assert_eq!(rt.state.selected().unwrap().id, current);
        assert_eq!(rt.state.selected_observations().len(), 1);
        for e in rt.elements().iter().filter(|e| {
            e.key.as_deref().is_some_and(|k| {
                k == "extension-editor-trigger" || k == "extension-profile-trigger"
            })
        }) {
            let [x, y, w, h] = e.rect;
            assert!(x >= 0. && y >= 0. && x + w <= size.0 && y + h <= size.1);
        }
        rt.state.extensions.profile = Some("no-longer-registered".into());
        rt.update(|s, _| s.rebuild_detected());
        rt.frame(5.);
        assert!(rt.state.extensions.profile.is_none());
        rt.update(|s, _| s.set_section(LibrarySection::Software));
        rt.frame(5.);
        assert!(rt.state.rows.is_empty());
        assert!(rt.find("extension-filters").is_none());
    }
}

fn notes_runtime(size: (f32, f32)) -> Runtime<State> {
    let mut rt = runtime(size);
    rt.update(|s, _| s.select_package("gh"));
    rt.frame(0.0);
    rt
}

#[test]
fn existing_icon_choices_support_real_clicks_themes_target_binding_and_reset() {
    use crate::{custom_icons::identity, icon_picker::Choice};
    for size in [(1024., 640.), (1280., 800.)] {
        for dark in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut rt = runtime(size);
            let mut source = String::new();
            let mut target = String::new();
            rt.update(|s, _| {
                s.dark = dark;
                s.custom_icons.directory = dir.path().into();
                let image = s
                    .icons
                    .load(&include_bytes!("../assets/providers/winget.png")[..]);
                image.wait().unwrap();
                let p = s
                    .catalog
                    .packages
                    .iter_mut()
                    .find(|p| p.id == "gh")
                    .unwrap();
                p.icon = Some(image);
                source = identity(p);
                s.select_package("bun");
                target = identity(s.selected().unwrap());
            });
            rt.frame(0.);
            tap(&mut rt, "choose-package-icon");
            rt.frame(1.);
            assert!(!rt.state.activity.dirty);
            tap(&mut rt, &format!("icon-app-{source}"));
            assert!(rt.state.custom_icons.picker.is_none());
            assert_eq!(
                rt.state.activity.data.icon_choices[&target],
                Choice::App(source)
            );
            assert!(rt.state.selected().unwrap().custom_icon.is_some());
            tap(&mut rt, "choose-package-icon");
            rt.frame(2.);
            tap(&mut rt, "icon-collection/segment1");
            rt.frame(3.);
            let tiles: Vec<_> = rt
                .elements()
                .into_iter()
                .filter(|e| {
                    e.key
                        .as_deref()
                        .is_some_and(|k| k.starts_with("icon-symbol-"))
                })
                .collect();
            assert_eq!(tiles.len(), 12);
            for (i, a) in tiles.iter().enumerate() {
                let [x, y, w, h] = a.rect;
                assert_eq!([w, h], [64., 64.]);
                assert!(x >= 0. && y >= 0. && x + w <= size.0 && y + h <= size.1);
                for b in &tiles[i + 1..] {
                    assert!(
                        x + w <= b.rect[0]
                            || b.rect[0] + b.rect[2] <= x
                            || y + h <= b.rect[1]
                            || b.rect[1] + b.rect[3] <= y
                    );
                }
            }
            rt.update(|s, _| s.select_package("gh"));
            rt.frame(3.);
            tap(&mut rt, "icon-symbol-terminal");
            assert!(rt.state.selected().unwrap().custom_symbol.is_none());
            rt.update(|s, _| s.select_package("bun"));
            rt.frame(3.);
            assert_eq!(
                rt.state.selected().unwrap().custom_symbol.unwrap().name,
                "symbol-terminal"
            );
            tap(&mut rt, "choose-package-icon");
            rt.frame(4.);
            rt.key(KeyEvent {
                key: Key::Escape,
                mods: Mods::default(),
            });
            rt.frame(4.);
            assert!(rt.state.custom_icons.picker.is_none());
            assert!(rt.state.selected().unwrap().custom_symbol.is_some());
            tap(&mut rt, "choose-package-icon");
            rt.frame(5.);
            tap(&mut rt, "icon-automatic");
            finish_custom_icon(&mut rt, "automatic");
            assert!(!rt.state.has_icon_override(rt.state.selected().unwrap()));
            assert!(!rt.state.activity.data.icon_choices.contains_key(&target));
        }
    }
}

#[test]
fn packages_are_separate_grouped_and_survive_software_inventory_refresh() {
    let mut s = State::new();
    super::package_ui::scene(&mut s);
    assert_eq!(s.rows.len(), 1);
    assert_eq!(s.rows[0].members.len(), 2);
    assert_eq!(s.selected().unwrap().name, "next");
    assert!(s.selected().unwrap().dependency.is_some());
    assert!(!s.source_loader.work.is_pending());
    s.rebuild_detected();
    assert!(s.section == LibrarySection::Packages);
    assert_eq!(s.rows[0].members.len(), 2);
    s.packages.project = "docs".into();
    s.set_dataset(s.dataset);
    assert_eq!(s.rows.len(), 1);
    assert_eq!(s.rows[0].members.len(), 1);
    assert!(s.selected().unwrap().installed.is_none());
    assert!(s.selected().unwrap().update_label().contains("^15.5.0"));
    s.set_section(LibrarySection::Software);
    assert!(
        s.rows
            .iter()
            .all(|r| s.catalog.packages[r.package].dependency.is_none())
    );
    s.set_dataset(Dataset::Detected);
    s.set_section(LibrarySection::Extensions);
    assert!(s.rows.is_empty());
    s.set_section(LibrarySection::Packages);
    assert_eq!(s.rows.len(), 1);
}

#[test]
fn package_controls_and_project_dialog_render_without_machine_inventory() {
    for size in [(1024.0, 640.0), (1280.0, 800.0)] {
        let mut rt = runtime(size);
        tap(&mut rt, "library-section/segment2");
        assert!(rt.state.section == LibrarySection::Packages);
        assert!(rt.state.rows.is_empty());
        assert!(rt.find("watchlist-bar").is_some());
        assert!(rt.find("project-controls").is_none());
        tap(&mut rt, "packages-view/segment1");
        assert!(rt.find("project-controls").is_some());
        tap(&mut rt, "open-project");
        assert!(rt.file_dialog().is_some());
        rt.resolve_file_dialog(None).unwrap();
        rt.frame(0.0);
        assert!(!rt.state.packages.work.is_pending());
        assert!(!rt.state.packages.persist);
        rt.update(|s, tx| {
            super::package_ui::scene(s);
            tx.instant();
        });
        rt.frame(0.0);
        assert!(rt.find("dependency-controls").is_some());
        assert!(rt.find("fetch-notes").is_some());
        assert!(rt.find("selected-observation-trigger").is_some());
        assert!(rt.find("engine-unavailable").is_none());
        assert!(rt.find("show-components").is_none());
        tap(&mut rt, "selected-observation-trigger");
        assert!(rt.state.observation_picker);
    }
}

fn finish_project_scan(rt: &mut Runtime<State>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let mut delivered = false;
        rt.update(|s, tx| {
            delivered = s.poll_packages();
            tx.instant();
        });
        rt.frame(0.0);
        if delivered {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "project scan timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn finish_custom_icon(rt: &mut Runtime<State>, stage: &str) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let mut delivered = false;
        rt.update(|s, tx| {
            delivered = s.poll_custom_icons();
            s.poll_icons();
            tx.instant();
        });
        rt.frame(0.0);
        if delivered && !rt.state.custom_icons.work.is_pending() {
            return;
        }
        assert!(
            std::time::Instant::now() < until,
            "{stage}: custom icon delivery timed out; pending={} dialog={} error={:?} status={}",
            rt.state.custom_icons.work.is_pending(),
            rt.state.custom_icons.dialog,
            rt.state.custom_icons.error,
            rt.state.status
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn local_icon_picker_cancels_safely_and_binds_the_choice_to_the_original_entry() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("chosen.png");
    std::fs::write(&source, include_bytes!("../assets/providers/winget.png")).unwrap();
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, _| {
        s.custom_icons.directory = dir.path().join("saved");
        s.select_package("bun");
    });
    rt.frame(0.0);
    tap(&mut rt, "choose-package-icon");
    rt.frame(1.0);
    tap(&mut rt, "icon-from-file");
    assert!(rt.file_dialog().is_some());
    rt.resolve_file_dialog(None).unwrap();
    rt.frame(0.0);
    assert!(!rt.state.custom_icons.dialog);
    assert!(!dir.path().join("saved").exists());
    tap(&mut rt, "choose-package-icon");
    tap(&mut rt, "icon-from-file");
    rt.update(|s, _| s.select_package("gh"));
    rt.resolve_file_dialog(Some(source.clone())).unwrap();
    rt.frame(0.0);
    finish_custom_icon(&mut rt, "import");
    assert!(rt.state.selected().unwrap().custom_icon.is_none());
    let custom = rt
        .state
        .catalog
        .packages
        .iter()
        .find(|p| p.id == "bun")
        .unwrap()
        .custom_icon
        .as_ref()
        .unwrap()
        .clone();
    custom.wait().unwrap();
    rt.update(|s, _| s.select_package("bun"));
    rt.frame(0.0);
    assert!(rt.find("reset-package-icon").is_some());
    std::fs::write(&source, b"not a picture").unwrap();
    tap(&mut rt, "choose-package-icon");
    tap(&mut rt, "icon-from-file");
    rt.resolve_file_dialog(Some(source.clone())).unwrap();
    rt.frame(0.0);
    finish_custom_icon(&mut rt, "invalid");
    assert!(
        rt.find("custom-icon-error").is_some(),
        "{:?}",
        rt.state.custom_icons.error
    );
    let elements = rt.elements();
    let reset = elements
        .iter()
        .find(|e| e.key.as_deref() == Some("reset-package-icon"))
        .unwrap()
        .rect;
    let copy = elements
        .iter()
        .find(|e| e.key.as_deref() == Some("copy-notes"))
        .unwrap()
        .rect;
    assert!(
        reset[0] + reset[2] <= copy[0],
        "Icon reset and copy controls overlap"
    );
    assert_eq!(
        rt.state
            .selected()
            .unwrap()
            .custom_icon
            .as_ref()
            .unwrap()
            .id(),
        custom.id()
    );
    tap(&mut rt, "reset-package-icon");
    finish_custom_icon(&mut rt, "reset");
    assert!(rt.state.selected().unwrap().custom_icon.is_none());
    assert!(rt.state.custom_icons.error.is_none());
    assert_eq!(
        std::fs::read(source).unwrap(),
        b"not a picture",
        "reset must not touch the chosen source file"
    );
}

#[test]
fn folder_selection_scans_real_manifests_and_failed_rescan_retains_previous_project() {
    let mut rt = runtime((1024.0, 640.0));
    tap(&mut rt, "library-section/segment2");
    tap(&mut rt, "packages-view/segment1");
    tap(&mut rt, "open-project");
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/project");
    rt.resolve_file_dialog(Some(root)).unwrap();
    rt.frame(0.0);
    finish_project_scan(&mut rt);
    assert_eq!(
        rt.state.rows.len(),
        2,
        "path={} error={:?} scan={:?}",
        rt.state.packages.path.text,
        rt.state.packages.error,
        rt.state.packages.scan
    );
    let next = rt.state.rows.iter().find(|r| r.name == "next").unwrap();
    assert_eq!(next.members.len(), 2);
    assert!(rt.state.packages.error.is_none());
    assert!(!rt.state.packages.persist);
    rt.update(|s, tx| {
        s.packages.path = TextEdit::new("C:/fixture/absent-project-folder");
        s.scan_project();
        tx.instant();
    });
    finish_project_scan(&mut rt);
    assert_eq!(rt.state.rows.len(), 2);
    assert!(rt.state.packages.error.is_some());
}

#[test]
#[ignore = "explicit live package history and shared-cache check"]
fn live_package_history_cache_for_native_render() {
    use releasescout_engine::{
        Channel, dependencies,
        http::{CacheOptions, HttpClient},
    };
    let mut s = State::new();
    super::package_ui::scene(&mut s);
    let dependency = s.selected().unwrap().dependency.clone().unwrap();
    let fetch = HttpClient::new(CacheOptions {
        directory: super::live::cache_directory(),
        public_only: true,
        ..Default::default()
    })
    .unwrap();
    let report = dependencies::notes(fetch, &dependency, Channel::Stable).unwrap();
    assert!(report.notes.iter().any(|n| n.authored_notes));
    assert!(report.notes.iter().all(|n| !n.preview));
    println!("{} real cached Next.js notes", report.notes.len());
    super::package_ui::notes_scene(&mut s);
    assert!(s.engine_query().error.is_none());
    assert!(!s.selected().unwrap().releases.is_empty());
}

#[test]
fn watchlist_is_project_free_editable_and_independent_of_inventory() {
    use releasescout_engine::watchlist::Group;
    let mut s = State::new();
    super::watchlist_ui::scene(&mut s);
    assert_eq!(s.rows.len(), 3);
    assert_eq!(s.packages.watch.entries.len(), 4);
    assert_eq!(s.inventory_summary(), "3 packages / 3 tracked");
    assert!(s.rows.iter().all(|r| {
        s.catalog.packages[r.package]
            .watchlist
            .as_ref()
            .unwrap()
            .ecosystem
            == releasescout_engine::watchlist::Ecosystem::Npm
    }));
    assert!(s.packages.scan.is_none());
    assert!(
        s.rows
            .iter()
            .all(|r| s.catalog.packages[r.package].watchlist.is_some()
                && s.catalog.packages[r.package].installed.is_none())
    );
    s.rebuild_detected();
    assert_eq!(s.rows.len(), 3);
    s.packages.watch.filter = 2;
    s.set_dataset(s.dataset);
    assert_eq!(s.rows.len(), 1);
    assert_eq!(s.selected().unwrap().name, "typescript");
    s.packages.watch.name = TextEdit::new("next");
    s.add_watchlist_entry();
    assert_eq!(s.rows.len(), 3);
    assert_eq!(s.selected().unwrap().name, "next");
    s.packages.watch.name = TextEdit::new("@scope/new-package");
    s.add_watchlist_entry();
    assert_eq!(s.rows.len(), 4);
    s.set_watchlist_group(1);
    assert_eq!(
        s.selected().unwrap().watchlist.as_ref().unwrap().group,
        Group::Tooling
    );
    let id = s.selected().unwrap().id.clone();
    s.live.active = Some(id.clone());
    s.remove_watchlist_entry();
    assert_eq!(s.rows.len(), 3);
    assert!(s.live.active.is_none());
    assert!(!s.live.sessions.contains_key(&id));
    assert!(!s.packages.persist);
    s.poll_watchlist();
    assert!(!s.packages.watch.save.is_pending());
    s.set_section(LibrarySection::Software);
    assert!(
        s.rows
            .iter()
            .all(|r| s.catalog.packages[r.package].watchlist.is_none())
    );
    s.set_section(LibrarySection::Packages);
    assert_eq!(s.rows.len(), 3);
}

#[test]
fn watchlist_controls_fit_and_follow_unfollow_without_project_or_version() {
    for size in [(1024.0, 640.0), (1280.0, 800.0)] {
        let mut rt = runtime(size);
        rt.update(|s, tx| {
            super::watchlist_ui::scene(s);
            tx.instant();
        });
        rt.frame(0.0);
        assert!(rt.find("project-controls").is_none());
        assert!(rt.find("watchlist-group").is_some());
        assert!(rt.find("watchlist-ecosystem").is_none());
        assert!(rt.find("category-badge-core").is_some());
        assert!(rt.find("category-badge-tooling").is_some());
        assert!(rt.find("fetch-notes").is_some());
        assert!(rt.find("selected-observation-trigger").is_none());
        tap(&mut rt, "watchlist-filter/segment2");
        assert_eq!(rt.state.rows.len(), 1);
        rt.update(|s, tx| {
            s.packages.watch.name = TextEdit::new("@tanstack/react-table");
            tx.instant();
        });
        rt.frame(0.0);
        tap(&mut rt, "track-package");
        assert_eq!(rt.state.rows.len(), 4);
        assert_eq!(rt.state.selected().unwrap().name, "@tanstack/react-table");
        tap(&mut rt, "unfollow-package");
        assert_eq!(rt.state.rows.len(), 3);
        assert!(!rt.state.packages.watch.save.is_pending());
        rt.key(KeyEvent {
            key: Key::Command('f'),
            mods: Mods::default(),
        });
        rt.frame(0.0);
        rt.key(KeyEvent {
            key: Key::Text("zod".into()),
            mods: Mods::default(),
        });
        rt.frame(0.0);
        assert_eq!(rt.state.query.text, "zod");
        assert!(rt.state.packages.watch.name.text.is_empty());
        for key in [
            "watchlist-bar",
            "selected-package",
            "fetch-notes",
            "watchlist-group",
        ] {
            let [x, y, w, h] = rt
                .elements()
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
                .rect;
            assert!(
                x >= 0.0 && y >= 0.0 && x + w <= size.0 + 0.5 && y + h <= size.1 + 0.5,
                "{key}: {x},{y},{w},{h}"
            );
        }
    }
}

#[test]
#[ignore = "explicit public watchlist notes and native-render cache check"]
fn live_watchlist_notes_model() {
    use releasescout_engine::{
        Channel,
        http::{CacheOptions, HttpClient},
        watchlist,
    };
    let mut s = State::new();
    super::watchlist_ui::scene(&mut s);
    let entry = s
        .packages
        .watch
        .entries
        .iter()
        .find(|e| e.name == "next")
        .unwrap()
        .clone();
    let fetch = HttpClient::new(CacheOptions {
        directory: super::live::cache_directory(),
        public_only: true,
        github_token: std::env::var("GITHUB_TOKEN").ok(),
        ..Default::default()
    })
    .unwrap();
    let report = watchlist::notes(fetch, &entry, Channel::Stable).unwrap();
    assert!(report.notes.iter().any(|n| n.authored_notes));
    assert!(report.notes.iter().all(|n| !n.preview));
    s.apply_engine_result(entry.id(), Ok(report));
    s.select_package(&entry.id());
    assert!(!s.selected().unwrap().releases.is_empty());
}

#[test]
fn software_source_lookup_and_discovered_controls_render_at_minimum_size() {
    for found in [false, true] {
        let mut rt = runtime((1024.0, 640.0));
        rt.update(|s, tx| {
            super::source_loader::scene(s, found);
            tx.instant();
        });
        rt.frame(0.0);
        assert!(rt.find("engine-unavailable").is_none());
        assert_eq!(rt.find("source-discovery-status").is_some(), !found);
        assert_eq!(rt.find("fetch-notes").is_some(), found);
        assert!(!rt.state.source_loader.work.is_pending());
    }
}

#[test]
fn installed_extension_notes_render_with_source_controls_and_no_update_claim() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, tx| {
        super::extension_notes_scene(s);
        tx.instant();
    });
    rt.frame(0.0);
    assert!(rt.find("extension-controls").is_some());
    assert!(rt.find("fetch-notes").is_some());
    assert!(rt.find("engine-unavailable").is_none());
    let p = rt.state.selected().unwrap();
    let id = p.id.clone();
    assert!(p.target.is_none());
    assert_eq!(p.releases.len(), 2);
    assert!(p.releases.iter().all(|r| !r.version.contains("1.3.0")));
    assert!(
        !rt.state.live.sessions[&id]
            .report
            .as_ref()
            .unwrap()
            .complete
    );
    tap(&mut rt, "extension-source/segment1");
    assert!(rt.state.live.sessions[&id].extension_published);
    assert!(rt.find("engine-channel").is_some());
    tap(&mut rt, "extension-source/segment0");
    assert!(!rt.state.live.sessions[&id].extension_published);
    assert!(rt.find("engine-channel").is_none());
}

#[test]
fn cli_executable_icons_do_not_override_product_branding() {
    let mut rt = runtime((1024.0, 640.0));
    rt.state.accept_scan(releasescout_engine::inventory::parse(releasescout_engine::inventory::Manager::Scoop,false,
        &serde_json::json!({"installed":{"apps":[{"Name":"unfamiliar-cli","Version":"1.0","Source":"custom"}]},
            "metadata":{"unfamiliar-cli":{"hasCommands":true,"hasShortcuts":false,"iconPath":"Z:/runtime-branded.exe","homepage":"https://product.example/"}}}).to_string()).unwrap());
    let p = rt.state.selected().unwrap();
    assert!(p.icon.is_none());
    assert_eq!(
        p.icon_plan.as_ref().unwrap().homepage.as_deref(),
        Some("https://product.example/")
    );
}

#[test]
fn extension_fetch_uses_its_originating_profile_after_switching_sections() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, tx| {
        super::extension_notes_scene(s);
        s.engine_query().cache = 2;
        tx.instant();
    });
    rt.frame(0.0);
    let id = rt.state.selected().unwrap().id.clone();
    tap(&mut rt, "fetch-notes");
    rt.state.set_section(LibrarySection::Software);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !rt.state.poll_engine() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(rt.state.selected().is_none());
    let report = rt.state.live.sessions[&id].report.as_ref().unwrap();
    assert_eq!(report.notes.len(), 2);
    assert_eq!(report.evidence[0].cache, "installed_file");
    rt.state.set_section(LibrarySection::Extensions);
    assert_eq!(rt.state.selected().unwrap().id, id);
    assert!(rt.state.selected().unwrap().target.is_none());
}

#[test]
fn local_apps_keep_unmapped_presence_and_render_icons_without_losing_fallbacks() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, tx| {
        super::local_apps_scene(s);
        tx.instant();
    });
    rt.frame(0.0);
    assert_eq!(rt.state.rows.len(), 2);
    assert!(rt.find("package-icon").is_some());
    assert!(rt.state.poll_icons());
    assert!(!rt.state.poll_icons());
    let p = rt.state.selected().unwrap();
    assert_eq!(p.name, "Local Desktop App");
    assert_eq!(p.manager_label(), "Local PC");
    assert!(p.reference_id.is_none());
    assert!(p.target.is_none());
    assert!(p.update_label().contains("no update source"));
    assert!(p.observation_label.contains("User"));
    rt.update(|s, _| {
        let id = s.selected().unwrap().id.clone();
        s.catalog
            .packages
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .icon = None;
    });
    rt.frame(0.0);
    assert!(rt.find("package-icon").is_none());
    assert!(rt.find("selected-package").is_some());
}

#[test]
fn library_sections_keep_search_selection_and_profiles_across_refreshes() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, tx| {
        super::portable_filter_scene(s);
        tx.instant();
    });
    rt.frame(0.0);
    rt.state.query = TextEdit::new("Independent");
    rt.state.refilter();
    let software = rt.state.selected().unwrap().id.clone();
    tap(&mut rt, "library-section/segment1");
    assert!(rt.state.query.text.is_empty());
    rt.state.query = TextEdit::new("vendor.shared");
    rt.state.refilter();
    rt.frame(0.0);
    rt.update(|s, _| {
        let id = s
            .catalog
            .packages
            .iter()
            .find(|p| {
                p.extension.is_some() && crate::extension_ui::profile_name(p) == "Work With Spaces"
            })
            .unwrap()
            .id
            .clone();
        s.select_package(&id);
    });
    rt.frame(0.);
    let extension = rt.state.selected().unwrap().id.clone();
    rt.state.live.sessions.get_mut(&extension).unwrap().from = TextEdit::new("per-profile query");
    rt.update(|s,tx| {
        s.accept_scan(releasescout_engine::inventory::parse(releasescout_engine::inventory::Manager::Scoop,false,
            &serde_json::json!({"installed":{"apps":[{"Name":"fresh-tool","Version":"1.0","Source":"main"}]}}).to_string()).unwrap());
        tx.instant();
    });
    rt.frame(0.0);
    assert!(rt.state.section == LibrarySection::Extensions);
    assert_eq!(rt.state.selected().unwrap().id, extension);
    assert_eq!(rt.state.selected_observations().len(), 1);
    tap(&mut rt, "library-section/segment0");
    assert_eq!(rt.state.query.text, "Independent");
    assert_eq!(rt.state.selected().unwrap().id, software);
    tap(&mut rt, "library-section/segment1");
    assert_eq!(rt.state.query.text, "vendor.shared");
    assert_eq!(rt.state.selected().unwrap().id, extension);
    assert_eq!(
        rt.state.live.sessions[&extension].from.text,
        "per-profile query"
    );
}

#[test]
fn extension_only_and_empty_sections_do_not_fall_back_to_software() {
    let mut rt = runtime_with(State::new(), (1024.0, 640.0));
    rt.update(|s, tx| {
        s.accept_scan(
            releasescout_engine::inventory::parse(
                releasescout_engine::inventory::Manager::VscodeInsiders,
                false,
                &serde_json::json!({"installed":"vendor.only@1.0"}).to_string(),
            )
            .unwrap(),
        );
        tx.instant();
    });
    rt.frame(0.0);
    assert!(rt.state.dataset == Dataset::Detected);
    assert!(rt.state.rows.is_empty());
    tap(&mut rt, "library-section/segment1");
    assert_eq!(rt.state.rows.len(), 1);
    assert!(rt.state.selected().unwrap().kind == EntryKind::Extension);
    tap(&mut rt, "library-section/segment0");
    assert!(rt.state.rows.is_empty());
    rt.update(|s, tx| {
        s.accept_scan(
            releasescout_engine::inventory::parse(
                releasescout_engine::inventory::Manager::VscodeInsiders,
                false,
                &serde_json::json!({"profiles":[]}).to_string(),
            )
            .unwrap(),
        );
        tx.instant();
    });
    rt.frame(0.0);
    assert!(rt.state.dataset == Dataset::Detected);
    assert!(rt.state.section == LibrarySection::Software);
    assert!(rt.state.rows.is_empty());
}

#[test]
fn system_filter_remains_an_accessible_icon_switch_in_the_filter_popover() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, _| {
            crate::sidebar_info::description_scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        assert!(rt.find("show-components").is_none());
        tap(&mut rt, "sidebar-filters-trigger");
        rt.frame(1.);
        for enabled in [false, true] {
            let elements = rt.elements();
            let toggle = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("show-components"))
                .unwrap();
            let search = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("package-filter"))
                .unwrap();
            let header = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("header"))
                .unwrap();
            let sort = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("sidebar-sort-trigger"))
                .unwrap();
            assert!((header.rect[0] - search.rect[0]).abs() < 0.5);
            assert!((header.rect[2] - search.rect[2]).abs() < 0.5);
            assert!((sort.rect[0] - search.rect[0]).abs() < 0.5);
            assert!(header.rect[1] - search.rect[1] - search.rect[3] <= 4.5);
            let popup = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("sidebar-filters"))
                .unwrap();
            let glyph = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("system-switch-icon"))
                .unwrap();
            let thumb = elements
                .iter()
                .find(|e| e.key.as_deref() == Some("thumb") && e.path.contains("show-components/"))
                .unwrap();
            assert_eq!(toggle.role, "switch");
            assert!(toggle.focusable);
            assert_eq!(toggle.label.as_deref(), Some("Show system & support"));
            assert_eq!(rt.state.show_components, enabled);
            assert_eq!(toggle.rect[2], 32.);
            assert!((toggle.rect[3] - 18.4).abs() < 0.05);
            assert_eq!(&thumb.rect[2..], &[16., 16.]);
            assert!(
                toggle.rect[0] >= popup.rect[0]
                    && toggle.rect[0] + toggle.rect[2] <= popup.rect[0] + popup.rect[2]
            );
            assert!(
                (thumb.target[0] - toggle.target[0] - if enabled { 15. } else { 1. }).abs() < 0.5
            );
            assert!(
                glyph.rect[0] >= thumb.rect[0]
                    && glyph.rect[0] + glyph.rect[2] <= thumb.rect[0] + thumb.rect[2]
            );
            assert!(
                glyph.rect[1] >= thumb.rect[1]
                    && glyph.rect[1] + glyph.rect[3] <= thumb.rect[1] + thumb.rect[3]
            );
            assert!(
                !elements
                    .iter()
                    .any(|e| e.text.as_deref() == Some("Show system & support"))
            );
            if !enabled {
                toggle_system(&mut rt);
                rt.frame(0.2);
            }
        }
        rt.key(KeyEvent {
            key: Key::Text(" ".into()),
            mods: Mods::default(),
        });
        rt.frame(0.2);
        assert!(
            !rt.state.show_components,
            "Space must toggle the focused switch"
        );
        assert!(rt.state.sidebar_filters_open);
        rt.update(|s, _| {
            s.query = TextEdit::new("no match for this query");
            s.refilter();
        });
        rt.frame(0.2);
        assert!(
            rt.find("show-components").is_some(),
            "empty results must not hide the filter"
        );
    }
}

#[test]
fn faceted_filters_apply_immediately_reset_and_keep_per_tab_preferences() {
    use crate::sidebar_filters::Update;
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, _| {
            crate::sidebar_info::scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        assert!(rt.find("sidebar-selection-mode").is_none());
        assert!(rt.find("sidebar-clear-selection").is_none());
        tap(&mut rt, "sidebar-filters-trigger");
        rt.frame(1.);
        let popup = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("sidebar-filters"))
            .unwrap()
            .rect;
        assert!(
            popup[0] >= 0.
                && popup[1] >= 0.
                && popup[0] + popup[2] <= size.0
                && popup[1] + popup[3] <= size.1,
            "{popup:?}"
        );
        tap(&mut rt, "filter-update/segment1");
        assert_eq!(rt.state.visible.len(), 1);
        assert_eq!(rt.state.selected().unwrap().name, "GitHub CLI");
        assert!(rt.state.sidebar_filters_open);
        tap(&mut rt, "filter-source/segment1");
        tap(&mut rt, "filter-notes/segment1");
        tap(&mut rt, "filter-provider-scoop");
        assert_eq!(crate::sidebar_filters::count(&rt.state), 4);
        assert_eq!(rt.state.visible.len(), 1);
        tap(&mut rt, "filter-provider-row-scoop");
        assert!(
            rt.state.activity.data.sidebar_filters[0]
                .providers
                .is_empty(),
            "Provider labels toggle their checkbox once"
        );
        tap(&mut rt, "filter-provider-scoop");
        assert!(
            rt.state.activity.data.sidebar_filters[0]
                .providers
                .contains("scoop"),
            "Checkbox presses must not toggle twice"
        );
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(1.);
        assert!(!rt.state.sidebar_filters_open);
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("sidebar-filters-trigger") && e.focused)
        );
        rt.update(|s, _| {
            s.set_section(LibrarySection::Packages);
        });
        rt.frame(1.);
        assert_eq!(crate::sidebar_filters::count(&rt.state), 0);
        tap(&mut rt, "sidebar-filters-trigger");
        rt.frame(1.);
        tap(&mut rt, "filter-notes/segment2");
        assert_eq!(crate::sidebar_filters::count(&rt.state), 1);
        rt.update(|s, _| s.set_section(LibrarySection::Software));
        rt.frame(1.);
        assert_eq!(crate::sidebar_filters::count(&rt.state), 4);
        assert_eq!(
            rt.state.activity.data.sidebar_filters[0].update,
            Update::Available
        );
        tap(&mut rt, "sidebar-filters-trigger");
        rt.frame(1.);
        tap(&mut rt, "sidebar-filters-reset");
        assert_eq!(rt.state.visible.len(), 3);
        assert_eq!(crate::sidebar_filters::count(&rt.state), 0);
        assert_eq!(rt.state.activity.data.sidebar_filters[2].notes, Some(false));
        assert!(rt.state.activity.dirty);
        assert!(rt.state.inventory.discovery.iter().all(|w| !w.is_pending()));
        assert!(!rt.state.live.work.is_pending());
        assert!(rt.find("filter-results").is_some());
    }
}

#[test]
fn filter_panel_sections_align_and_header_footer_stay_visible_with_all_providers() {
    use releasescout_engine::inventory::Manager;
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, _| {
            crate::sidebar_info::scene(s);
            s.dark = dark;
            s.sidebar_filters_open = true;
            s.activity.data.sidebar_filters[0]
                .providers
                .extend(Manager::ALL.iter().map(|m| m.id().to_owned()));
            s.refilter();
        });
        rt.frame(1.);
        let elements = rt.elements();
        let get = |key: &str| {
            elements
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
        };
        let popup = get("sidebar-filters").rect;
        let header = get("filter-panel-header").rect;
        let footer = get("filter-panel-footer").rect;
        assert!(header[1] >= popup[1] && footer[1] + footer[3] <= popup[1] + popup[3] + 0.5);
        assert!(popup[1] >= 0. && popup[1] + popup[3] <= size.1);
        for key in [
            "filter-panel-header",
            "sidebar-filters-reset",
            "filter-results",
            "filter-active-badge",
        ] {
            let e = get(key);
            assert!(
                e.visible[3] >= e.rect[3] - 0.5,
                "{key} must stay visible: {:?}/{:?}",
                e.rect,
                e.visible
            );
        }
        let ink = crate::theme::kit(dark).c.icon_muted;
        for key in [
            "filter-update-icon",
            "filter-source-icon",
            "filter-notes-icon",
            "filter-providers-icon",
        ] {
            let e = get(key);
            assert_eq!(&e.rect[2..], &[14., 14.]);
            assert_eq!(e.text_color, Some(ink));
        }
        for key in ["filter-update", "filter-source", "filter-notes"] {
            let heading = get(&format!("{key}-heading")).rect;
            let icon = get(&format!("{key}-icon")).rect;
            let label = get(&format!("{key}-label")).rect;
            assert!(
                ((icon[0] + label[0] + label[2]) / 2. - heading[0] - heading[2] / 2.).abs() < 0.5,
                "{key} must be horizontally centered"
            );
        }
        assert!(
            (get("filter-providers-icon").rect[0] - get("filter-providers-heading").rect[0]).abs()
                < 0.5
        );
        let scroll = get("filter-scroll").rect;
        rt.wheel_event(
            WheelEvent::new(0., -800.)
                .at(scroll[0] + scroll[2] / 2., scroll[1] + scroll[3] / 2.)
                .precise(true),
        );
        rt.frame(1.);
        let after = rt.elements();
        let rect = |key: &str| {
            after
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
                .rect
        };
        assert_eq!(rect("filter-panel-header"), header);
        assert_eq!(rect("filter-panel-footer"), footer);
        let system = after
            .iter()
            .find(|e| e.key.as_deref() == Some("show-components"))
            .unwrap();
        assert!(
            system.visible[3] >= 18.,
            "Last control must be reachable by scrolling"
        );
        tap(&mut rt, "sidebar-filters-reset");
        assert!(rt.find("filter-active-badge").is_none());
        assert_eq!(crate::sidebar_filters::count(&rt.state), 0);
    }
}

#[test]
fn a_system_only_machine_has_an_empty_detected_view_not_the_reference_allowlist() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s,tx| {
        s.accept_scan(releasescout_engine::inventory::parse(releasescout_engine::inventory::Manager::Store,false,
            &serde_json::json!([{"Name":"Future.SystemApp","PackageFamilyName":"Future.SystemApp_pub","Version":"1.0",
                "Publisher":"O=Microsoft Corporation","SignatureKind":"Store","HasCliAlias":false}]).to_string()).unwrap());
        tx.instant();
    });
    rt.frame(0.0);
    assert!(rt.state.dataset == Dataset::Detected);
    assert!(rt.state.rows.is_empty());
    toggle_system(&mut rt);
    assert_eq!(rt.state.rows.len(), 1);
    assert!(rt.state.selected().unwrap().component_reason.is_some());
    toggle_system(&mut rt);
    assert!(rt.state.rows.is_empty());
}

#[test]
fn portable_filtering_and_profile_memberships_work_without_catalog_allowlisting() {
    for size in [(1024.0, 640.0), (1280.0, 800.0)] {
        let mut rt = runtime(size);
        rt.update(|s, tx| {
            super::portable_filter_scene(s);
            tx.instant();
        });
        rt.frame(0.0);
        assert_eq!(rt.state.rows.len(), 5);
        assert_eq!(
            rt.state
                .catalog
                .packages
                .iter()
                .filter(|p| p.detected)
                .count(),
            14
        );
        assert_eq!(
            rt.state.status,
            "5 software groups / 5 system/support hidden"
        );
        assert!(
            rt.state
                .rows
                .iter()
                .all(|r| rt.state.catalog.packages[r.package].kind != EntryKind::Extension)
        );
        tap(&mut rt, "library-section/segment1");
        assert_eq!(rt.state.rows.len(), 4);
        assert_eq!(rt.state.status, "4 extension memberships");
        assert!(rt.find("show-components").is_none());
        assert!(
            rt.state
                .rows
                .iter()
                .all(|r| rt.state.catalog.packages[r.package].kind == EntryKind::Extension)
        );
        assert_eq!(rt.state.selected_observations().len(), 1);
        rt.update(|s, _| {
            let id = s
                .catalog
                .packages
                .iter()
                .find(|p| {
                    p.extension.is_some()
                        && crate::extension_ui::profile_name(p) == "Work With Spaces"
                })
                .unwrap()
                .id
                .clone();
            s.select_package(&id);
        });
        rt.frame(0.);
        assert!(
            rt.state
                .selected()
                .unwrap()
                .observation_label
                .contains("profile: Work With Spaces")
        );
        assert_eq!(
            rt.state.selected().unwrap().installed.as_deref(),
            Some("2.0-pre.1")
        );
        tap(&mut rt, "library-section/segment0");
        assert!(rt.state.rows.iter().any(|r| r.name == "Independent app"));
        assert!(rt.state.rows.iter().any(|r| r.name == "Unfamiliar SDK"));
        assert!(rt.state.rows.iter().any(|r| r.name == "Unfamiliar CLI"));
        assert!(!rt.state.rows.iter().any(|r| r.name == "System app"));
        toggle_system(&mut rt);
        assert_eq!(rt.state.rows.len(), 10);
        assert_eq!(
            rt.state.rows.iter().map(|r| r.members.len()).sum::<usize>(),
            10
        );
        assert_eq!(
            rt.state.status,
            "10 software groups / 5 system/support included"
        );
        assert!(rt.state.rows.iter().any(|r| r.name == "System app"));
        toggle_system(&mut rt);
        assert_eq!(rt.state.rows.len(), 5);
    }
}

#[test]
fn automatic_filtering_keeps_real_apps_and_cli_aliases_with_human_names() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, tx| {
        super::automatic_filter_scene(s);
        tx.instant();
    });
    rt.frame(0.0);
    assert_eq!(rt.state.rows.len(), 3);
    assert_eq!(rt.state.selected().unwrap().name, "PDF Reader by Xodo");
    assert!(rt.state.rows.iter().any(|r| r.name == "example.exe"));
    assert!(rt.state.rows.iter().any(|r| r.name == "Python"));
    assert_eq!(
        rt.state.status,
        "3 software groups / 2 system/support hidden"
    );
    toggle_system(&mut rt);
    assert_eq!(rt.state.rows.len(), 5);
    assert_eq!(
        rt.state.status,
        "5 software groups / 2 system/support included"
    );
    toggle_system(&mut rt);
    assert_eq!(rt.state.rows.len(), 3);
    assert_eq!(
        rt.state
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected)
            .count(),
        5
    );
}

#[test]
fn reconciled_sidebar_keeps_each_observation_and_its_query_after_switching() {
    for size in [(1024.0, 640.0), (1280.0, 800.0)] {
        let mut rt = runtime(size);
        rt.update(|s, tx| {
            super::reconciled_scene(s);
            tx.instant();
        });
        rt.frame(0.0);
        assert_eq!(rt.state.rows.len(), 3);
        assert_eq!(
            rt.state.rows.iter().map(|r| r.members.len()).sum::<usize>(),
            7
        );
        assert_eq!(rt.state.selected_observations().len(), 2);
        let python = rt
            .state
            .catalog
            .packages
            .iter()
            .find(|p| p.detected && p.reference_id.as_deref() == Some("python"))
            .unwrap()
            .id
            .clone();
        rt.update(|s, _| s.select_package(&python));
        rt.frame(0.0);
        assert_eq!(rt.state.selected_observations().len(), 3);
        assert_eq!(
            rt.state.selected().unwrap().installed.as_deref(),
            Some("3.11.9")
        );
        rt.state.live.sessions.get_mut(&python).unwrap().from = TextEdit::new("manual range");
        tap(&mut rt, "selected-observation-trigger");
        choose_observation(&mut rt, 1);
        assert_eq!(
            rt.state.selected().unwrap().installed.as_deref(),
            Some("3.14.2")
        );
        assert_eq!(rt.state.rows.len(), 3);
        tap(&mut rt, "selected-observation-trigger");
        choose_observation(&mut rt, 0);
        assert_eq!(rt.state.selected().unwrap().id, python);
        assert_eq!(rt.state.live.sessions[&python].from.text, "manual range");
    }
}

#[test]
fn component_filter_and_observation_switching_preserve_all_evidence() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, tx| {
        super::grouped_scene(s);
        tx.instant();
    });
    rt.frame(0.0);
    assert_eq!(
        rt.state
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected)
            .count(),
        7
    );
    assert_eq!(rt.state.rows.len(), 3);
    let selected = rt.state.selected().unwrap().id.clone();
    toggle_system(&mut rt);
    assert_eq!(rt.state.rows.len(), 5);
    assert_eq!(rt.state.selected().unwrap().id, selected);
    toggle_system(&mut rt);
    assert_eq!(rt.state.rows.len(), 3);
    assert_eq!(rt.state.selected_observations().len(), 2);
    tap(&mut rt, "selected-observation-trigger");
    choose_observation(&mut rt, 1);
    assert_eq!(
        rt.state.selected().unwrap().installed.as_deref(),
        Some("3.7.5")
    );
    assert_eq!(rt.state.rows.len(), 3);
    assert_eq!(
        rt.state
            .catalog
            .packages
            .iter()
            .filter(|p| p.detected)
            .count(),
        7
    );
    rt.state.query = TextEdit::new("chocolatey");
    rt.state.refilter();
    assert!(
        rt.state
            .visible
            .iter()
            .any(|&i| rt.state.rows[i].name == "Audacity")
    );
}

#[test]
fn detected_packages_render_real_versions_and_unmapped_entries_stay_visible() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s,tx|{
        let scan=releasescout_engine::inventory::parse(releasescout_engine::inventory::Manager::Scoop,false,&serde_json::json!({"installed":{"apps":[{"Name":"gh","Version":"2.0.0","Source":"main"},{"Name":"unmapped-fixture-tool","Version":"1.0.0","Source":"extras"}]}}).to_string()).unwrap();
        s.accept_scan(scan);tx.instant();
    });
    rt.frame(0.0);
    assert_eq!(rt.state.visible.len(), 2);
    let ids: Vec<_> = rt.state.rows.iter().map(|r| r.id.clone()).collect();
    for id in ids {
        tap(&mut rt, &id);
        assert!(rt.find("selected-package").is_some());
        assert!(rt.state.selected().unwrap().installed.is_some());
        if rt.state.selected().unwrap().name == "Unmapped Fixture Tool" {
            assert!(rt.find("source-discovery-status").is_some());
            assert_eq!(
                rt.state.source_status(),
                "No release-note hints in installed metadata"
            );
        }
    }
}

#[test]
fn demo_fixtures_have_two_synthetic_notes_and_no_installation_data() {
    let catalog: super::model::Catalog =
        serde_json::from_str(include_str!("../fixtures/demo-releases.json")).unwrap();
    assert_eq!(catalog.packages.len(), 6);
    for p in &catalog.packages {
        let repository = p.repository.as_deref().unwrap();
        assert!(repository.starts_with("https://example.invalid/demo/"));
        assert!(p.installed.is_none() && p.target.is_none() && p.manager.is_none());
        assert_eq!(p.releases.len(), 2);
        for r in &p.releases {
            assert!(r.url.starts_with(&format!("{repository}/releases/tag/")));
            assert!(!r.markdown.trim().is_empty());
        }
    }
}

#[test]
fn filtering_selects_only_a_visible_package_and_handles_empty_results() {
    let mut s = State::demo();
    s.query = TextEdit::new("SuPaBaSe");
    s.refilter();
    assert_eq!(s.visible.len(), 1);
    assert_eq!(s.selected().unwrap().id, "supabase");
    s.query = TextEdit::new("nothing-matches");
    s.refilter();
    assert!(s.selected().is_none());
    assert!(s.selected_markdown().is_empty());
    use releasescout_engine::inventory::{Manager, parse};
    let installed = serde_json::json!({
        "installed":{"apps":[{"Name":"gh","Version":"2.0.0","Source":"main"}]}
    });
    s.accept_scan(parse(Manager::Scoop, false, &installed.to_string()).unwrap());
    s.query = TextEdit::new("scoop");
    s.refilter();
    assert_eq!(s.selected().unwrap().provider, Some(Manager::Scoop));
}

#[test]
fn sorting_keeps_the_selected_identity() {
    let mut s = State::demo();
    let selected = s.table.cursor().unwrap().to_owned();
    let first = s.rows[s.visible[0]].name.clone();
    s.table.set_sort("name", SortDir::Descending);
    s.sort_visible();
    assert_ne!(s.rows[s.visible[0]].name, first);
    assert_eq!(s.table.cursor(), Some(selected.as_str()));
}

#[test]
fn clicking_a_row_changes_the_reader() {
    let mut rt = runtime((1280.0, 800.0));
    rt.update(|_, tx| tx.scroll_into_view("bun"));
    rt.frame(0.0);
    tap(&mut rt, "bun");
    assert_eq!(rt.state.selected().unwrap().id, "bun");
    assert!(rt.find("missing-notes").is_some());
}

#[test]
fn sidebar_rows_size_to_content_and_keep_badges_inside_without_empty_notes_column() {
    for dark in [false, true] {
        let mut s = State::new();
        {
            super::provider_badge::scene(&mut s);
            s.dark = dark;
            for row in &mut s.rows {
                row.name = format!("{} with a deliberately long software name", row.name);
            }
            for p in &mut s.catalog.packages {
                p.installed = Some("2026.11.12-build.123456789".into());
            }
        }
        let mut rt = Runtime::with_text(
            s,
            ui::view,
            (1024., 640.),
            Env {
                system_dark: dark,
                scale: 1.,
            },
            rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES),
        );
        // Virtual rows settle their content measurements after fixture changes.
        for _ in 0..6 {
            rt.frame(1. / 60.);
        }
        let elements = rt.elements();
        assert!(
            elements
                .iter()
                .all(|e| e.key.as_deref() != Some("sidebar-note-count"))
        );
        let [x, y, w, h] = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("viewport"))
            .unwrap()
            .rect;
        for e in elements
            .iter()
            .filter(|e| e.role == "row" && e.key.as_deref() != Some("header"))
        {
            assert!(e.rect[3] >= 32. + 2. * super::sidebar::ROW_PADDING_Y);
            assert!(e.rect[0] >= x && e.rect[0] + e.rect[2] <= x + w + 0.5);
            let inside = |key: &str| {
                elements
                    .iter()
                    .find(|child| {
                        child.key.as_deref() == Some(key)
                            && child.rect[1] >= e.rect[1]
                            && child.rect[1] + child.rect[3] <= e.rect[1] + e.rect[3]
                    })
                    .unwrap_or_else(|| panic!("missing {key} inside sidebar row {:?}", e.key))
            };
            let title = inside("sidebar-name");
            let version = inside("sidebar-version");
            let icon = inside("sidebar-icon");
            assert!(version.rect[1] >= title.rect[1] + title.rect[3]);
            assert!(title.rect[0] >= icon.rect[0] + icon.rect[2] + 10.);
            assert!(title.rect[2] >= e.rect[2] - 60.);
            assert!(version.rect[2] <= 76.);
            assert!(version.rect[0] > title.rect[0]);
            assert!(version.rect[0] + version.rect[2] <= e.rect[0] + e.rect[2]);
        }
        for e in elements.iter().filter(|e| {
            e.key
                .as_deref()
                .is_some_and(|key| key.starts_with("provider-badge-"))
                && e.rect[0] < x + w
        }) {
            assert!(e.rect[0] >= x && e.rect[0] + e.rect[2] <= x + w + 0.5);
            if e.rect[1] >= y && e.rect[1] + e.rect[3] <= y + h {
                assert_eq!(e.rect[3], 18.0);
            }
        }
    }
}

#[test]
fn description_editor_can_save_cancel_validate_and_restore_in_both_themes() {
    for dark in [false, true] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s, _| {
            s.dark = dark;
            s.select_package("gh");
        });
        rt.frame(0.);
        tap(&mut rt, "edit-description");
        rt.frame(1.);
        assert!(rt.find("description-editor-dialog").is_some());
        rt.key(KeyEvent {
            key: Key::Text("Personal GitHub notes".into()),
            mods: Mods::default(),
        });
        rt.frame(1.);
        assert_eq!(
            rt.state.description_editor.as_ref().unwrap().text.text,
            "Personal GitHub notes"
        );
        tap(&mut rt, "description-save");
        rt.frame(2.);
        assert!(rt.state.description_editor.is_none());
        assert_eq!(
            super::description_ui::selected(&rt.state),
            Some("Personal GitHub notes")
        );
        assert!(rt.find("selected-description").is_some());
        tap(&mut rt, "edit-description");
        rt.frame(3.);
        rt.update(|s, _| {
            s.description_editor.as_mut().unwrap().text = TextEdit::new("x".repeat(241))
        });
        rt.frame(3.);
        tap(&mut rt, "description-save");
        assert!(rt.state.description_editor.is_some());
        assert_eq!(
            super::description_ui::selected(&rt.state),
            Some("Personal GitHub notes")
        );
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(4.);
        assert!(rt.state.description_editor.is_none());
        tap(&mut rt, "edit-description");
        rt.frame(5.);
        rt.update(|s, _| {
            s.description_editor.as_mut().unwrap().text = TextEdit::new("Do not save this")
        });
        rt.frame(5.);
        tap(&mut rt, "description-cancel");
        assert_eq!(
            super::description_ui::selected(&rt.state),
            Some("Personal GitHub notes")
        );
        tap(&mut rt, "edit-description");
        rt.frame(6.);
        tap(&mut rt, "description-reset");
        assert!(rt.state.description_editor.is_none());
        assert!(super::description_ui::selected(&rt.state).is_none());
        let actions: Vec<_> = rt
            .elements()
            .into_iter()
            .filter(|e| {
                matches!(
                    e.key.as_deref(),
                    Some(
                        "edit-description"
                            | "choose-package-icon"
                            | "reset-package-icon"
                            | "copy-notes"
                            | "open-release"
                    )
                )
            })
            .collect();
        assert_eq!(actions.len(), 5);
        for pair in actions.windows(2) {
            assert!(pair[0].rect[0] + pair[0].rect[2] <= pair[1].rect[0]);
        }
    }
}

#[test]
fn sidebar_descriptions_clamp_to_one_line_with_metadata_below_the_icon_and_text() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut state = State::new();
        super::sidebar_info::description_scene(&mut state);
        state.dark = dark;
        let mut rt = Runtime::with_text(
            state,
            ui::view,
            size,
            Env {
                system_dark: dark,
                scale: 1.,
            },
            rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES),
        );
        for _ in 0..6 {
            rt.frame(1. / 60.);
        }
        let elements = rt.elements();
        let mut heights = Vec::new();
        let mut descriptions = 0;
        for entry in elements
            .iter()
            .filter(|e| e.role == "row" && e.key.as_deref() != Some("header"))
        {
            let inside = |key: &str| {
                elements.iter().find(|child| {
                    child.key.as_deref() == Some(key)
                        && child.rect[1] >= entry.rect[1]
                        && child.rect[1] + child.rect[3] <= entry.rect[1] + entry.rect[3]
                })
            };
            let title = inside("sidebar-name").unwrap();
            let version = inside("sidebar-version").unwrap();
            let icon = inside("sidebar-icon").unwrap();
            if let Some(desc) = inside("sidebar-description") {
                descriptions += 1;
                assert!(desc.rect[3] > 0. && desc.rect[3] <= 17.5, "{:?}", desc.rect);
                assert_eq!(desc.rect[0], title.rect[0]);
                assert!(desc.rect[1] >= title.rect[1] + title.rect[3]);
                assert!(desc.rect[1] - title.rect[1] - title.rect[3] <= 2.5);
                assert!(version.rect[1] >= desc.rect[1] + desc.rect[3]);
                let center = (title.rect[1] + desc.rect[1] + desc.rect[3]) / 2.;
                assert!((icon.rect[1] + icon.rect[3] / 2. - center).abs() < 1.);
                assert!(desc.rect[0] + desc.rect[2] <= entry.rect[0] + entry.rect[2]);
            }
            assert!(version.rect[1] >= icon.rect[1] + icon.rect[3]);
            heights.push(entry.rect[3]);
        }
        assert_eq!(descriptions, 2);
        heights.sort_by(f32::total_cmp);
        assert_eq!(heights.len(), 3);
        assert!(heights[1] > heights[0] + 4.);
        assert!((heights[2] - heights[1]).abs() < 1.);
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("This deliberately long fixture")
        );
    }
}

#[test]
fn sidebar_download_sits_beside_its_provider_and_hover_previews_without_executing() {
    for dark in [false, true] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s, _| {
            crate::sidebar_info::scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        let elements = rt.elements();
        let action = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("sidebar-update"))
            .unwrap();
        let title = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("sidebar-name"))
            .unwrap();
        assert!(action.rect[1] >= title.rect[1] + title.rect[3]);
        assert_eq!(action.help.as_deref(), Some("scoop update 'main/gh'"));
        let badge = elements
            .iter()
            .find(|e| {
                e.key.as_deref() == Some("provider-badge-scoop")
                    && e.rect[1] >= title.rect[1]
                    && e.rect[1] <= action.rect[1] + action.rect[3]
            })
            .unwrap();
        assert!(action.rect[0] >= badge.rect[0] + badge.rect[2]);
        rt.pointer_move(action.rect[0] + 11., action.rect[1] + 11.);
        rt.frame(0.2);
        assert_eq!(
            rt.help_tooltip().map(|t| t.0),
            Some("scoop update 'main/gh'")
        );
        assert!(rt.state.sidebar_actions.command.is_none());
        tap(&mut rt, "sidebar-update");
        assert!(rt.state.sidebar_actions.command.is_some());
        assert!(!rt.state.inventory.work.is_pending());
        assert!(rt.state.inventory.discovery.iter().all(|w| !w.is_pending()));
    }
}

#[test]
fn download_icons_render_major_minor_patch_colors_and_versions_stay_neutral() {
    use crate::sidebar_info::UpdateSize;
    for dark in [false, true] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s, _| {
            crate::sidebar_info::update_colors_scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        let k = crate::theme::kit(dark);
        let elements = rt.elements();
        let actions: Vec<_> = elements
            .iter()
            .filter(|e| e.key.as_deref() == Some("sidebar-update"))
            .collect();
        assert_eq!(actions.len(), 3);
        for action in actions {
            let size = match action.help.as_deref().unwrap() {
                "scoop update 'main/gh'" => UpdateSize::Major,
                "scoop update 'main/pnpm'" => UpdateSize::Patch,
                "scoop update 'main/uv'" => UpdateSize::Minor,
                command => panic!("Unexpected command: {command}"),
            };
            let glyph = elements
                .iter()
                .find(|e| {
                    e.key.as_deref() == Some("action-icon")
                        && e.path.starts_with(&format!("{}/", action.path))
                })
                .unwrap();
            assert_eq!(glyph.text_color, Some(size.color(dark)));
            assert_ne!(glyph.text_color, Some(k.c.text_muted));
        }
        for version in elements
            .iter()
            .filter(|e| e.key.as_deref() == Some("sidebar-version"))
        {
            assert_eq!(version.text_color, Some(k.c.text_muted));
        }
        let accessibility = rt.accessibility_json().to_string();
        for name in ["Major update", "Minor update", "Patch update"] {
            assert!(accessibility.contains(name));
        }
        rt.update(|s, _| {
            let record = s
                .inventory
                .scans
                .iter_mut()
                .flat_map(|scan| &mut scan.packages)
                .find(|p| p.id == "pnpm")
                .unwrap();
            record.available = Some("10.0.1.2".into());
            s.list_revision += 1;
        });
        rt.frame(1.);
        let elements = rt.elements();
        let action = elements
            .iter()
            .find(|e| {
                e.key.as_deref() == Some("sidebar-update")
                    && e.help.as_deref() == Some("scoop update 'main/pnpm'")
            })
            .unwrap();
        let glyph = elements
            .iter()
            .find(|e| {
                e.key.as_deref() == Some("action-icon")
                    && e.path.starts_with(&format!("{}/", action.path))
            })
            .unwrap();
        assert_eq!(
            glyph.text_color,
            Some(k.c.text_muted),
            "Four-part versions must not receive SemVer colors"
        );
        let package = rt
            .state
            .catalog
            .packages
            .iter()
            .find(|p| p.observation.as_ref().is_some_and(|o| o.id == "pnpm"))
            .unwrap()
            .clone();
        rt.update(|s, _| {
            s.catalog
                .packages
                .iter_mut()
                .find(|p| p.id == package.id)
                .unwrap()
                .observation
                .as_mut()
                .unwrap()
                .confirmed_update = false
        });
        assert!(
            crate::sidebar_actions::upgrade_size(
                &rt.state,
                rt.state
                    .catalog
                    .packages
                    .iter()
                    .find(|p| p.id == package.id)
                    .unwrap()
            )
            .is_none()
        );
    }
}

#[test]
fn software_provider_choice_activates_collapsible_sections_without_duplicates() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, _| {
            crate::sidebar_groups::provider_scene(s);
            s.table.set_sort("name", SortDir::Ascending);
            s.sort_visible();
            s.dark = dark;
        });
        rt.frame(1.);
        assert!(!crate::sidebar_groups::grouped(&rt.state));
        tap(&mut rt, "sidebar-sort-trigger");
        tap(&mut rt, "sidebar-sort-option-manager");
        rt.frame(0.016);
        assert!(crate::sidebar_groups::grouped(&rt.state));
        for id in ["pnpm", "scoop"] {
            assert!(
                rt.find(&format!("sidebar-group-software-provider-{id}-toggle"))
                    .is_some()
            );
        }
        let multiple = "sidebar-group-software-multiple-toggle";
        assert!(rt.find(multiple).is_some());
        assert_eq!(crate::sidebar_groups::shown(&rt.state).len(), 4);
        tap(&mut rt, multiple);
        rt.frame(0.016);
        assert_eq!(crate::sidebar_groups::shown(&rt.state).len(), 3);
        rt.key(KeyEvent {
            key: Key::Right,
            mods: Mods::default(),
        });
        rt.frame(0.016);
        assert_eq!(crate::sidebar_groups::shown(&rt.state).len(), 4);
        tap(&mut rt, "sidebar-sort-trigger");
        tap(&mut rt, "sidebar-sort-option-name");
        rt.frame(0.016);
        assert!(!crate::sidebar_groups::grouped(&rt.state));
        assert!(rt.find(multiple).is_none());
        assert_eq!(crate::sidebar_groups::shown(&rt.state).len(), 4);
    }
}

#[test]
fn description_hover_waits_and_scrolling_cancels_pending_and_open_previews() {
    let mut rt = runtime((1024., 640.));
    rt.update(|s, _| {
        s.set_dataset(Dataset::Stress);
        for p in &mut s.catalog.packages {
            p.description = Some("A **scrolling** description".into());
        }
        s.list_revision += 1;
    });
    rt.frame(1.);
    let description = |rt: &Runtime<State>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("sidebar-description") && e.visible[3] >= 16.)
            .unwrap()
            .rect
    };
    let desc = description(&rt);
    let (x, y) = (desc[0] + 5., desc[1] + 8.);
    rt.pointer_move(x, y);
    rt.frame(0.2);
    assert!(rt.find("description-preview").is_none());
    rt.wheel_event(WheelEvent::new(0., -20.).at(x, y).precise(true));
    rt.frame(0.45);
    assert!(rt.find("description-preview").is_none());
    rt.pointer_move(1000., 20.);
    rt.frame(0.7);
    let desc = description(&rt);
    let (x, y) = (desc[0] + 5., desc[1] + 8.);
    rt.pointer_move(x, y);
    rt.frame(0.3);
    assert!(rt.find("description-preview").is_none());
    rt.frame(0.35);
    assert!(rt.find("description-preview").is_some());
    rt.wheel_event(WheelEvent::new(0., -30.).at(x, y).precise(true));
    rt.frame(0.016);
    assert!(rt.find("description-preview").is_none());
    assert!(rt.state.description_hover.is_none());
}

#[test]
fn truncated_sidebar_descriptions_show_the_full_description_on_hover() {
    let mut rt = runtime((1024., 640.));
    rt.update(|s, _| crate::sidebar_info::description_scene(s));
    rt.frame(1.);
    let desc = rt
        .elements()
        .into_iter()
        .find(|e| {
            e.key.as_deref() == Some("sidebar-description")
                && e.text
                    .as_ref()
                    .is_some_and(|text| text.contains("deliberately long fixture"))
        })
        .unwrap();
    assert!(desc.help.is_none());
    rt.pointer_move(
        desc.rect[0] + desc.rect[2] / 2.,
        desc.rect[1] + desc.rect[3] / 2.,
    );
    rt.frame(0.);
    rt.frame(0.2);
    assert!(
        rt.find("description-preview").is_none(),
        "description opened before its hover delay"
    );
    rt.frame(0.45);
    assert!(rt.help_tooltip().is_none());
    let elements = rt.elements();
    let (popup_index, popup) = elements
        .iter()
        .enumerate()
        .find(|(_, e)| e.key.as_deref() == Some("description-preview"))
        .expect("description preview did not open");
    let [_, _, w, h] = popup.rect;
    assert!(w > desc.rect[2] && h > desc.rect[3]);
    let full_text = desc.text.as_deref().unwrap();
    assert!(
        elements.iter().any(|e| {
            // Rune's Markdown renderer adds bidi isolates around compound words.
            e.text.as_deref().is_some_and(|text| {
                text.chars()
                    .filter(|c| !matches!(c, '\u{2066}'..='\u{2069}'))
                    .eq(full_text.chars())
            }) && e.visible[3] >= e.rect[3] - 0.5
                && std::iter::successors(e.parent, |&i| elements[i].parent)
                    .any(|i| i == popup_index)
        }),
        "the full description must fit inside its preview"
    );
}

#[test]
fn markdown_description_previews_render_fit_and_remain_hoverable() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, _| {
            crate::description_preview::scene(s);
            s.description_hover = None;
            s.dark = dark;
        });
        rt.frame(1.);
        let desc = rt
            .elements()
            .into_iter()
            .find(|e| {
                e.key.as_deref() == Some("sidebar-description")
                    && e.text
                        .as_deref()
                        .is_some_and(|t| t.starts_with("Fast Python tooling"))
            })
            .unwrap();
        assert!(desc.rect[3] <= 17.5);
        assert!(!desc.text.as_ref().unwrap().contains("**"));
        assert!(!desc.text.as_ref().unwrap().contains("https://"));
        rt.pointer_move(desc.rect[0] + 10., desc.rect[1] + 8.);
        rt.frame(1.1);
        let elements = rt.elements();
        let popup = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("description-preview"))
            .unwrap();
        let [x, y, w, h] = popup.rect;
        assert!(
            x >= 0. && y >= 0. && x + w <= size.0 && y + h <= size.1,
            "{:?}",
            popup.rect
        );
        assert!(w <= 321. && h <= 281.);
        let in_popup = |e: &&rune::ElementInfo| {
            e.rect[0] >= x && e.rect[1] >= y && e.rect[0] < x + w && e.rect[1] < y + h
        };
        assert!(elements.iter().filter(in_popup).any(|e| {
            e.text
                .as_deref()
                .is_some_and(|t| t.contains("Fast Python tooling"))
        }));
        assert!(
            elements
                .iter()
                .filter(in_popup)
                .any(|e| e.text.as_deref().is_some_and(|t| t.contains("uvx")))
        );
        assert!(elements.iter().filter(in_popup).any(|e| e.role == "link"));
        assert!(rt.help_tooltip().is_none());

        rt.pointer_move(x + w / 2., y + h / 2.);
        rt.frame(0.016);
        rt.frame(0.2);
        assert!(
            rt.find("description-preview").is_some(),
            "preview closed during pointer bridge"
        );
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(1.6);
        assert!(rt.state.description_hover.is_none());

        // A stale hover must not cover an open Info panel or eat its Escape.
        rt.update(|s, _| crate::description_preview::scene(s));
        rt.frame(2.);
        let row = rt.state.rows.iter().find(|r| r.name == "uv").unwrap();
        let details = crate::sidebar_actions::anchor(row, crate::sidebar_actions::Mode::Details);
        tap(&mut rt, &details);
        rt.frame(2.1);
        assert!(rt.find("description-preview").is_none());
        assert!(rt.find("sidebar-panel-description").is_some());
        assert!(
            !rt.elements()
                .iter()
                .any(|e| e.text.as_deref().is_some_and(|t| t.contains("**Fast")))
        );
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(2.3);
        assert!(rt.state.sidebar_actions.panel.is_none());
    }
}

#[test]
fn long_markdown_descriptions_scroll_inside_a_bounded_preview() {
    let mut rt = runtime((1024., 640.));
    rt.update(|s, _| {
        crate::description_preview::scene(s);
        s.description_hover = None;
        let row = s.rows.iter().find(|r| r.name == "uv").unwrap();
        s.catalog.packages[row.package].description = Some(
            (0..30)
                .map(|i| format!("Paragraph {i}: **Python tooling** with `uv`.\n\n"))
                .collect(),
        );
    });
    rt.frame(1.);
    let desc = rt
        .elements()
        .into_iter()
        .find(|e| {
            e.key.as_deref() == Some("sidebar-description")
                && e.text
                    .as_deref()
                    .is_some_and(|t| t.starts_with("Paragraph 0:"))
        })
        .unwrap();
    rt.pointer_move(desc.rect[0] + 10., desc.rect[1] + 8.);
    rt.frame(0.65);
    let popup = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("description-preview"))
        .unwrap();
    let [x, y, w, h] = popup.rect;
    assert!(h <= 280. && y + h <= 640.);
    let before = rt
        .elements()
        .into_iter()
        .find(|e| {
            e.path.contains("description-preview/")
                && e.text
                    .as_deref()
                    .is_some_and(|t| t.starts_with("Paragraph 0:"))
        })
        .unwrap()
        .rect[1];
    rt.pointer_move(x + w / 2., y + h / 2.);
    rt.frame(0.016);
    rt.wheel_event(
        WheelEvent::new(0., -120.)
            .at(x + w / 2., y + h / 2.)
            .precise(true),
    );
    rt.frame(0.016);
    let after = rt
        .elements()
        .into_iter()
        .find(|e| {
            e.path.contains("description-preview/")
                && e.text
                    .as_deref()
                    .is_some_and(|t| t.starts_with("Paragraph 0:"))
        })
        .unwrap()
        .rect[1];
    assert!(after < before, "description preview did not scroll");
    assert!(rt.find("description-preview").is_some());
    assert_eq!(rt.state.sidebar_scroll.position.get().offset[1], 0.);
    rt.pointer_move(1000., 20.);
    rt.frame(0.016);
    rt.frame(0.2);
    assert!(rt.state.description_hover.is_none());
}

#[test]
fn markdown_description_previews_disappear_when_their_row_is_filtered_out() {
    let mut rt = runtime((1024., 640.));
    rt.update(|s, _| crate::description_preview::scene(s));
    rt.frame(1.);
    assert!(rt.find("description-preview").is_some());
    rt.update(|s, _| {
        s.query = TextEdit::new("nothing matches this");
        s.refilter();
    });
    rt.frame(2.);
    assert!(rt.find("description-preview").is_none());
}

#[test]
fn sidebar_hover_hints_are_compact_and_full_diagnostics_stay_in_info() {
    for dark in [false, true] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s, _| {
            crate::sidebar_info::scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        let elements = rt.elements();
        for el in elements.iter().filter(|e| e.rect[0] < 280.) {
            if let Some(hint) = &el.help {
                assert!(!hint.contains('\n'), "multiline sidebar hint: {hint}");
                assert!(
                    hint.chars().count() <= 160,
                    "oversized sidebar hint: {hint}"
                );
                assert!(!hint.contains("Inventory observed:"));
                assert!(!hint.contains("Release-note check failed:"));
            }
        }
        let version = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("sidebar-version"))
            .unwrap();
        let expected = version.help.clone().unwrap();
        assert!(expected.contains("view version history"));
        rt.pointer_move(
            version.rect[0] + version.rect[2] / 2.,
            version.rect[1] + version.rect[3] / 2.,
        );
        rt.frame(0.);
        rt.frame(0.2);
        let (hint, rect) = rt.help_tooltip().expect("native tooltip on hover");
        assert_eq!(hint, expected);
        assert!(rect[2] <= 280. && rect[3] <= 44., "{rect:?}");
        assert!(rect[0] >= 0. && rect[0] + rect[2] <= 1024.);

        let row = rt.state.rows.iter().find(|r| r.name == "pnpm").unwrap();
        let key = format!("sidebar-info-{}", crate::activity::hash(&row.id));
        tap(&mut rt, &key);
        rt.frame(1.);
        let details = rt.accessibility_json().to_string();
        assert!(details.contains("Temporary release-source request failure"));
        assert!(rt.find("package-info-observed").is_none());
        tap(&mut rt, "package-info-technical-toggle");
        assert!(rt.find("package-info-observed").is_some());
        assert!(rt.accessibility_json().to_string().contains("Last scan"));
    }
}

#[test]
fn sidebar_unread_and_update_indicators_follow_reader_navigation() {
    for dark in [false, true] {
        let mut rt = runtime((1024.0, 640.0));
        rt.update(|s, _| {
            super::sidebar_info::scene(s);
            s.dark = dark;
        });
        rt.frame(0.0);
        let gh = rt
            .state
            .rows
            .iter()
            .find(|r| r.name == "GitHub CLI")
            .unwrap()
            .id
            .clone();
        assert!(rt.find("sidebar-update").is_some());
        assert!(rt.find("sidebar-source-error").is_some());
        assert!(rt.find("sidebar-unread-count").is_some());
        assert!(rt.find("sidebar-version").is_some());
        assert!(rt.find("sidebar-release-age").is_none());
        for e in rt.elements().into_iter().filter(|e| {
            e.key.as_deref().is_some_and(|k| {
                matches!(
                    k,
                    "sidebar-unread-count"
                        | "sidebar-update"
                        | "sidebar-source-error"
                        | "sidebar-release-age"
                )
            })
        }) {
            assert!(
                e.rect[0] >= 0.0
                    && e.rect[0] + e.rect[2] <= 304.0
                    && e.rect[1] + e.rect[3] <= 640.0,
                "{:?}",
                e.rect
            );
        }
        tap(&mut rt, &gh);
        let row = rt.state.rows.iter().find(|r| r.id == gh).unwrap();
        assert_eq!(
            super::sidebar_info::summary(&rt.state, row, super::sidebar_info::today()).unread,
            0
        );
        assert!(rt.find("sidebar-update").is_some());
        let row = rt.state.rows.iter().find(|r| r.name == "pnpm").unwrap();
        assert_eq!(
            super::sidebar_info::summary(&rt.state, row, super::sidebar_info::today()).unread,
            1
        );
        let info = super::sidebar_info::summary(&rt.state, row, super::sidebar_info::today());
        assert!(info.age.is_some());
        assert!(info.tooltip.contains("Latest retrieved dated release:"));
        rt.update(|s, _| {
            for p in &mut s.catalog.packages {
                p.installed = None;
            }
        });
        rt.frame(0.0);
        assert!(rt.find("sidebar-release-age").is_some());
    }
}

#[test]
fn keyboard_navigation_changes_selection() {
    let mut rt = runtime((1280.0, 800.0));
    tap(&mut rt, "agent-browser");
    let before = rt.state.table.cursor().unwrap().to_owned();
    rt.key(KeyEvent {
        key: Key::Down,
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert_ne!(rt.state.table.cursor(), Some(before.as_str()));
}

#[test]
fn stress_dataset_mounts_a_bounded_number_of_rows() {
        let mut s = State::demo();
    s.set_dataset(Dataset::Stress);
    let mut rt = Runtime::with_text(
        s,
        ui::view,
        (1280.0, 800.0),
        Env {
            system_dark: false,
            scale: 1.0,
        },
        rune::text::TextSystem::new(crate::fonts::FACES, crate::fonts::FAMILIES),
    );
    rt.frame(0.0);
    assert_eq!(rt.state.visible.len(), 10_000);
    let tree = rt.accessibility_json();
    assert!(
        tree.to_string().len() < 250_000,
        "virtualized rows must not expose all 10,000 entries"
    );
}

#[test]
fn core_controls_are_present_at_minimum_window_size() {
    let rt = runtime((1024.0, 640.0));
    for key in [
        "package-filter",
        "packages",
        "copy-notes",
        "open-release",
        "note-mode",
        "global-theme",
        "global-refresh",
        "global-activity",
        "status-trigger",
    ] {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        assert!(
            (0.0..1024.0).contains(&x) && (0.0..640.0).contains(&y),
            "{key} is outside the window: {x}, {y}"
        );
    }
}

#[test]
fn sidebar_starts_with_sections_without_a_duplicate_library_heading_or_count() {
    let rt = runtime((1024., 640.));
    let elements = rt.elements();
    assert!(
        !elements
            .iter()
            .any(|e| e.text.as_deref() == Some("Library"))
    );
    assert!(!elements.iter().any(|e| {
        e.help
            .as_deref()
            .is_some_and(|help| help.starts_with("Matching / total"))
    }));
    let tabs = elements
        .iter()
        .find(|e| e.key.as_deref() == Some("library-section"))
        .unwrap();
    assert!(
        (tabs.rect[1] - 68.).abs() < 0.5,
        "Tabs start below the app header, without a heading gap: {:?}",
        tabs.rect
    );
    assert!(rt.find("status-trigger").is_some());
}

#[test]
fn global_chrome_fits_and_does_not_reserve_footer_space() {
    for (width, open) in [(304., true), (420., true), (304., false)] {
        for size in [(1024., 640.), (1280., 800.)] {
            let mut rt = runtime(size);
            rt.update(|s, _| {
                s.pane.set_width(width);
                s.pane.set_open(open);
            });
            rt.frame(1.);
            let elements = rt.elements();
            let rect = |key: &str| {
                elements
                    .iter()
                    .find(|e| e.key.as_deref() == Some(key))
                    .unwrap()
                    .rect
            };
            let nav = rect("workspace-view");
            let controls = rect("global-controls");
            assert!(
                nav[0] + nav[2] <= controls[0],
                "Navigation overlaps controls: {nav:?}, {controls:?}"
            );
            for key in ["global-activity", "global-refresh", "global-theme"] {
                let [x, y, w, h] = rect(key);
                assert!(
                    x >= 0. && y >= 0. && x + w <= size.0 + 0.5 && y + h <= 48.,
                    "{key}: {:?}",
                    rect(key)
                );
            }
            let pane = rect("notes-page");
            assert!(
                (pane[1] + pane[3] - size.1).abs() < 0.5,
                "Footer must not reserve viewport height: {pane:?}"
            );
            let trigger = rect("status-trigger");
            assert!((trigger[0] + trigger[2] - size.0 + 12.).abs() < 0.5);
            assert!((trigger[1] + trigger[3] - size.1 + 12.).abs() < 0.5);
            assert!(trigger[2] <= 280.5);
            assert!(rt.find("view-options-button").is_none());
        }
    }
}

#[test]
fn integrated_titlebar_keeps_controls_clear_in_both_themes_and_pane_states() {
    for dark in [false, true] {
        for (width, open) in [(304., true), (420., true), (304., false)] {
            for size in [(1024., 640.), (1280., 800.)] {
                let mut rt = runtime(size);
                let mut insets = rune::ChromeInsets::default();
                insets.trailing = rune::Titlebar::CAPTION_BUTTON_WIDTH * 3.;
                insets.height = rune_kit::shell::TOOLBAR_HEIGHT;
                rt.set_chrome_insets(insets);
                rt.update(|s, _| {
                    s.dark = dark;
                    s.pane.set_width(width);
                    s.pane.set_open(open);
                });
                rt.frame(1.);
                let elements = rt.elements();
                let rect = |key: &str| {
                    elements
                        .iter()
                        .find(|e| e.key.as_deref() == Some(key))
                        .unwrap()
                        .rect
                };
                let nav = rect("workspace-view");
                let controls = rect("global-controls");
                assert!(nav[0] + nav[2] <= controls[0], "{nav:?}, {controls:?}");
                let clearance = rect("caption-clearance");
                assert_eq!(clearance[2], insets.trailing);
                for key in [
                    "global-search",
                    "global-activity",
                    "global-refresh",
                    "global-theme",
                ] {
                    let [x, y, w, h] = rect(key);
                    assert!(x >= nav[0] + nav[2] && y >= 0. && y + h <= 48.);
                    assert!(
                        x + w <= size.0 - insets.trailing - 8.,
                        "{key}: {:?}",
                        rect(key)
                    );
                    assert!(rt.find(key).is_some(), "{key} must remain actionable");
                }
                assert_eq!(rect("toolbar")[3], 48.);
                assert!((rect("notes-page")[1] - 48.).abs() < 0.5);
            }
        }
    }
}

#[test]
fn floating_status_lifts_on_hover_and_stays_pinned_with_keyboard_access() {
    let mut rt = runtime((1024., 640.));
    let (x, y) = rt.find("status-trigger").unwrap();
    rt.pointer_move(x, y);
    rt.frame(0.016);
    rt.frame(0.08);
    assert!(rt.find("status-details").is_none());
    rt.frame(0.08);
    rt.frame(1.);
    assert!(rt.state.chrome.status_hover);
    let details = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("status-details"))
        .unwrap()
        .rect;
    assert!(details[0] >= 0. && details[1] >= 0. && details[0] + details[2] <= 1024.);
    let trigger = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("status-trigger"))
        .unwrap()
        .rect;
    assert!(
        (trigger[1] + trigger[3] - 624.).abs() < 0.5,
        "Hover lifts dock by four points: {trigger:?}"
    );
    rt.pointer_move(details[0] + 20., details[1] + 20.);
    rt.frame(0.3);
    assert!(rt.find("status-details").is_some());
    rt.pointer_move(500., 60.);
    rt.frame(0.016);
    rt.frame(0.2);
    assert!(!rt.state.chrome.status_hover);
    tap(&mut rt, "status-trigger");
    assert!(rt.state.chrome.status_pinned);
    rt.pointer_move(500., 60.);
    rt.frame(0.5);
    assert!(rt.find("status-details").is_some());
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    rt.frame(1.);
    assert!(!rt.state.chrome.status_pinned);
    assert!(rt.find("status-details").is_none());
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("status-trigger") && e.focused)
    );
    rt.key(KeyEvent {
        key: Key::Text(" ".into()),
        mods: Mods::default(),
    });
    rt.frame(0.);
    assert!(rt.state.chrome.status_pinned);
    rt.frame(1.);
    tap(&mut rt, "status-inventory");
    assert!(rt.state.inventory.open);
    assert!(rt.state.inventory.last_request.is_none());
}

#[test]
fn prefetch_is_visible_in_the_header_and_activity_details() {
    let mut rt = runtime((1280., 760.));
    rt.update(|s, _| crate::note_prefetch::activity_scene(s));
    rt.frame(1.);
    assert!(rt.accessibility_json().to_string().contains("Loading update notes"));
    assert!(rt.find("activity-notes-loading").is_none());
    tap(&mut rt, "global-activity");
    assert!(!rt.state.inventory.open);
    assert!(rt.state.chrome.status_pinned);
    let elements = rt.elements();
    for (key, value) in [
        ("activity-notes-loading-count", "1"),
        ("activity-notes-ready-count", "11"),
        ("activity-notes-unavailable-count", "2"),
    ] {
        assert!(elements.iter().any(|e| e.key.as_deref() == Some(key) && e.text.as_deref() == Some(value)));
    }
}

#[test]
fn activity_labels_lead_and_counts_align_right_without_losing_status_messages() {
    for dark in [false, true] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s, _| {
            crate::sidebar_info::scene(s);
            s.dark = dark;
            s.activity.data.sidebar_filters[0].update = crate::sidebar_filters::Update::Available;
            s.refilter();
            s.status = s.inventory_summary();
            s.chrome.status_pinned = true;
        });
        rt.frame(1.);
        let elements = rt.elements();
        let get = |key: &str| {
            elements
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
        };
        let trigger = get("status-trigger").rect;
        let label = get("status-label");
        let count = get("status-count");
        assert_eq!(label.text.as_deref(), Some("Software"));
        assert_eq!(count.text.as_deref(), Some("1 / 3"));
        assert!(label.rect[0] + label.rect[2] <= count.rect[0]);
        assert!((count.rect[0] + count.rect[2] - trigger[0] - trigger[2] + 12.).abs() < 0.5);
        let panel = get("status-details").rect;
        for key in [
            "activity-entries",
            "activity-support",
            "activity-pending",
            "activity-inventory-issues",
            "activity-notices",
        ] {
            let row = get(key).rect;
            let label = get(&format!("{key}-label")).rect;
            let count = get(&format!("{key}-count")).rect;
            assert!(label[0] > row[0] + 14.);
            assert!(label[0] + label[2] <= count[0]);
            assert!(
                (count[0] + count[2] - panel[0] - panel[2] + 12.).abs() < 0.5,
                "{key} must be flush right"
            );
        }
        assert!(
            rt.find("status").is_none(),
            "Inventory summaries should be rows, not a duplicated paragraph"
        );
        rt.update(|s, _| s.status = "Release notes copied".into());
        rt.frame(1.);
        assert!(rt.find("status-count").is_none());
        let elements = rt.elements();
        assert!(
            elements
                .iter()
                .any(|e| e.key.as_deref() == Some("status-label")
                    && e.text.as_deref() == Some("Release notes copied"))
        );
        assert!(rt.find("status").is_some());
    }
}

#[test]
fn floating_status_overlay_does_not_block_navigation_or_background_content() {
    let mut rt = notes_runtime((1024., 640.));
    tap(&mut rt, "workspace-view/segment1");
    assert!(rt.state.inventory.open);
    tap(&mut rt, "workspace-view/segment0");
    assert!(!rt.state.inventory.open);
    tap(&mut rt, "note-mode/segment1");
    assert!(rt.state.mode == super::model::NoteMode::Source);
    assert!(!rt.state.inventory.loading);
    assert!(
        rt.state
            .inventory
            .discovery
            .iter()
            .all(|work| !work.is_pending())
    );
    tap(&mut rt, "global-activity");
    assert!(rt.state.inventory.open);
    assert!(rt.state.inventory.last_request.is_none());
}

#[test]
fn global_notice_opens_inventory_with_the_counted_message_visible() {
    let mut rt = runtime((1024., 640.));
    let message = "Saved notes unavailable: fixture cache error";
    rt.update(|s, _| s.note_cache.error = Some(message.into()));
    rt.frame(0.);
    assert_eq!(crate::app_chrome::notice_messages(&rt.state), [message]);
    tap(&mut rt, "global-activity");
    assert!(rt.state.inventory.open);
    assert!(rt.state.inventory.notices_open);
    assert!(rt.find("inventory-notice-details").is_some());
    assert!(rt.accessibility_json().to_string().contains(message));
}

#[test]
fn recovered_notes_stay_visible_and_full_diagnostics_remain_expandable() {
    use releasescout_engine::{Basis, Evidence, Note, Report};
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, tx| {
        s.select_package("gh");
        s.apply_engine_result(
            "gh".into(),
            Ok(Report {
                package: "gh".into(),
                stream: "main".into(),
                scope: "GitHub CLI".into(),
                basis: Basis::Version,
                status: "partial".into(),
                complete: false,
                notes: vec![Note {
                    title: "3.0.0".into(),
                    version: Some("3.0.0".into()),
                    date: None,
                    url: "https://example.com/release".into(),
                    body: "Fixed a startup crash.".into(),
                    authored_notes: true,
                    preview: false,
                }],
                warnings: (0..12)
                    .map(|i| format!("Diagnostic {i}: {}", "upstream evidence details ".repeat(5)))
                    .collect(),
                evidence: vec![Evidence {
                    url: "https://example.com/release".into(),
                    fetched_at: "2026-11-11T00:00:00Z".into(),
                    cache: "stale_cache_upstream_error".into(),
                }],
                attempts: vec![],
            }),
        );
        tx.instant();
    });
    rt.frame(0.0);
    assert!(rt.find("timeline-item-0").unwrap().1 < 640.0);
    assert!(rt.find("engine-completeness").is_some());
    tap(&mut rt, "engine-details");
    assert!(rt.state.live.sessions["gh"].details);
    tap(&mut rt, "engine-details");
    assert!(!rt.state.live.sessions["gh"].details);
    assert!(rt.find("timeline-item-0").unwrap().1 < 640.0);
}

#[test]
fn local_inventory_popover_preserves_reader_space_and_stays_separate_from_note_evidence() {
    for dark in [false, true] {
        let mut rt = runtime((1024., 640.));
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|s, tx| {
            crate::update_status::scene(s);
            s.dark = dark;
            let id = s.selected().unwrap().id.clone();
            s.live.sessions.get_mut(&id).unwrap().report = None;
            s.catalog
                .packages
                .iter_mut()
                .find(|p| p.id == id)
                .unwrap()
                .releases
                .clear();
            let p = s.catalog.packages.iter_mut().find(|p| p.id == id).unwrap();
            p.observation.as_mut().unwrap().root = Some("C:/packages/gh".into());
            tx.instant();
        });
        rt.frame(1.);
        let before = rt.find("missing-notes").unwrap().1;
        assert!(rt.find("engine-details").is_none());
        assert!(rt.find("update-status").is_none());
        tap(&mut rt, "local-inventory-trigger");
        assert!(rt.find("local-inventory-popover").is_some());
        assert!((rt.find("missing-notes").unwrap().1 - before).abs() < 1.);
        let popup = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("local-inventory-popover"))
            .unwrap()
            .rect;
        assert!(
            popup[0] >= 0.
                && popup[0] + popup[2] <= 1024.
                && popup[1] >= 0.
                && popup[1] + popup[3] <= 640.
        );
        assert!(rt.find("inventory-info-installed").is_some());
        assert!(rt.accessibility_json().to_string().contains("2.0.0"));
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("No confirmed newer candidate")
        );
        assert!(rt.find("inventory-check-ok").is_some());
        assert!(rt.find("inventory-info-technical").is_none());
        tap(&mut rt, "inventory-info-technical-toggle");
        assert!(rt.find("inventory-info-location").is_some());
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("C:/packages/gh")
        );
        let popup = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("local-inventory-popover"))
            .unwrap()
            .rect;
        rt.wheel_event(
            WheelEvent::new(0., -500.)
                .at(popup[0] + popup[2] / 2., popup[1] + popup[3] / 2.)
                .precise(true),
        );
        rt.frame(2.);
        rt.frame(3.);
        tap(&mut rt, "inventory-info-location-copy");
        assert_eq!(clipboard.text().unwrap().as_str(), "C:/packages/gh");
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(1.);
        assert!(rt.state.reader_controls.inventory_open.is_none());
        assert!(!rt.state.reader_controls.inventory_technical);
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("local-inventory-trigger") && e.focused)
        );
        tap(&mut rt, "local-inventory-trigger");
        assert!(rt.find("inventory-info-technical").is_none());
        rt.pointer_down(8., 630.);
        rt.pointer_up(8., 630.);
        rt.frame(1.);
        assert!(rt.find("local-inventory-popover").is_none());
        rt.update(|s, tx| {
            crate::update_status::scene(s);
            tx.instant();
        });
        rt.frame(1.);
        assert!(rt.find("engine-completeness").is_some());
        assert!(rt.find("local-inventory-trigger").is_some());
        let before = rt.find("timeline-item-0").unwrap().1;
        tap(&mut rt, "local-inventory-trigger");
        assert!((rt.find("timeline-item-0").unwrap().1 - before).abs() < 1.);
        assert!(!rt.state.engine_query().details);
        let local_text = rt
            .elements()
            .iter()
            .filter(|e| e.path.contains("local-inventory-popover/"))
            .filter_map(|e| e.text.as_deref())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !local_text.contains("2.2.0"),
            "A publisher release must not become an inventory candidate"
        );
        assert!(local_text.contains("No confirmed newer candidate"));
        assert!(!local_text.contains("Checked;"));
        rt.update(|s, _| {
            let id = s
                .catalog
                .packages
                .iter()
                .find(|p| {
                    p.observation
                        .as_ref()
                        .is_some_and(|o| o.id == "fixture-unchecked")
                })
                .unwrap()
                .id
                .clone();
            s.select_package(&id);
        });
        rt.frame(1.);
        assert!(rt.state.reader_controls.inventory_open.is_none());
        tap(&mut rt, "local-inventory-trigger");
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("Candidates not checked")
        );
        assert!(rt.find("inventory-check-unchecked").is_some());
        assert!(rt.find("inventory-check-ok").is_none());
        assert!(rt.find("inventory-info-technical").is_none());
        assert!(!rt.state.live.work.is_pending());
        assert!(!rt.state.live.history.is_pending());
    }
}

#[test]
fn sidebar_filter_shortcut_focuses_the_editable_input_and_typing_filters() {
    let mut rt = runtime((1280.0, 800.0));
    rt.key(KeyEvent {
        key: Key::Command('f'),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("input") && e.focused)
    );
    rt.key(KeyEvent {
        key: Key::Text("uv".into()),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert_eq!(rt.state.query.text, "uv");
    assert_eq!(rt.state.selected().unwrap().id, "uv");
}

#[test]
fn copy_button_writes_the_selected_notes_to_the_clipboard() {
    let mut rt = notes_runtime((1280.0, 800.0));
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    let expected = rt.state.selected_markdown();
    tap(&mut rt, "copy-notes");
    assert_eq!(clipboard.text().unwrap().as_str(), expected);
    assert_eq!(rt.state.status, "Release notes copied");
}

#[test]
fn inventory_workspace_combines_sources_and_keeps_hidden_selections_explicit() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|s, tx| {
            crate::inventory_view::workspace_scene(s);
            s.dark = dark;
            tx.instant();
        });
        rt.frame(1.);
        assert!(rt.find("inventory-sources").is_some());
        assert!(rt.find("package-filter").is_none());
        assert_eq!(crate::inventory_workspace::records(&rt.state).len(), 12);
        let gh = inventory_key(&rt.state, "gh");
        let winget = inventory_key(&rt.state, "GitHub.cli");
        tap(&mut rt, &format!("inventory-select-{gh}"));
        tap(&mut rt, &format!("inventory-select-{winget}"));
        assert_eq!(rt.state.inventory.workspace.selected.len(), 2);
        tap(&mut rt, "inventory-provider-scoop");
        assert!(
            !crate::inventory_workspace::filtered(&rt.state)
                .iter()
                .any(|r| r.package.manager == releasescout_engine::inventory::Manager::Scoop)
        );
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("2 selected / 1 hidden")
        );
        tap(&mut rt, "inventory-copy-ids");
        let text = clipboard.text().unwrap();
        assert!(text.contains("gh") && text.contains("GitHub.cli"));
        tap(&mut rt, "inventory-review");
        assert_eq!(
            rt.state
                .inventory
                .workspace
                .review
                .as_ref()
                .unwrap()
                .items
                .len(),
            2
        );
        assert!(!crate::command::can_open(&rt.state));
        rt.frame(2.);
        tap(&mut rt, "inventory-review-copy");
        let commands = clipboard.text().unwrap();
        assert!(
            commands.contains("scoop update 'main/gh'"),
            "commands={commands:?}; status={}",
            rt.state.status
        );
        assert!(commands.contains("winget upgrade --id 'GitHub.cli'"));
        tap(&mut rt, "inventory-review-close");
        tap(&mut rt, "inventory-sources-all");
        tap(&mut rt, "inventory-columns-trigger");
        tap(&mut rt, "inventory-column-Id");
        assert!(
            rt.state
                .inventory
                .workspace
                .columns
                .contains(&crate::inventory_workspace::Column::Id)
        );
        tap(&mut rt, "inventory-density/segment1");
        assert!(!rt.state.inventory.workspace.compact);
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(2.);
        assert!(!rt.state.inventory.workspace.columns_open);
        tap(&mut rt, "inventory-sort-Installed");
        assert_eq!(
            rt.state.inventory.workspace.sort,
            crate::inventory_workspace::Column::Installed
        );
        tap(&mut rt, "inventory-sort-Installed");
        assert!(rt.state.inventory.workspace.descending);
        assert_eq!(rt.state.inventory.workspace.selected.len(), 2);
        assert!(!rt.state.inventory.work.is_pending() && !rt.state.live.work.is_pending());
    }
}

#[test]
fn inventory_context_menu_targets_the_clicked_observation_without_changing_selection() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|s, tx| {
            crate::inventory_view::workspace_scene(s);
            s.dark = dark;
            tx.instant();
        });
        rt.frame(1.);
        let gh = inventory_key(&rt.state, "gh");
        let fzf = inventory_key(&rt.state, "fzf");
        tap(&mut rt, &format!("inventory-select-{fzf}"));
        let row = format!("inventory-row-{gh}");
        let right_click = |rt: &mut Runtime<State>| {
            let (x, y) = rt.find(&row).unwrap();
            rt.secondary_down(x, y);
            rt.secondary_up(x, y);
            rt.frame(1.);
        };
        right_click(&mut rt);
        assert_eq!(rt.state.inventory.workspace.selected.len(), 1);
        assert!(rt.state.inventory.workspace.selected.contains(&fzf));
        let menu = rt.elements().into_iter().find(|e| e.role == "menu").unwrap().rect;
        assert!(menu[0] >= 0. && menu[0] + menu[2] <= size.0);
        assert!(menu[1] >= 0. && menu[1] + menu[3] <= size.1);
        tap_menu_item(&mut rt, "Copy package ID");
        assert_eq!(clipboard.text().unwrap().as_str(), "gh");
        assert!(rt.state.inventory.workspace.context.is_none());
        right_click(&mut rt);
        tap_menu_item(&mut rt, "Copy installed version");
        assert_eq!(clipboard.text().unwrap().as_str(), "2.0.0");
        right_click(&mut rt);
        tap_menu_item(&mut rt, "Review upgrade");
        let review = rt.state.inventory.workspace.review.as_ref().unwrap();
        assert_eq!(review.items.len(), 1);
        assert_eq!(review.items[0].id, "gh");
        assert!(review.items[0].command.as_deref().unwrap().contains("main/gh"));
        assert_eq!(rt.state.inventory.workspace.selected.len(), 1);
        tap(&mut rt, "inventory-review-close");
        right_click(&mut rt);
        tap_menu_item(&mut rt, "Installation details");
        assert_eq!(rt.state.inventory.details.as_deref(), Some(gh.as_str()));
        assert!(rt.find("inventory-record-details").is_some());
        let unknown = inventory_key(&rt.state, "fixture-unknown");
        let (x, y) = rt.find(&format!("inventory-row-{unknown}")).unwrap();
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        rt.frame(1.);
        assert!(!rt.elements().iter().any(|e| e.label.as_deref() == Some("Review upgrade")));
        tap_menu_item(&mut rt, "Copy installed version");
        assert_eq!(clipboard.text().unwrap().as_str(), "2.0.0", "Unknown versions must not copy another row's version");
        assert!(rt.state.inventory.workspace.context.is_some());
        rt.key(KeyEvent { key: Key::Escape, mods: Default::default() });
        rt.frame(1.);
        assert!(!rt.state.inventory.work.is_pending());
    }
}

#[test]
fn inventory_group_context_menu_supports_keyboard_dismissal_and_filtered_selection() {
    let mut rt = runtime((1280., 800.));
    rt.update(|s, tx| {
        crate::inventory_view::workspace_scene(s);
        tx.instant();
    });
    rt.frame(1.);
    let gh = inventory_key(&rt.state, "gh");
    let group = crate::inventory_workspace::filtered(&rt.state).into_iter()
        .find(|r| r.key == gh).unwrap().group_key().to_owned();
    let trigger = format!("inventory-context-group-{group}-trigger");
    rt.update(|_, tx| tx.focus(trigger.clone(), true));
    rt.frame(1.);
    rt.key(KeyEvent { key: Key::Function(10), mods: Mods { shift: true, ..Default::default() } });
    rt.frame(1.);
    tap_menu_item(&mut rt, "Select group");
    assert_eq!(rt.state.inventory.workspace.selected.len(), 2);
    let (x, y) = rt.find(&trigger).unwrap();
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    rt.frame(1.);
    assert!(!rt.elements().iter().any(|e| e.label.as_deref() == Some("Copy package ID")));
    tap_menu_item(&mut rt, "Collapse group");
    assert!(rt.state.inventory.workspace.collapsed_groups.contains(&group));
    assert!(rt.find(&format!("inventory-select-{gh}")).is_none());
    rt.key(KeyEvent { key: Key::ContextMenu, mods: Default::default() });
    rt.frame(1.);
    assert!(rt.state.inventory.workspace.context.is_some());
    rt.key(KeyEvent { key: Key::Escape, mods: Default::default() });
    rt.frame(1.);
    assert!(rt.state.inventory.workspace.context.is_none());
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some(&trigger) && e.focused));
    let (x, y) = rt.find(&trigger).unwrap();
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    rt.frame(1.);
    rt.pointer_down(10., 620.);
    rt.pointer_up(10., 620.);
    rt.frame(1.);
    assert!(rt.state.inventory.workspace.context.is_none());
    assert!(!rt.state.inventory.work.is_pending());
}

#[test]
fn inventory_product_groups_expand_and_select_matching_observations_without_losing_records() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, tx| {
            crate::inventory_view::workspace_scene(s);
            s.dark = dark;
            tx.instant();
        });
        rt.frame(1.);
        let first = inventory_key(&rt.state, "gh");
        let second = inventory_key(&rt.state, "GitHub.cli");
        let group = crate::inventory_workspace::filtered(&rt.state)
            .into_iter()
            .find(|r| r.key == first)
            .unwrap()
            .group_key()
            .to_owned();
        let toggle = format!("inventory-group-toggle-{group}");
        let select = format!("inventory-group-select-{group}");
        let first_select = format!("inventory-select-{first}");
        let second_select = format!("inventory-select-{second}");
        assert!(rt.find("inventory-branch-guide").is_some());
        tap(&mut rt, &select);
        assert!(rt.state.inventory.workspace.selected.contains(&first));
        assert!(rt.state.inventory.workspace.selected.contains(&second));
        tap(&mut rt, &first_select);
        assert_eq!(rt.state.inventory.workspace.selected.len(), 1);
        tap(&mut rt, &toggle);
        assert!(rt.find(&first_select).is_none());
        assert!(rt.find(&second_select).is_none());
        assert_eq!(crate::inventory_workspace::records(&rt.state).len(), 12);
        tap(&mut rt, &select);
        assert_eq!(rt.state.inventory.workspace.selected.len(), 2);
        tap(&mut rt, &toggle);
        assert!(rt.find(&first_select).is_some());
        assert!(rt.find(&second_select).is_some());
        tap(&mut rt, "inventory-provider-winget");
        assert!(
            rt.find(&toggle).is_none(),
            "A single matching observation is an ordinary row"
        );
        assert!(rt.find(&first_select).is_some());
        assert_eq!(rt.state.inventory.workspace.selected.len(), 2);
        assert!(!rt.state.inventory.work.is_pending());
    }
}

#[test]
fn inventory_window_mounts_only_nearby_rows_and_keeps_selection_through_jumps() {
    let mut rt = runtime((1280., 800.));
    rt.update(|s, tx| {
        crate::inventory_view::scene(s);
        let mut scan = s.inventory.selected_scan().unwrap().clone();
        let template = scan.packages[0].clone();
        scan.packages = (0..320)
            .map(|i| {
                let mut p = template.clone();
                p.id = format!("window-fixture-{i:03}");
                p.name = p.id.clone();
                p
            })
            .collect();
        s.accept_scan(scan);
        tx.instant();
    });
    rt.frame(1.);
    let mounted = |rt: &Runtime<State>| {
        rt.elements().iter().filter(|e| {
            e.key.as_deref().is_some_and(|key| key.starts_with("inventory-row-"))
        }).count()
    };
    let first = inventory_key(&rt.state, "window-fixture-000");
    let last = inventory_key(&rt.state, "window-fixture-319");
    assert!((1..100).contains(&mounted(&rt)));
    assert!(rt.find(&format!("inventory-row-{last}")).is_none());
    tap(&mut rt, &format!("inventory-select-{first}"));

    rt.update(|_, tx| tx.scroll_into_view(format!("inventory-item-{last}")));
    for _ in 0..8 {
        rt.frame(1. / 60.);
    }
    assert!((1..100).contains(&mounted(&rt)));
    let (x, y) = rt.find(&format!("inventory-row-{last}")).unwrap();
    assert!(y < 800.);
    for dy in [0., -9., -16., -24., -40.] {
        rt.pointer_move(x, y + dy);
        rt.frame(1. / 120.);
    }
    assert!((1..100).contains(&mounted(&rt)));
    assert!(rt.state.inventory.workspace.selected.contains(&first));

    rt.update(|s, tx| {
        s.inventory.filter = TextEdit::new("window-fixture-00");
        s.reset_inventory_rows(tx);
    });
    for _ in 0..3 {
        rt.frame(1. / 60.);
    }
    assert_eq!(crate::inventory_workspace::filtered(&rt.state).len(), 10);
    assert!(rt.find(&format!("inventory-row-{first}")).is_some());
    assert!(rt.find(&format!("inventory-row-{last}")).is_none());
    assert!(!rt.state.inventory.work.is_pending());
    assert!(rt.state.inventory.discovery.iter().all(|work| !work.is_pending()));
}

#[test]
fn inventory_table_sorts_every_header_with_mixed_version_formats() {
    use crate::inventory_workspace::{Column, filtered};

    let snapshot = std::env::var_os("CHRONVER_TEST_INVENTORY_SNAPSHOT");
    let mut rt = runtime((1440., 800.));
    rt.update(|s, tx| {
        if let Some(path) = &snapshot {
            for scan in releasescout_engine::matching::load_snapshot(std::path::Path::new(path))
                .expect("test inventory snapshot")
            {
                s.accept_scan(scan);
            }
        } else {
            crate::inventory_view::scene(s);
            let mut scan = s.inventory.selected_scan().unwrap().clone();
            let template = scan.packages[0].clone();
            let versions = [
                Some("2.0"),
                Some("10.0"),
                Some("1x"),
                Some("r100"),
                Some("r99"),
                Some("1.4.1_2"),
                Some("1.4.1_10"),
                Some("< 9.0"),
                Some("4.0-rc.2"),
                Some("4.0"),
                Some("1.2.3.4.g85e641b4"),
                Some("1.2.3.5.g806b5b27"),
                None,
            ];
            scan.packages = (0..320)
                .map(|i| {
                    let mut p = template.clone();
                    p.id = format!("sort-fixture-{i:03}");
                    p.name = format!("Sort Fixture {:03}", 319 - i);
                    p.installed = versions[(i * 7 + i / 9) % versions.len()].map(str::to_owned);
                    p.available = versions[(i * 11 + i / 5) % versions.len()].map(str::to_owned);
                    p.source = format!("bucket-{}", i % 3);
                    p.scope = (if i % 2 == 0 { "user" } else { "machine" }).into();
                    p
                })
                .collect();
            s.accept_scan(scan);
        }
        s.inventory.open = true;
        s.inventory.workspace.columns.extend(Column::OPTIONAL);
        let key = filtered(s)[0].key.clone();
        s.inventory.workspace.selected.insert(key);
        tx.instant();
    });
    rt.frame(1.);
    rt.update(|_, tx| tx.scroll_to_instant("inventory-scroll-area/scroll", 0., 2048.));
    rt.frame(1.);
    let count = filtered(&rt.state).len();
    let selected = rt.state.inventory.workspace.selected.clone();
    for column in [
        Column::Name,
        Column::Id,
        Column::Status,
        Column::Source,
        Column::Scope,
        Column::Candidate,
        Column::Installed,
    ] {
        let before = rt.state.inventory.workspace.descending;
        let already_selected = rt.state.inventory.workspace.sort == column;
        let header = format!("inventory-sort-{column:?}");
        let first_descending = already_selected && !before;
        for descending in [first_descending, !first_descending] {
            tap(&mut rt, &header);
            assert_eq!(rt.state.inventory.workspace.sort, column);
            assert_eq!(rt.state.inventory.workspace.descending, descending);
            let rows = filtered(&rt.state);
            assert_eq!(rows.len(), count);
            assert_eq!(rt.state.inventory.workspace.selected, selected);
            if snapshot.is_none() && matches!(column, Column::Installed | Column::Candidate) {
                let mut versions: Vec<_> = rows
                    .iter()
                    .map(|row| match column {
                        Column::Installed => row.package.installed.as_deref(),
                        _ => row.package.available.as_deref(),
                    })
                    .collect();
                versions.dedup();
                assert_eq!(versions.pop(), Some(None));
                let mut expected = [
                    "2.0", "4.0-rc.2", "4.0", "10.0", "r99", "r100", "1.4.1_2", "1.4.1_10",
                    "1.2.3.4.g85e641b4", "1.2.3.5.g806b5b27", "1x", "< 9.0",
                ].map(Some);
                if descending {
                    expected.reverse();
                }
                assert_eq!(versions, expected);
            }
        }
    }
}

#[test]
fn inventory_table_scrolls_within_the_workspace_on_both_axes() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, tx| {
            crate::inventory_view::scene(s);
            s.dark = dark;
            let mut scan = s.inventory.selected_scan().unwrap().clone();
            let template = scan.packages[0].clone();
            scan.packages = (0..80)
                .map(|i| {
                    let mut p = template.clone();
                    p.id = format!("scroll-fixture-{i:03}");
                    p.name = p.id.clone();
                    p
                })
                .collect();
            s.accept_scan(scan);
            s.inventory
                .workspace
                .columns
                .extend(crate::inventory_workspace::Column::OPTIONAL);
            tx.instant();
        });
        rt.frame(1.);
        let rect = |rt: &Runtime<State>, key: &str| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap_or_else(|| panic!("Missing element: {key}"))
                .rect
        };
        let area = rect(&rt, "inventory-scroll-area");
        let viewport = rt
            .elements()
            .into_iter()
            .find(|e| {
                e.key.as_deref() == Some("scroll") && (e.rect[0] - area[0]).abs() < 0.5
            })
            .unwrap()
            .rect;
        assert!(
            viewport[3] <= area[3] + 0.5,
            "Table viewport must fit its allocated space: {viewport:?} / {area:?}"
        );
        let first = inventory_key(&rt.state, "scroll-fixture-000");
        let key = format!("inventory-select-{first}");
        let before = rect(&rt, &key);
        let header = rect(&rt, "inventory-table-header");
        let toolbar = rect(&rt, "scan-filter");
        let x = area[0] + area[2] / 2.;
        let y = area[1] + area[3] / 2.;
        rt.wheel_event(WheelEvent::new(0., -180.).at(x, y).precise(true));
        rt.frame(1. / 60.);
        assert!(
            rect(&rt, &key)[1] < before[1] - 100.,
            "Mouse wheel must move the table rows"
        );
        assert_eq!(
            rect(&rt, "inventory-table-header")[1],
            header[1],
            "Column headers stay pinned"
        );
        assert_eq!(
            rect(&rt, "scan-filter"),
            toolbar,
            "Toolbar stays outside table scrolling"
        );
        rt.frame(1.);
        rt.wheel_event(WheelEvent::new(-180., 0.).at(x, y).precise(true));
        rt.frame(1. / 60.);
        if size.0 == 1024. {
            assert!(
                rect(&rt, &key)[0] < before[0] - 100.,
                "Optional columns remain horizontally scrollable"
            );
        }
        rt.update(|_, tx| tx.scroll_to_instant("inventory-scroll-area/scroll", 0., 10_000.));
        rt.frame(1.);
        rt.frame(1. / 60.);
        rt.frame(1. / 60.);
        let last = inventory_key(&rt.state, "scroll-fixture-079");
        assert!(rt.find(&format!("inventory-select-{last}")).is_some_and(|(_, y)| y < size.1));
        assert!(!rt.state.inventory.work.is_pending());
    }
}

#[test]
fn inventory_export_uses_a_save_dialog_and_the_captured_filtered_snapshot() {
    let mut rt = runtime((1280., 800.));
    rt.update(|s, tx| {
        crate::inventory_view::workspace_scene(s);
        tx.instant();
    });
    rt.frame(1.);
    tap(&mut rt, "inventory-export");
    assert_eq!(rt.file_dialog().unwrap().0, rune::FileDialogKind::Save);
    rt.resolve_file_dialog(None).unwrap();
    rt.frame(1.);
    assert!(!rt.state.inventory.workspace.export_busy);
    assert!(!rt.state.inventory.workspace.export.is_pending());
    tap(&mut rt, "inventory-export");
    rt.update(|s, _| s.inventory.filter = TextEdit::new("not matching anything"));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inventory.json");
    rt.resolve_file_dialog(Some(path.clone())).unwrap();
    rt.frame(2.);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !rt.state.poll_inventory_export() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(rt.state.inventory.workspace.export_error.is_none());
    assert!(!rt.state.inventory.workspace.export_busy);
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        json["observations"].as_array().unwrap().len(),
        12,
        "Changing the filter during Save must not replace the captured export"
    );
    assert!(!rt.state.inventory.work.is_pending());
}

#[test]
fn inventory_has_its_own_sidebar_and_search_without_changing_notes_layout() {
    let mut rt = notes_runtime((1280., 800.));
    rt.update(|s, tx| {
        s.pane.set_width(420.);
        s.pane.set_open(false);
        tx.instant();
    });
    rt.frame(1.);
    tap(&mut rt, "workspace-view/segment1");
    assert!(rt.find("inventory-sources").is_some());
    assert!(!rt.state.pane.is_open());
    rt.key(KeyEvent {
        key: Key::Command('f'),
        mods: Mods::default(),
    });
    rt.frame(2.);
    assert!(rt.state.inventory.filter.focused);
    assert!(!rt.state.pane.is_open());
    tap(&mut rt, "inventory-back");
    assert!(!rt.state.inventory.open);
    assert!(!rt.state.pane.is_open());
    assert!(rt.find("inventory-sources").is_none());
}

#[test]
fn inventory_table_filters_details_and_original_row_actions_are_read_only() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.update(|s, _| {
            crate::inventory_view::scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        let gh = format!("inventory-row-{}", inventory_key(&rt.state, "gh"));
        let pnpm = format!("inventory-row-{}", inventory_key(&rt.state, "pnpm"));
        let fzf_key = inventory_key(&rt.state, "fzf");
        let fzf = format!("inventory-row-{fzf_key}");
        let broken = format!(
            "inventory-row-{}",
            inventory_key(&rt.state, "fixture-broken")
        );
        let gh_details = format!("inventory-details-{}", inventory_key(&rt.state, "gh"));
        for key in [
            "scan-manager-trigger",
            "scan-installed",
            "scan-updates",
            "scan-filter",
            "inventory-status-filter",
            &gh,
        ] {
            let el = rt
                .elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap();
            let [x, y, w, h] = el.rect;
            assert!(
                x >= 0. && y >= 0. && x + w <= size.0 + 0.5 && y + h <= size.1 + 0.5,
                "{key}: {:?}",
                el.rect
            );
        }
        let selected = rt.state.selected().unwrap().id.clone();
        tap(&mut rt, "inventory-status-filter/segment1");
        let page = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("inventory-page"))
            .unwrap()
            .rect;
        let data = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(gh.as_str()))
            .unwrap()
            .rect;
        assert!(
            data[2] >= page[2] - 34.,
            "The table must fill its workspace, not remain at its minimum width: {data:?} / {page:?}"
        );
        assert!(rt.find(&gh).is_some());
        assert!(
            rt.find(&pnpm).is_none(),
            "Current rows must not appear in Updates"
        );
        assert_eq!(rt.state.selected().unwrap().id, selected);
        tap(&mut rt, &gh_details);
        assert!(
            rt.state.inventory.details.is_some(),
            "Info did not activate; button={:?}",
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(gh_details.as_str()))
        );
        assert!(rt.find("inventory-record-details").is_some());
        tap(&mut rt, "inventory-record-location-copy");
        assert_eq!(
            clipboard.text().unwrap().as_str(),
            "C:/fixture/scoop/apps/gh/current"
        );
        tap(&mut rt, "inventory-status-filter/segment2");
        assert!(rt.find("inventory-record-details").is_none());
        assert!(rt.find(&gh).is_none());
        assert!(rt.find(&pnpm).is_some());
        tap(&mut rt, "inventory-status-filter/segment3");
        assert!(rt.find(&broken).is_some());
        assert!(rt.find(&gh).is_none());
        tap(&mut rt, "inventory-status-filter/segment0");
        tap(&mut rt, "inventory-notices");
        assert!(rt.find("inventory-notice-details").is_some());
        tap(&mut rt, "inventory-notices");
        rt.update(|s, tx| {
            s.inventory.filter = TextEdit::new("GitHub");
            tx.instant();
        });
        rt.frame(2.);
        assert!(
            rt.find(&gh).is_some(),
            "Displayed names must be searchable, not just raw provider IDs"
        );
        assert!(rt.find(&fzf).is_none());
        rt.update(|s, _| {
            s.inventory.filter = TextEdit::new("fzf");
        });
        rt.frame(2.);
        assert!(rt.find(&fzf).is_some());
        assert!(rt.find(&gh).is_none());
        tap(&mut rt, &format!("use-range-{fzf_key}"));
        assert!(!rt.state.inventory.open);
        assert_eq!(rt.state.live.sessions[&selected].from.text, "0.74.0");
        assert_eq!(rt.state.live.sessions[&selected].to.text, "0.75.0");
        assert_eq!(
            rt.state.selected().unwrap().installed.as_deref(),
            Some("2.0.0")
        );
        assert!(!rt.state.inventory.work.is_pending());
        assert!(
            rt.state
                .inventory
                .discovery
                .iter()
                .all(|work| !work.is_pending())
        );
        assert!(!rt.state.live.work.is_pending());
    }
}

#[test]
fn sidebar_observation_select_is_one_line_and_switches_only_its_panel() {
    for dark in [true, false] {
        let mut rt = runtime((1024., 640.));
        rt.update(|s, _| {
            crate::sidebar_actions::observations_scene(s, false);
            s.dark = dark;
            for p in &mut s.catalog.packages {
                if let Some(o) = &mut p.observation {
                    o.source = "source not retained".into();
                    o.scope = "unknown".into();
                    p.observation_label =
                        "raw provider / ID / version / unknown / source not retained".into();
                }
            }
        });
        rt.frame(1.);
        let selected = rt.state.selected().unwrap().id.clone();
        let trigger = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("sidebar-observation-trigger"))
            .unwrap();
        assert_eq!(trigger.rect[3], 32.);
        let text = rt
            .elements()
            .iter()
            .filter(|e| e.path.contains("sidebar-observation-trigger/"))
            .filter_map(|e| e.text.as_deref())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(!text.contains("source not retained") && !text.contains("unknown"));
        tap(&mut rt, "sidebar-observation-trigger");
        let other = rt
            .state
            .catalog
            .packages
            .iter()
            .find(|p| p.observation.as_ref().is_some_and(|o| o.id == "GitHub.cli"))
            .unwrap()
            .id
            .clone();
        tap(&mut rt, &format!("sidebar-observation-option-{other}"));
        assert_eq!(
            rt.state.sidebar_actions.panel.as_ref().unwrap().package,
            other
        );
        assert_eq!(rt.state.selected().unwrap().id, selected);
        assert!(rt.find("sidebar-panel").is_some());
        assert!(!rt.state.live.work.is_pending());
        assert!(!rt.state.inventory.work.is_pending());
    }
}

#[test]
fn inventory_navigation_is_explicit_and_does_not_start_a_scan() {
    let mut rt = notes_runtime((1024.0, 640.0));
    tap(&mut rt, "workspace-view/segment1");
    assert!(rt.state.inventory.open);
    assert!(!rt.state.inventory.loading);
    assert!(rt.state.inventory.result.is_none());
    assert!(rt.find("selected-package").is_none());
    for key in ["scan-installed", "scan-updates", "inventory-back"] {
        let (x, y) = rt.find(key).unwrap();
        assert!((0.0..1024.0).contains(&x) && (0.0..640.0).contains(&y));
    }
    tap(&mut rt, "inventory-back");
    assert!(!rt.state.inventory.open);
    assert_eq!(rt.state.selected().unwrap().id, "gh");
    rt.update(|s, _| {
        s.inventory.manager = releasescout_engine::inventory::Manager::ALL.iter()
            .position(|m| *m == releasescout_engine::inventory::Manager::Npm).unwrap();
        s.inventory.result = Some(releasescout_engine::inventory::parse(
            releasescout_engine::inventory::Manager::Npm,
            true,
            &serde_json::json!({"root":"C:/fixture/global","installed":{"dependencies":{"fixture-tool":{"version":"1.0.0"}}},"installedExit":0,"updateExit":1,"updates":{"fixture-tool":{"current":"1.0.0","latest":"2.0.0"}}}).to_string(),
        ).unwrap());
    });
    rt.frame(0.0);
    tap(&mut rt, "workspace-view/segment1");
    let key = inventory_key(&rt.state, "fixture-tool");
    tap(&mut rt, &format!("use-range-{key}"));
    assert!(!rt.state.inventory.open);
    assert!(rt.find("detected-inventory").is_none());
    assert!(rt.find("selected-package").is_some());
    assert_eq!(rt.state.live.sessions["gh"].from.text, "1.0.0");
    assert_eq!(rt.state.live.sessions["gh"].to.text, "2.0.0");
}

#[test]
fn mode_and_theme_controls_change_the_rendered_state() {
    let mut rt = notes_runtime((1280.0, 800.0));
    tap(&mut rt, "note-mode/segment1");
    assert!(rt.state.mode == super::model::NoteMode::Source);
    assert!(rt.find("raw-notes").is_some());
    tap(&mut rt, "global-theme");
    assert!(!rt.state.dark);
}

#[test]
fn all_package_headers_fit_without_overlapping_actions_at_minimum_size() {
    let mut rt = runtime((1024.0, 640.0));
    let ids: Vec<_> = rt.state.rows.iter().map(|r| r.id.clone()).collect();
    for id in ids {
        rt.update(|s, _| s.table.select(id.clone()));
        rt.frame(0.0);
        let elements = rt.elements();
        let bounds = |key: &str| {
            elements
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
                .rect
        };
        let title = bounds("selected-package");
        for key in [
            "selected-package",
            "open-release",
            "copy-notes",
            "note-mode",
            "version-range",
        ] {
            let [x, y, w, h] = bounds(key);
            assert!(
                x >= 0.0 && y >= 0.0 && x + w <= 1024.5 && y + h <= 640.5,
                "{id}: {key} outside window: {x}, {y}, {w}, {h}"
            );
            if key == "open-release" || key == "copy-notes" {
                assert!(
                    title[0] + title[2] <= x
                        || x + w <= title[0]
                        || title[1] + title[3] <= y
                        || y + h <= title[1],
                    "{id}: title overlaps {key}"
                );
            }
        }
    }
}

#[test]
fn candidate_failures_do_not_leak_between_packages_or_hide_provider_diagnostics() {
    let mut rt = runtime((1024., 640.));
    rt.update(|s, _| crate::sidebar_info::candidate_scope_scene(s));
    rt.frame(1.);
    let healthy = rt
        .state
        .rows
        .iter()
        .filter(|r| {
            rt.state.catalog.packages[r.package]
                .observation
                .as_ref()
                .is_some_and(|o| matches!(o.id.as_str(), "convex" | "cross-env"))
        })
        .collect::<Vec<_>>();
    assert_eq!(healthy.len(), 2);
    for row in healthy {
        let info = crate::sidebar_info::summary(&rt.state, row, crate::sidebar_info::today());
        assert!(!info.failed, "{} incorrectly marked failed", row.name);
        assert!(!info.tooltip.contains("@scope/uninstalled"));
        assert!(!info.tooltip.contains("fixture-broken"));
        assert!(
            info.tooltip
                .contains("Checked; no confirmed newer candidate")
        );
        let obs = rt.state.catalog.packages[row.package]
            .observation
            .as_ref()
            .unwrap();
        assert!(
            rt.state
                .activity
                .data
                .candidate_checks
                .contains_key(&obs.check_key)
        );
    }
    assert!(
        !rt.accessibility_json()
            .to_string()
            .contains("@scope/uninstalled")
    );
    let broken = rt
        .state
        .rows
        .iter()
        .find(|r| {
            rt.state.catalog.packages[r.package]
                .observation
                .as_ref()
                .is_some_and(|o| o.id == "fixture-broken")
        })
        .unwrap();
    let info = crate::sidebar_info::summary(&rt.state, broken, crate::sidebar_info::today());
    assert!(info.failed);
    assert!(
        info.tooltip
            .contains("ambiguous or stale update identity: fixture-broken")
    );
    assert!(!info.tooltip.contains("@scope/uninstalled"));
    let obs = rt.state.catalog.packages[broken.package]
        .observation
        .as_ref()
        .unwrap();
    assert!(
        !rt.state
            .activity
            .data
            .candidate_checks
            .contains_key(&obs.check_key)
    );
    rt.update(|s, _| {
        s.sidebar_actions.panel = None;
        s.inventory.open = true;
        s.inventory.manager = releasescout_engine::inventory::Manager::ALL
            .iter()
            .position(|m| *m == releasescout_engine::inventory::Manager::Pnpm)
            .unwrap();
        s.inventory.result = None;
    });
    rt.frame(0.016);
    assert!(rt.find("inventory-notices").is_some());
    rt.frame(1.);
    tap(&mut rt, "inventory-notices");
    rt.frame(2.);
    assert!(
        rt.accessibility_json()
            .to_string()
            .contains("ambiguous or stale update identity: @scope/uninstalled")
    );
}

#[test]
fn sidebar_sort_select_supports_all_choices_direction_and_unknown_dates() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = runtime(size);
        rt.update(|s, _| {
            crate::sidebar_info::scene(s);
            s.dark = dark;
        });
        rt.frame(1.);
        let selected = rt.state.table.cursor().unwrap().to_owned();
        let elements = rt.elements();
        let header = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("header"))
            .unwrap();
        let trigger = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("sidebar-sort-trigger"))
            .unwrap();
        assert!(trigger.rect[1] >= header.rect[1]);
        assert!(
            trigger.rect[1] + trigger.rect[3] <= header.rect[1] + header.rect[3] + 0.5,
            "sort trigger {:?} escapes header {:?}",
            trigger.rect,
            header.rect
        );
        for entry in elements
            .iter()
            .filter(|e| e.role == "row" && e.key.as_deref() != Some("header"))
        {
            assert!(entry.rect[1] >= trigger.rect[1] + trigger.rect[3]);
        }
        tap(&mut rt, "sidebar-sort-trigger");
        let elements = rt.elements();
        let popup = elements
            .iter()
            .find(|e| e.key.as_deref() == Some("sidebar-sort-content"))
            .unwrap();
        let [x, y, w, h] = popup.rect;
        assert!(x >= 0. && y >= 0. && x + w <= size.0 && y + h <= size.1);
        assert!(
            w <= 217. && h <= 170.,
            "sort popup is not compact: {:?}",
            popup.rect
        );
        assert!(trigger.rect[3] <= 28.5);
        for key in ["name", "date", "checked", "update", "manager"] {
            let option = elements
                .iter()
                .find(|e| e.key.as_deref() == Some(&format!("sidebar-sort-option-{key}")))
                .unwrap();
            assert!(option.rect[3] >= 31.5 && option.rect[3] <= 32.5);
            assert!(option.help.is_none());
            let glyph = elements
                .iter()
                .find(|e| e.key.as_deref() == Some(&format!("sidebar-sort-icon-{key}")))
                .unwrap();
            assert_eq!(&glyph.rect[2..], &[16., 16.]);
            assert!(
                glyph.rect[1] >= option.rect[1]
                    && glyph.rect[1] + glyph.rect[3] <= option.rect[1] + option.rect[3]
            );
        }
        assert!(
            !elements
                .iter()
                .any(|e| e.text.as_deref() == Some("Alphabetical"))
        );
        let options: Vec<_> = elements
            .iter()
            .filter(|e| e.role == "option")
            .filter_map(|e| e.label.as_deref())
            .collect();
        assert_eq!(
            options,
            [
                "Name",
                "Release date",
                "Last checked",
                "Download available",
                "Provider"
            ]
        );
        tap(&mut rt, "sidebar-sort-option-date");
        assert!(!rt.state.sidebar_sort_open);
        assert_eq!(rt.state.table.sort(), Some(("date", SortDir::Descending)));
        let names = |s: &State| {
            s.visible
                .iter()
                .map(|&i| s.rows[i].name.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        assert_eq!(names(&rt.state), "GitHub CLI,pnpm,Unmapped Fixture Tool");
        tap(&mut rt, "sidebar-sort-direction");
        assert_eq!(names(&rt.state), "pnpm,GitHub CLI,Unmapped Fixture Tool");
        tap(&mut rt, "sidebar-sort-trigger");
        tap(&mut rt, "sidebar-sort-option-update");
        assert_eq!(rt.state.rows[rt.state.visible[0]].name, "GitHub CLI");
        tap(&mut rt, "sidebar-sort-trigger");
        tap(&mut rt, "sidebar-sort-option-checked");
        assert_eq!(
            rt.state.table.sort(),
            Some(("checked", SortDir::Descending))
        );
        assert_eq!(rt.state.table.cursor(), Some(selected.as_str()));
        tap(&mut rt, "sidebar-sort-trigger");
        rt.key(KeyEvent {
            key: Key::Escape,
            mods: Mods::default(),
        });
        rt.frame(0.016);
        assert!(!rt.state.sidebar_sort_open);
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("sidebar-sort-trigger") && e.focused)
        );
    }
}

#[test]
fn image_only_descriptions_have_no_empty_sidebar_line_or_info_section() {
    let mut rt = runtime((1024., 640.));
    let source = "[![Only badge](https://example.com/badge.svg)](https://example.com)";
    rt.update(|s, _| {
        crate::description_preview::scene(s);
        s.description_hover = None;
        let row = s.rows.iter().find(|r| r.name == "uv").unwrap();
        let id = s.catalog.packages[row.package].id.clone();
        s.catalog.packages[row.package].description = Some(source.into());
        s.select_package(&id);
    });
    rt.frame(1.);
    assert_eq!(
        rt.elements()
            .iter()
            .filter(|e| e.key.as_deref() == Some("sidebar-description"))
            .count(),
        1
    );
    assert!(rt.find("selected-description").is_none());
    let row = rt.state.rows.iter().find(|r| r.name == "uv").unwrap();
    assert_eq!(
        crate::sidebar_info::description(&rt.state, row),
        Some(source)
    );
    let key = crate::sidebar_actions::anchor(row, crate::sidebar_actions::Mode::Details);
    tap(&mut rt, &key);
    rt.frame(0.016);
    assert!(rt.find("sidebar-panel-description").is_none());
    assert!(!rt.accessibility_json().to_string().contains("Only badge"));
}

#[test]
fn manager_sort_remains_available_in_the_sidebar() {
    let mut rt = runtime((1024.0, 640.0));
    let selected = rt.state.table.cursor().unwrap().to_owned();
    tap(&mut rt, "sidebar-sort-trigger");
    tap(&mut rt, "sidebar-sort-option-manager");
    assert_eq!(rt.state.table.sort(), Some(("manager", SortDir::Ascending)));
    let labels: Vec<_> = rt
        .state
        .visible
        .iter()
        .map(|&i| rt.state.catalog.packages[rt.state.rows[i].package].manager_label())
        .collect();
    assert!(labels.windows(2).all(|p| p[0] <= p[1]));
    assert_eq!(rt.state.table.cursor(), Some(selected.as_str()));
    assert!(!rt.state.sidebar_sort_open);
}

#[test]
fn timeline_spine_connects_markers_and_dates_precede_titles() {
    for size in [(1024.0, 640.0), (1280.0, 800.0)] {
        let rt = notes_runtime(size);
        let elements = rt.elements();
        let bounds = |key: &str| {
            elements
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap_or_else(|| panic!("missing {key}"))
                .rect
        };
        let first = bounds("timeline-indicator-0");
        let second = bounds("timeline-indicator-1");
        let spine = bounds("timeline-separator-0");
        assert_eq!([first[2], first[3]], [16.0, 16.0]);
        assert_eq!(spine[2], 1.0);
        assert!((first[0] + 8.0 - (spine[0] + spine[2] / 2.)).abs() < 0.5);
        assert!((first[0] - second[0]).abs() < 0.5);
        assert!((spine[1] - (first[1] + 18.0)).abs() < 0.5);
        assert!((spine[1] + spine[3] - (second[1] - 2.0)).abs() < 0.5);
        assert!(
            !elements
                .iter()
                .any(|e| e.key.as_deref() == Some("timeline-separator-1")),
            "the final marker must not have a trailing line"
        );
        for (i, release) in rt.state.selected().unwrap().releases.iter().enumerate() {
            let date = bounds(&format!("timeline-date-{i}"));
            let title = bounds(&format!("release-{}", release.version));
            let marker = bounds(&format!("timeline-indicator-{i}"));
            assert!((date[0] - title[0]).abs() < 0.5);
            assert!((date[0] - marker[0] - 32.0).abs() < 0.5);
            assert!(date[1] + date[3] <= title[1]);
        }
    }
}

#[test]
fn changelog_shows_the_saved_range_not_the_unsubmitted_filter_or_internal_id() {
    for dark in [false, true] {
        let mut rt = runtime((900., 640.));
        rt.update(|s, _| {
            crate::note_cache::saved_range_scene(s);
            s.dark = dark;
            s.engine_query().to = rune::TextEdit::new("9.0.0");
        });
        rt.frame(1.);
        assert!(rt.find("engine-saved-notes").is_some());
        let elements = rt.elements();
        let range = elements.iter().find(|e| e.key.as_deref() == Some("engine-loaded-range")).unwrap();
        let range_text: Vec<_> = elements.iter()
            .filter(|e| e.path.starts_with(&range.path))
            .filter_map(|e| e.text.as_deref())
            .collect();
        assert_eq!(range_text, ["2.0.0", "2.2.0"]);
        assert!(!rt.accessibility_json().to_string().contains("version-notes"));
        tap(&mut rt, "engine-details");
        assert!(rt.accessibility_json().to_string().contains("version-notes"));
        tap(&mut rt, "engine-details");
        rt.update(|s, _| s.engine_query().restored = false);
        rt.frame(1.);
        assert!(rt.find("engine-loaded-range").is_some());
        assert!(rt.find("engine-saved-notes").is_none());
    }
}

#[test]
fn changelog_evidence_is_compact_expandable_and_copy_is_per_release() {
    for dark in [false, true] {
        let mut rt = notes_runtime((1024., 640.));
        let evidence = "Synthetic release provenance";
        rt.update(|s, _| {
            s.dark = dark;
            let p = s.catalog.packages.iter_mut().find(|p| p.id == "gh").unwrap();
            p.reference_text = Some(evidence.into());
        });
        rt.frame(1.);
        assert!(
            !rt.accessibility_json()
                .to_string()
                .contains(evidence)
        );
        let before = rt.find("timeline-item-0").unwrap().1;
        tap(&mut rt, "copy-release-0");
        assert_eq!(rt.state.status, "Release copied");
        assert!(!rt.state.live.work.is_pending());
        tap(&mut rt, "engine-details");
        assert!(rt.state.live.sessions["gh"].details);
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains(evidence)
        );
        tap(&mut rt, "engine-details");
        assert!((rt.find("timeline-item-0").unwrap().1 - before).abs() < 1.);
        tap(&mut rt, "note-mode/segment1");
        assert!(rt.find("raw-notes").is_some());
        assert!(rt.find("copy-release-0").is_none());
    }
}

#[test]
fn changelog_long_titles_unknown_dates_and_markdown_fit_the_reader() {
    for (size, dark) in [((1024., 640.), true), ((1280., 800.), false)] {
        let mut rt = notes_runtime(size);
        rt.update(|s, _| {
            s.dark = dark;
            s.pane.set_width(420.);
            let p = s.catalog.packages.iter_mut().find(|p| p.id == "gh").unwrap();
            p.releases[0].version = "Release with a long publisher-authored title and additional compatibility information".into();
            p.releases[0].published.clear();
            p.releases[0].markdown = "## Improvements\n\n- **Fixed** configuration loading.\n- Supports `--profile`.\n\n> Check compatibility before upgrading.\n\n```sh\ngh api repos/owner/repo/releases --paginate\n```\n\n| Change | Status |\n| --- | --- |\n| Startup | Fixed |".into();
        });
        rt.frame(1.);
        let elements = rt.elements();
        let bounds = |key: &str| {
            elements
                .iter()
                .find(|e| e.key.as_deref() == Some(key))
                .unwrap()
                .rect
        };
        let title = bounds(&format!(
            "release-{}",
            rt.state.selected().unwrap().releases[0].version
        ));
        let copy = bounds("copy-release-0");
        assert!(
            title[2] > 0. && title[3] > 26.,
            "long titles must wrap: {title:?}"
        );
        assert!(
            title[0] + title[2] <= copy[0],
            "title collides with copy action"
        );
        for rect in [title, copy, bounds("timeline-content-0")] {
            assert!(rect[0] >= 420. && rect[0] + rect[2] <= size.0, "{rect:?}");
        }
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("Publication date unknown")
        );
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("compatibility before upgrading")
        );
    }
}

#[test]
fn shadcn_neutral_theme_has_achromatic_surfaces_text_and_accents() {
    for dark in [false, true] {
        let c = super::theme::kit(dark).c;
        for color in [
            c.page,
            c.bg_base,
            c.bg_deep,
            c.layer_01,
            c.layer_02,
            c.layer_03,
            c.layer_04,
            c.text_base,
            c.text_muted,
            c.text_faint,
            c.text_accent,
            c.icon_base,
            c.icon_muted,
            c.icon_accent,
            c.border_base,
            c.border_muted,
            c.border_strong,
            c.border_focus,
            c.hover,
            c.pressed,
            c.surface_base,
            c.surface_raised,
            c.bg_button_neutral,
        ] {
            assert!(
                (color.r - color.g).abs() < 0.00001 && (color.g - color.b).abs() < 0.00001,
                "non-neutral theme token: {}",
                color.hex()
            );
        }
        assert!((c.page.to_oklab()[0] - if dark { 0.145 } else { 1.0 }).abs() < 0.00001);
        assert!((c.text_muted.to_oklab()[0] - if dark { 0.708 } else { 0.556 }).abs() < 0.00001);
    }
}

#[test]
fn first_launch_renders_without_inventory_or_local_catalog() {
    let rt = runtime_with(State::new(), (1024.0, 640.0));
    assert!(rt.state.dataset == Dataset::Detected);
    assert!(rt.state.catalog.packages.is_empty());
    assert!(rt.state.selected().is_none());
    assert!(rt.find("timeline-indicator-0").is_none());
    assert!(rt.find("viewport").is_some());
}

#[test]
fn demo_rows_do_not_claim_installation_versions_or_providers() {
    let s = State::demo();
    assert!(!s.catalog.packages.is_empty());
    for p in &s.catalog.packages {
        assert!(p.installed.is_none() && p.target.is_none());
        assert!(p.manager.is_none() && p.observation.is_none());
        assert!(!p.detected);
        assert!(p.releases.is_empty() || p.sample_notes);
        let mark = p.monogram();
        assert!(!mark.is_empty() && mark.chars().count() <= 2);
        assert_eq!(mark, p.monogram());
    }
}

#[test]
fn demo_packages_without_notes_have_honest_empty_states_and_no_copy_action() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, _| s.select_package("agent-browser"));
    rt.frame(0.0);
    assert!(rt.find("missing-notes").is_some());
    assert!(rt.find("timeline-indicator-0").is_none());
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    tap(&mut rt, "copy-notes");
    assert!(clipboard.text().is_none());
    assert_eq!(rt.state.status, "Demo data / no scan");
    rt.update(|s, _| s.select_package("gh"));
    rt.frame(0.0);
    assert!(rt.find("missing-notes").is_none());
    assert!(rt.state.selected_markdown().starts_with("Synthetic sample notes;"));
}

#[test]
fn demo_releases_do_not_cross_product_channels() {
    let s = State::demo();
    let preview = s
        .catalog
        .packages
        .iter()
        .find(|p| p.id == "terminal-preview")
        .unwrap();
    assert!(preview.releases.is_empty());
    assert!(!preview.sample_notes);
}

fn scroll_point(rt: &Runtime<State>) -> (f32, f32) {
    rt.find("viewport").unwrap()
}

fn step_scroll(rt: &mut Runtime<State>) {
    for _ in 0..120 {
        rt.frame(1.0 / 120.0);
    }
}

#[test]
fn sidebar_notched_wheel_moves_over_multiple_frames_not_one_jump() {
    let mut rt = runtime((1280.0, 800.0));
    let (x, y) = scroll_point(&rt);
    rt.wheel_event(WheelEvent::new(0.0, -120.0).at(x, y));
    rt.frame(1.0 / 120.0);
    let first = rt.state.sidebar_scroll.position.get().offset[1];
    assert!(
        first > 0.0 && first < 120.0,
        "first frame should be partway through the glide, got {first}"
    );
    let mut previous = first;
    for _ in 0..120 {
        rt.frame(1.0 / 120.0);
        let at = rt.state.sidebar_scroll.position.get().offset[1];
        assert!(
            at + 0.1 >= previous && at <= 120.5,
            "glide must not jump backwards or overshoot"
        );
        previous = at;
    }
    assert!((previous - 120.0).abs() < 0.5);
    assert_eq!(rt.state.selected().unwrap().id, "agent-browser");
}

#[test]
fn sidebar_precise_wheel_is_direct_and_cancels_notch_smoothing() {
    let mut rt = runtime((1280.0, 800.0));
    let (x, y) = scroll_point(&rt);
    rt.wheel_event(WheelEvent::new(0.0, -240.0).at(x, y));
    rt.frame(1.0 / 120.0);
    let before = rt.state.sidebar_scroll.position.get().offset[1];
    rt.wheel_event(WheelEvent::new(0.0, -20.0).at(x, y).precise(true));
    rt.frame(1.0 / 120.0);
    let after = rt.state.sidebar_scroll.position.get().offset[1];
    assert!((after - before - 20.0).abs() < 0.5);
    step_scroll(&mut rt);
    assert!((rt.state.sidebar_scroll.position.get().offset[1] - after).abs() < 0.5);
}

#[test]
fn sidebar_accumulates_notches_clamps_and_reverses() {
    let mut rt = runtime((1280.0, 800.0));
    let (x, y) = scroll_point(&rt);
    for _ in 0..3 {
        rt.wheel_event(WheelEvent::new(0.0, -80.0).at(x, y));
        rt.frame(1.0 / 120.0);
    }
    step_scroll(&mut rt);
    assert!((rt.state.sidebar_scroll.position.get().offset[1] - 240.0).abs() < 0.5);
    rt.wheel_event(WheelEvent::new(0.0, 40.0).at(x, y));
    step_scroll(&mut rt);
    assert!((rt.state.sidebar_scroll.position.get().offset[1] - 200.0).abs() < 0.5);
    rt.wheel_event(WheelEvent::new(0.0, -1_000_000.0).at(x, y));
    step_scroll(&mut rt);
    let at = rt.state.sidebar_scroll.position.get();
    assert!((at.offset[1] - at.max[1]).abs() < 0.5);
    rt.wheel_event(WheelEvent::new(0.0, 1_000_000.0).at(x, y));
    step_scroll(&mut rt);
    assert!(rt.state.sidebar_scroll.position.get().offset[1] < 0.5);
}

#[test]
fn sidebar_keyboard_end_home_and_page_navigation_remain_functional() {
    let mut rt = runtime((1280.0, 800.0));
    for _ in 0..6 {
        rt.frame(1. / 60.);
    }
    assert!(rt.state.sidebar_scroll.visible_rows.len() > 1);
    tap(&mut rt, "agent-browser");
    for key in [Key::PageDown, Key::End] {
        rt.key(KeyEvent {
            key,
            mods: Mods::default(),
        });
        step_scroll(&mut rt);
    }
    let expected = rt.state.rows[*rt.state.visible.last().unwrap()].id.clone();
    assert_eq!(rt.state.table.cursor(), Some(expected.as_str()));
    assert!(rt.state.sidebar_scroll.position.get().offset[1] > 0.0);
    rt.key(KeyEvent {
        key: Key::Home,
        mods: Mods::default(),
    });
    step_scroll(&mut rt);
    assert_eq!(rt.state.table.cursor(), Some("agent-browser"));
    assert!(rt.state.sidebar_scroll.position.get().offset[1] < 0.5);
}

#[test]
fn filtering_during_a_glide_cancels_the_old_scroll_target() {
    let mut rt = runtime((1280.0, 800.0));
    let (x, y) = scroll_point(&rt);
    rt.wheel_event(WheelEvent::new(0.0, -1000.0).at(x, y));
    rt.frame(1.0 / 120.0);
    rt.key(KeyEvent {
        key: Key::Command('f'),
        mods: Mods::default(),
    });
    rt.frame(1.0 / 120.0);
    rt.key(KeyEvent {
        key: Key::Text("bun".into()),
        mods: Mods::default(),
    });
    step_scroll(&mut rt);
    assert_eq!(rt.state.visible.len(), 1);
    assert_eq!(rt.state.selected().unwrap().id, "bun");
    assert!(rt.state.sidebar_scroll.position.get().offset[1] < 0.5);
    rt.update(|s, tx| {
        s.query = TextEdit::new("");
        s.refilter();
        tx.scroll_to_instant("viewport", 0.0, 0.0);
    });
    rt.frame(1.0 / 120.0);
    let (x, y) = scroll_point(&rt);
    rt.wheel_event(WheelEvent::new(0.0, -40.0).at(x, y));
    step_scroll(&mut rt);
    assert!((rt.state.sidebar_scroll.position.get().offset[1] - 40.0).abs() < 0.5);
}

#[test]
fn sidebar_preserves_grid_accessibility_roles() {
    let rt = runtime((1280.0, 800.0));
    let elements = rt.elements();
    assert_eq!(
        elements.iter().filter(|e| e.role == "columnheader").count(),
        1
    );
    let rows = elements.iter().filter(|e| e.role == "row").count();
    let cells = elements.iter().filter(|e| e.role == "gridcell").count();
    assert!(rows > 1);
    assert_eq!(cells, rows - 1);
}

#[test]
fn virtualized_rows_remain_selectable_after_a_long_scroll() {
    let mut rt = runtime((1280.0, 800.0));
    rt.update(|s, _| s.set_dataset(Dataset::Stress));
    rt.frame(1.0 / 120.0);
    let (x, y) = scroll_point(&rt);
    rt.wheel_event(WheelEvent::new(0.0, -4000.0).at(x, y).precise(true));
    step_scroll(&mut rt);
    let row = rt
        .elements()
        .into_iter()
        .find(|e| e.role == "row" && e.visible[3] >= 50.0)
        .unwrap();
    let id = row.key.unwrap();
    tap(&mut rt, &id);
    assert_eq!(rt.state.table.cursor(), Some(id.as_str()));
    assert!(rt.accessibility_json().to_string().len() < 250_000);
}

#[test]
fn dataset_fixtures_are_internal_not_exposed_in_the_header() {
    let mut rt = runtime((1024.0, 640.0));
    for dataset in [Dataset::Reference, Dataset::Stress, Dataset::Detected] {
        rt.update(|s, _| s.set_dataset(dataset));
        rt.frame(1.);
        assert!(rt.find("global-source-trigger").is_none());
        assert!(rt.find("global-activity").is_some());
        assert!(rt.find("global-refresh").is_some());
        assert!(rt.find("global-theme").is_some());
        assert!(
            !rt.accessibility_json()
                .to_string()
                .contains("Inventory source")
        );
        if dataset == Dataset::Stress {
            assert_eq!(rt.state.visible.len(), 10_000);
        }
    }
}

#[test]
fn sidebar_filter_shortcut_reopens_a_collapsed_package_pane() {
    let mut rt = runtime((1024.0, 640.0));
    rt.update(|s, _| s.pane.set_open(false));
    rt.frame(0.0);
    assert!(rt.find("sidebar-brand").is_none());
    rt.key(KeyEvent {
        key: Key::Command('f'),
        mods: Mods::default(),
    });
    rt.frame(0.0);
    assert!(rt.state.pane.is_open());
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("input") && e.focused)
    );
}
