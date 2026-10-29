use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Outcome;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub task: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    agent: Agent,
    active: bool,
}

fn clean(s: &str, max: usize) -> String {
    // Discard escape sequences, not merely the ESC byte.
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ('@'..='~').contains(&ch) {
                        break;
                    }
                }
            }
            continue;
        }
        if c.is_control() {
            if c == '\n' || c == '\r' {
                break;
            }
            continue;
        }
        if out.chars().count() >= max {
            break;
        }
        out.push(c);
    }
    out.trim().to_owned()
}
fn field(v: &Value, key: &str, max: usize) -> Option<String> {
    v.get(key)?
        .as_str()
        .map(|s| clean(s, max))
        .filter(|s| !s.is_empty())
}
fn cache_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        let uid = unsafe { libc::geteuid() };
        let path = PathBuf::from(format!("/tmp/clistatus-codex-agents-{uid}"));
        match fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
        let meta = fs::symlink_metadata(&path).ok()?;
        if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o777 != 0o700 {
            return None;
        }
        Some(path)
    }
    #[cfg(not(unix))]
    {
        Some(dirs::data_local_dir()?.join("clistatus-codex/agents"))
    }
}
fn key(id: &str) -> String {
    Sha256::digest(id.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn paths(dir: &Path, session: &str) -> (PathBuf, PathBuf) {
    let hash = key(session);
    (
        dir.join(format!("{hash}.json")),
        dir.join(format!("{hash}.lock")),
    )
}
fn read_entries(path: &Path) -> Vec<Entry> {
    let Ok(mut f) = fs::File::open(path) else {
        return vec![];
    };
    if f.metadata().map(|m| m.len() > 262_144).unwrap_or(true) {
        return vec![];
    }
    let mut s = String::new();
    if f.read_to_string(&mut s).is_err() {
        return vec![];
    }
    let Ok(entries) = serde_json::from_str::<Vec<Entry>>(&s) else {
        return vec![];
    };
    if entries.len() > 100
        || entries.iter().any(|e| {
            let a = &e.agent;
            a.id.is_empty()
                || clean(&a.id, 256) != a.id
                || a.name.is_empty()
                || clean(&a.name, 100) != a.name
                || a.model.as_ref().is_some_and(|s| clean(s, 100) != *s)
                || a.effort.as_ref().is_some_and(|s| clean(s, 30) != *s)
                || a.task.as_ref().is_some_and(|s| clean(s, 140) != *s)
        })
    {
        return vec![];
    }
    entries
}

#[cfg(unix)]
fn lock(file: &fs::File) -> bool {
    use std::os::fd::AsRawFd;
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) == 0 }
}
#[cfg(not(unix))]
fn lock(_: &fs::File) -> bool {
    true
}
fn private_options() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

// Only query the thread itself and verify that it belongs to this parent.
fn lookup(parent: &str, id: &str) -> Option<(Agent, Option<String>)> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|p| p.join(".codex")))?;
    let db = home.join("state_5.sqlite");
    if !db.is_file() {
        return None;
    }
    let quote = |s: &str| s.replace('\'', "''");
    let sql = format!(
        "SELECT id, agent_path, agent_nickname, model, reasoning_effort, name, substr(first_user_message, 1, 512) AS first_user_message FROM threads WHERE (id = '{}' OR agent_path = '{}') AND json_valid(source) AND json_extract(source, '$.subagent.thread_spawn.parent_thread_id') = '{}' LIMIT 1",
        quote(id),
        quote(id),
        quote(parent)
    );
    let output = Command::new("sqlite3")
        .arg("-readonly")
        .arg("-json")
        .arg("-cmd")
        .arg(".timeout 100")
        .arg(db)
        .arg(sql)
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() > 65536 {
        return None;
    }
    let rows: Vec<Value> = serde_json::from_slice(&output.stdout).ok()?;
    let row = rows.first()?;
    let path = field(row, "agent_path", 256);
    let name = path
        .as_deref()
        .and_then(|p| p.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| field(row, "name", 100))
        .or_else(|| field(row, "agent_nickname", 100))
        .unwrap_or_default();
    Some((
        Agent {
            id: field(row, "id", 256)?,
            name,
            model: field(row, "model", 100),
            effort: field(row, "reasoning_effort", 30),
            task: plaintext_field(row, "first_user_message", 140),
        },
        path,
    ))
}
fn object(v: &Value) -> Value {
    if let Some(s) = v.as_str() {
        serde_json::from_str(s).unwrap_or(Value::Null)
    } else {
        v.clone()
    }
}
fn plaintext_field(v: &Value, key: &str, max: usize) -> Option<String> {
    field(v, key, max).filter(|s| !s.starts_with("gAAAA"))
}

fn coalesce(
    entries: &mut Vec<Entry>,
    id: &str,
    db: Option<&(Agent, Option<String>)>,
    pending_name: Option<&str>,
) -> Option<usize> {
    let matches = |e: &Entry| {
        e.agent.id == id
            || db.is_some_and(|(a, path)| {
                e.agent.id == a.id
                    || path.as_ref().is_some_and(|p| e.agent.id == *p)
                    || (!a.name.is_empty() && (e.agent.id == a.name || e.agent.name == a.name))
            })
            || pending_name.is_some_and(|name| e.agent.id == name)
    };
    let first = entries.iter().position(&matches)?;
    let mut i = first + 1;
    while i < entries.len() {
        if matches(&entries[i]) {
            let other = entries.remove(i);
            let target = &mut entries[first];
            target.agent.model = target.agent.model.take().or(other.agent.model);
            target.agent.effort = target.agent.effort.take().or(other.agent.effort);
            target.agent.task = target.agent.task.take().or(other.agent.task);
            target.active |= other.active;
        } else {
            i += 1;
        }
    }
    Some(first)
}

pub fn register_task(name: &str, task: &str) -> Outcome {
    let session = std::env::var("CODEX_THREAD_ID")
        .or_else(|_| std::env::var("CODEX_SESSION_ID"))
        .map_err(|_| "Codex session ID is unavailable".to_string())?;
    if session.is_empty() {
        return Err("Codex session ID is unavailable".into());
    }
    let name = clean(name.rsplit('/').next().unwrap_or(name), 100);
    let task = clean(task, 140);
    if name.is_empty() || task.is_empty() || task.starts_with("gAAAA") {
        return Err("Agent name and plaintext task are required".into());
    }
    let dir = cache_dir().ok_or("Agent cache directory is unavailable")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (path, lock_path) = paths(&dir, &session);
    let guard = private_options()
        .open(lock_path)
        .map_err(|e| e.to_string())?;
    if !lock(&guard) {
        return Err("Cannot lock agent cache".into());
    }
    let mut entries = read_entries(&path);
    if let Some(entry) = entries
        .iter_mut()
        .find(|e| e.agent.name == name || e.agent.id == name)
    {
        entry.agent.task = Some(task);
    } else if entries.len() < 100 {
        entries.push(Entry {
            agent: Agent {
                id: name.clone(),
                name,
                model: None,
                effort: None,
                task: Some(task),
            },
            active: false,
        });
    } else {
        return Err("Agent cache is full".into());
    }
    let data = serde_json::to_vec(&entries).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    let mut f = private_options()
        .truncate(true)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    f.write_all(&data).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}
fn process_event(dir: &Path, event: &Value) {
    let Some(session) = field(event, "session_id", 512) else {
        return;
    };
    let Some(kind) = field(event, "hook_event_name", 64) else {
        return;
    };
    if !matches!(
        kind.as_str(),
        "SubagentStart" | "SubagentStop" | "SessionEnd" | "PostToolUse"
    ) {
        return;
    }
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let (path, lock_path) = paths(dir, &session);
    let Ok(guard) = private_options().open(lock_path) else {
        return;
    };
    if !lock(&guard) {
        return;
    }
    let mut entries = read_entries(&path);
    match kind.as_str() {
        "SessionEnd" => entries.clear(),
        "SubagentStart" | "SubagentStop" => {
            let Some(id) = field(event, "agent_id", 256) else {
                return;
            };
            let db = lookup(&session, &id);
            let found = coalesce(&mut entries, &id, db.as_ref(), None);
            if kind == "SubagentStop" {
                if let Some(i) = found {
                    entries[i].active = false;
                }
            } else {
                let has_db = db.is_some();
                let mut agent = db.map(|(a, _)| a).unwrap_or(Agent {
                    id: id.clone(),
                    name: field(event, "agent_type", 100).unwrap_or_else(|| id.clone()),
                    model: None,
                    effort: None,
                    task: None,
                });
                if agent.name.is_empty() {
                    agent.name = field(event, "agent_type", 100).unwrap_or_else(|| id.clone());
                }
                if let Some(i) = found {
                    let old = &entries[i].agent;
                    if !has_db {
                        agent.name = old.name.clone();
                    }
                    agent.model = agent.model.or_else(|| old.model.clone());
                    agent.effort = agent.effort.or_else(|| old.effort.clone());
                    agent.task = agent.task.or_else(|| old.task.clone());
                    entries[i] = Entry {
                        agent,
                        active: true,
                    };
                } else if entries.len() < 100 {
                    entries.push(Entry {
                        agent,
                        active: true,
                    });
                }
            }
        }
        "PostToolUse" => {
            let Some(tool) = field(event, "tool_name", 128) else {
                return;
            };
            let action = tool.rsplit(['.', ':']).next().unwrap_or("");
            let action = action.strip_prefix("collaboration").unwrap_or(action);
            if !matches!(
                action,
                "spawn_agent" | "Agent" | "followup_task" | "interrupt_agent"
            ) {
                return;
            }
            let input = object(event.get("tool_input").unwrap_or(&Value::Null));
            let response = object(event.get("tool_response").unwrap_or(&Value::Null));
            if response.is_null()
                || response.get("isError").and_then(Value::as_bool) == Some(true)
                || response.get("error").is_some()
            {
                return;
            }
            let id = if action == "spawn_agent" || action == "Agent" {
                field(&response, "agent_id", 256)
                    .or_else(|| field(&response, "agent_name", 256))
                    .or_else(|| field(&response, "task_name", 256))
            } else {
                field(&input, "target", 256)
            };
            let Some(id) = id else { return };
            let db = lookup(&session, &id);
            let pending_name = field(&input, "task_name", 100);
            let found = coalesce(&mut entries, &id, db.as_ref(), pending_name.as_deref());
            if action == "interrupt_agent" {
                if let Some(i) = found {
                    entries[i].active = false;
                }
            } else {
                let mut a = db.map(|(a, _)| a).unwrap_or(Agent {
                    id: id.clone(),
                    name: field(&input, "task_name", 100)
                        .or_else(|| field(&response, "nickname", 100))
                        .unwrap_or_else(|| id.rsplit('/').next().unwrap_or(&id).to_owned()),
                    model: None,
                    effort: None,
                    task: None,
                });
                if a.name.is_empty() {
                    a.name = id.rsplit('/').next().unwrap_or(&id).to_owned();
                }
                a.model = a.model.or_else(|| field(&input, "model", 100));
                a.effort = a.effort.or_else(|| field(&input, "reasoning_effort", 30));
                a.task = plaintext_field(&input, "message", 140)
                    .or_else(|| plaintext_field(&input, "description", 140))
                    .or(a.task);
                if let Some(i) = found {
                    let old = &entries[i];
                    a.model = a.model.or_else(|| old.agent.model.clone());
                    a.effort = a.effort.or_else(|| old.agent.effort.clone());
                    a.task = a.task.or_else(|| old.agent.task.clone());
                    entries[i] = Entry {
                        agent: a,
                        active: old.active || action == "followup_task",
                    };
                } else if action != "followup_task" && entries.len() < 100 {
                    entries.push(Entry {
                        agent: a,
                        active: false,
                    });
                }
            }
        }
        _ => unreachable!(),
    }
    if let Ok(data) = serde_json::to_vec(&entries)
        && data.len() <= 262_144
    {
        let tmp = path.with_extension("tmp");
        if let Ok(mut f) = private_options().truncate(true).open(&tmp)
            && f.write_all(&data).is_ok()
        {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

pub fn hook() -> Outcome {
    let mut input = String::new();
    io::stdin()
        .take(262_145)
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())?;
    if let Ok(event) = serde_json::from_str::<Value>(&input)
        && event.get("hook_event_name").and_then(Value::as_str) == Some("UserPromptSubmit")
        && let Ok(exe) = std::env::current_exe()
    {
        let command = format!("'{}'", exe.to_string_lossy().replace('\'', "'\\''"));
        let context = format!(
            "Before each collaboration.spawn_agent or collaboration.followup_task, use exec_command to run {command} agent-task NAME TASK, passing the agent task_name (or followup target) and a short plaintext description of the actual task as separate shell-quoted arguments. Do this in the parent session before calling the collaboration tool."
        );
        println!(
            "{}",
            serde_json::json!({"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":context}})
        );
        return Ok(());
    }
    if input.len() <= 262_144
        && let (Ok(event), Some(dir)) = (serde_json::from_str::<Value>(&input), cache_dir())
    {
        process_event(&dir, &event);
    }
    println!("{{}}");
    Ok(())
}

pub fn active(session_id: &str) -> Vec<Agent> {
    if session_id.is_empty() {
        return vec![];
    }
    let Some(dir) = cache_dir() else {
        return vec![];
    };
    read_entries(&paths(&dir, session_id).0)
        .into_iter()
        .filter(|e| e.active)
        .map(|e| e.agent)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    #[test]
    fn coalesces_pending_path_and_uuid_and_stops_all_aliases() {
        let agent = |id: &str, task: Option<&str>, active| Entry {
            agent: Agent {
                id: id.into(),
                name: "worker".into(),
                model: None,
                effort: None,
                task: task.map(str::to_owned),
            },
            active,
        };
        let mut entries = vec![
            agent("worker", Some("Inspect the failure"), false),
            agent("/root/worker", None, false),
            agent("uuid", None, true),
        ];
        let db = (
            agent("uuid", None, false).agent,
            Some("/root/worker".into()),
        );
        let i = coalesce(&mut entries, "uuid", Some(&db), None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[i].agent.task.as_deref(),
            Some("Inspect the failure")
        );
        assert!(entries[i].active);
        entries[i].active = false;
        assert!(entries.iter().all(|e| !e.active));
    }
    fn dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "clistatus-agent-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    fn agents(dir: &Path, session: &str) -> Vec<Agent> {
        read_entries(&paths(dir, session).0)
            .into_iter()
            .filter(|e| e.active)
            .map(|e| e.agent)
            .collect()
    }
    #[test]
    fn lifecycle_is_scoped_and_deduplicated() {
        let d = dir();
        let start = json!({"hook_event_name":"SubagentStart", "session_id":"../parent-a", "agent_id":"child", "agent_type":"worker", "model":"parent-model"});
        process_event(&d, &start);
        process_event(&d, &start);
        process_event(
            &d,
            &json!({"hook_event_name":"SubagentStart", "session_id":"parent-b", "agent_id":"child", "agent_type":"worker"}),
        );
        assert_eq!(agents(&d, "../parent-a").len(), 1);
        assert_eq!(agents(&d, "../parent-a")[0].model, None);
        process_event(
            &d,
            &json!({"hook_event_name":"SubagentStop", "session_id":"../parent-a", "agent_id":"child"}),
        );
        assert!(agents(&d, "../parent-a").is_empty());
        assert_eq!(agents(&d, "parent-b").len(), 1);
        process_event(
            &d,
            &json!({"hook_event_name":"SessionEnd", "session_id":"parent-b"}),
        );
        assert!(agents(&d, "parent-b").is_empty());
        let _ = fs::remove_dir_all(d);
    }
    #[test]
    fn metadata_correlates_and_failed_spawn_is_ignored() {
        let d = dir();
        process_event(
            &d,
            &json!({"hook_event_name":"PostToolUse", "session_id":"p-test", "tool_name":"collaboration.spawn_agent", "tool_input":{"task_name":"work", "message":"Inspect\u{001b}[31m code\nsecret", "model":"child-model", "reasoning_effort":"low"}, "tool_response":{"agent_name":"/root/work"}}),
        );
        assert!(agents(&d, "p-test").is_empty());
        process_event(
            &d,
            &json!({"hook_event_name":"SubagentStart", "session_id":"p-test", "agent_id":"/root/work", "agent_type":"worker"}),
        );
        let a = agents(&d, "p-test");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].task.as_deref(), Some("Inspect code"));
        assert_eq!(a[0].model.as_deref(), Some("child-model"));
        process_event(
            &d,
            &json!({"hook_event_name":"PostToolUse", "session_id":"p-test", "tool_name":"spawn_agent", "tool_input":{}, "tool_response":{"error":"failed", "agent_id":"other"}}),
        );
        assert_eq!(read_entries(&paths(&d, "p-test").0).len(), 1);
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn native_task_name_response_enriches_an_already_started_agent() {
        let d = dir();
        process_event(
            &d,
            &json!({"hook_event_name":"SubagentStart", "session_id":"native-parent", "agent_id":"/root/wait_luna_low_60s", "agent_type":"worker"}),
        );
        process_event(
            &d,
            &json!({"hook_event_name":"PostToolUse", "session_id":"native-parent", "tool_name":"collaboration.spawn_agent", "tool_input":json!({"task_name":"wait_luna_low_60s", "message":"оставаться активным 60 секунд для проверки отображения строки статуса", "model":"gpt-6-luna", "reasoning_effort":"low"}).to_string(), "tool_response":json!({"task_name":"/root/wait_luna_low_60s"}).to_string()}),
        );
        let a = agents(&d, "native-parent");
        assert_eq!(a.len(), 1);
        assert_eq!(
            a[0].task.as_deref(),
            Some("оставаться активным 60 секунд для проверки отображения строки статуса")
        );
        assert_eq!(a[0].model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(a[0].effort.as_deref(), Some("low"));
        let _ = fs::remove_dir_all(d);
    }

    #[test]
    fn accepts_native_collaboration_tool_name() {
        let d = dir();
        process_event(
            &d,
            &json!({"hook_event_name":"PostToolUse", "session_id":"native-parent", "tool_name":"collaborationspawn_agent", "tool_input":{"task_name":"worker", "message":"Inspect the failure"}, "tool_response":{"task_name":"/root/worker"}}),
        );
        let entries = read_entries(&paths(&d, "native-parent").0);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].agent.task.as_deref(),
            Some("Inspect the failure")
        );
        let _ = fs::remove_dir_all(d);
    }
}
