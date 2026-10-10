//! Read-only native plugin smoke check. Never publishes, creates or merges a PR.
use git_agent_plugin_sdk::{Remote, RepositoryContext, Request, merge_store, permission_allowed};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

fn main() {
    if let Err(error) = probe() {
        eprintln!("Plugin probe failed: {error}");
        std::process::exit(1);
    }
}
fn probe() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let directory = PathBuf::from(
        args.first()
            .ok_or("Usage: git-agent-plugin-probe PLUGIN_DIRECTORY [OWNER/REPO] [PR_NUMBER]")?,
    );
    let plugin = git_agent::plugins::Extension::load(&directory)?;
    if let Some(index) = args.iter().position(|arg| arg == "--match-repository") {
        let root = args.get(index + 1).ok_or("Missing repository directory")?;
        let mut git = std::process::Command::new("git");
        git.args(["-C", root, "config", "--get-regexp", r"^remote\..*\.url$"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            git.creation_flags(0x08000000);
        }
        let result = git.output().map_err(|e| e.to_string())?;
        if !result.status.success() && result.status.code() != Some(1) {
            return Err("Cannot read repository remotes".into());
        }
        let remotes = String::from_utf8(result.stdout)
            .map_err(|e| e.to_string())?
            .lines()
            .filter_map(|line| {
                let (key, url) = line.split_once(char::is_whitespace)?;
                let name = key.strip_prefix("remote.")?.strip_suffix(".url")?;
                Some(Remote {
                    name: name.into(),
                    url: url.trim().into(),
                })
            })
            .collect();
        let context = RepositoryContext {
            root: root.clone(),
            remotes,
            ..Default::default()
        };
        println!(
            "plugin={} repository={} matches={}",
            plugin.manifest.id,
            root,
            plugin.applies_to(&context)
        );
        return Ok(());
    }
    let root = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let repo = args.get(1).cloned().unwrap_or("adoin/git-Agent".into());
    let context = RepositoryContext {
        root,
        language: "zh".into(),
        remotes: vec![Remote {
            name: "origin".into(),
            url: format!("https://github.com/{repo}.git"),
        }],
        ..Default::default()
    };
    let mut request = Request {
        page: String::new(),
        action: "open".into(),
        context,
        inputs: BTreeMap::from([("repo".into(), repo)]),
        state: serde_json::Value::Null,
        store: serde_json::Value::Null,
        event: None,
        confirmed: false,
    };
    let opened = plugin.invoke(&request, None, Arc::new(AtomicBool::new(false)))?;
    if opened.ui.is_none() {
        return Err("Plugin did not return a layout".into());
    }
    for page in &plugin.manifest.contributes.pages {
        if !permission_allowed(&request.store, page.permission.as_deref()) {
            continue;
        }
        let mut page_request = request.clone();
        page_request.page = page.id.clone();
        plugin
            .invoke(&page_request, None, Arc::new(AtomicBool::new(false)))?
            .ui
            .ok_or("Missing page layout")?
            .validate()?;
    }
    request.state = opened.state;
    request.inputs.extend(opened.inputs);
    request.action = "refresh".into();
    let listed = plugin.invoke(&request, None, Arc::new(AtomicBool::new(false)))?;
    println!(
        "plugin={} abi={} pages={} layout=ok list_rows={} pending={} draft={} open={} stars={}",
        plugin.manifest.id,
        plugin.manifest.api_version,
        plugin.manifest.contributes.pages.len(),
        listed.state["rows"].as_array().map_or(0, Vec::len),
        listed.badge.unwrap_or(0),
        listed.state["counts"]["draft"],
        listed.state["counts"]["open"],
        listed.repository_tab_count.unwrap_or(0)
    );
    if let Some(patch) = &listed.store_patch {
        merge_store(&mut request.store, patch);
    }
    if args.iter().any(|arg| arg == "--pr-details") {
        let number = args
            .get(2)
            .filter(|number| number.parse::<u64>().is_ok())
            .ok_or("PR number required")?;
        let mut detail = request.clone();
        detail.page = "prs".into();
        detail.action = "view".into();
        detail.inputs.insert("number".into(), number.clone());
        let response = plugin.invoke(&detail, None, Arc::new(AtomicBool::new(false)))?;
        response
            .ui
            .as_ref()
            .ok_or("Missing detail layout")?
            .validate()?;
        let files = response.state["diff_files"]
            .as_array()
            .ok_or("Missing changed files")?;
        let patches = files
            .iter()
            .filter(|file| {
                file["patch"]
                    .as_str()
                    .is_some_and(|patch| !patch.is_empty())
            })
            .count();
        let diff = response.state["diff"]
            .as_str()
            .ok_or_else(|| format!("Missing PR diff: {}", response.notice))?;
        let headers = diff
            .lines()
            .filter(|line| line.starts_with("diff --git "))
            .count();
        if patches != headers {
            return Err(format!("Parsed {patches}/{headers} file patches"));
        }
        if response.navigate.as_deref() != Some("pr_detail") {
            return Err("Did not enter PR details".into());
        }
        println!(
            "pr={number} details=ok files={} patches={patches} diff_bytes={}",
            files.len(),
            diff.len()
        );
    }
    if args.iter().any(|arg| arg == "--store-access") {
        println!(
            "store_account={} role={} create={} workflow_run={}",
            request.store["account"]["login"],
            request.store["account"]["role"],
            request.store["access"]["create"],
            request.store["access"]["workflow_run"]
        );
        if let Some(number) = args.get(2).filter(|number| number.parse::<u64>().is_ok()) {
            let mut detail = request.clone();
            detail.page = "pr_detail".into();
            detail.action = "view".into();
            detail.inputs.insert("number".into(), number.clone());
            let response = plugin.invoke(&detail, None, Arc::new(AtomicBool::new(false)))?;
            if let Some(patch) = &response.store_patch {
                merge_store(&mut request.store, patch);
            }
            println!(
                "pr_access={} merge={} close={} update={} review={}",
                number,
                request.store["access"]["merge"],
                request.store["access"]["close"],
                request.store["access"]["update"],
                request.store["access"]["review"]
            );
            let mut denied = detail;
            denied.action = "merge".into();
            denied.store = serde_json::json!({"access":{"merge":false}});
            if plugin
                .invoke(&denied, None, Arc::new(AtomicBool::new(false)))
                .is_ok()
            {
                return Err("Store permission dispatch gate was bypassed".into());
            }
            println!("store_permission_dispatch=blocked");
        }
    }
    if args.iter().any(|arg| arg == "--locales") {
        for language in ["zh", "en"] {
            let mut layout = opened.ui.clone().ok_or("Missing layout")?;
            layout.localize(&plugin.dictionary, language);
            if serde_json::to_string(&layout)
                .map_err(|e| e.to_string())?
                .contains("{{i18n:")
            {
                return Err("Unresolved plugin translation".into());
            }
            let title = plugin
                .dictionary
                .localized(&plugin.manifest.contributes.pages[0].title, language);
            println!(
                "locale={language} title={title} open={} draft={} keys={}",
                plugin.dictionary.get("value.open", language),
                plugin.dictionary.get("value.Draft", language),
                plugin.dictionary.0[language].len()
            );
        }
    }
    if args.iter().any(|arg| arg == "--page-sizes") {
        for size in [10, 20, 30, 50] {
            let mut resized = request.clone();
            resized.page = "prs".into();
            resized.action = "page_size_changed".into();
            resized.state = listed.state.clone();
            resized.inputs.extend(listed.inputs.clone());
            resized
                .inputs
                .insert("pr_page_size".into(), size.to_string());
            let response = plugin.invoke(&resized, None, Arc::new(AtomicBool::new(false)))?;
            response
                .ui
                .as_ref()
                .ok_or("Missing paged layout")?
                .validate()?;
            if response.state["page"] != 1 || response.inputs["pr_page_size"] != size.to_string() {
                return Err("Page size did not apply".into());
            }
            println!(
                "page_size={size} page={} pages={} rows={}",
                response.state["page"],
                response.state["pages"],
                response.state["rows"].as_array().map_or(0, Vec::len)
            );
        }
    }
    if args.iter().any(|arg| arg == "--metric-filters") {
        for filter in ["pending", "draft", "open"] {
            let mut filtered_request = request.clone();
            filtered_request.page = "prs".into();
            filtered_request.action = format!("filter_{filter}");
            filtered_request.state = listed.state.clone();
            filtered_request.inputs.extend(listed.inputs.clone());
            let response =
                plugin.invoke(&filtered_request, None, Arc::new(AtomicBool::new(false)))?;
            response
                .ui
                .as_ref()
                .ok_or("Missing filtered page")?
                .validate()?;
            if response.inputs.get("filter").map(String::as_str) != Some(filter)
                || response.state["page"].as_u64() != Some(1)
            {
                return Err(format!("Metric filter did not apply: {filter}"));
            }
            let rows = response.state["rows"]
                .as_array()
                .ok_or("Missing filtered rows")?;
            if rows.iter().any(|row| match filter {
                "pending" => row["kind"] == "Draft",
                "draft" => row["kind"] != "Draft",
                _ => false,
            }) {
                return Err(format!(
                    "Metric filter returned incorrect PR kinds: {filter}"
                ));
            }
            println!(
                "metric={filter} total={} rows={} page=1",
                response.state["total"],
                rows.len()
            );
        }
    }
    if let Some(number) = args.get(2).filter(|s| s.parse::<u64>().is_ok()) {
        request.state = listed.state;
        request.inputs.extend(listed.inputs);
        request.inputs.insert("number".into(), number.clone());
        request.action = "diff".into();
        let detail = plugin.invoke(&request, None, Arc::new(AtomicBool::new(false)))?;
        println!(
            "pr={} head={} diff_bytes={}",
            number,
            detail.state["detail"]["headRefOid"],
            detail.state["diff"].as_str().map_or(0, str::len)
        );
    }
    if args.iter().any(|s| s == "--sync-hook") {
        request.page = "prs".into();
        request.action = "repository.remote_sync.started".into();
        request.event = Some(git_agent_plugin_sdk::RemoteSyncEvent {
            operation_id: 1,
            operation: "fetch".into(),
            source: "probe".into(),
        });
        let response = plugin.invoke(&request, None, Arc::new(AtomicBool::new(false)))?;
        println!(
            "sync_hook=ok rows={} pending={} stars={}",
            response
                .state_patch
                .as_ref()
                .and_then(|state| state["rows"].as_array())
                .map_or(0, Vec::len),
            response.badge.unwrap_or(0),
            response.repository_tab_count.unwrap_or(0)
        );
    }
    if args.iter().any(|s| s == "--resource-pagination") {
        for (page, prefix) in [
            ("issues", "issue"),
            ("actions", "run"),
            ("workflows", "workflow"),
        ] {
            for size in [10, 20, 30, 50] {
                let mut list_request = request.clone();
                list_request.page = page.into();
                list_request.action = format!("{prefix}_page_size_changed");
                list_request
                    .inputs
                    .insert(format!("{prefix}_page_size"), size.to_string());
                let response =
                    plugin.invoke(&list_request, None, Arc::new(AtomicBool::new(false)))?;
                response.ui.ok_or("Missing resource layout")?.validate()?;
                println!(
                    "resource={page} size={size} total={} pages={}",
                    response.state[format!("{prefix}_total")],
                    response.state[format!("{prefix}_pages")]
                );
            }
            let mut hook = request.clone();
            hook.page = page.into();
            hook.action = "repository.remote_sync.started".into();
            let response = plugin.invoke(&hook, None, Arc::new(AtomicBool::new(false)))?;
            if response.state_patch.is_none() {
                return Err("Missing resource sync patch".into());
            }
            println!("resource={page} sync_hook=ok");
        }
    }
    if args.iter().any(|s| s == "--resources") {
        for page in ["issues", "actions", "releases", "repository"] {
            request.page = page.into();
            request.action = "refresh".into();
            let response = plugin.invoke(&request, None, Arc::new(AtomicBool::new(false)))?;
            response.ui.ok_or("Missing resource page")?.validate()?;
            println!("page={page} gh=ok");
        }
    }
    Ok(())
}
