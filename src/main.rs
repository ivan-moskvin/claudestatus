//! The Claude limits status line, and its own upkeep.
//!
//! With no arguments it behaves as a `statusLine` command for Claude Code: the
//! session JSON arrives on stdin, the status line goes to stdout. Subcommands
//! exist for the human only: register in the settings, check for an update and
//! install it.

mod codex;
mod codex_agent_hooks;
mod codex_agents;
mod codex_launcher;
mod codex_runtime;
mod i18n;
mod install;
mod panel;
mod paths;
mod snapshot;
mod statusline;
mod update;

use i18n::tf;

/// What a subcommand answers with. An error is a message ready to be printed:
/// it is the CLI that speaks to the human, and it speaks the language i18n
/// picked.
pub type Outcome = Result<(), String>;

const USAGE: &str = "clistatus — Claude and Codex limits in your terminal.

Usage:
  clistatus            status line: session JSON on stdin, line on stdout
  clistatus install    register in ~/.claude/settings.json
  clistatus install codex  install the Codex footer bridge for our renderer
  clistatus codex [ARGS]   launch Codex with our colored status line
  clistatus render codex   render Codex session JSON from stdin
  clistatus check      check whether a new version is out
  clistatus update     download the latest version and replace itself
  clistatus uninstall  remove the status line, the cache and the binary
  clistatus divoom     limits panel on a Divoom Times Gate (divoom help)
  clistatus version    print the version
  clistatus help       this help

Environment:
  CLISTATUS_LANG=ru|en         force the interface language
  CLISTATUS_NO_AUTO_UPDATE=1   do not check for updates in the background
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        // The check is started in any case: even empty input means Claude Code
        // is alive and somebody is looking at the line.
        statusline::run(update::available());
        // The panel lives only while the bridge is running. Start it from here:
        // the check takes milliseconds and does nothing without a Times Gate on
        // the network.
        panel::ensure_running();
        update::auto_check();
        return;
    };

    // --install is still understood: old install scripts spell it that way.
    let result: Outcome = match command.as_str() {
        "agent-task" if args.len() == 3 => codex_agents::register_task(&args[1], &args[2]),
        "agent-hook" if args.len() == 1 => codex_agents::hook(),
        "render" if args.get(1).is_some_and(|target| target == "codex") && args.len() == 2 => {
            statusline::run_codex(None);
            Ok(())
        }
        "codex" => codex_runtime::launch(&args[1..]),
        "install" | "--install" => match &args[1..] {
            [] => install::install(),
            [target] if target == "claude" => install::install(),
            [target] if target == "codex" => codex_runtime::install(),
            [target] if target == "codex-native" => codex::install(),
            _ => Err(i18n::t("Usage: clistatus install [claude|codex]").into()),
        },
        "uninstall" if args.get(1).is_some_and(|target| target == "codex") && args.len() == 2 => {
            codex_runtime::uninstall()
        }
        "uninstall" => codex_launcher::uninstall().and_then(|()| install::uninstall()),
        "divoom" => panel::run(&args[1..]),
        "check" => update::check(args.iter().any(|arg| arg == "--quiet")),
        "update" => update::update(),
        "version" | "--version" | "-v" => {
            println!("{}", version());
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print!("{}", i18n::t(USAGE));
            Ok(())
        }
        other => {
            eprint!(
                "{}",
                tf!("Unknown command: {0}\n\n{1}", other, i18n::t(USAGE))
            );
            std::process::exit(2);
        }
    };

    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

/// The version is stamped into a release build by CI. A hand-made build has
/// none — such a build counts as outdated and never lights the update mark up.
pub fn version() -> String {
    match option_env!("CLISTATUS_VERSION") {
        Some(tag) if !tag.is_empty() => tag.to_string(),
        _ => i18n::t("built from source").to_string(),
    }
}
