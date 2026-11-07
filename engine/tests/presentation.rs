use releasescout_engine::{
    inventory::{Manager, parse},
    presentation::{component_reason, observations},
};
use serde_json::json;

#[test]
fn packaged_background_entries_are_hidden_but_cli_aliases_and_unknowns_stay() {
    for (visible, alias, hidden) in [
        (Some(false), Some(false), true),
        (Some(false), Some(true), false),
        (Some(true), Some(false), false),
        (None, None, false),
    ] {
        let s=parse(Manager::Store,false,&json!([{"Name":"Vendor.TechnicalID","FriendlyName":"A real application","PackageFamilyName":"Vendor.TechnicalID_pub","Version":"1.0","HasVisibleApp":visible,"HasExecutionAlias":alias}]).to_string()).unwrap();
        assert_eq!(s.packages[0].name, "A real application");
        assert_eq!(s.packages[0].id, "Vendor.TechnicalID_pub");
        assert_eq!(component_reason(&s.packages[0]).is_some(), hidden);
    }
}

#[test]
fn runtime_rules_are_source_scoped_and_do_not_hide_development_tools() {
    for (id, source, hidden) in [
        ("Microsoft.UI.Xaml.2.8", "winget", true),
        ("Microsoft.WindowsAppRuntime.2", "winget", true),
        ("Microsoft.VCRedist.2015+.x64", "winget", true),
        ("Microsoft.DotNet.DesktopRuntime.10", "winget", true),
        ("Microsoft.UI.Xaml.2.8", "private-source", false),
        ("Microsoft.PowerShell", "winget", false),
        ("Python.Python.3.14", "winget", false),
        ("OpenJS.NodeJS.20", "winget", false),
        ("Microsoft.VisualStudioCode.Insiders", "winget", false),
    ] {
        let s=parse(Manager::Winget,false,&json!({"installed":{"Sources":[{"SourceDetails":{"Name":source},"Packages":[{"PackageIdentifier":id,"Version":"1.0"}]}]}}).to_string()).unwrap();
        assert_eq!(
            component_reason(&s.packages[0]).is_some(),
            hidden,
            "{id} from {source}"
        );
    }
}

#[test]
fn system_flags_must_both_be_present_and_unknowns_remain_visible() {
    for (flags, hidden) in [
        (json!({"NonRemovable":true,"SignatureKind":"System"}), true),
        (
            json!({"NonRemovable":false,"SignatureKind":"System"}),
            false,
        ),
        (json!({"NonRemovable":true,"SignatureKind":"Store"}), false),
        (json!({}), false),
    ] {
        let mut p = json!({"Name":"Example","PackageFamilyName":"Example_pub","Version":"1.0"});
        p.as_object_mut()
            .unwrap()
            .extend(flags.as_object().unwrap().clone());
        let s = parse(Manager::Store, false, &json!([p]).to_string()).unwrap();
        assert_eq!(component_reason(&s.packages[0]).is_some(), hidden);
    }
}
#[test]
fn choco_helpers_need_receipt_evidence_and_wrappers_preserve_their_members() {
    let s=parse(Manager::Chocolatey,false,&json!({"installed":"cmake|4.0\ncmake.install|4.0\nhelper.extension|1.0\nKB12345|1.0\nunknown.extension|1.0", "metadata":{
        "cmake":{"wrapperTarget":"cmake.install"},"helper.extension":{"tags":"chocolatey core extension"},"KB12345":{"tags":"Microsoft Windows Update"}}}).to_string()).unwrap();
    assert!(component_reason(&s.packages[2]).is_some());
    assert!(component_reason(&s.packages[3]).is_some());
    assert!(component_reason(&s.packages[4]).is_none());
    let groups = observations(&[s]);
    assert_eq!(groups.len(), 5);
    assert_eq!(groups[0].group, groups[1].group);
    assert!(groups[0].wrapper);
    assert!(!groups[1].wrapper);
}
#[test]
fn missing_mismatched_or_cyclic_wrapper_targets_are_not_collapsed() {
    for (rows, metadata) in [
        ("alias|1.0", json!({"alias":{"wrapperTarget":"missing"}})),
        (
            "alias|1.0\nreal|2.0",
            json!({"alias":{"wrapperTarget":"real"}}),
        ),
        (
            "alias|1.0\nreal|1.0",
            json!({"alias":{"wrapperTarget":"real"},"real":{"wrapperTarget":"alias"}}),
        ),
    ] {
        let s = parse(
            Manager::Chocolatey,
            false,
            &json!({"installed":rows,"metadata":metadata}).to_string(),
        )
        .unwrap();
        let groups = observations(&[s]);
        assert!(groups.iter().all(|p| !p.wrapper));
        if groups.len() > 1 {
            assert_ne!(groups[0].group, groups[1].group);
        }
    }
}
#[test]
fn reviewed_cross_manager_identity_groups_versions_without_equating_installations() {
    let choco = parse(
        Manager::Chocolatey,
        false,
        &json!({"installed":"audacity|3.7.4"}).to_string(),
    )
    .unwrap();
    let winget=parse(Manager::Winget,false,&json!({"installed":{"Sources":[{"SourceDetails":{"Name":"winget"},"Packages":[{"PackageIdentifier":"Audacity.Audacity","Version":"3.7.4"}]}]}}).to_string()).unwrap();
    let groups = observations(&[choco, winget]);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].group, groups[1].group);
}
#[test]
fn runtime_versions_share_a_product_without_losing_versions_or_hiding_extensions() {
    let s = parse(
        Manager::Chocolatey,
        false,
        &json!({"installed":"python311|3.11.0\npython314|3.14.0"}).to_string(),
    )
    .unwrap();
    let groups = observations(std::slice::from_ref(&s));
    assert_eq!(groups[0].group, groups[1].group);
    assert_eq!(s.packages[0].installed.as_deref(), Some("3.11.0"));
    assert_eq!(s.packages[1].installed.as_deref(), Some("3.14.0"));
    assert!(groups.iter().all(|g| g.component_reason.is_none()));
    let s = parse(
        Manager::VscodeInsiders,
        false,
        &json!({"installed":"publisher.extension@1.0.0"}).to_string(),
    )
    .unwrap();
    assert!(component_reason(&s.packages[0]).is_none());
}

#[test]
fn reconciled_products_group_across_managers_despite_different_version_formats() {
    let choco = parse(Manager::Chocolatey, false, &json!({"installed":
        "cursoride|3.10.20\nobsidian|1.10.3\nmongodb-compass|1.49.1\npgadmin4|9.2.0\nintellijidea-community|2025.1.0\npython314|3.14.2\nwingetui|3.3.6"
    }).to_string()).unwrap();
    let ids = [
        ("Anysphere.Cursor", "3.10.20"),
        ("Obsidian.Obsidian", "1.10.3"),
        ("MongoDB.Compass.Full", "1.49.1.0"),
        ("PostgreSQL.pgAdmin", "9.2"),
        ("JetBrains.IntelliJIDEA.Community", "2025.1"),
        ("Python.Python.3.14", "3.14.2"),
    ];
    let winget = parse(Manager::Winget, false, &json!({"installed":{"Sources":[
        {"SourceDetails":{"Name":"winget"},"Packages":ids.iter().map(|(id, version)|
            json!({"PackageIdentifier":id,"Version":version})).collect::<Vec<_>>()},
        {"SourceDetails":{"Name":"msstore"},"Packages":[{"PackageIdentifier":"XPFFTQ032PTPHF","Version":"2026.3.3"}]}
    ]}}).to_string()).unwrap();
    let scans = [choco, winget];
    let groups = observations(&scans);
    assert_eq!(groups.len(), 14);
    for i in 0..7 {
        assert_eq!(groups[i].group, groups[i + 7].group, "pair {i}");
    }
    // Version disagreement is retained, not used as evidence of another product.
    assert_eq!(scans[0].packages[6].installed.as_deref(), Some("3.3.6"));
    assert_eq!(scans[1].packages[6].installed.as_deref(), Some("2026.3.3"));
}

#[test]
fn editor_extensions_group_by_canonical_id_not_editor_or_profile() {
    let scans: Vec<_> = Manager::EDITORS.into_iter().map(|manager| parse(manager, false,
        &json!({"profiles":[{"id":"work","name":"Work","installed":"Publisher.Tool@1.0.0\nvendor.other@2.0.0"}]}).to_string()).unwrap()).collect();
    let groups = observations(&scans);
    assert_eq!(groups[0].group, groups[2].group);
    assert_eq!(groups[0].group, groups[4].group);
    assert_ne!(groups[0].group, groups[1].group);
    assert!(groups.iter().all(|g| g.component_reason.is_none()));
}

#[test]
fn store_registrations_and_winget_observations_share_exact_product_identities() {
    let store = parse(Manager::Store, false, &json!([
        {"Name":"Claude","PackageFamilyName":"Claude_pzs8sxrjxfjjc","Version":"3.6.0"},
        {"Name":"Microsoft.WindowsTerminalPreview","PackageFamilyName":"Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe","Version":"1.26.0"},
        {"Name":"OpenAI.Codex","PackageFamilyName":"OpenAI.Codex_2p2nqsd0c76g0","Version":"26.1103.7773.0"}
    ]).to_string()).unwrap();
    let winget = parse(
        Manager::Winget,
        false,
        &json!({"installed":{"Sources":[
            {"SourceDetails":{"Name":"winget"},"Packages":[
                {"PackageIdentifier":"Anthropic.Claude","Version":"3.6.0"},
                {"PackageIdentifier":"Microsoft.WindowsTerminal.Preview","Version":"1.26.0"},
                {"PackageIdentifier":"OpenAI.ChatGPT","Version":"26.1103.7773.0"}
            ]}
        ]}})
        .to_string(),
    )
    .unwrap();
    let groups = observations(&[store, winget]);
    assert_eq!(groups.len(), 6);
    for i in 0..3 {
        assert_eq!(groups[i].group, groups[i + 3].group);
    }
}

#[test]
fn product_grouping_preserves_component_channel_source_and_publisher_boundaries() {
    let mut scan = parse(
        Manager::Winget,
        false,
        &json!({"installed":{"Sources":[
            {"SourceDetails":{"Name":"winget"},"Packages":[
                {"PackageIdentifier":"Microsoft.VisualStudioCode","Version":"1.0"},
                {"PackageIdentifier":"Microsoft.VisualStudioCode.Insiders","Version":"1.0"},
                {"PackageIdentifier":"RedHat.Podman","Version":"5.0"},
                {"PackageIdentifier":"Podman.PodmanDesktop","Version":"1.0"},
                {"PackageIdentifier":"Anysphere.Cursor","Version":"1.0"},
                {"PackageIdentifier":"AnotherPublisher.Cursor","Version":"1.0"}
            ]},
            {"SourceDetails":{"Name":"private-source"},"Packages":[
                {"PackageIdentifier":"Anysphere.Cursor","Version":"1.0"}
            ]}
        ]}})
        .to_string(),
    )
    .unwrap();
    // Names and equal versions do not establish identity.
    for p in &mut scan.packages {
        p.name = "Same display name".into();
    }
    let groups = observations(&[scan]);
    let unique: std::collections::HashSet<_> = groups.iter().map(|g| &g.group).collect();
    assert_eq!(unique.len(), 7);
}

#[test]
fn unknown_product_groups_installations_but_not_unrelated_provider_identities() {
    let mut scan = parse(
        Manager::Chocolatey,
        false,
        &json!({"installed":"unmapped-tool|1.0"}).to_string(),
    )
    .unwrap();
    let mut second = scan.packages[0].clone();
    second.scope = "other user".into();
    second.root = Some("C:/other-install".into());
    second.installed = Some("2.0".into());
    scan.packages.push(second);
    let mut another = scan.packages[0].clone();
    another.id = "another-tool".into();
    another.name = scan.packages[0].name.clone();
    scan.packages.push(another);
    let groups = observations(&[scan]);
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[0].group, groups[1].group);
    assert_ne!(groups[0].group, groups[2].group);
}

#[test]
fn scoop_runtime_rule_includes_extras_without_hiding_custom_bucket_packages() {
    for (source, hidden) in [("main", true), ("extras", true), ("custom", false)] {
        let s = parse(
            Manager::Scoop,
            false,
            &json!({"installed":{"apps":[
                {"Name":"windowsdesktop-runtime","Version":"10.0","Source":source}
            ]}})
            .to_string(),
        )
        .unwrap();
        assert_eq!(component_reason(&s.packages[0]).is_some(), hidden);
    }
}
