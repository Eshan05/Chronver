//! Explicit machine audit. Ordinary tests never scan, fetch, or write reports.
use crate::model::State;
use releasescout_engine::{
    Catalog, Engine, Error, Result,
    http::{CacheOptions, Document, Fetcher, HttpClient},
    inventory::{self, Manager, ProcessRunner},
    matching, source_discovery,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
fn save(path: &Path, value: &Value) {
    let mut file = tempfile_for(path);
    use std::io::Write;
    file.write_all(serde_json::to_string_pretty(value).unwrap().as_bytes())
        .unwrap();
    file.flush().unwrap();
    drop(file);
    fs::rename(path.with_extension("tmp"), path).unwrap();
}
fn tempfile_for(path: &Path) -> fs::File {
    fs::File::create(path.with_extension("tmp")).unwrap()
}

#[test]
#[ignore = "explicit saved-inventory update-state audit; no scans, network or preference writes"]
fn saved_software_update_status_audit() {
    use crate::{model::PackageRow, sidebar_filters::{self, Update}};
    let directory = Path::new("artifacts/update-status-audit");
    fs::create_dir_all(directory).unwrap();
    let snapshot = crate::live::cache_directory().with_file_name("inventory.json");
    let before = fs::read(&snapshot).unwrap();
    let preferences = crate::live::cache_directory().with_file_name("activity.json");
    let before_preferences = fs::read(&preferences).ok();
    let mut s = State::new();
    s.inventory.scans = matching::load_snapshot(&snapshot).unwrap();
    s.rebuild_detected();
    s.restore_activity();
    s.activity.data.sidebar_filters[0] = Default::default();
    s.show_components = false;
    s.query = rune::TextEdit::new("");
    s.refilter();
    let mut totals = BTreeMap::<String, usize>::new();
    let mut reasons = BTreeMap::<String, usize>::new();
    let rows: Vec<_> = s.visible.iter().map(|&i| {
        let row = &s.rows[i];
        let status = sidebar_filters::update(&s, row);
        let status = format!("{status:?}").to_lowercase();
        *totals.entry(status.clone()).or_default() += 1;
        let records: Vec<_> = row.members.iter().map(|&i| {
            let p = &s.catalog.packages[i];
            let single = PackageRow { id: row.id.clone(), name: p.name.clone(), package: i, members: vec![i] };
            let member_state = sidebar_filters::update(&s, &single);
            let o = p.observation.as_ref();
            let reason = if o.is_some_and(|o|o.confirmed_update) { "confirmed_update" }
                else if !p.provider.is_some_and(releasescout_engine::inventory_refresh::supports_candidates) { "unsupported_provider" }
                else if o.is_none_or(|o|!o.requested) { "not_checked" }
                else if o.is_some_and(|o|!o.succeeded) { "failed_or_incomplete_check" }
                else if p.installed.is_none() { "missing_installed_version" }
                else if member_state == Update::Current { "checked_current" }
                else { "uncomparable_candidate" };
            let record = o.and_then(|o|s.inventory.scans.iter().flat_map(|scan|&scan.packages)
                .find(|record| p.provider == Some(record.manager) && crate::sidebar_info::check_key(record) == o.check_key));
            json!({"provider":p.provider.map(|m|m.id()),"id":o.map(|o|&o.id),
                "installed":p.installed,"candidate":p.target,"state":format!("{member_state:?}").to_lowercase(),
                "reason":reason,"requested":o.map(|o|o.requested),"succeeded":o.map(|o|o.succeeded),
                "confirmed_update":o.map(|o|o.confirmed_update),"detail":o.and_then(|o|o.detail.as_ref()),
                "record":record})
        }).collect();
        let signature = records.iter().filter(|r|r["state"]=="unknown")
            .filter_map(|r|r["reason"].as_str()).collect::<std::collections::BTreeSet<_>>()
            .into_iter().collect::<Vec<_>>().join(" + ");
        if status == "unknown" { *reasons.entry(signature.clone()).or_default() += 1; }
        json!({"group":row.id,"name":row.name,"status":status,"reason":signature,"observations":records})
    }).collect();
    let unknown: Vec<_> = rows.iter().filter(|r|r["status"]=="unknown").cloned().collect();
    let scans: Vec<_> = s.inventory.scans.iter().map(|scan|json!({
        "provider":scan.manager.id(),"scanned_at":scan.checked_at,"candidate_checked_at":scan.candidate_time(),
        "requested":scan.update_check_requested,"succeeded":scan.update_check_succeeded,
        "warnings":scan.warnings,"records":scan.packages.len()
    })).collect();
    let report = json!({"totals":totals,"unknown_count":unknown.len(),"reason_totals":reasons,"scans":scans,"unknown":unknown,"all":rows});
    let name = std::env::var("RELEASESCOUT_UPDATE_AUDIT_FILE").unwrap_or_else(|_|"current.json".into());
    assert!(!name.contains(['/', '\\']), "audit output must be a filename");
    let path = directory.join(name);
    save(&path, &report);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["unknown"].as_array().unwrap().len(), report["unknown_count"].as_u64().unwrap() as usize);
    assert_eq!(fs::read(&snapshot).unwrap(), before, "audit must not alter saved inventory");
    assert_eq!(fs::read(&preferences).ok(), before_preferences, "audit must not alter preferences");
    println!("{}; reasons {}; report {}", report["totals"], report["reason_totals"], path.display());
}

#[test]
#[ignore = "explicit saved-inventory description audit; no scans, network or preference writes"]
fn saved_software_description_coverage() {
    let directory = Path::new("artifacts/description-audit");
    fs::create_dir_all(directory).unwrap();
    let snapshot = crate::live::cache_directory().with_file_name("inventory.json");
    let preferences = crate::live::cache_directory().with_file_name("activity.json");
    let before_snapshot = fs::read(&snapshot).unwrap();
    let before_preferences = fs::read(&preferences).ok();
    let mut s = State::new();
    for scan in matching::load_snapshot(&snapshot).unwrap() {
        s.accept_scan(scan);
    }
    s.restore_activity();
    assert!(!s.show_components);
    assert!(s.section == crate::model::LibrarySection::Software);
    let rows: Vec<_> = s
        .visible
        .iter()
        .map(|&i| {
            let row = &s.rows[i];
            let selected = &s.catalog.packages[row.package];
            let records: Vec<_> = row
                .members
                .iter()
                .filter_map(|&i| {
                    let p = &s.catalog.packages[i];
                    let observation = p.observation.as_ref()?;
                    let record = s
                        .inventory
                        .scans
                        .iter()
                        .flat_map(|scan| &scan.packages)
                        .find(|record| {
                            p.provider == Some(record.manager)
                                && crate::sidebar_info::check_key(record) == observation.check_key
                        })?;
                    Some(
                        json!({"manager":record.manager.id(),"id":record.id,"name":record.name,
                "publisher":record.evidence.publisher,"homepage":record.homepage,
                "repository":record.repository,"catalog_id":p.reference_id,
                "description":record.evidence.description}),
                    )
                })
                .collect();
            json!({"group":row.id,"name":row.name,"catalog_id":selected.reference_id,
            "description":crate::sidebar_info::description(&s,row),"records":records})
        })
        .collect();
    let missing: Vec<_> = rows
        .iter()
        .filter(|row| row["description"].is_null())
        .cloned()
        .collect();
    save(
        &directory.join("input.json"),
        &json!({"total":rows.len(),"missing_count":missing.len(),"missing":missing,"all":rows}),
    );
    assert_eq!(fs::read(&snapshot).unwrap(), before_snapshot);
    assert_eq!(fs::read(&preferences).ok(), before_preferences);
    println!(
        "{} Software rows / {} missing descriptions; artifacts/description-audit/input.json",
        rows.len(),
        missing.len()
    );
}

#[test]
#[ignore = "explicit compile of completed sourced-description research into the bundled catalog"]
fn compile_researched_descriptions() {
    use crate::descriptions::{Entry, Selector};
    use std::collections::BTreeSet;
    struct Product {
        name: String,
        description: String,
        sources: BTreeSet<String>,
        selectors: BTreeSet<Selector>,
    }
    let directory = Path::new("artifacts/description-audit");
    let baseline = directory.join("baseline.json");
    let input: Value = serde_json::from_slice(
        &fs::read(if baseline.exists() {
            baseline.clone()
        } else {
            directory.join("input.json")
        })
        .unwrap(),
    )
    .unwrap();
    let missing = input["missing"].as_array().unwrap();
    let mut results = BTreeMap::new();
    for entry in fs::read_dir("research/descriptions").unwrap() {
        let path = entry.unwrap().path();
        if !path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("batch-")
            || path.extension().is_none_or(|e| e != "json")
        {
            continue;
        }
        let rows: Vec<Value> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for row in rows {
            let index = row["index"].as_u64().unwrap() as usize;
            assert!(index < missing.len());
            assert_eq!(
                row["name"], missing[index]["name"],
                "research identity mismatch at {index}"
            );
            assert!(
                results.insert(index, row).is_none(),
                "duplicate research index {index}"
            );
        }
    }
    assert_eq!(
        results.len(),
        missing.len(),
        "finish every research batch before compiling"
    );
    let mut products: BTreeMap<String, Product> = BTreeMap::new();
    let mut unresolved = Vec::new();
    let mut variants = Vec::new();
    for (index, row) in &results {
        match row["status"].as_str().unwrap() {
            "unresolved" => {
                assert!(row["description"].is_null());
                assert!(!row["notes"].as_str().unwrap().trim().is_empty());
                unresolved.push(row.clone());
                continue;
            }
            "verified" => {}
            other => panic!("incomplete research status {other} at {index}"),
        }
        let name = row["product"].as_str().unwrap().trim();
        let description = row["description"].as_str().unwrap().trim();
        assert!(!name.is_empty() && !description.is_empty() && description.chars().count() <= 160);
        let sources = row["source_urls"].as_array().unwrap();
        assert!(!sources.is_empty());
        let product = products
            .entry(name.to_lowercase())
            .or_insert_with(|| Product {
                name: name.into(),
                description: description.into(),
                sources: BTreeSet::new(),
                selectors: BTreeSet::new(),
            });
        if product.description != description {
            variants.push(json!({"index":index,"product":name,"selected":product.description,"alternate":description}));
        }
        product
            .sources
            .extend(sources.iter().map(|s| s.as_str().unwrap().to_owned()));
        for record in missing[*index]["records"].as_array().unwrap() {
            if let Some(id) = record["catalog_id"].as_str() {
                product
                    .selectors
                    .insert(Selector::Catalog { id: id.into() });
            }
            let manager = record["manager"].as_str().unwrap();
            if manager == "local" {
                product.selectors.insert(Selector::Local {
                    name: crate::descriptions::local_name(record["name"].as_str().unwrap()),
                    publisher: record["publisher"].as_str().map(|s| s.to_lowercase()),
                });
            } else {
                product.selectors.insert(Selector::Package {
                    manager: manager.into(),
                    id: record["id"].as_str().unwrap().into(),
                });
            }
        }
    }
    let entries: Vec<_> = products
        .into_values()
        .map(|p| Entry {
            product: p.name,
            description: p.description,
            sources: p.sources.into_iter().collect(),
            selectors: p.selectors.into_iter().collect(),
        })
        .collect();
    let json = serde_json::to_string_pretty(&entries).unwrap();
    crate::descriptions::validate_catalog(&json).unwrap();
    if !baseline.exists() {
        save(&baseline, &input);
    }
    save(
        &directory.join("research-review.json"),
        &json!({"researched":results.len(),"products":entries.len(),"unresolved":unresolved,"description_variants":variants}),
    );
    fs::write("assets/software-descriptions.json", json).unwrap();
    println!(
        "Compiled {} product descriptions / {} unresolved research entries",
        entries.len(),
        unresolved.len()
    );
}

struct AuditFetch {
    online: HttpClient,
    offline: HttpClient,
    blocked: RefCell<HashMap<String, String>>,
    log: RefCell<Vec<Value>>,
}
impl Fetcher for AuditFetch {
    fn get(&self, url: &str) -> Result<Document> {
        let origin = url::Url::parse(url)
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_default();
        let result = if let Some(reason) = self.blocked.borrow().get(&origin) {
            self.offline
                .get(url)
                .map_err(|_| Error::Source(format!("audit blocked origin {origin}: {reason}")))
        } else {
            self.online.get(url)
        };
        if let Err(Error::Http {
            status: 403 | 429, ..
        }) = &result
        {
            self.blocked
                .borrow_mut()
                .insert(origin, result.as_ref().unwrap_err().to_string());
        }
        self.log.borrow_mut().push(match &result {
            Ok(d) => json!({"url":url,"evidence":d.evidence}),
            Err(e) => json!({"url":url,"error":e.to_string()}),
        });
        result
    }
}

#[test]
#[ignore = "explicit live machine source-coverage audit; read-only inventory and public requests"]
fn real_software_source_coverage() {
    let output = std::env::var_os("RELEASESCOUT_AUDIT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "artifacts/source-coverage-audit".into());
    let directory = output.as_path();
    fs::create_dir_all(directory).unwrap();
    let snapshot = crate::live::cache_directory().with_file_name("inventory.json");
    let before = fs::read(&snapshot).ok();
    let mut scans =
        matching::load_snapshot(&snapshot).expect("a saved native inventory is required");
    let mut providers = vec![];
    if std::env::var_os("RELEASESCOUT_AUDIT_REUSE_SCAN").is_some() {
        scans =
            serde_json::from_slice(&fs::read(directory.join("inventory-refresh.json")).unwrap())
                .unwrap();
        providers
            .push(json!({"mode":"reuse previous audit scan; consult recorded scan timestamps"}));
    } else {
        for manager in Manager::ALL.into_iter().filter(|m| !m.is_editor()) {
            println!("Refreshing installed metadata: {}", manager.id());
            match inventory::scan(manager,false,&ProcessRunner::default()) {
                Ok(scan)=>{
                    providers.push(json!({"manager":manager.id(),"status":"refreshed","count":scan.packages.len(),"checked_at":scan.checked_at,"warnings":scan.warnings}));
                    scans.retain(|s|s.manager!=manager);scans.push(scan);
                }
                Err(e)=>providers.push(json!({"manager":manager.id(),"status":"refresh_failed_saved_scan_retained","error":e.to_string()})),
            }
            save(&directory.join("providers.json"), &json!(providers));
        }
        fs::write(
            directory.join("inventory-refresh.json"),
            serde_json::to_vec_pretty(&scans).unwrap(),
        )
        .unwrap();
    }
    let mut state = State::new();
    for scan in &scans {
        state.accept_scan(scan.clone());
    }
    assert!(!state.inventory.persist);
    assert!(!state.show_components);
    assert!(state.dataset == crate::model::Dataset::Detected);
    let options = CacheOptions {
        directory: crate::live::cache_directory(),
        public_only: true,
        github_token: std::env::var("GITHUB_TOKEN").ok(),
        ..Default::default()
    };
    let fetch = AuditFetch {
        online: HttpClient::new(options.clone()).unwrap(),
        offline: HttpClient::new(CacheOptions {
            offline: true,
            ..options
        })
        .unwrap(),
        blocked: RefCell::new(HashMap::new()),
        log: RefCell::new(vec![]),
    };
    let mut results: Vec<Value> = state
        .rows
        .iter()
        .map(|r| json!({"group":r.id,"name":r.name,"status":"pending"}))
        .collect();
    let recheck = std::env::var("RELEASESCOUT_AUDIT_RECHECK")
        .ok()
        .map(|s| s.split('|').map(str::to_owned).collect::<Vec<_>>());
    if let Some(names) = &recheck {
        let previous: Value =
            serde_json::from_slice(&fs::read(directory.join("coverage.json")).unwrap()).unwrap();
        for row in &mut results {
            if !names.iter().any(|n| row["name"].as_str() == Some(n)) {
                let old = previous["results"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["group"] == row["group"])
                    .expect("recheck requires the same inventory groups");
                *row = old.clone();
            }
        }
    }
    let started = now();
    for (index, row) in state.rows.iter().enumerate() {
        if recheck
            .as_ref()
            .is_some_and(|names| !names.contains(&row.name))
        {
            continue;
        }
        let ui = &state.catalog.packages[row.package];
        let mut profile = state
            .live
            .catalog
            .packages
            .iter()
            .find(|p| p.id == ui.id)
            .unwrap()
            .clone();
        let mode = if profile.streams.is_empty() {
            "automatic"
        } else {
            "reviewed"
        };
        let offset = fetch.log.borrow().len();
        let mut result = json!({"group":row.id,"name":row.name,"selected_observation":ui.id,"installed":ui.installed,"method":mode,"tracking":profile.tracking,"hints":ui.source_hints,"observations":row.members.iter().map(|&i|json!({"id":state.catalog.packages[i].id,"label":state.catalog.packages[i].observation_label})).collect::<Vec<_>>(),"streams":[]});
        let mut outcome = "not_found";
        if !matches!(profile.tracking.as_str(), "track" | "low_priority") {
            outcome = "inactive";
        } else {
            if profile.streams.is_empty() {
                if let Some(hints) = ui.source_hints.as_ref().filter(|h| h.has_hints()) {
                    let found = source_discovery::discover(&fetch, hints);
                    result["discovery_attempts"] = json!(found.attempts);
                    profile.streams = found.streams;
                    profile.identity = "metadata_discovered".into();
                } else {
                    outcome = "missing_hints";
                }
            }
            if !profile.streams.is_empty() {
                outcome = "source_found_no_authored_notes";
                let engine = Engine {
                    catalog: Catalog {
                        schema_version: 1,
                        packages: vec![profile.clone()],
                    },
                    fetcher: &fetch,
                };
                let mut streams = vec![];
                for stream in &profile.streams {
                    match engine.history(&profile.id, Some(&stream.id), None, false) {
                        Ok(page) => {
                            let authored = page
                                .report
                                .notes
                                .iter()
                                .filter(|n| n.authored_notes)
                                .count();
                            if authored > 0 {
                                outcome = "authored_notes";
                            }
                            streams.push(json!({"stream":stream,"status":page.report.status,"note_count":page.report.notes.len(),"authored_count":authored,"complete":page.report.complete,"warnings":page.report.warnings,"attempts":page.report.attempts,"evidence":page.report.evidence,"notes":page.report.notes.iter().map(|n|json!({"version":n.version,"title":n.title,"url":n.url,"authored":n.authored_notes})).collect::<Vec<_>>()}));
                        }
                        Err(e) => streams.push(json!({"stream":stream,"error":e.to_string()})),
                    }
                }
                result["streams"] = json!(streams);
            }
        }
        let log = fetch.log.borrow()[offset..].to_vec();
        let reached_sparse = result["streams"]
            .as_array()
            .is_some_and(|streams| streams.iter().any(|s| s.get("status").is_some()))
            || result["discovery_attempts"]
                .as_array()
                .is_some_and(|attempts| {
                    attempts.iter().any(|a| {
                        a.as_str().is_some_and(|s| {
                            s.contains("source returned no authored release notes")
                        })
                    })
                });
        result["encountered_access_block"] =
            json!(log.iter().any(|l| l["error"].as_str().is_some_and(|e| {
                e.contains("audit blocked origin") || e.contains("403") || e.contains("429")
            })));
        if outcome != "authored_notes" && reached_sparse {
            outcome = "source_found_no_authored_notes";
        } else if outcome != "authored_notes"
            && log.iter().any(|l| {
                l["error"].as_str().is_some_and(|e| {
                    e.contains("audit blocked origin") || e.contains("403") || e.contains("429")
                })
            })
        {
            outcome = "access_or_rate_blocked";
        } else if outcome == "not_found" && log.iter().any(|l| l.get("error").is_some()) {
            outcome = "lookup_failed";
        }
        result["status"] = json!(outcome);
        result["requests"] = json!(log);
        result["checked_at_epoch"] = json!(now());
        results[index] = result;
        save(
            &directory.join("coverage.json"),
            &json!({"started_epoch":started,"finished":results.iter().all(|r|r["status"]!="pending"),"rows_total":state.rows.len(),"rows_completed":results.iter().filter(|r|r["status"]!="pending").count(),"providers":providers,"results":results,"blocked_origins":*fetch.blocked.borrow()}),
        );
        println!(
            "{}/{} {}: {}",
            index + 1,
            state.rows.len(),
            row.name,
            outcome
        );
    }
    assert_eq!(
        before,
        fs::read(snapshot).ok(),
        "audit must not mutate the native saved inventory"
    );
    let mut counts = BTreeMap::new();
    for result in &results {
        *counts
            .entry(result["status"].as_str().unwrap())
            .or_insert(0) += 1;
    }
    let mut markdown = format!(
        "# Actual Software Source Audit\n\n{} default Software groups. Extensions and hidden system/support records excluded.\n\nOne paged history read per configured stream; not complete historical coverage or update compatibility. Inventory snapshot unchanged.\n\n## Results\n\n",
        results.len()
    );
    for (status, count) in &counts {
        markdown.push_str(&format!("- {status}: {count}\n"));
    }
    markdown.push_str("\n## Entries\n\n| Software | Result | Method |\n|---|---|---|\n");
    for r in &results {
        markdown.push_str(&format!(
            "| {} | {} | {} |\n",
            r["name"].as_str().unwrap().replace('|', "/"),
            r["status"].as_str().unwrap(),
            r["method"].as_str().unwrap()
        ));
    }
    fs::write(directory.join("report.md"), markdown).unwrap();
    println!("Audit reconciled: {} groups, {:?}", results.len(), counts);
}

impl Fetcher for &AuditFetch {
    fn get(&self, url: &str) -> Result<Document> {
        (**self).get(url)
    }
}
