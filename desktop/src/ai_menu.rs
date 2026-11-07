use crate::{
    ai_summary::{self, Connection, Provider, Request, ReviewPlan, Summary},
    dropdown,
    model::{NoteMode, Package, PackageRow, State},
    sidebar_actions,
};
use releasescout_engine::Report;
use rune::{Color, El, Icon, Latest, TextEdit, TextStyle, Tx, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant, Dialog, Kit, Markdown, TextInput, glyphs};
use std::{collections::VecDeque, sync::Arc};
use zeroize::Zeroize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Intent {
    Latest,
    Last20,
    Installed,
    Breaking,
    Description,
    Name,
}
impl Intent {
    fn icon(self) -> &'static Icon {
        match self {
            Self::Latest => &Icon {
                name: "menu-latest",
                view: 24.,
                body: include_str!("../assets/status/file-text.svg"),
            },
            Self::Last20 => &Icon {
                name: "menu-history",
                view: 24.,
                body: include_str!("../assets/status/history.svg"),
            },
            Self::Installed => &Icon {
                name: "menu-compare",
                view: 24.,
                body: include_str!("../assets/status/git-compare-arrows.svg"),
            },
            Self::Breaking => &Icon {
                name: "menu-breaking",
                view: 24.,
                body: include_str!("../assets/status/triangle-alert.svg"),
            },
            Self::Description => &Icon {
                name: "menu-description",
                view: 24.,
                body: include_str!("../assets/status/pencil-line.svg"),
            },
            Self::Name => &Icon {
                name: "menu-name",
                view: 24.,
                body: include_str!("../assets/status/case-sensitive.svg"),
            },
        }
    }
    pub const ALL: [Self; 6] = [
        Self::Latest,
        Self::Last20,
        Self::Installed,
        Self::Breaking,
        Self::Description,
        Self::Name,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Latest => "Brief latest update",
            Self::Last20 => "Brief last 20 updates",
            Self::Installed => "Summarize since installed",
            Self::Breaking => "Explain breaking changes",
            Self::Description => "Update description",
            Self::Name => "Update name",
        }
    }
}
static COPY: Icon = Icon {
    name: "menu-copy",
    view: 24.,
    body: include_str!("../assets/status/copy.svg"),
};
pub struct Open {
    pub row: Option<String>,
    pub package: String,
    pub key: String,
}
const READER_MENU: &str = "ai-reader-actions";
const READER_TRIGGER: &str = "ai-reader-actions-trigger";
pub struct Preview {
    pub name: String,
    pub intent: Intent,
    pub package: String,
}

struct Brief {
    package: String,
    intent: Intent,
    releases: usize,
    range: Option<String>,
    provider: Provider,
    model: String,
    model_label: String,
    partial: bool,
    connection: Option<Arc<Connection>>,
    pending: VecDeque<Request>,
    total: usize,
    completed: usize,
    summary: Summary,
    error: Option<&'static str>,
}

pub struct Ui {
    pub open: Option<Open>,
    pub preview: Option<Preview>,
    pub work: Latest<Result<Summary, &'static str>>,
    pub metadata: crate::ai_metadata::Ui,
    pub model_file_enabled: bool,
    connection: Option<Arc<Connection>>,
    model_label: Option<String>,
    settings: Option<Settings>,
    brief: Option<Brief>,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            open: None,
            preview: None,
            work: Latest::default(),
            metadata: Default::default(),
            model_file_enabled: false,
            connection: None,
            model_label: None,
            settings: None,
            brief: None,
        }
    }
}

impl Ui {
    pub fn modal_open(&self) -> bool {
        self.preview.is_some() || self.settings.is_some()
    }

    pub fn poll(&mut self) -> bool {
        let metadata_changed = self.metadata.poll();
        let (Some(result), Some(brief)) = (self.work.take(), self.brief.as_mut()) else {
            return metadata_changed;
        };
        match result {
            Ok(summary) => {
                brief.completed += 1;
                brief.summary.merge(summary);
                self.next_batch();
            }
            Err(error) => {
                brief.error = Some(error);
                brief.pending.clear();
                brief.connection = None;
            }
        }
        true
    }

    fn next_batch(&mut self) {
        let Some(brief) = &mut self.brief else { return };
        if let (Some(request), Some(connection)) =
            (brief.pending.pop_front(), brief.connection.clone())
        {
            self.work.request(request, move |request| {
                ai_summary::generate(&connection, request)
            });
        } else {
            brief.connection = None;
        }
    }

    pub fn invalidate(&mut self, package: &str) {
        if self
            .brief
            .as_ref()
            .is_some_and(|brief| brief.package == package)
        {
            self.work.cancel();
            self.brief = None;
        }
    }

    fn forget_key(&mut self) {
        self.connection = None;
        self.model_label = None;
        self.work.cancel();
        self.brief = None;
    }

    fn model_label<'a>(&'a self, connection: &'a Connection) -> &'a str {
        self.model_label.as_deref().unwrap_or(&connection.model)
    }
}

struct Settings {
    provider: Provider,
    model: TextEdit,
    endpoint: TextEdit,
    key: TextEdit,
    query: TextEdit,
    manual: bool,
    models: crate::ai_models::Catalog,
    catalog_error: Option<&'static str>,
    error: Option<&'static str>,
}

impl Drop for Settings {
    fn drop(&mut self) {
        self.key.text.zeroize();
    }
}

fn open_settings(s: &mut State) {
    let current = s.ai.connection.as_deref();
    let provider = current.map_or(Provider::default(), |c| c.provider);
    let (models, catalog_error) = settings_models(s.ai.model_file_enabled);
    let model = current.map_or(models.default_model(provider), |c| &c.model);
    s.ai.settings = Some(Settings {
        provider,
        model: TextEdit::new(model),
        endpoint: TextEdit::new(current.and_then(Connection::custom_url).unwrap_or("")),
        key: TextEdit::new("").secure(),
        query: TextEdit::new(""),
        manual: !models
            .models(provider)
            .iter()
            .any(|entry| entry.id == model),
        models,
        catalog_error,
        error: None,
    });
}

pub fn open_settings_tx(s: &mut State, tx: &mut Tx) {
    open_settings(s);
    let manual = s.ai.settings.as_ref().is_some_and(|draft| draft.manual);
    tx.focus(
        if manual {
            "ai-model-field"
        } else {
            "ai-model-search"
        },
        false,
    );
}

fn choose_provider(s: &mut State, provider: Provider) {
    if let Some(draft) = &mut s.ai.settings
        && draft.provider != provider
    {
        let active = s.ai.connection.as_ref().filter(|c| c.provider == provider);
        draft.provider = provider;
        let model = active.map_or(draft.models.default_model(provider), |c| c.model.as_str());
        draft.manual = !draft
            .models
            .models(provider)
            .iter()
            .any(|entry| entry.id == model);
        draft.model = TextEdit::new(model);
        draft.endpoint = TextEdit::new(active.and_then(|c| c.custom_url()).unwrap_or(""));
        draft.query.clear();
        draft.key.text.zeroize();
        draft.key.clear();
        draft.error = None;
    }
}

fn model_matches(model: &crate::ai_models::Model, query: &str) -> bool {
    let label = model.label.to_lowercase();
    let id = model.id.to_ascii_lowercase();
    query.split_whitespace().all(|word| {
        let word = word.to_lowercase();
        label.contains(&word) || id.contains(&word)
    })
}

fn settings_models(use_file: bool) -> (crate::ai_models::Catalog, Option<&'static str>) {
    if use_file {
        match crate::ai_models::load(&crate::ai_models::path()) {
            Ok(models) => return (models, None),
            Err(_) => {
                return (
                    crate::ai_models::bundled().clone(),
                    Some("Couldn't read ai-models.json. Showing built-in suggestions."),
                );
            }
        }
    }
    (crate::ai_models::bundled().clone(), None)
}

fn close_settings(s: &mut State) {
    s.ai.settings = None;
}

fn save_settings(s: &mut State) {
    let Some(draft) = &mut s.ai.settings else {
        return;
    };
    match Connection::new(
        draft.provider,
        &draft.model.text,
        &draft.endpoint.text,
        &draft.key.text,
        s.ai.connection.as_deref(),
    ) {
        Ok(connection) => {
            s.ai.model_label = draft
                .models
                .models(connection.provider)
                .iter()
                .find(|entry| entry.id == connection.model)
                .map(|entry| entry.label.clone());
            s.ai.connection = Some(Arc::new(connection));
            close_settings(s);
        }
        Err(error) => draft.error = Some(error),
    }
}

const LOAD_NOTES: &str = "Load authored release notes for this entry first.";
const LOAD_RANGE: &str =
    "Load the range from your installed version to the available update first.";
const NO_RANGE: &str = "No installed-to-update version range is available for this entry.";

fn source<'a>(s: &'a State, id: &str, intent: Intent) -> Result<(&'a Report, bool), &'static str> {
    let q = s.live.sessions.get(id).ok_or(LOAD_NOTES)?;
    if intent == Intent::Installed {
        let (from, to) = s.installed_update_range(id).ok_or(NO_RANGE)?;
        if !q
            .report_cache
            .as_ref()
            .is_some_and(|c| c.matches_version_range(from, to, q))
        {
            return Err(LOAD_RANGE);
        }
    }
    let report = q
        .report
        .as_ref()
        .filter(|report| {
            report.status != "source_failed"
                && report
                    .notes
                    .iter()
                    .any(|n| n.authored_notes && !n.body.trim().is_empty())
        })
        .ok_or(LOAD_NOTES)?;
    Ok((report, q.next.is_some()))
}

fn prepare(s: &State, id: &str, intent: Intent) -> Result<ReviewPlan, &'static str> {
    let (report, more_pages) = source(s, id, intent)?;
    let limit = match intent {
        Intent::Latest => Some(1),
        Intent::Last20 => Some(20),
        Intent::Installed | Intent::Breaking => {
            return ReviewPlan::full(report, more_pages, intent == Intent::Breaking);
        }
        _ => return Err("Not available yet."),
    };
    Request::new(report, limit, more_pages, false)
        .map(ReviewPlan::from)
        .ok_or(LOAD_NOTES)
}

fn close_preview(s: &mut State) {
    s.ai.preview = None;
    s.ai.metadata.close();
}

fn open_action(s: &mut State, package: &str, name: &str, intent: Intent, tx: &mut Tx) {
    s.ai.metadata.close();
    s.ai.preview = Some(Preview {
        package: package.into(),
        name: name.into(),
        intent,
    });
    match intent {
        Intent::Name => crate::ai_metadata::open(s, package, crate::ai_metadata::Field::Name),
        Intent::Description => {
            crate::ai_metadata::open(s, package, crate::ai_metadata::Field::Description)
        }
        _ if s.note_cache.enabled
            && s.catalog
                .packages
                .iter()
                .any(|p| p.id == package && crate::sidebar_filters::source(s, p)) =>
        {
            load_for_action(s, tx)
        }
        _ => {}
    }
}

fn load_for_action(s: &mut State, tx: &mut Tx) {
    let Some(preview) = &s.ai.preview else { return };
    let id = preview.package.clone();
    if s.live.active.is_some() || s.note_prefetch.loading(&id) {
        return;
    }
    let range = match prepare(s, &id, preview.intent) {
        Err(LOAD_RANGE) => s
            .installed_update_range(&id)
            .map(|(a, b)| (a.to_owned(), b.to_owned())),
        Err(LOAD_NOTES) => None,
        _ => return,
    };
    s.select_package(&id);
    if !s.selected().is_some_and(|p| p.id == id) {
        return;
    }
    s.inventory.open = false;
    if let Some((from, to)) = range {
        let q = s.engine_query();
        q.range = true;
        q.from = TextEdit::new(from);
        q.to = TextEdit::new(to);
    }
    s.fetch_notes();
    tx.scroll_to_instant("release-notes", 0.0, 0.0);
}

fn start_summary(s: &mut State, tx: &mut Tx) {
    let Some(preview) = &s.ai.preview else { return };
    let intent = preview.intent;
    let id = preview.package.clone();
    let Some(connection) = s.ai.connection.clone().filter(|_| !s.ai.work.is_pending()) else {
        return;
    };
    let Ok(request) = prepare(s, &id, intent) else {
        return;
    };
    let range = if intent == Intent::Installed {
        s.installed_update_range(&id)
            .map(|(from, to)| format!("{from} to {to}"))
    } else {
        None
    };
    if !s.selected().is_some_and(|p| p.id == id) {
        s.select_package(&id);
    }
    if !s.selected().is_some_and(|p| p.id == id) {
        return;
    }
    s.inventory.open = false;
    s.mode = NoteMode::Ai;
    s.reader_controls.release_index_open = false;
    s.reader_controls.reset_scroll();
    let mut summary = Summary::default();
    for batch in &request.batches {
        summary.merge(batch.publisher_findings());
    }
    s.ai.brief = Some(Brief {
        package: id,
        intent,
        releases: request.releases,
        range,
        provider: connection.provider,
        model: connection.model.clone(),
        model_label: s.ai.model_label(&connection).to_owned(),
        partial: request.partial,
        total: request.batches.len(),
        completed: 0,
        connection: Some(connection),
        pending: request.batches.into(),
        summary,
        error: None,
    });
    close_preview(s);
    s.ai.next_batch();
    tx.scroll_to_instant("release-notes", 0.0, 0.0);
}

pub fn key(r: &PackageRow) -> String {
    format!("sidebar-ai-{}", crate::activity::hash(&r.id))
}
pub fn button(s: &State, r: &PackageRow, k: &Kit) -> El {
    let menu = key(r);
    let trigger = format!("{menu}-trigger");
    let package = s.catalog.packages[r.package].id.clone();
    let row = r.id.clone();
    sidebar_actions::action(
        trigger.clone(),
        &glyphs::SPARKLE,
        &format!("AI actions for {}", r.name),
        k,
    )
    .expanded(s.ai.open.as_ref().is_some_and(|o| o.key == menu))
    .on_press_tx(move |s: &mut State, tx| {
        tx.focus(&trigger, false);
        s.ai.open = Some(Open {
            row: Some(row.clone()),
            package: package.clone(),
            key: menu.clone(),
        });
    })
}

fn close(s: &mut State) {
    s.ai.open = None;
}

pub fn scene(s: &mut State) {
    crate::sidebar_actions::scene(s, crate::sidebar_actions::Mode::Details);
    s.sidebar_actions.panel = None;
    let r = s
        .rows
        .iter()
        .find(|r| Some(r.id.as_str()) == s.table.cursor())
        .unwrap();
    s.ai.open = Some(Open {
        row: Some(r.id.clone()),
        package: s.catalog.packages[r.package].id.clone(),
        key: key(r),
    });
}

pub fn settings_scene(s: &mut State, provider: Provider) {
    open_settings(s);
    s.ai.model_file_enabled = true;
    choose_provider(s, provider);
}

pub fn reader_scene(s: &mut State) {
    use releasescout_engine::{Basis, Note};
    use serde_json::json;
    let id = s.selected().unwrap().id.clone();
    let citations: Vec<_> = ["v3.4.0", "v3.3.0"]
        .into_iter()
        .map(|label| ai_summary::Citation {
            label: label.into(),
            url: Some(format!("https://example.org/releases/{label}")),
        })
        .collect();
    s.engine_query().report = Some(Report {
        package: id.clone(),
        stream: "main".into(),
        scope: "history".into(),
        basis: Basis::Version,
        status: "available".into(),
        complete: true,
        notes: citations
            .iter()
            .map(|citation| Note {
                title: citation.label.clone(),
                version: Some(citation.label.clone()),
                date: None,
                url: citation.url.clone().unwrap(),
                body: "Sample release notes.".into(),
                authored_notes: true,
                preview: false,
            })
            .collect(),
        warnings: Vec::new(),
        evidence: Vec::new(),
        attempts: Vec::new(),
    });
    let point = |release, text| json!({"release": release, "text": text, "evidence": "Sample release notes."});
    let body = serde_json::from_value(json!({
        "breaking_changes": [
            point(0, "The old `--legacy-export` flag was removed. Use `--format` in existing scripts."),
            point(1, "Saved layouts made before 2.0 need to be re-exported."),
        ],
        "highlights": [
            point(0, "Large exports now run in the background and can be cancelled."),
            point(0, "PDF exports honor both the page size and the crop area."),
            point(1, "Fixed an issue that could leave drafts unsaved when closing multiple tabs."),
        ],
    })).unwrap();
    s.mode = NoteMode::Ai;
    s.ai.brief = Some(Brief {
        package: id,
        intent: Intent::Last20,
        releases: 2,
        range: None,
        provider: Provider::OpenAi,
        model: "gpt-6-luna".into(),
        model_label: "GPT-6 Luna".into(),
        partial: true,
        connection: None,
        pending: VecDeque::new(),
        total: 1,
        completed: 1,
        summary: Summary {
            body,
            citations,
            discarded: 0,
        },
        error: None,
    });
}

pub fn preview_scene(s: &mut State) {
    reader_scene(s);
    let p = s.selected().unwrap();
    s.ai.preview = Some(Preview {
        package: p.id.clone(),
        name: s.display_name(p).into(),
        intent: Intent::Latest,
    });
    s.ai.model_label = Some("GPT-6 Luna".into());
    s.ai.connection = Some(Arc::new(
        Connection::new(Provider::OpenAi, "gpt-6-luna", "", "fixture-key", None).unwrap(),
    ));
}

pub fn view(s: &State, k: &Kit, layers: &mut Vec<El>) {
    if let Some(open) = &s.ai.open
        && match &open.row {
            Some(row) => crate::sidebar_groups::shown(s)
                .iter()
                .any(|&i| s.rows[i].id == *row),
            None => {
                !s.inventory.open
                    && s.mode == NoteMode::Ai
                    && s.selected().is_some_and(|p| p.id == open.package)
            }
        }
        && let Some(p) = s.catalog.packages.iter().find(|p| p.id == open.package)
    {
        let name = s.display_name(p);
        let header = row()
            .key("ai-menu-title")
            .px(8.)
            .py(6.)
            .gap(8.)
            .center_y()
            .child(crate::ui::mark(p, k, 16.).key("ai-menu-title-icon"))
            .child(
                text(name)
                    .key("ai-menu-title-label")
                    .font(TextStyle::new(14., 500.).lh(20.))
                    .color(k.c.text_muted)
                    .min_w(0.)
                    .grow(1.)
                    .truncate()
                    .help_when_truncated(name),
            );
        let mut items = col().items_stretch();
        for (index, intent) in Intent::ALL.into_iter().enumerate() {
            if intent == Intent::Description {
                items = items.child(dropdown::separator("ai-menu-edit-separator", k));
            }
            let name = s.display_name(p).to_owned();
            let package = p.id.clone();
            items = items.child(
                dropdown::item(
                    format!("{}-item{index}", open.key),
                    intent.label(),
                    intent.icon(),
                    k,
                )
                .on_press_tx(move |s: &mut State, tx| {
                    open_action(s, &package, &name, intent, tx);
                    s.ai.open = None;
                    s.sidebar_actions.panel = None;
                }),
            );
        }
        let id = p.id.clone();
        items = items
            .child(dropdown::separator("ai-menu-copy-separator", k))
            .child(
                dropdown::item(format!("{}-copy", open.key), "Copy AI context", &COPY, k).on_press(
                    move |s: &mut State| {
                        s.ai.open = None;
                        s.prepare_brief(&id);
                    },
                ),
            );
        dropdown::with_header(&open.key, 256., header, items, k, layers, close);
    }
    if let Some(settings) = &s.ai.settings {
        settings_view(s, settings, k, layers);
    } else if let Some(preview) = &s.ai.preview {
        let content = if matches!(preview.intent, Intent::Description | Intent::Name) {
            let body = col().w_full().padding(16.).gap(12.).child(
                text(&preview.name)
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base)
                    .wrap_anywhere(),
            );
            let buttons = row()
                .w_full()
                .gap(8.)
                .justify_end()
                .child(cancel_button("Close", k));
            let (body, buttons) =
                crate::ai_metadata::view(s, k, s.ai.connection.as_ref(), body, buttons);
            body.child(buttons)
        } else {
            review_preview(s, preview, k)
        };
        Dialog::new("ai-action-preview", preview.intent.label())
            .open(true)
            .fit()
            .width(440.)
            .on_close(close_preview)
            .child(content)
            .build(k, layers);
    }
}

fn cancel_button(label: &str, k: &Kit) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .on_press(close_preview)
        .build(k)
        .key("ai-action-close")
}

fn release_count(count: usize) -> String {
    format!(
        "{count} {}",
        if count == 1 { "release" } else { "releases" }
    )
}

fn preview_row(label: &'static str, value: El, k: &Kit) -> El {
    row()
        .w_full()
        .items_start()
        .gap(10.)
        .child(detail(label, k).w(68.))
        .child(value.grow(1.).min_w(0.).shrink())
}

fn review_preview(s: &State, preview: &Preview, k: &Kit) -> El {
    let request = prepare(s, &preview.package, preview.intent);
    let connection = s.ai.connection.as_deref();
    let mut identity = row().center_y().gap(9.).w_full();
    if let Some(package) = s.catalog.packages.iter().find(|p| p.id == preview.package) {
        identity = identity.child(crate::ui::mark(package, k, 28.).a11y_hidden());
    }
    identity = identity.child(
        text(&preview.name)
            .font(TextStyle::new(16., 600.).lh(22.))
            .color(k.c.text_base)
            .min_w(0.)
            .shrink()
            .wrap_anywhere(),
    );
    if let Ok(request) = &request {
        identity = identity.child(el().grow(1.)).child(
            detail(release_count(request.releases), k)
                .px(7.)
                .py(3.)
                .radius(4.)
                .fill(k.c.layer_02)
                .key("ai-preview-release-count"),
        );
    }

    let mut body = col().w_full().px(16.).pb(12.).gap(10.).child(identity);
    let mut footer = row().w_full().center_y().gap(6.).px(12.).py(8.);
    let settings_button = |variant| {
        Button::new("AI settings")
            .variant(variant)
            .on_press_tx(open_settings_tx)
            .build(k)
            .key("ai-configure")
    };
    match request {
        Ok(request) => {
            let (provider, model) = connection
                .map_or((Provider::OpenAi, Provider::OpenAi.default_model()), |c| {
                    (c.provider, c.model.as_str())
                });
            let tokens = request.estimate_input_tokens(provider, model);
            let batches = request.batches.len();
            let mut usage = row().center_y().gap(8.).child(
                text(format!("~{tokens} tokens"))
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base)
                    .wrap_anywhere()
                    .key("ai-input-estimate"),
            );
            if batches > 1 {
                usage = usage.child(detail(format!("{batches} requests"), k).shrink());
            }
            let mut rows = col()
                .w_full()
                .gap(8.)
                .child(preview_row("Est. input", usage, k));
            if let Some(c) = connection {
                rows = rows
                    .child(preview_row(
                        "Model",
                        model_identity(c.provider, &c.model, s.ai.model_label(c), false, k),
                        k,
                    ))
                    .child(preview_row(
                        "Sends to",
                        detail(c.custom_url().unwrap_or_else(|| c.destination()), k)
                            .font(k.ty.text_small.mono())
                            .color(k.c.text_base)
                            .key("ai-preview-destination"),
                        k,
                    ));
                footer = footer
                    .child(settings_button(ButtonVariant::Ghost))
                    .child(el().grow(1.))
                    .child(cancel_button("Cancel", k))
                    .child(
                        Button::new("Send & summarize")
                            .variant(ButtonVariant::Contrast)
                            .disabled(s.ai.work.is_pending())
                            .on_press_tx(start_summary)
                            .build(k)
                            .key("ai-generate"),
                    );
            } else {
                rows = rows.child(preview_row("Model", detail("Not configured", k), k));
                footer = footer
                    .child(el().grow(1.))
                    .child(cancel_button("Cancel", k))
                    .child(settings_button(ButtonVariant::Contrast));
            }
            body = body.child(reader_rule(k)).child(rows);
            if request.partial {
                body = body.child(coverage_notice(k));
            }
        }
        Err(error) => {
            let loading = s.live.active.is_some() || s.note_prefetch.loading(&preview.package);
            let source_error = s
                .live
                .sessions
                .get(&preview.package)
                .and_then(|q| q.error.as_deref());
            body = body.child(detail(
                if loading {
                    "Loading release notes..."
                } else {
                    source_error.unwrap_or(error)
                },
                k,
            ));
            footer = footer
                .child(el().grow(1.))
                .child(cancel_button("Cancel", k));
            if matches!(error, LOAD_RANGE | LOAD_NOTES) {
                footer = footer.child(
                    Button::new(if error == LOAD_RANGE {
                        "Load update range"
                    } else {
                        "Load notes"
                    })
                    .disabled(loading)
                    .on_press_tx(load_for_action)
                    .build(k)
                    .key("ai-load-range"),
                );
            }
        }
    }
    col()
        .w_full()
        .child(body)
        .child(reader_rule(k))
        .child(footer)
}

fn provider_icon(provider: Provider, k: &Kit) -> (&'static Icon, Color) {
    static OPENAI: Icon = Icon {
        name: "ai-provider-openai",
        view: 24.,
        body: include_str!("../assets/ai-providers/openai.svg"),
    };
    static CLAUDE: Icon = Icon {
        name: "ai-provider-claude",
        view: 24.,
        body: include_str!("../assets/ai-providers/claude.svg"),
    };
    static GEMINI: Icon = Icon {
        name: "ai-provider-gemini",
        view: 24.,
        body: include_str!("../assets/ai-providers/gemini.svg"),
    };
    static CUSTOM: Icon = Icon {
        name: "ai-provider-custom",
        view: 24.,
        body: include_str!("../assets/symbols/terminal.svg"),
    };
    match provider {
        Provider::OpenAi => (&OPENAI, k.c.icon_base),
        Provider::Claude => (
            &CLAUDE,
            rune::color::hex(if k.dark { "#e89173" } else { "#ad4e32" }).unwrap(),
        ),
        Provider::Gemini => (
            &GEMINI,
            rune::color::hex(if k.dark { "#82b2ff" } else { "#286bcd" }).unwrap(),
        ),
        Provider::Custom => (&CUSTOM, k.c.icon_muted),
    }
}

fn model_identity(provider: Provider, id: &str, label: &str, badge: bool, k: &Kit) -> El {
    let (mark, tint) = provider_icon(provider, k);
    let identity = row()
        .center_y()
        .gap(6.)
        .min_w(0.)
        .shrink()
        .help(format!("{}: {id}", provider.label()))
        .child(icon(mark, 14.).color(tint).a11y_hidden())
        .child(
            text(label)
                .font(k.ty.ui)
                .color(k.c.text_base)
                .truncate()
                .min_w(0.),
        );
    if badge {
        identity.px(7.).h(24.).radius(4.).fill(k.c.layer_02)
    } else {
        identity
    }
}

fn settings_view(s: &State, settings: &Settings, k: &Kit, layers: &mut Vec<El>) {
    const MODEL_ROW_HEIGHT: f32 = 33.;
    const MODEL_ROW_GAP: f32 = 1.;
    let divider = crate::theme::separator(k);
    let mut rail = col()
        .w(50.)
        .min_w(50.)
        .py(5.)
        .gap(2.)
        .items_center()
        .key("ai-provider-rail");
    for provider in Provider::ALL {
        let (mark, tint) = provider_icon(provider, k);
        let selected = provider == settings.provider;
        rail = rail.child(
            sidebar_actions::action_colored(
                format!("ai-provider-option-{}", provider.id()),
                mark,
                provider.label(),
                tint,
                k,
            )
            .size(34., 32.)
            .focus_selected(selected)
            .fill(if selected {
                k.c.layer_02
            } else {
                Color::TRANSPARENT
            })
            .on_press_tx(move |s: &mut State, tx| {
                choose_provider(s, provider);
                tx.scroll_to_instant("ai-model-scroll", 0., 0.);
            }),
        );
    }

    let choices = settings.models.models(settings.provider);
    let input = if settings.manual {
        TextInput::new(&settings.model)
            .width(None)
            .label("Model ID")
            .placeholder("Enter model ID")
            .on_edit(|s: &mut State| &mut s.ai.settings.as_mut().unwrap().model)
            .build(k)
            .key("ai-model-field")
    } else {
        TextInput::new(&settings.query)
            .plain()
            .height(36.)
            .leading_icon(&glyphs::SEARCH)
            .leading_icon_size(14.)
            .width(None)
            .label("Search models")
            .placeholder("Search models...")
            .on_edit(|s: &mut State| &mut s.ai.settings.as_mut().unwrap().query)
            .on_change(|_: &mut State, tx| tx.scroll_to_instant("ai-model-scroll", 0., 0.))
            .on_key(|s: &mut State, event, tx| {
                if event.key == rune::Key::Down
                    && let Some(draft) = &s.ai.settings
                    && let Some(index) = draft
                        .models
                        .models(draft.provider)
                        .iter()
                        .position(|model| model_matches(model, &draft.query.text))
                {
                    tx.focus(&format!("ai-model-option-{index}"), false);
                    return true;
                }
                false
            })
            .on_submit(|s: &mut State, _| {
                if let Some(draft) = &mut s.ai.settings
                    && let Some(model) = draft
                        .models
                        .models(draft.provider)
                        .iter()
                        .find(|model| model_matches(model, &draft.query.text))
                {
                    draft.model = TextEdit::new(&model.id);
                    draft.error = None;
                }
            })
            .build(k)
            .key("ai-model-search")
    };
    let mut search = row()
        .w_full()
        .center_y()
        .gap(4.)
        .child(input.grow(1.).min_w(0.));
    if !choices.is_empty() {
        let (glyph, label) = if settings.manual {
            (&glyphs::SEARCH, "Browse suggested models")
        } else {
            (Intent::Description.icon(), "Enter model ID manually")
        };
        search = search.child(
            sidebar_actions::action("ai-manual-model", glyph, label, k).on_press_tx(
                |s: &mut State, tx| {
                    if let Some(draft) = &mut s.ai.settings {
                        draft.manual = !draft.manual;
                        draft.query.clear();
                        tx.focus(
                            if draft.manual {
                                "ai-model-field"
                            } else {
                                "ai-model-search"
                            },
                            false,
                        );
                    }
                },
            ),
        );
    }
    #[cfg(windows)]
    if s.ai.model_file_enabled {
        static FILE: Icon = Icon {
            name: "ai-edit-model-catalog",
            view: 24.,
            body: include_str!("../assets/status/file-text.svg"),
        };
        search = search.child(
            sidebar_actions::action("ai-edit-model-list", &FILE, "Edit model list in Notepad", k)
                .on_press(|s: &mut State| {
                    if let Some(draft) = &mut s.ai.settings {
                        draft.catalog_error = crate::ai_models::edit_file()
                            .err()
                            .map(|_| "Couldn't open the model list in Notepad.");
                    }
                }),
        );
    }

    let mut panel = col()
        .grow(1.)
        .min_w(0.)
        .px(10.)
        .pb(5.)
        .gap(4.)
        .child(search);
    if !settings.manual {
        let (mark, tint) = provider_icon(settings.provider, k);
        let mut visible = 0;
        let mut rows = col()
            .w_full()
            .gap(MODEL_ROW_GAP)
            .role("listbox")
            .label("Models")
            .focus_group();
        for (index, model) in choices.iter().enumerate() {
            if !model_matches(model, &settings.query.text) {
                continue;
            }
            visible += 1;
            let id = model.id.clone();
            let selected = id == settings.model.text;
            rows = rows.child(
                row()
                    .w_full()
                    .h(MODEL_ROW_HEIGHT)
                    .px(6.)
                    .center_y()
                    .gap(7.)
                    .radius(4.)
                    .key(format!("ai-model-option-{index}"))
                    .role("option")
                    .label(format!("{} ({id})", model.label))
                    .help(&id)
                    .focusable(true)
                    .focus_selected(selected)
                    .focus_highlight(k.c.hover)
                    .hover(k.c.hover)
                    .on_press(move |s: &mut State| {
                        if let Some(draft) = &mut s.ai.settings {
                            draft.model = TextEdit::new(&id);
                            draft.error = None;
                        }
                    })
                    .child(icon(mark, 14.).color(tint).a11y_hidden())
                    .child(
                        text(&model.label)
                            .font(k.ty.ui_medium)
                            .color(k.c.text_base)
                            .grow(1.)
                            .min_w(0.)
                            .truncate()
                            .a11y_hidden(),
                    )
                    .children(selected.then(|| {
                        icon(k.glyph.check_small, 14.)
                            .color(k.c.icon_base)
                            .a11y_hidden()
                    })),
            );
        }
        panel = panel.child(el().w_full().h(1.).fill(divider));
        panel = if visible == 0 {
            panel.child(
                detail("No matching models", k)
                    .px(6.)
                    .py(8.)
                    .key("ai-model-no-results"),
            )
        } else {
            panel.child(
                rune_kit::scroll_bar(k, "ai-model-scroll", "models", rows)
                    .w_full()
                    .h((visible.min(4) as f32) * (MODEL_ROW_HEIGHT + MODEL_ROW_GAP))
                    .min_h(0.),
            )
        };
    }
    if settings.provider == Provider::Custom {
        panel = panel.child(
            TextInput::new(&settings.endpoint)
                .width(None)
                .label("Chat Completions base URL")
                .placeholder("https://provider.example/v1")
                .on_edit(|s: &mut State| &mut s.ai.settings.as_mut().unwrap().endpoint)
                .build(k)
                .key("ai-endpoint-field"),
        );
    }
    if let Some(error) = settings.catalog_error {
        panel = panel.child(detail(error, k).key("ai-model-list-error"));
    }

    let keep_key = s.ai.connection.as_ref().is_some_and(|c| {
        c.provider == settings.provider && c.custom_url().unwrap_or("") == settings.endpoint.text
    });
    let (key_icon, _) = provider_icon(settings.provider, k);
    let key_provider = match settings.provider {
        Provider::OpenAi => "OpenAI",
        Provider::Claude => "Anthropic",
        Provider::Gemini => "Google AI",
        Provider::Custom => "Custom endpoint",
    };
    let mut footer = row().w_full().gap(8.).center_y().px(12.).py(9.).child(
        TextInput::new(&settings.key)
            .width(None)
            .leading_icon(key_icon)
            .leading_icon_size(16.)
            .label(format!("{key_provider} API key"))
            .placeholder(if keep_key {
                format!("Keep current {key_provider} key")
            } else {
                format!("{key_provider} API key (session only)")
            })
            .on_edit(|s: &mut State| &mut s.ai.settings.as_mut().unwrap().key)
            .on_submit(|s: &mut State, _| save_settings(s))
            .build(k)
            .key("ai-key-field")
            .grow(1.)
            .min_w(0.),
    );
    if s.ai.connection.is_some() {
        footer = footer.child(
            sidebar_actions::action("ai-forget-key", &glyphs::CLOSE, "Disconnect AI session", k)
                .on_press(|s: &mut State| {
                    s.ai.forget_key();
                    close_settings(s);
                }),
        );
    }
    footer = footer.child(
        Button::new("Use for session")
            .variant(ButtonVariant::Contrast)
            .on_press(save_settings)
            .build(k)
            .key("ai-save-key"),
    );
    let mut dialog = Dialog::new("ai-settings", "AI settings")
        .open(true)
        .fit()
        .width(480.)
        .on_close(close_settings)
        .child(
            row()
                .w_full()
                .items_stretch()
                .child(rail)
                .child(el().w(1.).fill(divider))
                .child(panel),
        )
        .child(el().w_full().h(1.).fill(divider))
        .child(footer);
    if let Some(error) = settings.error {
        dialog = dialog.child(
            detail(error, k)
                .color(k.c.danger.fg)
                .px(10.)
                .pb(8.)
                .key("ai-settings-error"),
        );
    }
    dialog.build(k, layers);
}

fn detail(value: impl Into<String>, k: &Kit) -> El {
    text(value.into())
        .font(k.ty.text_small)
        .color(k.c.text_muted)
        .wrap_anywhere()
}

pub fn reader(s: &State, package: &Package, k: &Kit) -> El {
    let id = package.id.clone();
    let open =
        s.ai.open
            .as_ref()
            .is_some_and(|o| o.row.is_none() && o.package == id);
    let actions = Button::new("AI actions")
        .icon(&glyphs::CHEVRON_DOWN)
        .variant(ButtonVariant::Outline)
        .expanded(open)
        .on_press_tx(move |s: &mut State, tx| {
            tx.focus(READER_TRIGGER, false);
            s.ai.open = Some(Open {
                row: None,
                package: id.clone(),
                key: READER_MENU.into(),
            });
        })
        .build(k)
        .key(READER_TRIGGER);
    let settings = row()
        .key("ai-reader-settings")
        .role("button")
        .label("AI settings")
        .focusable(true)
        .focus_ring(1., 0., k.c.border_focus)
        .hover(k.c.hover)
        .center_y()
        .min_w(0.)
        .max_w(196.)
        .shrink()
        .px(7.)
        .h(26.)
        .gap(6.)
        .radius(4.)
        .on_press_tx(open_settings_tx)
        .child(
            icon(&glyphs::SPARKLE, 14.)
                .color(k.c.icon_muted)
                .a11y_hidden(),
        )
        .child(
            text("AI settings")
                .font(k.ty.ui)
                .color(k.c.text_muted)
                .min_w(0.)
                .truncate()
                .a11y_hidden(),
        );
    let content = summary_panel(s, package, k).unwrap_or_else(|| {
        row()
            .gap(8.)
            .center_y()
            .py(6.)
            .child(icon(&glyphs::SPARKLE, 16.).color(k.c.icon_muted))
            .child(detail("No AI review yet", k))
    });
    col()
        .key("ai-reader")
        .w_full()
        .gap(12.)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .gap(8.)
                .child(actions)
                .child(settings),
        )
        .child(reader_rule(k))
        .child(content)
}

fn reader_rule(k: &Kit) -> El {
    el().w_full()
        .h(1.)
        .fill(crate::theme::separator(k))
        .a11y_hidden()
}

fn reader_notice(message: impl Into<String>, mark: &'static Icon, tint: Color, k: &Kit) -> El {
    row()
        .w_full()
        .gap(7.)
        .items_start()
        .px(8.)
        .py(5.)
        .radius(4.)
        .fill(k.c.layer_02)
        .child(icon(mark, 13.).color(tint).mt(1.).a11y_hidden())
        .child(detail(message, k).grow(1.).min_w(0.))
}

fn coverage_notice(k: &Kit) -> El {
    reader_notice(
        "Some release notes are missing or shortened.",
        Intent::Breaking.icon(),
        k.c.text_muted,
        k,
    )
    .key("ai-coverage-notice")
}

impl Brief {
    fn scope_label(&self) -> String {
        let count = release_count(self.releases);
        let status = if self.completed == self.total {
            "reviewed"
        } else {
            "in scope"
        };
        match &self.range {
            Some(range) => format!("{count} {status}  ·  {range}"),
            None => format!("{count} {status}"),
        }
    }
}

fn summary_panel(s: &State, package: &Package, k: &Kit) -> Option<El> {
    let brief =
        s.ai.brief
            .as_ref()
            .filter(|brief| brief.package == package.id)?;
    s.live.sessions.get(&package.id)?.report.as_ref()?;
    let warn = Intent::Breaking.icon();
    let title = col()
        .gap(2.)
        .grow(1.)
        .min_w(0.)
        .child(
            text(brief.intent.label())
                .font(TextStyle::new(16., 600.).lh(22.))
                .color(k.c.text_base),
        )
        .child(
            row()
                .center_y()
                .gap(5.)
                .child(
                    icon(&glyphs::FILE_TEXT, 13.)
                        .color(k.c.icon_muted)
                        .a11y_hidden(),
                )
                .child(
                    detail(brief.scope_label(), k)
                        .min_w(0.)
                        .shrink()
                        .key("ai-summary-scope"),
                ),
        );
    let dismiss = sidebar_actions::action(
        "ai-dismiss-summary",
        &glyphs::CLOSE,
        if brief.connection.is_some() {
            "Stop review"
        } else {
            "Dismiss summary"
        },
        k,
    )
    .on_press(|s: &mut State| {
        s.ai.work.cancel();
        if let Some(brief) = &mut s.ai.brief
            && brief.connection.take().is_some()
        {
            brief.pending.clear();
            brief.error =
                Some("Review stopped. A request already sent may still complete at the provider.");
        } else {
            s.ai.brief = None;
        }
    });
    let mut panel = col().w_full().gap(10.).key("ai-summary").child(
        row()
            .w_full()
            .gap(8.)
            .items_start()
            .child(title)
            .child(
                model_identity(brief.provider, &brief.model, &brief.model_label, true, k)
                    .max_w(164.)
                    .key("ai-summary-model"),
            )
            .child(dismiss),
    );
    if brief.partial {
        panel = panel.child(coverage_notice(k));
    }
    if brief.connection.is_some() {
        panel = panel.child(
            reader_notice(
                format!(
                    "Reviewing request {} of {}...",
                    brief.completed + 1,
                    brief.total
                ),
                &glyphs::SPARKLE,
                k.c.icon_muted,
                k,
            )
            .key("ai-batch-progress"),
        );
    } else if let Some(error) = brief.error {
        panel = panel.child(
            reader_notice(
                format!(
                    "Reviewed {} of {} batches. {error}",
                    brief.completed, brief.total
                ),
                warn,
                k.c.danger.fg,
                k,
            )
            .key("ai-batch-error"),
        );
    } else if brief.total > 1 {
        panel = panel.child(detail(
            format!("Reviewed all {} planned batches.", brief.total),
            k,
        ));
    }
    let summary = &brief.summary;
    if summary.discarded > 0 {
        panel = panel.child(reader_notice(
            "Some AI findings were omitted because their quotes did not match the cited releases.",
            warn,
            k.c.text_muted,
            k,
        ));
    }
    if brief.completed != 0 || !summary.body.breaking_changes.is_empty() {
        panel = panel.child(summary_section(
            summary,
            true,
            brief.connection.is_some(),
            k,
        ));
        if brief.intent != Intent::Breaking && brief.completed != 0 {
            panel = panel.child(summary_section(
                summary,
                false,
                brief.connection.is_some(),
                k,
            ));
        }
    }
    Some(panel)
}

fn summary_section(summary: &Summary, breaking: bool, pending: bool, k: &Kit) -> El {
    let (key, heading, mark, bullets, empty) = if breaking {
        (
            "breaking",
            "Breaking changes",
            Intent::Breaking.icon(),
            &summary.body.breaking_changes,
            "No breaking changes found in the notes reviewed.",
        )
    } else {
        (
            "highlights",
            "Highlights",
            &glyphs::SPARKLE,
            &summary.body.highlights,
            "No highlights found in the notes reviewed.",
        )
    };
    let mut section = col()
        .w_full()
        .key(format!("ai-section-{key}"))
        .child(
            row()
                .w_full()
                .center_y()
                .gap(7.)
                .pt(6.)
                .pb(6.)
                .child(icon(mark, 14.).color(k.c.icon_muted).a11y_hidden())
                .child(text(heading).font(k.ty.ui_medium).color(k.c.text_base))
                .child(el().grow(1.))
                .child(detail(bullets.len().to_string(), k)),
        )
        .child(reader_rule(k));
    if bullets.is_empty() && !pending {
        section = section.child(detail(empty, k).py(8.));
    }
    for (index, bullet) in bullets.iter().enumerate() {
        let citation = &summary.citations[bullet.release];
        let source =
            finding_source(citation, bullet.publisher, k).key(format!("ai-source-{key}-{index}"));
        section = section.child(
            row()
                .w_full()
                .items_start()
                .gap(10.)
                .py(7.)
                .key(format!("ai-finding-{key}-{index}"))
                .child(
                    Markdown::new(
                        crate::description_preview::without_images(&bullet.text).as_ref(),
                    )
                    .autolink(false)
                    .style(crate::theme::changelog_markdown(k))
                    .scroll_wide(true)
                    .selectable(true)
                    .tasks_read_only()
                    .build(k)
                    .grow(1.)
                    .shrink()
                    .min_w(0.)
                    .key(format!("ai-text-{key}-{index}")),
                )
                .child(source),
        );
        if index + 1 < bullets.len() {
            section = section.child(reader_rule(k));
        }
    }
    section
}

fn finding_source(citation: &ai_summary::Citation, publisher: bool, k: &Kit) -> El {
    let label = &citation.label;
    let mut tag = row()
        .center_y()
        .gap(4.)
        .px(6.)
        .h(22.)
        .max_w(126.)
        .min_w(0.)
        .radius(4.)
        .fill(k.c.layer_02)
        .help(format!(
            "{}{}",
            if publisher {
                "Publisher notes: "
            } else {
                "Release source: "
            },
            label
        ))
        .child(
            text(label)
                .font(k.ty.text_xsmall.mono())
                .color(k.c.text_muted)
                .min_w(0.)
                .truncate(),
        );
    if let Some(url) = citation.url.clone() {
        tag = tag
            .role("button")
            .label(format!("Open release {label}"))
            .focusable(true)
            .focus_ring(1., 0., k.c.border_focus)
            .hover(k.c.hover)
            .child(
                icon(&glyphs::ARROW_RIGHT, 11.)
                    .color(k.c.icon_muted)
                    .a11y_hidden(),
            )
            .on_press_tx(move |s: &mut State, tx| {
                if tx.open_url(&url).is_err() {
                    s.status = "Could not open the release source".into();
                }
            });
    }
    let mut source = col().items_end().gap(2.).max_w(126.).child(tag);
    if publisher {
        source = source.child(
            text("Publisher")
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted),
        );
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::{
        io::Write,
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn ai_full_review_keeps_completed_findings_after_a_failed_batch() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let mut report = ai_summary::tests::report();
        report.notes.remove(0);
        for major in 3..=8 {
            let mut next = report.notes[0].clone();
            next.version = Some(format!("{major}.0.0"));
            report.notes.push(next);
        }
        let plan = ReviewPlan::full(&report, false, false).unwrap();
        assert_eq!(plan.batches.len(), 3);
        let mut ui = Ui {
            brief: Some(Brief {
                package: "example".into(),
                intent: Intent::Installed,
                releases: plan.releases,
                range: Some("1.0.0 to 8.0.0".into()),
                provider: Provider::Custom,
                model: "test".into(),
                model_label: "test".into(),
                partial: plan.partial,
                connection: Some(Arc::new(
                    Connection::new(Provider::Custom, "test", &endpoint, "", None).unwrap(),
                )),
                pending: plan.batches.into(),
                total: 3,
                completed: 0,
                summary: Summary::default(),
                error: None,
            }),
            ..Ui::default()
        };
        let server = thread::spawn(move || {
            for label in ["2.0.0", "5.0.0"] {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("Batch did not arrive: {e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let (_, payload) = ai_summary::tests::read_mock_request(&mut stream);
                let input: Value =
                    serde_json::from_str(payload["messages"][1]["content"].as_str().unwrap())
                        .unwrap();
                assert_eq!(input["releases"][0]["label"], label);
                if label == "2.0.0" {
                    let summary = json!({"highlights": [{
                        "release": 0, "text": "The exporter now works offline.",
                        "evidence": "The exporter now works offline."
                    }], "breaking_changes": []});
                    let body = json!({"choices": [{"finish_reason": "stop", "message": {"content": summary.to_string()}}]}).to_string();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .unwrap();
                } else {
                    write!(stream, "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                }
            }
        });
        assert!(!ui.work.is_pending());
        ui.next_batch();
        let deadline = Instant::now() + Duration::from_secs(10);
        while ui.brief.as_ref().unwrap().connection.is_some() && Instant::now() < deadline {
            ui.poll();
            thread::sleep(Duration::from_millis(5));
        }
        server.join().unwrap();
        let brief = ui.brief.as_ref().unwrap();
        assert_eq!(brief.completed, 1);
        assert_eq!(
            brief.scope_label(),
            "7 releases in scope  ·  1.0.0 to 8.0.0"
        );
        assert_eq!(brief.summary.body.highlights.len(), 1);
        assert!(brief.error.is_some_and(|e| e.contains("quota")));
        assert!(brief.pending.is_empty());
        assert!(!ui.work.is_pending());
    }
}
