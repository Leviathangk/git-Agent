use git_agent_plugin_sdk::{RepositoryContext, Request};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

#[test]
#[ignore = "Run scripts/dev.ps1 -SmokeTestSeconds 5 first to build and stage the DLL"]
fn actual_dynamic_library_entry_layout_errors_and_cancellation() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/dev-plugins/github");
    let plugin = git_agent::plugins::Extension::load(&directory).unwrap();
    let mut request = Request {
        page: String::new(),
        action: "open".into(),
        context: RepositoryContext {
            root: env!("CARGO_MANIFEST_DIR").into(),
            language: "zh".into(),
            remotes: vec![git_agent_plugin_sdk::Remote {
                name: "origin".into(),
                url: "https://github.com/owner/repo.git".into(),
            }],
            ..Default::default()
        },
        inputs: BTreeMap::from([("repo".into(), "owner/repo".into())]),
        state: serde_json::Value::Null,
        store: serde_json::Value::Null,
        event: None,
        confirmed: false,
    };
    let response = plugin
        .invoke(&request, None, Arc::new(AtomicBool::new(false)))
        .unwrap();
    response.ui.unwrap().validate().unwrap();
    request.action = "unknown".into();
    assert!(
        plugin
            .invoke(&request, None, Arc::new(AtomicBool::new(false)))
            .unwrap_err()
            .contains("Unknown")
    );
    assert!(
        plugin
            .invoke(&request, None, Arc::new(AtomicBool::new(true)))
            .unwrap_err()
            .contains("cancelled")
    );

    let mut unmatched = request.clone();
    unmatched.context.remotes[0].url = "https://gitlab.example.com/owner/repo.git".into();
    assert!(
        plugin
            .invoke(&unmatched, None, Arc::new(AtomicBool::new(false)))
            .unwrap_err()
            .contains("does not match")
    );
    let mut denied = request.clone();
    denied.action = "merge".into();
    denied.store = serde_json::json!({"access":{"merge":false}});
    assert!(
        plugin
            .invoke(&denied, None, Arc::new(AtomicBool::new(false)))
            .unwrap_err()
            .contains("permission")
    );
    // A duplicate manifest must be rejected before executing its library entry.
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("plugin-discovery-test-{}", std::process::id()));
    let first = base.join("a");
    let duplicate = base.join("b");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&duplicate).unwrap();
    let manifest: git_agent_plugin_sdk::Manifest =
        serde_json::from_slice(&std::fs::read(directory.join("plugin.json")).unwrap()).unwrap();
    let platform = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let library = &manifest.library[platform];
    for destination in [&first, &duplicate] {
        std::fs::copy(
            directory.join("plugin.json"),
            destination.join("plugin.json"),
        )
        .unwrap();
        std::fs::copy(directory.join(library), destination.join(library)).unwrap();
        let mut icons = manifest
            .contributes
            .pages
            .iter()
            .map(|page| page.toolbar_icon.as_str())
            .filter(|path| !path.is_empty())
            .collect::<std::collections::HashSet<_>>();
        if let Some(toolbar) = &manifest.contributes.toolbar {
            if !toolbar.icon.is_empty() {
                icons.insert(&toolbar.icon);
            }
        }
        if let Some(badge) = &manifest.contributes.repository_tab_badge {
            icons.insert(&badge.icon);
        }
        for icon in &manifest.contributes.icons {
            icons.insert(icon);
        }
        for file in manifest.locales.values() {
            let target = destination.join(file);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(directory.join(file), target).unwrap();
        }
        for icon in icons {
            let target = destination.join(icon);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::copy(directory.join(icon), target).unwrap();
        }
    }
    let (plugins, errors) = git_agent::plugins::discover(&base);
    assert_eq!(plugins.len(), 1);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("Duplicate plugin ID"));
    drop(plugins);
    std::fs::remove_dir_all(&base).unwrap();
}
