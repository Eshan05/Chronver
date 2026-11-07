//! Reconcile a saved snapshot without scanning the machine or checking for updates.
use releasescout_engine::{catalog, matching, presentation};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("provide an inventory snapshot path")?;
    let details = std::env::args().any(|arg| arg == "--details");
    let scans = matching::load_snapshot(std::path::Path::new(&path))?;
    let observations = presentation::observations(&scans);
    let bindings = matching::bindings();
    let catalog = catalog::bundled()?;
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    let mut hidden = 0;
    let mut reference_products = BTreeSet::new();
    let mut reference_groups = BTreeSet::new();
    let mut extension_groups = BTreeSet::new();
    let mut extension_profiles = BTreeSet::new();
    let mut hidden_reasons = BTreeMap::<String, usize>::new();
    for o in &observations {
        if o.component_reason.is_some() {
            hidden += 1;
            *hidden_reasons
                .entry(o.component_reason.clone().unwrap())
                .or_default() += 1;
            continue;
        }
        let p = &scans[o.scan].packages[o.package];
        if p.manager == releasescout_engine::inventory::Manager::VscodeInsiders {
            extension_groups.insert(&o.group);
            extension_profiles.insert((&p.scope, &p.root));
        }
        let binding = matching::resolve(p, &bindings);
        if let Some(b) = binding {
            reference_products.insert(b.product.clone());
            reference_groups.insert(&o.group);
        }
        let name = o
            .name
            .clone()
            .or_else(|| {
                binding
                    .and_then(|b| catalog.packages.iter().find(|p| p.id == b.product))
                    .map(|p| p.name.clone())
            })
            .unwrap_or_else(|| p.name.clone());
        groups.entry(&o.group).or_default().push(json!({
            "name":name, "manager":p.manager.id(), "id":p.id,
            "installed":p.installed, "source":p.source,
            "reference":binding.map(|b| &b.product), "wrapper":o.wrapper,
        }));
    }
    let visible = observations.len() - hidden;
    let mut result = json!({
        "raw_observations":observations.len(),
        "hidden_component_observations":hidden,
        "visible_observations":visible,
        "display_groups":groups.len(),
        "app_tool_display_groups":groups.len()-extension_groups.len(),
        "extension_display_groups":extension_groups.len(),
        "extension_profiles":extension_profiles.len(),
        "hidden_reasons":hidden_reasons,
        "grouped_observations":visible - groups.len(),
        "groups_with_multiple_observations":groups.values().filter(|v| v.len()>1).count(),
        "matched_reference_products":reference_products.len(),
        "reference_display_groups":reference_groups.len(),
        "additional_display_groups":groups.len()-reference_groups.len(),
        "unmatched_reference_products":catalog.packages.len()-reference_products.len(),
        "limitation":"Additional groups include genuine extra products and identities not yet reconciled; they are not a verified unique-app count."
    });
    if details {
        result["groups"] = json!(groups.values().collect::<Vec<_>>());
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
