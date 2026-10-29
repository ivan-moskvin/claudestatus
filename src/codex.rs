use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::{Outcome, i18n::tf};

const DEFAULT_ITEMS: &[&str] = &["model-with-reasoning", "context-remaining", "current-dir"];
const LIMIT_ITEMS: &[&str] = &["five-hour-limit", "weekly-limit"];

pub fn install() -> Outcome {
    let path = config_path()?;
    if write_config(&path)? {
        println!(
            "{}",
            tf!("Codex limit indicators enabled in {0}", path.display())
        );
        if backup_path(&path).exists() {
            println!(
                "{}",
                tf!("The previous settings are in {0}.bak", path.display())
            );
        }
    } else {
        println!(
            "{}",
            tf!(
                "Codex limit indicators are already enabled in {0}",
                path.display()
            )
        );
    }
    println!(
        "{}",
        crate::i18n::t(
            "Restart Codex CLI to apply. These are native Codex limits; custom Claude bars and Divoom snapshots are not available through this integration."
        )
    );
    Ok(())
}

fn config_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("CODEX_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .ok_or_else(|| tf!("could not determine our own path: {0}", "no home directory"))?;
    Ok(home.join("config.toml"))
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

fn write_config(path: &Path) -> Result<bool, String> {
    let original = match std::fs::read_to_string(path) {
        Ok(data) => Some(data),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(tf!("could not read {0}: {1}", path.display(), err)),
    };
    let updated = enable_limits(original.as_deref().unwrap_or_default(), path)?;
    if original.as_deref() == Some(updated.as_str()) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| tf!("could not create {0}: {1}", dir.display(), err))?;
    }
    if let Some(data) = original {
        std::fs::write(backup_path(path), data)
            .map_err(|err| tf!("could not back up {0}: {1}", path.display(), err))?;
    }
    std::fs::write(path, updated)
        .map_err(|err| tf!("could not write {0}: {1}", path.display(), err))?;
    Ok(true)
}

fn enable_limits(input: &str, path: &Path) -> Result<String, String> {
    let mut config = input
        .parse::<DocumentMut>()
        .map_err(|_| tf!("{0} does not parse — fix it by hand", path.display()))?;
    if !config.contains_key("tui") {
        config["tui"] = Item::Table(Table::new());
    }
    let invalid = || {
        tf!(
            "{0}: tui must be a table and tui.status_line must be an array of strings",
            path.display()
        )
    };
    let tui = config["tui"].as_table_like_mut().ok_or_else(invalid)?;
    if !tui.contains_key("status_line") {
        let items: Array = DEFAULT_ITEMS.iter().copied().collect();
        tui.insert("status_line", value(items));
    }
    let items = tui
        .get_mut("status_line")
        .and_then(Item::as_array_mut)
        .ok_or_else(invalid)?;
    if items.iter().any(|item| item.as_str().is_none()) {
        return Err(invalid());
    }
    for id in LIMIT_ITEMS {
        if !items.iter().any(|item| item.as_str() == Some(id)) {
            items.push(*id);
        }
    }
    Ok(config.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(input: &str) -> String {
        enable_limits(input, Path::new("config.toml")).unwrap()
    }

    fn items(config: &str) -> Vec<String> {
        let document = config.parse::<DocumentMut>().unwrap();
        document["tui"]["status_line"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn adds_limits_after_default_footer_items() {
        assert_eq!(
            items(&edit("")),
            [
                "model-with-reasoning",
                "context-remaining",
                "current-dir",
                "five-hour-limit",
                "weekly-limit"
            ]
        );
    }

    #[test]
    fn preserves_comments_other_tables_and_existing_order() {
        let input = "# personal config\nmodel = 'example' # chosen model\n\n[tui]\n# footer\nstatus_line = ['git-branch', 'weekly-limit'] # my order\nanimations = false\n\n[tui.model_availability_nux]\nexample = 1\n";
        let output = edit(input);
        assert_eq!(
            items(&output),
            ["git-branch", "weekly-limit", "five-hour-limit"]
        );
        assert!(output.contains("# personal config\nmodel = 'example' # chosen model"));
        assert!(output.contains("# footer\nstatus_line ="));
        assert!(output.contains("# my order\nanimations = false"));
        assert!(output.contains("[tui.model_availability_nux]\nexample = 1"));
        assert_eq!(edit(&output), output);
    }

    #[test]
    fn explicitly_empty_footer_gets_only_requested_indicators() {
        assert_eq!(
            items(&edit("[tui]\nstatus_line = []\n")),
            ["five-hour-limit", "weekly-limit"]
        );
    }

    #[test]
    fn supports_inline_tui_table() {
        let output = edit("tui = { animations = false, status_line = ['weekly-limit'] }\n");
        assert_eq!(items(&output), ["weekly-limit", "five-hour-limit"]);
        assert!(output.contains("animations = false"));
    }

    #[test]
    fn refuses_invalid_config_and_wrong_types() {
        for input in [
            "[broken",
            "tui = true",
            "[tui]\nstatus_line = 'model'",
            "[tui]\nstatus_line = [123]",
        ] {
            assert!(
                enable_limits(input, Path::new("config.toml")).is_err(),
                "{input}"
            );
        }
    }

    #[test]
    fn backup_survives_repeat_install_and_invalid_input_is_untouched() {
        let dir = std::env::temp_dir().join(format!("clistatus-codex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let original = "# keep me\nmodel = 'example'\n";
        std::fs::write(&path, original).unwrap();
        assert!(write_config(&path).unwrap());
        assert!(!write_config(&path).unwrap());
        assert_eq!(
            std::fs::read_to_string(backup_path(&path)).unwrap(),
            original
        );
        std::fs::write(&path, "[broken").unwrap();
        assert!(write_config(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[broken");
        assert_eq!(
            std::fs::read_to_string(backup_path(&path)).unwrap(),
            original
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
