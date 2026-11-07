use releasescout_engine::{
    catalog,
    inventory::{Manager, parse},
    matching,
};
use serde_json::json;

#[test]
fn registration_profiles_need_publisher_or_exact_homepage_and_survive_new_versions_and_ids() {
    let bindings = matching::bindings();
    let mut scan = parse(Manager::Local, false, &json!({"installed":[{
        "Id":"ARP\\User\\X64\\unfamiliar-registration","Name":"Firefox Developer Edition (x64 en-US)","Version":"190.0","Scope":"User",
        "metadata":{"publisher":"Mozilla"}
    }]}).to_string()).unwrap();
    let p = &mut scan.packages[0];
    assert_eq!(
        matching::resolve(p, &bindings).unwrap().stream.as_deref(),
        Some("developer")
    );
    p.evidence.publisher = Some("Unrelated publisher".into());
    assert!(matching::resolve(p, &bindings).is_none());
    p.name = "WinDirStat 8.9.0".into();
    p.homepage = Some("http://windirstat.info/".into());
    assert_eq!(
        matching::resolve(p, &bindings).unwrap().product,
        "windirstat"
    );
    p.homepage = Some("https://windirstat.info.attacker.example/".into());
    assert!(matching::resolve(p, &bindings).is_none());
    p.name = "Postman Plugin 9.0".into();
    p.evidence.publisher = Some("Postman".into());
    assert!(matching::resolve(p, &bindings).is_none());
}

#[test]
fn bindings_reference_real_products_and_components() {
    matching::validate(&catalog::bundled().unwrap()).unwrap();
    let scan=parse(Manager::Scoop,false,&json!({"installed":{"apps":[{"Name":"supabase","Version":"2.0.0","Source":"main"},{"Name":"unknown-tool","Version":"1.0"}]}}).to_string()).unwrap();
    let bindings = matching::bindings();
    assert_eq!(
        matching::resolve(&scan.packages[0], &bindings)
            .unwrap()
            .stream
            .as_deref(),
        Some("cli")
    );
    assert!(matching::resolve(&scan.packages[1], &bindings).is_none());
    let mut imposter = scan.packages[0].clone();
    imposter.source = "custom-bucket".into();
    assert!(matching::resolve(&imposter, &bindings).is_none());
    imposter.manager = Manager::VscodeInsiders;
    assert!(matching::resolve(&imposter, &bindings).is_none());
}
#[test]
fn snapshots_round_trip_and_replace_without_installing_anything() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inventory.json");
    let scan = parse(Manager::Store, false, "[]").unwrap();
    matching::save_snapshot(&path, &[scan]).unwrap();
    assert_eq!(matching::load_snapshot(&path).unwrap().len(), 1);
    matching::save_snapshot(&path, &[]).unwrap();
    assert!(matching::load_snapshot(&path).unwrap().is_empty());
    std::fs::write(&path, "not JSON").unwrap();
    assert!(matching::load_snapshot(&path).is_err());
}

#[test]
fn bucket_changes_and_store_ids_resolve_only_in_their_reviewed_sources() {
    let bindings = matching::bindings();
    for source in ["main", "extras"] {
        let s = parse(
            Manager::Scoop,
            false,
            &json!({"installed":{"apps":[
                {"Name":"mongosh","Version":"2.0","Source":source}
            ]}})
            .to_string(),
        )
        .unwrap();
        assert_eq!(
            matching::resolve(&s.packages[0], &bindings)
                .unwrap()
                .product,
            "mongosh"
        );
    }
    let s = parse(Manager::Winget, false, &json!({"installed":{"Sources":[
        {"SourceDetails":{"Name":"msstore"},"Packages":[{"PackageIdentifier":"XP89DCGQ3K6VLD","Version":"0.1"}]},
        {"SourceDetails":{"Name":"private-source"},"Packages":[{"PackageIdentifier":"XP89DCGQ3K6VLD","Version":"0.1"}]}
    ]}}).to_string()).unwrap();
    assert_eq!(
        matching::resolve(&s.packages[0], &bindings)
            .unwrap()
            .product,
        "powertoys"
    );
    assert!(matching::resolve(&s.packages[1], &bindings).is_none());
}
