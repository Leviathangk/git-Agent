use serde_json::{Value, json};

/// Output allowance includes reasoning. DeepSeek's current models support 384K;
/// keep other Claude-compatible providers within a broadly supported allowance.
pub(crate) fn merge_output_tokens(model: &str) -> u32 {
    if model.trim().to_ascii_lowercase().starts_with("deepseek") {
        384_000
    } else {
        32_768
    }
}

pub(crate) fn validate_complete_response(response: &Value) -> Result<(), String> {
    let truncated = response["stop_reason"] == "max_tokens"
        || response
            .pointer("/choices/0/finish_reason")
            .is_some_and(|reason| reason == "length");
    if truncated {
        let tokens = response
            .pointer("/usage/output_tokens")
            .or_else(|| response.pointer("/usage/completion_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or_default();
        return Err(format!(
            "AI response reached the output token limit ({tokens} tokens); the answer is incomplete and no partial suggestions were accepted"
        ));
    }
    Ok(())
}

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
    fn reasoning_allowance_and_incomplete_responses() {
        assert_eq!(merge_output_tokens("deepseek-flash"), 384_000);
        assert_eq!(merge_output_tokens("deepseek-v4-flash"), 384_000);
        assert_eq!(merge_output_tokens("claude-opus-4-8"), 32_768);
        assert!(validate_complete_response(&json!({"stop_reason":"max_tokens","usage":{"output_tokens":8192},"content":[{"type":"thinking"}]})).unwrap_err().contains("8192"));
        assert!(
            validate_complete_response(&json!({"choices":[{"finish_reason":"length"}]})).is_err()
        );
        assert!(validate_complete_response(&json!({"stop_reason":"tool_use"})).is_ok());
    }
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
