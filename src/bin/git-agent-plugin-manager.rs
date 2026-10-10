//! CLI counterpart of the native plugin manager, also used for package smoke tests.
use git_agent::plugin_manager::{Storage, catalog};
use git_agent_plugin_sdk::{RepositoryContext, Request};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let root = if args.first().is_some_and(|arg| arg == "--plugin-dir") {
        if args.len() < 3 {
            return Err("--plugin-dir requires a directory and command".into());
        }
        let path = PathBuf::from(args.remove(1));
        args.remove(0);
        path
    } else {
        git_agent::plugin_manager::default_storage().root
    };
    let storage = Storage::new(root);
    let command = args.first().map(String::as_str).unwrap_or("list");
    let arg = |index: usize| {
        args.get(index)
            .map(String::as_str)
            .ok_or("Missing command argument")
    };
    match command {
        "install"=>{storage.install(&PathBuf::from(arg(1)?))?;},
        "enable"=>{storage.set_global(arg(1)?,true)?;},
        "disable"=>{storage.set_global(arg(1)?,false)?;},
        "repository"=>{let enabled=match arg(3)? {"inherit"=>None,"on"=>Some(true),"off"=>Some(false),_=>return Err("Expected inherit/on/off".into())};storage.set_repository(arg(2)?,arg(1)?,enabled)?;},
        "uninstall"=>{storage.uninstall(arg(1)?)?;},
        "apply-pending"=>{storage.activate_pending()?;},
        "list"|"probe"=>{},
        _=>return Err("Commands: list, install PACKAGE, enable ID, disable ID, repository ID PATH inherit|on|off, uninstall ID, apply-pending, probe ID [REPO]".into()),
    }
    let data = catalog(&storage, &[storage.root.join("extensions")], false)?;
    if command == "probe" {
        let id = arg(1)?;
        let root = args.get(2).cloned().unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        });
        if !data.registry.enabled(id, &root) {
            return Err("Plugin is disabled for this scope".into());
        }
        let entry = data
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or("Plugin is not installed")?;
        let plugin = git_agent::plugins::Extension::load(&entry.path)?;
        let request = Request {
            action: "open".into(),
            page: plugin
                .manifest
                .contributes
                .pages
                .first()
                .ok_or("Plugin has no page")?
                .id
                .clone(),
            context: RepositoryContext {
                root,
                language: "zh".into(),
                ..Default::default()
            },
            inputs: BTreeMap::new(),
            state: Default::default(),
            store: Default::default(),
            event: None,
            confirmed: false,
        };
        let response = plugin.invoke(&request, None, Arc::new(AtomicBool::new(false)))?;
        response.ui.ok_or("Plugin returned no layout")?.validate()?;
        println!(
            "{}",
            json!({"id":id,"version":plugin.manifest.version,"layout_valid":true,"api_version":plugin.manifest.api_version})
        );
    } else {
        let entries=data.entries.iter().map(|entry|json!({"id":entry.id,"name":entry.manifest.as_ref().map(|m|m.name.as_str()),"version":entry.manifest.as_ref().map(|m|m.version.as_str()),"enabled":data.registry.enabled(&entry.id,""),"installed":data.registry.installed.get(&entry.id),"error":entry.error,"missing_tools":entry.missing_tools,"path":entry.path})).collect::<Vec<_>>();
        println!(
            "{}",
            json!({"directory":storage.root,"plugins":entries,"registry":data.registry})
        );
    }
    Ok(())
}
