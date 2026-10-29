use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Outcome;

fn config_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .ok_or("Codex home not found")?;
    Ok(home.join("hooks.json"))
}

fn command(renderer: &Path) -> String {
    format!(
        "'{}' agent-hook",
        renderer.to_string_lossy().replace('\'', "'\\''")
    )
}

fn edit(input: &str, command: &str, remove: bool) -> Result<String, String> {
    let mut document: Value = serde_json::from_str(input).map_err(|error| error.to_string())?;
    let root = document
        .as_object_mut()
        .ok_or("hooks.json must be an object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("hooks must be an object")?;
    for (event, matcher) in [
        ("SubagentStart", None),
        ("SubagentStop", None),
        ("SessionEnd", None),
        ("UserPromptSubmit", None),
        (
            "PostToolUse",
            Some("spawn_agent|followup_task|interrupt_agent|list_agents|^Agent$"),
        ),
    ] {
        if remove && !hooks.contains_key(event) {
            continue;
        }
        let groups = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("Hook groups must be arrays")?;
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                handlers.retain(|handler| {
                    handler.get("command").and_then(Value::as_str) != Some(command)
                });
            }
        }
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|handlers| !handlers.is_empty())
        });
        if !remove {
            let mut group = json!({"hooks":[{"type":"command","command":command,"timeout":5}]});
            if let Some(matcher) = matcher {
                group["matcher"] = matcher.into();
            }
            groups.push(group);
        }
    }
    serde_json::to_string_pretty(&document)
        .map(|text| text + "\n")
        .map_err(|error| error.to_string())
}

fn write(renderer: &Path, remove: bool) -> Outcome {
    let path = config_path()?;
    let input = match std::fs::read_to_string(&path) {
        Ok(input) => input,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if remove {
                return Ok(());
            }
            "{}".into()
        }
        Err(error) => return Err(error.to_string()),
    };
    let updated = edit(&input, &command(renderer), remove)?;
    if input == updated {
        return Ok(());
    }
    let backup = path.with_file_name("hooks.json.before-clistatus-agents");
    if path.is_file() && !backup.exists() {
        std::fs::copy(&path, &backup).map_err(|error| error.to_string())?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_file_name(format!("hooks.json.clistatus-{}.tmp", std::process::id()));
    std::fs::write(&temporary, updated).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

pub fn install(renderer: &Path) -> Outcome {
    write(renderer, false)
}
pub fn uninstall(renderer: &Path) -> Outcome {
    write(renderer, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_other_hooks_and_undoes_only_our_handlers() {
        let input = r#"{"description":"mine","hooks":{"SubagentStart":[{"hooks":[{"type":"command","command":"existing"}]}]}}"#;
        let installed = edit(input, "ours agent-hook", false).unwrap();
        assert_eq!(
            edit(&installed, "ours agent-hook", false).unwrap(),
            installed
        );
        let restored: Value =
            serde_json::from_str(&edit(&installed, "ours agent-hook", true).unwrap()).unwrap();
        assert_eq!(restored["description"], "mine");
        assert_eq!(
            restored["hooks"]["SubagentStart"][0]["hooks"][0]["command"],
            "existing"
        );
        assert_eq!(
            restored["hooks"]["SubagentStart"].as_array().unwrap().len(),
            1
        );
    }
}
