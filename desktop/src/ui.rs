use crate::{
    model::{Dataset, LibrarySection, NoteMode, Package, PackageRow, State},
    provider_badge,
    rich_select::{self, OptionRow, RichSelect},
    sidebar, sidebar_actions, sidebar_info, theme,
};
use rune::{Color, Cx, El, KeyEvent, ScrollSpec, TextStyle, Tx, col, el, icon, row, text};
use rune_kit::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Markdown,
    SegmentedControl, TextInput, glyphs, scroll_bar,
    shell::{SplitView, Toolbar},
};

fn small(label: impl Into<String>, k: &Kit) -> El {
    text(label).font(k.ty.text_xsmall).color(k.c.text_muted)
}

fn mono(label: impl Into<String>, k: &Kit) -> El {
    text(label).font(k.ty.mono).color(k.c.text_muted)
}

pub(crate) fn mark(p: &Package, k: &Kit, size: f32) -> El {
    if let Some(glyph) = p.custom_symbol {
        return el()
            .size(size, size)
            .radius(6.)
            .fill(k.c.layer_02)
            .center()
            .child(icon(glyph, size * 0.7).color(k.c.text_base))
            .a11y_hidden()
            .key("package-symbol");
    }
    if let Some(image) = p
        .custom_icon
        .as_ref()
        .filter(|i| i.size().is_some())
        .or(p.icon.as_ref())
        && image.size().is_some()
    {
        return el()
            .size(size, size)
            .pixels_fit(image.pixels(), rune::SurfaceFit::Fit)
            .a11y_hidden()
            .key("package-icon");
    }
    let fallback = p.monogram();
    let (label, color) = match p.reference_id.as_deref().unwrap_or(&p.id) {
        "gh" => ("gh", k.c.text_accent),
        "pnpm" => (
            "pn",
            rune::color::hex(if k.dark { "#d8bd82" } else { "#8b671c" }).unwrap(),
        ),
        "powertoys" => (
            "PT",
            rune::color::hex(if k.dark { "#c4a6d1" } else { "#885c9a" }).unwrap(),
        ),
        "ripgrep" => (
            "rg",
            rune::color::hex(if k.dark { "#d3a591" } else { "#965d45" }).unwrap(),
        ),
        "uv" => ("uv", k.c.success.fg),
        _ => (fallback.as_str(), k.c.text_muted),
    };
    el().size(size, size)
        .radius(6.0)
        .fill(color.alpha(0.09))
        .border(1.0, color.alpha(0.12))
        .center()
        .child(
            text(label)
                .font(TextStyle::new(if size > 30.0 { 16.0 } else { 11.0 }, 500.0).mono())
                .color(color),
        )
        .a11y_hidden()
}

fn open_link(s: &mut State, url: &str, tx: &mut Tx) {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        s.status = "Only HTTP and HTTPS links can be opened".into();
        return;
    }
    s.status = match tx.open_url(url) {
        Ok(()) => "Opened source in browser".into(),
        Err(error) => format!("Could not open source: {error}"),
    };
}

fn sidebar_version(s: &State, r: &PackageRow) -> Option<String> {
    let mut versions: Vec<_> = r
        .members
        .iter()
        .filter_map(|&i| s.catalog.packages[i].installed.as_deref())
        .collect();
    versions.sort_unstable();
    versions.dedup();
    match versions.as_slice() {
        [] => None,
        [version] => Some((*version).into()),
        _ => Some(format!("{} versions", versions.len())),
    }
}

fn package_row(s: &State, i: usize, k: &Kit) -> El {
    let r = &s.rows[s.visible[i]];
    let p = &s.catalog.packages[r.package];
    let mut provider_members: Vec<_> = r.members.iter().map(|&i| &s.catalog.packages[i]).collect();
    provider_members.sort_by_key(|p| p.display_priority);
    let providers = provider_badge::unique(provider_members.iter().filter_map(|p| p.provider));
    let info = sidebar_info::summary(s, r, sidebar_info::today());
    let manager_status = (s.section == LibrarySection::Software
        && r.members
            .iter()
            .any(|&i| s.catalog.packages[i].observation.is_some()))
    .then(|| crate::sidebar_filters::assessment(s, r));
    let version = sidebar_version(s, r);
    let row_id = r.id.clone();
    let package_id = p.id.clone();
    let checked = s.sidebar_actions.selected.contains(&r.id);
    let upgrade = providers
        .first()
        .filter(|_| info.update)
        .and_then(|&provider| sidebar_actions::badge_upgrade(s, r, provider));
    let context = if p.extension.is_some() {
        let profiles: std::collections::BTreeSet<_> = r
            .members
            .iter()
            .filter_map(|&i| crate::extension_ui::profile_key(&s.catalog.packages[i]))
            .collect();
        row()
            .grow(1.)
            .min_w(0.)
            .gap(6.)
            .center_y()
            .child(provider_badge::strip(&providers, r.members.len(), k))
            .child(
                small(
                    if profiles.len() > 1 {
                        format!("{} profiles", profiles.len())
                    } else {
                        crate::extension_ui::profile_name(p).to_owned()
                    },
                    k,
                )
                .grow(1.)
                .min_w(0.)
                .truncate()
                .help_when_truncated(crate::extension_ui::profile_name(p)),
            )
    } else if let Some(entry) = &p.watchlist {
        row()
            .grow(1.)
            .min_w(0.)
            .gap(6.)
            .center_y()
            .child(provider_badge::badge(
                releasescout_engine::inventory::Manager::Npm,
                k,
            ))
            .child(crate::watchlist_ui::category_badge(entry.group, k))
    } else if !providers.is_empty() {
        row()
            .grow(1.)
            .min_w(0.)
            .gap(6.0)
            .center_y()
            .child(provider_badge::strip_with_status(
                &providers,
                r.members.len(),
                manager_status,
                k,
            ))
            .children(upgrade.map(|(package, command)| {
                let id = package.id.clone();
                let size = sidebar_actions::upgrade_size(s, package);
                sidebar_actions::action_colored(
                    "sidebar-update",
                    &sidebar_info::DOWNLOAD,
                    "Review upgrade command",
                    size.map_or(k.c.text_muted, |size| size.color(k.dark)),
                    k,
                )
                .description(size.map_or("Provider-confirmed update", |size| size.label()))
                .help(command)
                .on_press(move |s: &mut State| s.prepare_upgrade(&id))
            }))
    } else {
        row().grow(1.).min_w(0.).gap(6.0).center_y().child(
            small(
                if r.members.len() > 1 {
                    format!("{} observations", r.members.len())
                } else {
                    p.subtitle()
                },
                k,
            )
            .truncate()
            .help_when_truncated(p.subtitle())
            .grow(1.0)
            .min_w(0.0),
        )
    };
    let detail = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .child(
            row()
                .w_full()
                .gap(7.0)
                .center_y()
                .child(
                    row()
                        .key(sidebar_actions::anchor(r, sidebar_actions::Mode::Details))
                        .role("button")
                        .label(format!("Details for {}", r.name))
                        .focusable(true)
                        .focus_ring(1., 0., k.c.border_focus)
                        .grow(1.)
                        .min_w(0.)
                        .on_press_tx(move |s: &mut State, tx| {
                            s.select_package(&package_id);
                            s.mark_current_notes_read();
                            tx.scroll_to_instant("release-notes", 0., 0.);
                            s.open_sidebar_panel(&row_id, sidebar_actions::Mode::Details);
                        })
                        .child(
                            text(&r.name)
                                .key("sidebar-name")
                                .font(TextStyle::new(14.0, 500.0))
                                .color(k.c.text_base.alpha(
                                    if s.table.cursor() == Some(r.id.as_str()) {
                                        1.0
                                    } else {
                                        0.9
                                    },
                                ))
                                .grow(1.0)
                                .min_w(0.0)
                                .truncate()
                                .help_when_truncated(&r.name),
                        ),
                )
                .children(info.failed.then(|| {
                    icon(&glyphs::CIRCLE_X, 14.0)
                        .color(k.c.text_muted)
                        .key("sidebar-source-error")
                        .label("A check failed")
                        .help("A check failed; view details in Info")
                }))
                .children((p.extension.is_some() && r.members.iter().any(|&i| crate::extension_ui::has_update(s, &s.catalog.packages[i]))).then(|| {
                    icon(&sidebar_info::UPDATE, 14.).color(k.c.success.fg)
                        .key("sidebar-extension-update")
                        .label("Extension gallery update available")
                        .help("A compatible gallery update was found for an editor profile.")
                }))
                .children((info.unread > 0).then(|| {
                    row()
                        .h(18.0)
                        .min_w(18.0)
                        .px(4.0)
                        .radius(4.0)
                        .fill(k.c.layer_02)
                        .center()
                        .key("sidebar-unread-count")
                        .label(format!("{} unread notes", info.unread))
                        .child(
                            small(
                                if info.unread > 99 {
                                    "99+".into()
                                } else {
                                    info.unread.to_string()
                                },
                                k,
                            )
                            .color(k.c.text_base),
                        )
                        .help(format!("{} unread release notes", info.unread))
                })),
        )
        .children(
            sidebar_info::description(s, r)
                .and_then(|description| crate::description_preview::label(description, r, k, true)),
        );
    let metadata = row()
        .w_full()
        .gap(5.0)
        .center_y()
        .child(context)
        .children(version.as_ref().or(info.age.as_ref()).map(|value| {
            let value = small(value, k)
                .key(if version.is_some() {
                    "sidebar-version"
                } else {
                    "sidebar-release-age"
                })
                .max_w(76.)
                .truncate()
                .help(if version.is_some() {
                    format!("Installed: {value}; view version history")
                } else {
                    format!("Latest retrieved release: {value}")
                });
            if version.is_some() {
                let row_id = r.id.clone();
                row()
                    .key(sidebar_actions::anchor(r, sidebar_actions::Mode::Versions))
                    .role("button")
                    .label(format!("Versions for {}", r.name))
                    .focusable(true)
                    .radius(4.)
                    .px(4.)
                    .py(2.)
                    .hover(k.c.hover)
                    .focus_ring(1., 0., k.c.border_focus)
                    .on_press(move |s: &mut State| {
                        s.open_sidebar_panel(&row_id, sidebar_actions::Mode::Versions)
                    })
                    .child(value)
            } else {
                value
            }
        }))
        .child(
            row()
                .gap(2.)
                .center_y()
                .child(sidebar_actions::info_button(r, k))
                .child(crate::ai_menu::button(s, r, k)),
        );
    row().w_full().px(10.).child(
        col()
            .role("gridcell")
            .w_full()
            .gap(6.)
            .child(
                row()
                    .w_full()
                    .center_y()
                    .gap(11.)
                    .child(
                        el().size(28., 28.)
                            .key("sidebar-icon")
                            .child(
                                mark(p, k, 28.0).group_opacity(if checked { 0. } else { 1. }, 0.),
                            )
                            .child(
                                sidebar_actions::checkbox(s, r, k)
                                    .absolute(0., 0.)
                                    .group_opacity(if checked { 1. } else { 0. }, 1.),
                            ),
                    )
                    .child(detail),
            )
            .child(metadata),
    )
}

fn inventory(s: &State, k: &Kit, reduced: bool, layers: &mut Vec<El>) -> El {
    let extensions = s.section == LibrarySection::Extensions;
    let packages = s.section == LibrarySection::Packages;
    let filter = TextInput::new(&s.query)
        .label(if packages {
            "Filter packages"
        } else if extensions {
            "Filter extensions"
        } else {
            "Filter software"
        })
        .placeholder(if packages {
            "Find packages..."
        } else if extensions {
            "Find extensions..."
        } else {
            "Find software..."
        })
        .leading_icon(&glyphs::SEARCH)
        .clear_button()
        .on_edit(|s: &mut State| &mut s.query)
        .on_change(|s: &mut State, tx| {
            if s.command.revealed.take().is_some() {
                s.set_dataset(s.dataset);
            } else {
                s.refilter();
            }
            tx.scroll_to_instant("viewport", 0.0, 0.0);
            tx.scroll_to_instant("release-notes", 0.0, 0.0);
        })
        .build(k)
        .w_full()
        .key("package-filter");
    let table = sidebar::list(s, k, reduced, package_row, layers);
    col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(14.0)
        .px(14.0)
        .pt(20.0)
        .pb(16.0)
        .child({
            SegmentedControl::new(
                &["Software", "Extensions", "Packages"],
                Some(s.section as usize),
            )
            .width(None)
            .label("Library section")
            .segment_state(
                1,
                if s.dataset == Dataset::Detected {
                    rune_kit::State::Rest
                } else {
                    rune_kit::State::Disabled
                },
            )
            .on_pick(|s: &mut State, i, tx| {
                s.set_section(
                    [
                        LibrarySection::Software,
                        LibrarySection::Extensions,
                        LibrarySection::Packages,
                    ][i],
                );
                s.inventory.open = false;
                s.mark_current_notes_read();
                tx.scroll_to_instant("viewport", 0.0, 0.0);
                tx.scroll_to_instant("release-notes", 0.0, 0.0);
                tx.instant();
            })
            .build(k)
            .key("library-section")
        })
        .child(
            col()
                .w_full()
                .grow(1.)
                .min_h(0.)
                .gap(4.)
                .child(filter)
                .children(crate::command::reveal_notice(s, k))
                .children(extensions.then(|| crate::extension_ui::filters(s, k, layers)))
                .child(table),
        )
        .child(
            col()
                .gap(6.0)
                .px(7.0)
                .child(
                    row()
                        .gap(7.0)
                        .center_y()
                        .child(icon(&glyphs::FOLDER, 13.0).color(k.c.text_faint))
                        .child(small(
                            if packages {
                                if s.packages.watch.project_mode {
                                    "Project dependencies"
                                } else {
                                    "Personal watchlist"
                                }
                            } else {
                                match s.dataset {
                                    Dataset::Stress => "Synthetic inventory",
                                    Dataset::Detected if extensions => "Editor extensions",
                                    Dataset::Detected => "Detected software",
                                    Dataset::Reference => "Demo data",
                                }
                            },
                            k,
                        )),
                )
                .help(if packages {
                    if s.packages.watch.project_mode {
                        "package.json"
                    } else {
                        "Published releases"
                    }
                } else if s.dataset == Dataset::Detected {
                    "Last successful local scans"
                } else {
                    "Demo data / not a machine scan"
                }),
        )
}

fn identity_label(p: &Package) -> String {
    if let Some(raw) = &p.repository
        && let Ok(url) = url::Url::parse(raw)
    {
        let host = url.host_str().unwrap_or("").trim_start_matches("www.");
        if host == "github.com" {
            return url.path().trim_matches('/').trim_end_matches(".git").into();
        }
        if host == "marketplace.visualstudio.com" {
            return url
                .query_pairs()
                .find(|(k, _)| k == "itemName")
                .map(|(_, id)| format!("Marketplace / {id}"))
                .unwrap_or_else(|| "Marketplace".into());
        }
        if host == "npmjs.com" {
            return format!("npmjs.com / {}", url.path().trim_start_matches("/package/"));
        }
        if !host.is_empty() {
            return host.into();
        }
    }
    p.installation_label().into()
}

fn reader_header(s: &State, p: &Package, k: &Kit, layers: &mut Vec<El>) -> El {
    let url = p
        .releases
        .first()
        .filter(|r| r.url.starts_with("https://") || r.url.starts_with("http://"))
        .map(|r| r.url.clone())
        .or_else(|| p.repository.clone());
    let actions = row()
        .key("reader-actions")
        .gap(4.0)
        .center_y()
        .child(
            IconButton::new(&crate::description_ui::EDIT)
                .variant(IconButtonVariant::Ghost)
                .label("Edit description")
                .help(if s.activity.writable() {
                    "Edit description"
                } else {
                    "Saved preferences unavailable"
                })
                .disabled(!s.activity.writable())
                .on_press_tx(|s: &mut State, tx| {
                    s.open_description_editor();
                    tx.focus("description-input", true);
                })
                .build(k)
                .size(28., 28.)
                .key("edit-description"),
        )
        .child({
            IconButton::new(&crate::custom_icons::CHOOSE)
                .variant(IconButtonVariant::Ghost)
                .label(if s.has_icon_override(p) {
                    "Change icon"
                } else {
                    "Choose icon"
                })
                .help(if s.has_icon_override(p) {
                    "Change custom icon"
                } else {
                    "Choose icon"
                })
                .disabled(s.custom_icons.dialog || s.custom_icons.work.is_pending())
                .on_press(|s: &mut State| s.open_icon_picker())
                .build(k)
                .size(28., 28.)
                .key("choose-package-icon")
        })
        .child({
            IconButton::new(&crate::custom_icons::RESET)
                .variant(IconButtonVariant::Ghost)
                .label("Use automatic icon")
                .help("Use automatic icon")
                .disabled(
                    !s.has_icon_override(p)
                        || s.custom_icons.dialog
                        || s.custom_icons.work.is_pending(),
                )
                .on_press(|s: &mut State| s.reset_icon())
                .build(k)
                .size(28., 28.)
                .key("reset-package-icon")
        })
        .child(
            IconButton::new(&glyphs::COPY)
                .variant(IconButtonVariant::Ghost)
                .label("Copy release notes")
                .help("Copy release notes")
                .disabled(p.releases.is_empty())
                .on_press_tx(|s: &mut State, tx| {
                    tx.copy(s.selected_markdown());
                    s.status = "Release notes copied".into();
                })
                .build(k)
                .size(28., 28.)
                .key("copy-notes"),
        )
        .children(p.watchlist.as_ref().map(|_| {
            IconButton::new(&glyphs::STAR_FILLED)
                .variant(IconButtonVariant::Ghost)
                .label("Unfollow package")
                .help("Unfollow package")
                .disabled(s.packages.watch.blocked)
                .on_press(|s: &mut State| s.remove_watchlist_entry())
                .build(k)
                .size(28., 28.)
                .key("unfollow-package")
        }))
        .children(crate::background_watch_ui::reader_button(s, p, k))
        .child(
            Button::new("Source")
                .icon(&glyphs::ARROW_RIGHT)
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Large)
                .disabled(url.is_none())
                .on_press_tx(move |s: &mut State, tx| {
                    if let Some(url) = &url {
                        open_link(s, url, tx);
                    }
                })
                .build(k)
                .radius(6.)
                .key("open-release"),
        );
    let metadata = row()
        .w_full()
        .gap(8.)
        .center_y()
        .justify_end()
        .flex_wrap()
        .key("version-range")
        .child(if p.watchlist.is_some() {
            provider_badge::badge(releasescout_engine::inventory::Manager::Npm, k)
        } else {
            p.provider
                .map(|m| provider_badge::badge(m, k))
                .unwrap_or_else(|| small(p.subtitle(), k))
        })
        .children(
            p.watchlist
                .as_ref()
                .map(|_| crate::reader_controls::category(s, k, layers)),
        )
        .children(p.watchlist.is_none().then(|| small("/", k)))
        .children(p.watchlist.is_none().then(|| {
            mono(p.installed.as_deref().unwrap_or("Version unrecorded"), k)
                .font(k.ty.text_xsmall.mono())
                .max_w(320.)
                .wrap_anywhere()
        }))
        .children(p.extension.as_ref().map(|_| {
            row()
                .key("selected-extension-profile")
                .gap(6.)
                .center_y()
                .max_w(260.)
                .min_w(0.)
                .child(crate::profile_icons::view(
                    p.extension.as_ref().and_then(|e| e.profile_icon.as_deref()),
                    k,
                    14.,
                ))
                .child(
                    small(crate::extension_ui::profile_name(p), k)
                        .min_w(0.)
                        .truncate()
                        .help(crate::extension_ui::profile_name(p)),
                )
        }))
        .children(crate::extension_ui::update_label(s, p, k))
        .children(p.target.as_ref().map(|target| {
            row()
                .gap(10.)
                .center_y()
                .max_w(320.)
                .min_w(0.)
                .child(icon(&glyphs::ARROW_RIGHT, 13.).color(k.c.text_faint))
                .child(
                    mono(target, k)
                        .font(k.ty.text_xsmall.mono())
                        .color(
                            if p.observation.as_ref().is_some_and(|o| o.confirmed_update) {
                                k.c.success.fg
                            } else {
                                k.c.text_muted
                            },
                        )
                        .help(
                            if p.observation.as_ref().is_some_and(|o| o.confirmed_update) {
                                "Provider-confirmed update candidate"
                            } else {
                                "Provider candidate; newer version not confirmed"
                            },
                        )
                        .min_w(0.)
                        .wrap_anywhere(),
                )
        }));
    col()
        .key("reader-header")
        .w_full()
        .px(24.0)
        .pt(20.0)
        .pb(16.0)
        .gap(12.0)
        .child(
            row()
                .w_full()
                .items_stretch()
                .justify_between()
                .gap(16.0)
                .flex_wrap()
                .child(
                    col()
                        .grow(1.)
                        .min_w(220.)
                        .gap(12.)
                        .child(
                            row()
                                .w_full()
                                .min_w(0.)
                                .gap(12.0)
                                .center_y()
                                .child(mark(p, k, 40.0))
                                .child(
                                    col()
                                        .grow(1.)
                                        .min_w(0.)
                                        .gap(3.0)
                                        .child(
                                            text(s.display_name(p))
                                                .font(TextStyle::new(20.0, 600.0).lh(26.0))
                                                .color(k.c.text_base)
                                                .w_full()
                                                .min_w(0.)
                                                .truncate()
                                                .help_when_truncated(s.display_name(p))
                                                .key("selected-package"),
                                        )
                                        .child(
                                            small(identity_label(p), k)
                                                .w_full()
                                                .min_w(0.)
                                                .truncate()
                                                .key("reader-identity"),
                                        ),
                                ),
                        )
                        .children(
                            s.rows
                                .iter()
                                .find(|r| Some(r.id.as_str()) == s.table.cursor())
                                .and_then(|r| {
                                    sidebar_info::description(s, r).and_then(|description| {
                                        crate::description_preview::label(description, r, k, false)
                                    })
                                }),
                        ),
                )
                .child(
                    col()
                        .key("reader-utilities")
                        .w(320.)
                        .gap(8.)
                        .items_end()
                        .child(actions)
                        .child(metadata)
                        .children(
                            (s.section == LibrarySection::Software && p.observation.is_some())
                                .then(|| {
                                    s.rows
                                        .iter()
                                        .find(|r| Some(r.id.as_str()) == s.table.cursor())
                                        .map(|r| crate::update_status::overview(s, r, p, k))
                                })
                                .flatten(),
                        ),
                ),
        )
        .children(
            s.custom_icons
                .error
                .as_ref()
                .filter(|(key, _)| key == &crate::custom_icons::identity(p))
                .map(|(_, error)| small(error, k).wrap_anywhere().key("custom-icon-error")),
        )
}

pub(crate) fn date_parts(published: &str) -> (String, &str) {
    let date = published.get(..10).unwrap_or(published);
    let month = date
        .get(5..7)
        .and_then(|m| m.parse::<usize>().ok())
        .and_then(|m| m.checked_sub(1))
        .and_then(|m| {
            [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ]
            .get(m)
            .copied()
        });
    match (month, date.get(8..10), date.get(..4)) {
        (Some(month), Some(day), Some(year)) => (format!("{month} {day}"), year),
        _ => (date.to_owned(), ""),
    }
}

fn engine_controls(s: &State, k: &Kit, layers: &mut Vec<El>) -> El {
    let p = s.selected().unwrap();
    if p.watchlist.is_some() {
        return crate::watchlist_ui::controls(s, k, layers);
    }
    if p.dependency.is_some() {
        return crate::package_ui::controls(s, k, layers);
    }
    if p.extension.is_some() {
        return crate::extension_ui::controls(s, k, layers);
    }
    let q = &s.live.sessions[&p.id];
    let profile = s
        .live
        .catalog
        .packages
        .iter()
        .find(|x| x.id == p.id)
        .unwrap();
    let mut area = crate::reader_controls::area("engine-controls");
    if let Some(reason) = s.live.unavailable(&p.id) {
        if p.source_hints.is_some() && matches!(profile.tracking.as_str(), "track" | "low_priority")
        {
            return area
                .child(small(s.source_status(), k).key("source-discovery-status"))
                .child(
                    row().child(
                        Button::new("Retry source lookup")
                            .icon(&glyphs::ARROW_RIGHT)
                            .disabled(
                                s.source_loader.work.is_pending()
                                    || p.source_hints.as_ref().is_none_or(|h| !h.has_hints()),
                            )
                            .on_press(|s: &mut State| s.retry_source())
                            .build(k)
                            .key("retry-source-lookup"),
                    ),
                );
        }
        return area.child(small(reason, k).key("engine-unavailable"));
    }
    let stream = &profile.streams[q.component];
    let selector = RichSelect {
        key: "engine-component",
        label: "Release component",
        current: &stream.id,
        open: q.component_open,
        popup_width: 440.,
        value: text(&stream.id)
            .font(k.ty.ui_compact)
            .color(k.c.text_base)
            .truncate(),
        options: profile
            .streams
            .iter()
            .map(|stream| {
                let basis = match stream.basis {
                    releasescout_engine::Basis::Version => "Version history",
                    releasescout_engine::Basis::Date => "Dated updates",
                };
                let detail = format!("{} / {basis}", stream.scope);
                OptionRow {
                    id: stream.id.clone(),
                    label: stream.id.clone(),
                    description: detail.clone(),
                    content: row()
                        .gap(10.)
                        .center_y()
                        .child(icon(&glyphs::FILE_TEXT, 20.).color(k.c.text_muted))
                        .child(rich_select::text_content(&stream.id, &detail, k).grow(1.)),
                }
            })
            .collect(),
        on_toggle: |s: &mut State| {
            let q = s.engine_query();
            q.component_open = !q.component_open;
        },
        on_close: |s: &mut State| s.engine_query().component_open = false,
        on_choose: |s: &mut State, value, _| {
            let id = s.selected().unwrap().id.clone();
            let profile = s.live.catalog.packages.iter().find(|p| p.id == id).unwrap();
            let Some(index) = profile.streams.iter().position(|s| s.id == value) else {
                return;
            };
            let channel = match profile.streams[index].channel {
                releasescout_engine::Channel::Stable => 0,
                releasescout_engine::Channel::Preview => 1,
                releasescout_engine::Channel::All => 2,
            };
            let q = s.engine_query();
            q.component = index;
            q.component_open = false;
            q.channel = channel;
            q.from = rune::TextEdit::new("");
            q.to = rune::TextEdit::new("");
            q.inventory_range = false;
        },
    }
    .build_compact(k, layers)
    .w(148.);
    area = area.child(
        row()
            .w_full()
            .flex_wrap()
            .gap(8.0)
            .center_y()
            .child(
                SegmentedControl::new(&["History", "Range"], Some(usize::from(q.range)))
                    .width(Some(156.0))
                    .label("Fetch mode")
                    .on_pick(|s: &mut State, i, _| {
                        let q = s.engine_query();
                        q.range = i == 1;
                        q.inventory_range = false;
                    })
                    .build(k)
                    .key("engine-mode"),
            )
            .children((profile.streams.len() > 1).then_some(selector))
            .child(crate::reader_controls::actions(
                s,
                k,
                layers,
                q.range && stream.basis != releasescout_engine::Basis::Date,
            )),
    );
    if q.range {
        let dated = stream.basis == releasescout_engine::Basis::Date;
        area = area.child(
            row()
                .w_full()
                .flex_wrap()
                .gap(8.0)
                .center_y()
                .child(small(if dated { "Since" } else { "From" }, k))
                .child(
                    TextInput::new(&q.from)
                        .label(if dated { "Since date" } else { "From version" })
                        .placeholder(if dated {
                            "Since: YYYY-MM-DD"
                        } else {
                            "From version"
                        })
                        .on_edit(|s: &mut State| {
                            let q = s.engine_query();
                            q.inventory_range = false;
                            &mut q.from
                        })
                        .height(30.)
                        .build(k)
                        .w(148.0)
                        .radius(6.)
                        .border(1., k.c.border_base)
                        .key("range-from"),
                )
                .child(icon(&glyphs::ARROW_RIGHT, 14.).color(k.c.text_faint))
                .child(small(if dated { "Until" } else { "To" }, k))
                .child(
                    TextInput::new(&q.to)
                        .label(if dated { "Until date" } else { "To version" })
                        .placeholder(if dated {
                            "Until: YYYY-MM-DD"
                        } else {
                            "To version"
                        })
                        .on_edit(|s: &mut State| {
                            let q = s.engine_query();
                            q.inventory_range = false;
                            &mut q.to
                        })
                        .height(30.)
                        .build(k)
                        .w(148.0)
                        .radius(6.)
                        .border(1., k.c.border_base)
                        .key("range-to"),
                ),
        );
    }
    area.description(&stream.scope)
}

fn observation_label(p: &Package) -> String {
    if p.extension.is_some() {
        return crate::extension_ui::membership_label(p);
    }
    format!(
        "{} / {}",
        p.installed.as_deref().unwrap_or("Unresolved"),
        p.dependency
            .as_ref()
            .map(|d| format!(
                "{} / {}",
                d.project.split(" / ").next().unwrap_or(&d.project),
                d.kind
            ))
            .unwrap_or_else(|| p
                .provider
                .map(provider_badge::label)
                .unwrap_or_else(|| p.manager_label())
                .into())
    )
}

fn observation_select(s: &State, k: &Kit, layers: &mut Vec<El>) -> Option<El> {
    let observations = s.selected_observations();
    if observations.len() <= 1 {
        return None;
    }
    let p = s.selected()?;
    let options = observations
        .into_iter()
        .map(|p| {
            let title = if p.extension.is_some() {
                crate::extension_ui::profile_name(p)
            } else if let Some(d) = &p.dependency {
                &d.project
            } else {
                p.installed.as_deref().unwrap_or("Unresolved")
            };
            let detail = p.reference_text.as_deref().unwrap_or(&p.observation_label);
            OptionRow {
                id: p.id.clone(),
                label: observation_label(p),
                description: p.observation_label.clone(),
                content: row().gap(10.).center_y().child(mark(p, k, 24.)).child(
                    col()
                        .grow(1.)
                        .min_w(0.)
                        .gap(4.)
                        .child(
                            row()
                                .gap(8.)
                                .center_y()
                                .child(
                                    text(title)
                                        .font(k.ty.ui_medium)
                                        .color(k.c.text_base)
                                        .grow(1.)
                                        .min_w(0.)
                                        .truncate(),
                                )
                                .children(p.provider.map(|m| provider_badge::badge(m, k))),
                        )
                        .child(small(detail, k).truncate()),
                ),
            }
        })
        .collect();
    Some(
        RichSelect {
            key: "selected-observation",
            label: "Installed observation",
            current: &p.id,
            open: s.observation_picker,
            popup_width: 520.,
            options,
            value: row().gap(8.).center_y().child(mark(p, k, 20.)).child(
                text(observation_label(p))
                    .font(k.ty.ui_compact)
                    .color(k.c.text_base)
                    .grow(1.)
                    .min_w(0.)
                    .truncate(),
            ),
            on_toggle: |s: &mut State| s.observation_picker = !s.observation_picker,
            on_close: |s: &mut State| s.observation_picker = false,
            on_choose: |s: &mut State, id, tx| {
                if s.selected_observations().iter().any(|p| p.id == id) {
                    s.select_package(id);
                    s.mark_current_notes_read();
                    tx.instant();
                }
            },
        }
        .build(k, layers)
        .w(420.),
    )
}

fn evidence_toggle(details: bool, notices: usize, k: &Kit) -> El {
    Button::new(if notices == 0 {
        "Details".into()
    } else {
        format!("Details ({notices})")
    })
    .variant(ButtonVariant::Ghost)
    .icon(if details {
        &glyphs::CHEVRON_UP
    } else {
        &glyphs::CHEVRON_DOWN
    })
    .expanded(details)
    .help("Release source and coverage details")
    .on_press_tx(|s: &mut State, tx| {
        let q = s.engine_query();
        q.details = !q.details;
        tx.instant();
    })
    .build(k)
    .h(26.)
    .radius(6.)
    .key("engine-details")
}

fn notes_context(query: &crate::live::Query, k: &Kit) -> Option<El> {
    let bounds = query.report_cache.as_ref().and_then(crate::note_cache::Context::range_bounds);
    if bounds.is_none() && !query.restored {
        return None;
    }
    let mut context = row().key("engine-context").w_full().center_y().gap(8.);
    if let Some((from, to)) = bounds {
        let description = format!("Release notes after {from} through {to}");
        context = context.child(
            row()
                .center_y()
                .gap(6.)
                .min_w(0.)
                .shrink()
                .help(&description)
                .description(description)
                .key("engine-loaded-range")
                .child(small(from, k).font(k.ty.text_small.mono()).min_w(0.).truncate())
                .child(icon(&glyphs::ARROW_RIGHT, 12.).color(k.c.icon_muted).a11y_hidden())
                .child(small(to, k).font(k.ty.text_small.mono()).min_w(0.).truncate()),
        );
    }
    if query.restored {
        context = context.child(
            small("Saved notes", k)
                .px(6.)
                .py(2.)
                .radius(4.)
                .fill(k.c.layer_02)
                .help("Loaded from this device")
                .key("engine-saved-notes"),
        );
    }
    Some(context)
}

fn reader(s: &State, k: &Kit, layers: &mut Vec<El>, cx: &Cx<State>) -> El {
    let Some(p) = s.selected() else {
        return col()
            .w_full()
            .h_full()
            .center()
            .gap(14.0)
            .child(icon(&glyphs::FILE_TEXT, 28.0).color(k.c.icon_muted))
            .child(small(
                if s.section == LibrarySection::Packages {
                    "No package selected"
                } else if s.section == LibrarySection::Extensions {
                    "No extension selected"
                } else {
                    "No software selected"
                },
                k,
            ));
    };
    let indexed = s.mode == NoteMode::Rendered && p.releases.len() > 1;
    let wide_index = indexed
        && cx
            .layout_with("notes-page", |r| r[2] >= crate::release_index::WIDE_READER)
            .unwrap_or(false);
    let active = if indexed {
        crate::release_index::active(cx, p.releases.len().min(s.live.sessions[&p.id].shown))
    } else {
        0
    };
    // Keep the kit's keyboard/focus behavior, with a flat tab treatment.
    let mut tabs_kit = k.clone();
    tabs_kit.c.layer_01 = Color::TRANSPARENT;
    tabs_kit.c.bg_base = Color::TRANSPARENT;
    tabs_kit.c.border_base = Color::TRANSPARENT;
    tabs_kit.c.border_strong = Color::TRANSPARENT;
    let note_modes = [NoteMode::Rendered, NoteMode::Source, NoteMode::Ai];
    let selected_mode = note_modes.iter().position(|mode| *mode == s.mode).unwrap_or(0);
    let tabs = row()
        .w_full()
        .px(28.0)
        .h(42.0)
        .center_y()
        .justify_between()
        .border_sides([1.0, 0.0, 1.0, 0.0], theme::separator(k))
        .child(
            SegmentedControl::new(
                &["Changelog", "Markdown", "AI"],
                Some(selected_mode),
            )
            .label("Release note view")
            .segment_state(
                0,
                if p.releases.is_empty() {
                    rune_kit::State::Disabled
                } else {
                    rune_kit::State::Rest
                },
            )
            .segment_state(
                1,
                if p.releases.is_empty() {
                    rune_kit::State::Disabled
                } else {
                    rune_kit::State::Rest
                },
            )
            .width(Some(300.0))
            .on_pick(move |s: &mut State, i, tx| {
                s.reader_controls.release_index_open = false;
                s.reader_controls.reset_scroll();
                s.mode = note_modes[i];
                tx.scroll_to_instant("release-notes", 0.0, 0.0);
            })
            .build(&tabs_kit)
            .h(42.0)
            .child(
                el().w(100.0)
                    .h(2.0)
                    .absolute(selected_mode as f32 * 100.0, 40.0)
                    .fill(k.c.text_accent)
                    .a11y_hidden(),
            )
            .key("note-mode"),
        )
        .child(if indexed && !wide_index {
            crate::release_index::compact(s, k, active, layers)
        } else {
            small(
                if p.sample_notes {
                    format!("{} sample releases", p.releases.len())
                } else if let Some(report) = s.live.sessions[&p.id].report.as_ref() {
                    format!(
                        "{} notes",
                        report
                            .notes
                            .iter()
                            .filter(|note| note.authored_notes)
                            .count()
                    )
                } else {
                    "Not fetched".into()
                },
                k,
            )
        });
    let mut document = col()
        .key("release-document")
        .w_full()
        .max_w(920.0)
        .px(28.)
        .py(24.);
    let query = &s.live.sessions[&p.id];
    if s.mode == NoteMode::Ai {
        return reader_frame(
            s, p, k, cx, layers, tabs, document.py(16.).child(crate::ai_menu::reader(s, p, k)), None,
        );
    }
    let local_inventory = p.detected
        && p.extension.is_none()
        && p.dependency.is_none()
        && p.watchlist.is_none()
        && p.reference_text.is_some();
    let prefetching = s.note_prefetch.loading(&p.id) && query.report.is_none();
    if s.live.active.as_deref() == Some(&p.id) {
        document = document.child(small("Fetching release notes...", k).key("engine-loading"));
    } else if prefetching {
        document = document.child(small("Loading notes for the available update...", k).key("prefetch-loading"));
    }
    if let Some(error) = &query.error {
        document = document.child(
            text(error)
                .font(k.ty.ui)
                .color(k.c.text_base)
                .wrap_anywhere()
                .key("engine-error"),
        );
    }
    if let Some(report) = &query.report {
        let warnings = report
            .warnings
            .iter()
            .cloned()
            .chain(
                report
                    .attempts
                    .iter()
                    .filter(|a| a.outcome == "failed")
                    .map(|a| a.detail.clone()),
            )
            .collect::<Vec<_>>();
        let details = warnings.join("\n");
        let provenance = report
            .evidence
            .iter()
            .map(|e| format!("{} / {}", e.cache, e.fetched_at))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join("; ");
        document = document.child(
            col()
                .w_full()
                .gap(8.0)
                .pb(24.0)
                .key("engine-report")
                .children(notes_context(query, k))
                .child(
                    row()
                        .w_full()
                        .center_y()
                        .gap(8.0)
                        .children(
                            local_inventory
                                .then(|| crate::reader_controls::local_inventory(s, k, layers)),
                        )
                        .child(
                            icon(
                                if report.complete {
                                    &glyphs::CHECK
                                } else {
                                    &glyphs::FILE_TEXT
                                },
                                14.,
                            )
                            .color(k.c.icon_muted),
                        )
                        .child(
                            small(
                                if report.status == "source_failed" {
                                    "Source unavailable"
                                } else if report.complete {
                                    "Complete coverage"
                                } else if report.evidence.iter().any(|e| {
                                    e.cache == "offline_cache" || e.cache.starts_with("stale_cache")
                                }) {
                                    "Cached snapshot / partial coverage"
                                } else {
                                    "Partial or unverified coverage"
                                },
                                k,
                            )
                            .grow(1.)
                            .min_w(0.)
                            .wrap_anywhere()
                            .key("engine-completeness"),
                        )
                        .child(evidence_toggle(query.details, warnings.len(), k)),
                )
                .children(query.details.then(|| {
                    col()
                        .gap(3.)
                        .key("engine-source-details")
                        .children((!report.scope.is_empty()).then(|| {
                            small(&report.scope, k).wrap_anywhere().selectable(true)
                        }))
                        .children((!query.caption.is_empty()).then(|| {
                            small(&query.caption, k).wrap_anywhere().selectable(true)
                        }))
                }))
                .children(
                    (query.details && !details.is_empty())
                        .then(|| small(details, k).wrap_anywhere().selectable(true)),
                )
                .children(
                    (query.details && !provenance.is_empty())
                        .then(|| small(provenance, k).wrap_anywhere().selectable(true)),
                ),
        );
    }
    if query.report.is_none() && p.reference_text.is_some() && p.watchlist.is_none() {
        document = document.child(if local_inventory {
            row()
                .w_full()
                .pb(24.)
                .child(crate::reader_controls::local_inventory(s, k, layers))
        } else {
            row()
                .w_full()
                .gap(8.)
                .center_y()
                .pb(24.)
                .child(icon(&glyphs::FILE_TEXT, 14.).color(k.c.icon_muted))
                .child(
                    small(
                        if p.sample_notes {
                            "Sample notes / installed range unverified"
                        } else if p.extension.is_some() {
                            "Installed extension changelog"
                        } else if p.dependency.is_some() {
                            "Project dependency"
                        } else if p.detected {
                            "Local inventory"
                        } else {
                            "Reference entry / installation unverified"
                        },
                        k,
                    )
                    .grow(1.)
                    .min_w(0.)
                    .wrap_anywhere(),
                )
                .child(evidence_toggle(query.details, 0, k))
        });
    }
    if let Some(reference) = &p.reference_text
        && p.watchlist.is_none()
        && !local_inventory
        && query.details
    {
        if p.extension.is_some() {
            document = document.child(small(reference, k).wrap_anywhere().pb(14.0));
        } else {
            document =
                document.child(
                    col()
                        .w_full()
                        .gap(8.0)
                        .pb(28.0)
                        .child(small(
                            if p.dependency.is_some() {
                                "Project manifest / dependency declaration".into()
                            } else if p.detected {
                                "Local inventory / detected observation".into()
                            } else {
                                format!("{} / Reference entry", p.reference_file())
                            },
                            k,
                        ))
                        .child(small(p.update_label(), k).key("update-status"))
                        .children(
                            p.component_reason
                                .as_ref()
                                .map(|reason| small(reason, k).key("component-reason")),
                        )
                        .child(
                            text(reference)
                                .font(k.ty.ui)
                                .color(k.c.text_base)
                                .wrap_anywhere(),
                        )
                        .children(p.sample_notes.then(|| {
                            small("Synthetic sample notes / not published releases", k)
                        })),
                );
        }
    }
    if p.releases.is_empty() && !prefetching {
        document = document.child(
            col()
                .w_full()
                .gap(10.0)
                .py(24.0)
                .key("missing-notes")
                .child(icon(&glyphs::FILE_TEXT, 24.0).color(k.c.text_faint))
                .child(
                    text(if s.live.active.as_deref() == Some(&p.id) {
                        "Loading release notes"
                    } else if query.error.is_some()
                        || query
                            .report
                            .as_ref()
                            .is_some_and(|r| r.status == "source_failed")
                    {
                        "Could not load release notes"
                    } else if query.report.is_some() {
                        "No matching releases in this window"
                    } else {
                        "No release notes loaded"
                    })
                    .font(k.ty.body_medium)
                    .color(k.c.text_base),
                )
                .child(small(
                    if query.report.is_some() || query.error.is_some() {
                        "No update availability was inferred"
                    } else if s.live.catalog.packages.iter().any(|profile| {
                        profile.id == p.id && profile.identity == "metadata_discovered"
                    }) {
                        "Discovered source / notes not fetched"
                    } else {
                        "Release source not checked"
                    },
                    k,
                )),
        );
    } else if s.mode == NoteMode::Source {
        document = document.child(
            text(s.selected_markdown())
                .font(k.ty.mono)
                .color(k.c.text_base)
                .selectable(true)
                .wrap_anywhere()
                .key("raw-notes"),
        );
    } else {
        for (i, release) in p.releases.iter().take(query.shown).enumerate() {
            let url = release.url.clone();
            let remote = url.starts_with("https://") || url.starts_with("http://");
            let source = row()
                .gap(8.0)
                .center_y()
                .grow(1.)
                .min_w(0.)
                .child(
                    text(&release.version)
                        .font(TextStyle::new(18.0, 600.0).lh(26.0))
                        .color(k.c.text_base)
                        .grow(1.)
                        .min_w(0.)
                        .wrap_anywhere(),
                )
                .children(remote.then(|| icon(&glyphs::ARROW_RIGHT, 12.0).color(k.c.text_faint)));
            let source = if remote {
                source
                    .role("link")
                    .help("Open release source")
                    .label(format!("Release {} source", release.version))
                    .focusable(true)
                    .focus_ring(1., 3., k.c.border_focus)
                    .on_press_tx(move |s: &mut State, tx| open_link(s, &url, tx))
                    .key(format!("release-{}", release.version))
            } else {
                source
                    .label("Installed changelog section")
                    .key(format!("release-{}", release.version))
            };
            let (day, year) = date_parts(&release.published);
            let date = if year.is_empty() {
                if day.is_empty() {
                    "Publication date unknown".into()
                } else {
                    day
                }
            } else {
                format!("{day}, {year}")
            };
            let last = i + 1 == p.releases.len().min(query.shown);
            let mut rail = el().w(32.0).a11y_hidden().child(
                el().size(16.0, 16.0)
                    .radius(8.0)
                    .border(1.0, k.c.border_strong)
                    .fill(k.c.bg_base)
                    .center()
                    .child(el().size(4., 4.).radius(2.).fill(if i == 0 {
                        k.c.text_base
                    } else {
                        k.c.text_muted
                    }))
                    .absolute(0.0, 0.0)
                    .key(format!("timeline-indicator-{i}")),
            );
            if !last {
                rail = rail.child(
                    el().w(1.0)
                        .pin(Some(7.5), Some(18.0), None, Some(2.0))
                        .fill(k.c.border_muted)
                        .key(format!("timeline-separator-{i}")),
                );
            }
            let body = if release.markdown.trim().is_empty() {
                "No authored notes retrieved for this release."
            } else {
                &release.markdown
            };
            let notes = col()
                .grow(1.0)
                .min_w(0.0)
                .gap(16.0)
                .pb(if last { 0.0 } else { 40.0 })
                .key(format!("timeline-content-{i}"))
                .child(
                    col()
                        .w_full()
                        .gap(4.0)
                        .child(
                            text(date)
                                .font(TextStyle::new(12.0, 500.0).lh(16.0))
                                .color(k.c.text_muted)
                                .key(format!("timeline-date-{i}")),
                        )
                        .child(
                            row().w_full().gap(12.).center_y().child(source).child(
                                IconButton::new(&glyphs::COPY)
                                    .variant(IconButtonVariant::Ghost)
                                    .label(format!("Copy release {}", release.version))
                                    .help("Copy this release")
                                    .disabled(release.markdown.trim().is_empty())
                                    .on_press_tx({
                                        let markdown = format!(
                                            "## {}\n\n{}",
                                            release.version, release.markdown
                                        );
                                        move |s: &mut State, tx| {
                                            tx.copy(markdown.clone());
                                            s.status = "Release copied".into();
                                        }
                                    })
                                    .build(k)
                                    .size(26., 26.)
                                    .key(format!("copy-release-{i}")),
                            ),
                        ),
                )
                .child(
                    Markdown::new(body)
                        .style(theme::changelog_markdown(k))
                        .selectable(true)
                        .scroll_wide(true)
                        .tasks_read_only()
                        .on_link_tx({
                            let base = release.url.clone();
                            move |s: &mut State, link: &str, tx: &mut Tx| {
                                if let Ok(url) =
                                    url::Url::parse(&base).and_then(|base| base.join(link))
                                    && url.username().is_empty()
                                    && url.password().is_none()
                                {
                                    open_link(s, url.as_str(), tx);
                                }
                            }
                        })
                        .build(k)
                        .key(format!("notes-{}", release.version)),
                );
            document = document.child(
                row()
                    .w_full()
                    .items_stretch()
                    .key(format!("timeline-item-{i}"))
                    .child(rail)
                    .child(notes),
            );
        }
        if p.releases.len() > query.shown {
            document = document.child(
                Button::new(format!(
                    "Show more ({} / {})",
                    query.shown,
                    p.releases.len()
                ))
                .on_press(|s: &mut State| s.show_more_notes())
                .build(k)
                .key("more-notes"),
            );
        }
    }
    if !query.range
        && query.shown >= p.releases.len()
        && query.next.as_ref().is_some_and(|next| {
            s.live
                .catalog
                .packages
                .iter()
                .find(|profile| profile.id == p.id)
                .and_then(|profile| profile.streams.get(query.component))
                .is_some_and(|stream| stream.id == next.stream)
        })
    {
        document = document.child(
            Button::new("Load older")
                .disabled(s.live.active.as_deref() == Some(&p.id))
                .on_press(|s: &mut State| s.fetch_history(true))
                .build(k)
                .key("load-older"),
        );
    }
    reader_frame(
        s, p, k, cx, layers, tabs, document,
        wide_index.then(|| crate::release_index::rail(s, k, active)),
    )
}

fn reader_frame(
    s: &State,
    p: &Package,
    k: &Kit,
    cx: &Cx<State>,
    layers: &mut Vec<El>,
    tabs: El,
    document: El,
    release_rail: Option<El>,
) -> El {
    let notes_position = s.reader_controls.notes_position.clone();
    let notes_owner = p.id.clone();
    let reduced = cx.is_reduced_motion();
    let viewport = col()
        .key("release-notes")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .on_scroll_offset(move |at| notes_position.set(at))
        .on_wheel(move |s: &mut State, event, tx| {
            s.reader_controls
                .scroll_wheel(&notes_owner, event, tx, reduced)
        })
        .child(document.pb(crate::app_chrome::DOCK_CLEARANCE));
    let observation_picker = observation_select(s, k, layers);
    col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .fill(k.c.bg_base)
        .child(reader_header(s, p, k, layers))
        .children(observation_picker.map(|picker| {
            row()
                .w_full()
                .px(24.0)
                .pb(8.0)
                .child(picker)
                .key("observation-picker")
        }))
        .child(engine_controls(s, k, layers))
        .child(tabs)
        .child(
            row()
                .key("changelog-body")
                .w_full()
                .grow(1.)
                .min_h(0.)
                .items_stretch()
                .child(
                    scroll_bar(k, "notes-area", "release-notes", viewport)
                        .grow(1.)
                        .min_w(0.)
                        .min_h(0.),
                )
                .children(release_rail),
        )
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let k = theme::kit(s.dark);
    let (shell_kit, shell_background) = theme::shell(&k, s.dark, s.backdrop.active);
    cx.help_look(theme::tooltip(s.dark, &k));
    cx.caption_color(k.c.text_base);
    let chrome = cx.chrome_insets();
    let watch_workspace = !s.inventory.open && s.background_watch.in_workspace();
    let sidebar_open = !watch_workspace
        && if s.inventory.open {
            s.inventory.workspace.pane.is_open()
        } else {
            s.pane.is_open()
        };
    let watch_enabled = s.background_watch.enabled();
    let compact_at = if watch_enabled { 930. } else { 880. };
    let compact_header = cx
        .layout_with("toolbar", move |r| r[2] < compact_at)
        .unwrap_or(true);
    let workspaces: &[&str] = if watch_enabled {
        &["Notes", "Inventory", "Watch"]
    } else {
        &["Notes", "Inventory"]
    };
    let active_workspace = if watch_workspace {
        2
    } else {
        usize::from(s.inventory.open)
    };
    let mut layers = Vec::new();
    let controls = row()
        .gap(8.)
        .center_y()
        .child(crate::app_chrome::header(s, &shell_kit, compact_header))
        .children((chrome.trailing > 0.).then(|| {
            el().key("caption-clearance")
                .w(chrome.trailing)
                .h(1.)
                .a11y_hidden()
        }));
    let context = row()
        .gap(12.0)
        .center_y()
        .children((!sidebar_open).then(|| crate::app_chrome::brand(&shell_kit).key("toolbar-brand")))
        .child(
            SegmentedControl::new(workspaces, Some(active_workspace))
                .width(Some(if watch_enabled { 234.0 } else { 190.0 }))
                .label("Workspace view")
                .on_pick(|s: &mut State, i, tx| {
                    s.inventory.workspace.context = None;
                    if i == 2 {
                        s.open_watch_workspace();
                    } else {
                        s.background_watch.leave_workspace();
                        s.inventory.open = i == 1;
                        s.mark_current_notes_read();
                    }
                    tx.instant();
                })
                .build(&shell_kit)
                .key("workspace-view"),
        );
    let (reader, inventory_counts) = if s.inventory.open {
        let (view, counts) = crate::inventory_ui::view(s, &k, &mut layers, cx);
        (view.key("inventory-page"), Some(counts))
    } else if watch_workspace {
        (crate::background_watch_ui::workspace(s, &k, cx), None)
    } else {
        let content = reader(s, &k, &mut layers, cx).key("notes-page");
        let reader = if s.section == LibrarySection::Packages {
            col()
                .w_full()
                .h_full()
                .min_h(0.0)
                .child(crate::watchlist_ui::bar(s, &k, &mut layers))
                .child(content.grow(1.0).min_h(0.0))
        } else {
            content
        };
        (reader, None)
    };
    let panes = if s.inventory.open {
        SplitView::new(reader).sidebar(
            &s.inventory.workspace.pane,
            |s: &mut State| &mut s.inventory.workspace.pane,
            crate::inventory_sources::view(s, &shell_kit),
        )
    } else if watch_workspace {
        SplitView::new(reader)
    } else {
        SplitView::new(reader).sidebar(
            &s.pane,
            |s: &mut State| &mut s.pane,
            inventory(s, &shell_kit, cx.is_reduced_motion(), &mut layers),
        )
    };
    let panes = panes
        .toolbar(Toolbar::new().leading(context).trailing(controls))
        .titlebar_inset(chrome.leading)
        .sidebar_padding(0.0, 0.0)
        .frame_inset(0.0)
        .build(&shell_kit)
        .w_full()
        .grow(1.0)
        .min_h(0.0);
    crate::sidebar_filters::view(s, &k, &mut layers);
    crate::description_preview::view(s, &k, &mut layers);
    crate::icon_picker::view(s, &k, &mut layers);
    crate::description_ui::view(s, &k, &mut layers);
    crate::sidebar_actions::view(s, &k, &mut layers);
    crate::ai_menu::view(s, &k, &mut layers);
    crate::command::view(s, &k, &mut layers);
    crate::background_watch_ui::view(s, &k, &mut layers);
    col()
        .key("app")
        .w_full()
        .h_full()
        .fill(shell_background)
        .child(panes)
        .child(crate::app_chrome::footer(s, &k, inventory_counts))
        .children(sidebar_open.then(|| {
            crate::app_chrome::brand(&shell_kit)
                .key("sidebar-brand")
                .absolute(chrome.leading + 48., 13.)
        }))
        .children(layers)
}

pub fn on_key(s: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    if s.command.open.is_some() {
        return false;
    }
    if event.key == rune::Key::Escape && crate::description_preview::dismiss(s) {
        true
    } else if event.is_command('k') && crate::command::can_open(s) {
        if !tx.key_repeat() {
            crate::command::open(s, tx);
        }
        true
    } else if event.is_command('f') && crate::command::can_open(s) {
        if s.inventory.open {
            s.inventory.workspace.columns_open = false;
            tx.focus("scan-filter", true);
        } else {
            s.pane.set_open(true);
            tx.focus("package-filter", true);
        }
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_versions_preserve_uncertainty_and_distinct_observations() {
        let mut s = State::demo();
        let mut r = PackageRow {
            id: "fixture".into(),
            name: "Fixture".into(),
            package: 0,
            members: vec![0],
        };
        s.catalog.packages[0].installed = None;
        s.catalog.packages[0].target = Some("2.0".into());
        assert_eq!(sidebar_version(&s, &r), None);
        s.catalog.packages[0].installed = Some("1.0".into());
        assert_eq!(sidebar_version(&s, &r).as_deref(), Some("1.0"));
        r.members.push(1);
        s.catalog.packages[1].installed = Some("1.1".into());
        assert_eq!(sidebar_version(&s, &r).as_deref(), Some("2 versions"));
        s.catalog.packages[1].installed = Some("1.0".into());
        assert_eq!(sidebar_version(&s, &r).as_deref(), Some("1.0"));
    }
}
