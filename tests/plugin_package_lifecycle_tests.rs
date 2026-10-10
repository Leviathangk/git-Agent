use git_agent::plugin_manager::{Storage, catalog, load_enabled};
use git_agent_plugin_sdk::{RepositoryContext, Request, merge_store};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use zip::write::SimpleFileOptions;

#[test]
#[ignore = "requires GIT_AGENT_TEMPLATE_PACKAGE built by the independent template"]
fn real_minimal_template_package_installs_updates_with_loaded_dll_and_uninstalls_on_restart() {
    let package = PathBuf::from(
        std::env::var("GIT_AGENT_TEMPLATE_PACKAGE").expect("set GIT_AGENT_TEMPLATE_PACKAGE"),
    );
    let root = PathBuf::from(
        std::env::var("GIT_AGENT_PACKAGE_TEST_DIR").unwrap_or_else(|_| {
            std::env::temp_dir()
                .join(format!(
                    "git-agent-real-plugin-package-{}",
                    std::process::id()
                ))
                .to_string_lossy()
                .into_owned()
        }),
    );
    fs::create_dir_all(&root).unwrap();
    let storage = Storage::new(root.clone());
    assert!(
        storage.load().unwrap().installed.is_empty(),
        "use a fresh package test directory"
    );
    storage.install(&package).unwrap();
    let data = catalog(&storage, &[], false).unwrap();
    let (plugins, errors) = load_enabled(&data);
    assert!(errors.is_empty());
    assert_eq!(plugins.len(), 1);
    let plugin = plugins[0].clone();
    let id = plugin.manifest.id.clone();
    assert_eq!(
        id, "example",
        "Use the independently packaged minimal example for this write-free fixture"
    );
    let old_version = plugin.manifest.version.clone();
    let mut request = Request {
        action: "open".into(),
        page: "home".into(),
        context: RepositoryContext {
            root: "fixture-repository".into(),
            language: "zh".into(),
            ..Default::default()
        },
        inputs: BTreeMap::new(),
        state: Value::Null,
        store: Value::Null,
        event: None,
        confirmed: false,
    };
    let opened = plugin
        .invoke(&request, None, Arc::new(AtomicBool::new(false)))
        .unwrap();
    opened.ui.unwrap().validate().unwrap();
    merge_store(&mut request.store, opened.store_patch.as_ref().unwrap());
    request.inputs = opened.inputs;
    request.state = opened.state;
    request.action = "increment".into();
    let incremented = plugin
        .invoke(&request, None, Arc::new(AtomicBool::new(false)))
        .unwrap();
    assert_eq!(incremented.state["counter"], 1);
    storage.set_global(&id, false).unwrap();
    let disabled = catalog(&storage, &[], false).unwrap();
    let (loaded, errors) = load_enabled(&disabled);
    assert!(loaded.is_empty() && errors.is_empty());
    storage
        .set_repository("D:/workspace/test-repo", &id, Some(false))
        .unwrap();
    storage.set_global(&id, true).unwrap();
    assert!(
        !storage
            .load()
            .unwrap()
            .enabled(&id, "D:/workspace/test-repo")
    );
    let upgraded = root.join("example-update.gaplugin");
    let mut input = zip::ZipArchive::new(fs::File::open(&package).unwrap()).unwrap();
    let mut output = zip::ZipWriter::new(fs::File::create(&upgraded).unwrap());
    for index in 0..input.len() {
        let mut entry = input.by_index(index).unwrap();
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if ["plugin.json", "package.json"].contains(&name.as_str()) {
            let mut value: Value = serde_json::from_slice(&bytes).unwrap();
            value["version"] = Value::String("0.2.0".into());
            bytes = serde_json::to_vec_pretty(&value).unwrap();
        }
        output
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap();
    let before = fs::read(storage.directory(&id, &old_version).join("plugin.json")).unwrap();
    let pending = storage.install(&upgraded).unwrap();
    assert_eq!(pending.installed[&id].version, old_version);
    assert_eq!(
        pending.installed[&id].pending_version.as_deref(),
        Some("0.2.0")
    );
    assert_eq!(
        fs::read(storage.directory(&id, &old_version).join("plugin.json")).unwrap(),
        before,
        "loaded version must remain untouched"
    );
    drop(plugin);
    drop(plugins);
    let restarted = storage.activate_pending().unwrap();
    assert_eq!(restarted.installed[&id].version, "0.2.0");
    let updated = git_agent::plugins::Extension::load(&storage.directory(&id, "0.2.0")).unwrap();
    assert_eq!(updated.manifest.version, "0.2.0");
    storage.uninstall(&id).unwrap();
    assert!(
        storage.directory(&id, "0.2.0").exists(),
        "uninstall while DLL is held must only schedule removal"
    );
    drop(updated);
    let removed = storage.activate_pending().unwrap();
    assert!(!removed.installed.contains_key(&id));
    assert!(removed.disabled.contains(&id));
    assert!(!root.join("packages").join(&id).exists());
    assert!(upgraded.exists());
    fs::write(root.join("verified.json"),serde_json::to_vec_pretty(&serde_json::json!({"id":id,"initial_version":old_version,"updated_version":"0.2.0","real_dll_invocation":true,"global_and_repository_switches":true,"update_while_dll_loaded":true,"deferred_uninstall":true,"preferences_retained":true})).unwrap()).unwrap();
}
