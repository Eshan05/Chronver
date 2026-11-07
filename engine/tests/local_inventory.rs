use releasescout_engine::{
    inventory::{Manager, parse},
    presentation::{component_reason, observations},
};
use serde_json::json;

fn local(id: &str, name: &str) -> releasescout_engine::inventory::Scan {
    parse(Manager::Local, true, &json!({"installed":[{
        "Id":id,"Name":name,"Version":"4.2","Scope":"User","InstallLocation":"Z:/other-machine/App",
        "IconPath":"Z:/other-machine/App/app.exe","metadata":{"publisher":"Unknown vendor"}
    }]}).to_string()).unwrap()
}

#[test]
fn local_presence_has_native_names_scope_unknown_candidates_and_no_installer_claim() {
    let scan = local("ARP\\User\\X64\\new-app", "Unfamiliar Desktop App");
    let p = &scan.packages[0];
    assert_eq!(p.name, "Unfamiliar Desktop App");
    assert_eq!(p.scope, "User");
    assert_eq!(p.root.as_deref(), Some("Z:/other-machine/App"));
    assert_eq!(p.icon_path.as_deref(), Some("Z:/other-machine/App/app.exe"));
    assert_eq!(p.evidence.registration_id.as_deref(), Some(p.id.as_str()));
    assert!(!p.install_origin_known);
    assert!(p.available.is_none());
    assert!(
        p.candidate_note
            .as_deref()
            .unwrap()
            .contains("no update source")
    );
    assert!(!scan.update_check_succeeded);
    assert!(component_reason(p).is_none());
    assert!(
        parse(Manager::Local, false, r#"{"installed":[]}"#)
            .unwrap()
            .packages
            .is_empty()
    );
    assert!(parse(Manager::Local, false, "{}").is_err());
    assert!(
        parse(
            Manager::Local,
            false,
            r#"{"installed":[{"Name":"no identity"}]}"#
        )
        .is_err()
    );
}

#[test]
fn only_verified_registration_joins_local_presence_to_one_existing_product() {
    let id = "ARP\\User\\X64\\new-app";
    let local = local(id, "Unfamiliar Desktop App");
    let winget=parse(Manager::Winget,false,&json!({"installed":{"Sources":[{
        "SourceDetails":{"Name":"winget"},"Packages":[{"PackageIdentifier":"NewVendor.App","Version":"4.2"}]
    }]},"metadata":{"winget/NewVendor.App":{"name":"Unfamiliar Desktop App","registrationId":id}}}).to_string()).unwrap();
    let found = observations(&[local.clone(), winget.clone()]);
    assert_eq!(found[0].group, found[1].group);
    let mut unverified = winget.clone();
    unverified.packages[0].evidence.registration_id = None;
    let found = observations(&[local.clone(), unverified]);
    assert_ne!(
        found[0].group, found[1].group,
        "equal names and versions are not an identity"
    );
    let mut ambiguous = winget.clone();
    ambiguous.packages[0].id = "OtherVendor.App".into();
    let found = observations(&[local, winget, ambiguous]);
    assert_ne!(found[0].group, found[1].group);
    assert_ne!(found[0].group, found[2].group);
}

#[test]
fn local_installer_components_remain_reversible_not_discarded() {
    let mut scan = local("ARP\\Machine\\X86\\helper", "Unknown Helper");
    scan.packages[0].evidence.system_component = Some(true);
    let found = observations(&[scan]);
    assert_eq!(found.len(), 1);
    assert!(found[0].component_reason.is_some());
}
#[test]
fn publisher_scoped_framework_roles_are_portable_but_not_an_application_denylist() {
    for (name, publisher, hidden) in [
        (
            "Microsoft Visual C++ 2038 Redistributable (ARM64)",
            "Microsoft Corporation",
            true,
        ),
        (
            "Microsoft Windows Desktop Runtime - 12.4.17 (x64)",
            "Microsoft Corporation",
            true,
        ),
        (
            "Microsoft .NET Runtime - 14.7.0 (x64)",
            "Microsoft Corporation",
            true,
        ),
        ("Microsoft .NET SDK 14.7.0", "Microsoft Corporation", false),
        ("Runtime Inspector", "Microsoft Corporation", false),
        (
            "Microsoft Visual C++ 2038 Redistributable",
            "Unrelated Publisher",
            false,
        ),
    ] {
        let mut s = local("ARP\\Machine\\X64\\unfamiliar-id", name);
        s.packages[0].evidence.publisher = Some(publisher.into());
        assert_eq!(
            observations(&[s])[0].component_reason.is_some(),
            hidden,
            "{name}"
        );
    }
}
#[test]
fn runtime_role_follows_only_a_verified_physical_receipt_not_an_entire_product_group() {
    let receipt = "ARP\\Machine\\X64\\runtime-receipt";
    let local = local(receipt, "Any Runtime Display Name");
    let mut winget=parse(Manager::Winget,false,&json!({"installed":{"Sources":[{"SourceDetails":{"Name":"winget"},"Packages":[{"PackageIdentifier":"Microsoft.VCRedist.2015+.x64","Version":"4.2"}]}]},"metadata":{"winget/Microsoft.VCRedist.2015+.x64":{"registrationId":receipt}}}).to_string()).unwrap();
    let groups = observations(&[local.clone(), winget.clone()]);
    assert!(groups[0].component_reason.is_some());
    winget.packages[0].evidence.registration_id = None;
    let groups = observations(&[local, winget]);
    assert!(groups[0].component_reason.is_none());
}

#[test]
fn font_payload_evidence_hides_components_without_an_app_name_denylist() {
    for (evidence, hidden) in [(Some(true), true), (Some(false), false), (None, false)] {
        let mut scan = local(
            "ARP\\Machine\\X86\\unfamiliar-registration",
            "Any Product Name",
        );
        scan.packages[0].evidence.font_only = evidence;
        let result = observations(&[scan]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].component_reason.is_some(), hidden);
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use releasescout_engine::inventory::{ProcessRunner, Runner};

    #[test]
    fn appx_resources_use_literal_absolute_and_relative_reference_forms() {
        let helper = include_str!("../src/inventory/appx-presentation.ps1");
        // No installed packages or registry state needed. Resolve against a fixture loader.
        let helper = helper.replace("[ReleaseScoutResources]::Resolve(", "Test-Resource (");
        let script = format!(
            r#"$ErrorActionPreference='Stop';{helper}
function Test-Resource($ref) {{
    if($ref -eq '@{{Fixture_1.0_neutral_pub?ms-resource://Fixture/Resources/AppName}}') {{return 'Localized App'}}
    if($ref -eq '@{{Fixture_1.0_neutral_pub?ms-resource://Fixture/Title}}') {{return 'Localized Title'}}
    return $null
}}
$p=[pscustomobject]@{{Name='Fixture';PackageFullName='Fixture_1.0_neutral_pub'}}
@{{names=@((Resolve-AppxName $p 'Literal App'),(Resolve-AppxName $p 'ms-resource:AppName'),
 (Resolve-AppxName $p 'ms-resource:/Title'),(Resolve-AppxName $p 'ms-resource://Fixture/Title'));
 missing=(Resolve-AppxName $p 'ms-resource:Missing')}}|ConvertTo-Json -Compress
"#
        );
        let output = ProcessRunner::default().run(&script).unwrap();
        assert_eq!(output.code, 0, "{}", output.stderr);
        let result: serde_json::Value = serde_json::from_str(&output.stdout).unwrap();
        assert_eq!(
            result,
            json!({"names":[
                "Literal App",
                "Localized App",
                "Localized Title",
                "Localized Title"
            ],"missing":null})
        );
    }

    #[test]
    fn appx_icons_follow_manifest_qualifiers_and_cannot_escape_the_package_root() {
        let helper = include_str!("../src/inventory/appx-presentation.ps1");
        let script = format!(
            r#"$ErrorActionPreference='Stop';{helper}
function Get-RegisteredIcon($path) {{if($path.EndsWith('App.scale-150.png')) {{return $path}}}}
$p=[pscustomobject]@{{InstallLocation='Z:\other machine\app'}}
[xml]$manifest='<Package><Applications><Application><VisualElements Square44x44Logo="Assets\App.png"/></Application></Applications></Package>'
$valid=Get-AppxIcon $p $manifest
$manifest.Package.Applications.Application.VisualElements.Square44x44Logo='..\escape\App.png'
@{{valid=$valid;escape=(Get-AppxIcon $p $manifest)}}|ConvertTo-Json -Compress
"#
        );
        let out = ProcessRunner::default().run(&script).unwrap();
        assert_eq!(out.code, 0, "{}", out.stderr);
        let result: serde_json::Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(
            result,
            json!({"valid":"Z:\\other machine\\app\\Assets\\App.scale-150.png","escape":null})
        );
    }
}
