use anyhow::Result;
use colored::Colorize;

pub fn load_prefix() -> Option<String> {
    let path = crate::config::system_prefix_path();
    if !path.exists() {
        return None;
    }
    std::fs::read_to_string(&path)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Prepend system_prefix.md content to the `system` field of an Anthropic request body.
/// Returns the modified bytes, or the original slice if nothing to inject.
pub fn inject_system(body: &[u8], prefix: &str) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return body.to_vec();
    };

    match value.get("system") {
        Some(serde_json::Value::String(existing)) => {
            let combined = format!("{prefix}\n\n{existing}");
            value["system"] = serde_json::Value::String(combined);
        }
        Some(serde_json::Value::Array(_)) => {
            // system is already a content block array; prepend a text block
            if let Some(arr) = value["system"].as_array_mut() {
                let block = serde_json::json!({
                    "type": "text",
                    "text": prefix
                });
                arr.insert(0, block);
            }
        }
        None | Some(_) => {
            value["system"] = serde_json::Value::String(prefix.to_string());
        }
    }

    serde_json::to_vec(&value).unwrap_or_else(|_| body.to_vec())
}

pub fn show() -> Result<()> {
    let path = crate::config::system_prefix_path();
    if !path.exists() {
        println!("No system prefix configured. Create one:");
        println!("  ctx inject --edit");
        return Ok(());
    }
    let content = std::fs::read_to_string(&path)?;
    println!("{}", "~/.ctx/system_prefix.md".bold());
    println!("{}", "─".repeat(48));
    println!("{content}");
    Ok(())
}

pub fn edit() -> Result<()> {
    crate::config::ensure_dir()?;
    let path = crate::config::system_prefix_path();
    if !path.exists() {
        std::fs::write(&path, DEFAULT_PREFIX)?;
        println!("Created default prefix at {}", path.display());
    }
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string());
    std::process::Command::new(&editor).arg(&path).status()?;
    Ok(())
}

pub fn disable() -> Result<()> {
    let mut config = crate::config::Config::load();
    config.inject_enabled = false;
    config.save()?;
    println!("{} System prompt injection disabled", "✓".green());
    Ok(())
}

pub fn enable() -> Result<()> {
    let mut config = crate::config::Config::load();
    config.inject_enabled = true;
    config.save()?;
    println!("{} System prompt injection enabled", "✓".green());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_prepends_to_string_system() {
        let body = serde_json::to_vec(&serde_json::json!({
            "system": "original instructions",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        let result = inject_system(&body, "prefix text");
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(
            v["system"].as_str().unwrap(),
            "prefix text\n\noriginal instructions"
        );
    }

    #[test]
    fn inject_inserts_block_at_front_of_array_system() {
        let body = serde_json::to_vec(&serde_json::json!({
            "system": [{"type": "text", "text": "existing block"}],
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        let result = inject_system(&body, "prefix text");
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        let arr = v["system"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["text"].as_str().unwrap(), "prefix text");
        assert_eq!(arr[0]["type"].as_str().unwrap(), "text");
        assert_eq!(arr[1]["text"].as_str().unwrap(), "existing block");
    }

    #[test]
    fn inject_sets_system_when_field_missing() {
        let body = serde_json::to_vec(&serde_json::json!({
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        let result = inject_system(&body, "prefix text");
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["system"].as_str().unwrap(), "prefix text");
    }

    #[test]
    fn inject_passthrough_on_invalid_json() {
        let result = inject_system(b"not json", "prefix");
        assert_eq!(result, b"not json");
    }

    #[test]
    fn inject_preserves_other_fields() {
        let body = serde_json::to_vec(&serde_json::json!({
            "model": "claude-sonnet-4-6",
            "max_tokens": 1024,
            "system": "original",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .unwrap();
        let result = inject_system(&body, "prefix");
        let v: serde_json::Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(v["model"].as_str().unwrap(), "claude-sonnet-4-6");
        assert_eq!(v["max_tokens"].as_u64().unwrap(), 1024);
    }
}

const DEFAULT_PREFIX: &str = r#"# Workspace Standards

## Code style
- No em dashes in any output
- Concise responses; avoid restating what was just done
- No trailing summaries after completing a task

## Commits
- Conventional commits: feat/fix/refactor/chore
- Co-authored-by footer when using AI assistance

## Reviews
- Flag security issues (injection, auth, secrets in code) before anything else
- Prefer editing existing files over creating new ones
"#;
