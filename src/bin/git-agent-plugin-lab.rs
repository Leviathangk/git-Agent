//! Explicit, restricted write smoke test for the disposable test-repo merge lab.
use git_agent_plugin_sdk::{Remote, RepositoryContext, Request, Response, merge_store};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

fn apply(request: &mut Request, response: Response) -> Result<Response, String> {
    if !response.error.is_empty() {
        return Err(response.error.clone());
    }
    if let Some(store) = &response.store {
        request.store = store.clone();
    }
    for patch in &response.store_updates {
        merge_store(&mut request.store, patch);
    }
    if let Some(patch) = &response.store_patch {
        merge_store(&mut request.store, patch);
    }
    request.state = response.state.clone();
    request.inputs.extend(response.inputs.clone());
    if let Some(page) = &response.navigate {
        request.page = page.clone();
    }
    Ok(response)
}

fn main() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 4 || !args.iter().any(|arg| arg == "--allow-test-repo-writes") {
        return Err("Usage: git-agent-plugin-lab LAB_ROOT METHOD create|merge|preview|confirm --allow-test-repo-writes".into());
    }
    let root = PathBuf::from(&args[0]);
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(root.join("manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if manifest["source"] != "D:\\workspace\\test-repo" {
        return Err("Not the authorized test-repo lab".into());
    }
    let method = &args[1];
    if !["merge", "squash", "rebase", "conflict"].contains(&method.as_str()) {
        return Err("Invalid lab case".into());
    }
    let case = &manifest["remote_cases"][method];
    let base = case["base"].as_str().ok_or("Missing test base")?;
    let head = case["head"].as_str().ok_or("Missing test head")?;
    if ![base, head]
        .iter()
        .all(|branch| branch.starts_with("codex/merge-validation/"))
    {
        return Err("Write target is outside test branches".into());
    }
    let plugin = git_agent::plugins::Extension::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/dev-plugins/github"),
    )?;
    let invoke = |request: &Request| plugin.invoke(request, None, Arc::new(AtomicBool::new(false)));
    let invoke_apply = |request: &mut Request| {
        let response = invoke(request)?;
        apply(request, response)
    };
    let mut request = Request {
        page: "prs".into(),
        action: "refresh".into(),
        context: RepositoryContext {
            root: manifest["master"].as_str().unwrap().into(),
            language: "zh".into(),
            remotes: vec![Remote {
                name: "origin".into(),
                url: "git@github.com:adoin/test-repo.git".into(),
            }],
            ..Default::default()
        },
        inputs: BTreeMap::from([
            ("repo".into(), "adoin/test-repo".into()),
            (
                "merge_method".into(),
                if method == "conflict" {
                    "merge".into()
                } else {
                    method.clone()
                },
            ),
        ]),
        state: Value::Null,
        store: Value::Null,
        event: None,
        confirmed: false,
    };
    let preview_path = root.join(format!("preview-{method}.json"));
    if args[2] == "confirm" {
        request = serde_json::from_slice(&std::fs::read(&preview_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        request.confirmed = true;
    } else {
        invoke_apply(&mut request)?;
        if args[2] == "create" {
            request.page = "pr_create".into();
            request.action = "create".into();
            request.inputs.extend(BTreeMap::from([
                ("title".into(), format!("合并回归测试：{method}")),
                (
                    "body".into(),
                    "自动化测试用 PR；仅合入专用测试分支。验证 GitHub 插件的预览、权限及合并行为。"
                        .into(),
                ),
                ("base".into(), base.into()),
                ("head".into(), head.into()),
                ("draft".into(), "no".into()),
            ]));
        } else {
            request.inputs.insert(
                "number".into(),
                case["number"]
                    .as_u64()
                    .ok_or("Missing PR number")?
                    .to_string(),
            );
            request.action = "view".into();
            invoke_apply(&mut request)?;
            if request.state["detail"]["baseRefName"] != base
                || request.state["detail"]["headRefName"] != head
            {
                return Err("PR does not belong to this lab".into());
            }
            request.action = "merge".into();
        }
        let preview = invoke_apply(&mut request)?;
        if preview.confirmation.is_none() {
            return Err("Plugin skipped the required write preview".into());
        }
        std::fs::write(&preview_path, serde_json::to_vec_pretty(&request).unwrap())
            .map_err(|e| e.to_string())?;
        if args[2] == "preview" {
            println!("{}", json!({"method":method,"preview":true}));
            return Ok(());
        }
        request.confirmed = true;
    }
    let result = invoke_apply(&mut request)?;
    std::fs::write(
        root.join(format!("plugin-{}-{method}.json", args[2])),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "{}",
        json!({"method":method,"notice":result.notice,"state":result.state["detail"]["state"]})
    );
    Ok(())
}
