//! Reversible display classification. Raw observations are never removed or merged.
use crate::{
    inventory::{InstalledPackage, Manager, Scan},
    matching,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct ComponentRule {
    manager: String,
    source: Option<String>,
    id_pattern: String,
    reason: String,
}
fn rules() -> &'static [(ComponentRule, regex::Regex)] {
    static RULES: OnceLock<Vec<(ComponentRule, regex::Regex)>> = OnceLock::new();
    RULES.get_or_init(|| {
        let rules: Vec<ComponentRule> =
            serde_json::from_str(include_str!("../data/component-rules.json"))
                .expect("bundled component rules");
        rules
            .into_iter()
            .map(|r| {
                let pattern = regex::Regex::new(&r.id_pattern).expect("component ID pattern");
                (r, pattern)
            })
            .collect()
    })
}

#[derive(Deserialize)]
struct Alias {
    manager: String,
    id: String,
    source: Option<String>,
    product: String,
    name: String,
}
pub struct Observation {
    pub scan: usize,
    pub package: usize,
    pub group: String,
    pub name: Option<String>,
    pub component_reason: Option<String>,
    pub wrapper: bool,
}
fn managed_app_reason(p: &InstalledPackage) -> Option<String> {
    if p.manager != Manager::Store {
        return None;
    }
    let e = &p.evidence;
    if e.non_removable == Some(true) && e.signature_kind.as_deref() == Some("System") {
        return Some("Non-removable, system-signed Windows component".into());
    }
    if e.has_cli_alias.or(e.has_execution_alias) == Some(false) {
        if e.is_inbox == Some(true) {
            return Some("Windows inbox app (OS registration)".into());
        }
        if e.has_preinstalled_task == Some(true) {
            return Some("Windows/OEM preinstalled configuration app (manifest)".into());
        }
        let microsoft = e.publisher.as_deref().is_some_and(|publisher| {
            publisher
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("O=Microsoft Corporation"))
        });
        let regular_package = matches!(e.signature_kind.as_deref(), Some("Store" | "System"))
            || (e.signature_kind.as_deref() == Some("Developer")
                && e.is_development_mode == Some(false));
        if microsoft && regular_package && e.is_development_mode != Some(true) {
            return Some("Microsoft-managed packaged app; no CLI execution alias".into());
        }
    }
    None
}
pub fn component_reason(p: &InstalledPackage) -> Option<String> {
    if p.manager.is_editor() {
        return None;
    }
    if p.evidence.font_only == Some(true) {
        return Some(
            "Font-only installer payload, without application shortcuts (MSI tables)".into(),
        );
    }
    if let Some(reason) = managed_app_reason(p) {
        return Some(reason);
    }
    let e = &p.evidence;
    if matches!(p.manager, Manager::Local | Manager::Winget)
        && e.publisher.as_deref().is_some_and(|s| {
            s.trim()
                .trim_end_matches('.')
                .eq_ignore_ascii_case("Microsoft Corporation")
        })
    {
        static FRAMEWORK: OnceLock<regex::Regex> = OnceLock::new();
        let role=FRAMEWORK.get_or_init(||regex::Regex::new(r"(?i)^Microsoft (?:Visual C\+\+ .+ Redistributable|Windows Desktop Runtime\s+-?\s*[0-9]|\.NET (?:Runtime|Host(?: FX Resolver)?)\s+-?\s*[0-9]|XNA Framework Redistributable)").unwrap());
        if role.is_match(&p.name) {
            return Some("Publisher-declared shared framework/redistributable role".into());
        }
    }
    if e.system_component == Some(true) || e.parent_package.is_some() {
        return Some("Windows installer marks this as a system/child component".into());
    }
    if e.release_type.as_deref().is_some_and(|s| {
        ["update", "hotfix", "security update"]
            .iter()
            .any(|kind| s.eq_ignore_ascii_case(kind))
    }) {
        return Some("Windows servicing/update package (installer metadata)".into());
    }
    if e.runtime_only == Some(true) {
        return Some("Runtime-only payload, without its SDK/compiler".into());
    }
    if e.library_only == Some(true) {
        return Some("Library-only installed payload, without a standalone executable".into());
    }
    if e.background_only == Some(true) {
        return Some(
            "Autostart GUI helper without an application shortcut (Windows registration)".into(),
        );
    }
    if matches!(p.manager, Manager::Npm | Manager::Pnpm | Manager::Bun)
        && p.scope == "global"
        && e.has_commands == Some(false)
    {
        return Some("Global library package has no CLI entry point (installed manifest)".into());
    }
    let no_entry_points = e.has_commands == Some(false) && e.has_shortcuts == Some(false);
    if no_entry_points && e.is_dependency == Some(true) {
        return Some("Dependency payload has no command or application shortcut (manifest)".into());
    }
    let tags: HashSet<_> = e
        .package_tags
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    if p.manager == Manager::Chocolatey && e.wrapper_target.is_none() {
        if tags.contains("workload") {
            return Some("Installer workload, not a standalone application (receipt tags)".into());
        }
        if ["redistributable"].iter().any(|tag| tags.contains(*tag))
            || (no_entry_points
                && ["runtime", "library", "libraries"]
                    .iter()
                    .any(|tag| tags.contains(*tag)))
        {
            return Some("Runtime/library support package (receipt tags)".into());
        }
    }
    if no_entry_points && e.support_data == Some(true) {
        return Some(
            "Completion/certificate data has no command or application shortcut (manifest)".into(),
        );
    }
    if let Some((rule, _)) = rules().iter().find(|(r, pattern)| {
        r.manager == p.manager.id()
            && r.source.as_ref().is_none_or(|s| s == &p.source)
            && pattern.is_match(&p.id)
    }) {
        return Some(rule.reason.clone());
    }
    if p.manager == Manager::Store
        && p.evidence.has_visible_app == Some(false)
        && p.evidence.has_execution_alias == Some(false)
    {
        return Some(
            "Packaged background/support component (no visible application or CLI alias)".into(),
        );
    }
    if p.manager == Manager::Chocolatey {
        let tags = p.evidence.package_tags.to_ascii_lowercase();
        if p.id.to_ascii_lowercase().ends_with(".extension")
            && tags.contains("chocolatey")
            && tags.contains("extension")
        {
            return Some("Chocolatey support extension (package metadata)".into());
        }
        if p.id
            .to_ascii_uppercase()
            .strip_prefix("KB")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
            && tags.contains("windows update")
        {
            return Some("Windows prerequisite/KB package (package metadata)".into());
        }
    }
    None
}
pub fn observations(scans: &[Scan]) -> Vec<Observation> {
    let bindings = matching::bindings();
    let aliases: Vec<Alias> = serde_json::from_str(include_str!("../data/inventory-groups.json"))
        .expect("bundled product aliases");
    let mut result = vec![];
    for (si, scan) in scans.iter().enumerate() {
        for (pi, p) in scan.packages.iter().enumerate() {
            let mut target = pi;
            let mut visited = HashSet::new();
            while visited.insert(target) {
                let current = &scan.packages[target];
                if current.manager != Manager::Chocolatey {
                    break;
                }
                let Some(id) = &current.evidence.wrapper_target else {
                    break;
                };
                let candidates: Vec<_> = scan
                    .packages
                    .iter()
                    .enumerate()
                    .filter(|(_, q)| {
                        q.id.eq_ignore_ascii_case(id)
                            && q.source == current.source
                            && q.scope == current.scope
                            && q.root == current.root
                            && q.installed.is_some()
                            && q.installed == current.installed
                    })
                    .map(|(i, _)| i)
                    .collect();
                if candidates.len() != 1 {
                    break;
                }
                let next = candidates[0];
                if visited.contains(&next) {
                    target = pi;
                    break;
                }
                target = next;
            }
            let canonical = &scan.packages[target];
            let binding = matching::resolve(canonical, &bindings);
            let alias = aliases.iter().find(|a| {
                a.manager == canonical.manager.id()
                    && a.id == canonical.id
                    && a.source.as_ref().is_none_or(|s| s == &canonical.source)
            });
            let identity = if let Some(b) = binding {
                format!(
                    "product:{}/{}",
                    b.product,
                    b.stream.as_deref().unwrap_or("-")
                )
            } else if let Some(a) = alias {
                format!("product:{}/-", a.product)
            } else {
                // A provider identity describes a product, not a particular installation.
                // Roots, scopes and versions stay on each raw record beneath that product.
                if canonical.manager.is_editor() {
                    serde_json::to_string(&("editor-extension", canonical.id.to_ascii_lowercase()))
                        .unwrap()
                } else {
                    serde_json::to_string(&(
                        canonical.manager.id(),
                        &canonical.id,
                        &canonical.source,
                    ))
                    .unwrap()
                }
            };
            let group = format!("group-{:x}", Sha256::digest(identity));
            result.push(Observation {
                scan: si,
                package: pi,
                group,
                name: binding
                    .and_then(|b| b.display_name.clone())
                    .or_else(|| alias.map(|a| a.name.clone())),
                component_reason: component_reason(canonical).or_else(|| component_reason(p)),
                wrapper: target != pi,
            });
        }
    }
    // Local presence is not a new product when WinGet's unique, version-checked
    // registry join identifies the very same receipt. Never merge on display names.
    for i in 0..result.len() {
        let p = &scans[result[i].scan].packages[result[i].package];
        if p.manager != Manager::Local {
            continue;
        }
        if result[i].component_reason.is_none() {
            result[i].component_reason = result.iter().find_map(|o| {
                let q = &scans[o.scan].packages[o.package];
                if q.manager == Manager::Winget
                    && q.evidence
                        .registration_id
                        .as_deref()
                        .is_some_and(|id| id.eq_ignore_ascii_case(&p.id))
                {
                    component_reason(q)
                } else {
                    None
                }
            });
        }
        let groups: HashSet<_> = result
            .iter()
            .filter_map(|o| {
                let q = &scans[o.scan].packages[o.package];
                (q.manager == Manager::Winget
                    && q.evidence
                        .registration_id
                        .as_deref()
                        .is_some_and(|id| id.eq_ignore_ascii_case(&p.id)))
                .then(|| o.group.clone())
            })
            .collect();
        if groups.len() == 1 {
            result[i].group = groups.into_iter().next().unwrap();
        }
    }
    // OS-managed app policy follows verified product identities across providers.
    // Local support footprints do not: a stale library-only receipt must not hide
    // another standalone installation of the same product.
    let hidden: std::collections::HashMap<_, _> = result
        .iter()
        .filter_map(|o| {
            managed_app_reason(&scans[o.scan].packages[o.package])
                .map(|reason| (o.group.clone(), reason))
        })
        .collect();
    for o in &mut result {
        if o.component_reason.is_none() {
            o.component_reason = hidden.get(&o.group).cloned();
        }
    }
    result
}
