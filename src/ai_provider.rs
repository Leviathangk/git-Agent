use serde_json::{Value, json};

/// Let Claude use its model default. Modern models can reject disabled thinking
/// or forced tool choice; validate the returned tool/text payload at the caller.
pub(crate) fn claude_tool_body(
    model: &str,
    system: &str,
    prompt: &str,
    tool: &str,
    description: &str,
    schema: Value,
    max_tokens: u32,
) -> Value {
    json!({
        "model":model.trim(), "max_tokens":max_tokens,
        "system":system, "messages":[{"role":"user","content":prompt}],
        "tools":[{"name":tool,"description":description,"input_schema":schema}],
        "tool_choice":{"type":"auto","disable_parallel_tool_use":true}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn claude_tools_do_not_force_incompatible_thinking_or_sampling_options() {
        for model in [
            "claude-opus-4-8",
            "claude-sonnet-4-5",
            "custom-claude-model",
        ] {
            let body = claude_tool_body(
                model,
                "system",
                "prompt",
                "result",
                "description",
                json!({"type":"object"}),
                8192,
            );
            assert!(body.get("thinking").is_none());
            assert!(body.get("temperature").is_none());
            assert!(body.get("output_config").is_none());
            assert_eq!(body["tool_choice"]["type"], "auto");
            assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
            assert_eq!(body["max_tokens"], 8192);
        }
    }
}
