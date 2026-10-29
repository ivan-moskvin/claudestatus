//! The `divoom` subcommand: a panel with the Claude limits on the screen of a
//! Divoom Times Gate.
//!
//! Everything about the device — finding it, serving it frames, giving the
//! screen back — belongs to divoomkit. What is here is ours alone: which device
//! was chosen, what goes on the picture, and the fact that nobody starts the
//! bridge by hand.

pub mod config;
pub mod daemon;
mod render;
pub mod usage;

use std::io::Write;
use std::time::Duration;

use divoomkit::{Bridge, ClockFace, Device, Discover, Error, Pacing, Tick, discover};

use crate::Outcome;
use crate::i18n::{t, tf};
use config::{Config, Missing};

pub use daemon::{ensure_running, stop};

const USAGE: &str = "clistatus divoom — limits panel on the screen of a Divoom Times Gate.

Usage:
  clistatus divoom on [N]       find the Divoom devices and turn the panel on device N
  clistatus divoom off          turn the panel off and give the screen its clock face back
  clistatus divoom              keep the panel updated (works while running)
  clistatus divoom once         send the panel once and exit
  clistatus divoom screen [N]   show or take over screen 1–5
  clistatus divoom preview FILE save a frame to a file without touching the device

The settings are divoom.json in the application directory, created by on.
";

/// The snapshot is rewritten on every call of the status line, that is often:
/// poll briskly, reading a file costs nothing.
const POLL: Duration = Duration::from_secs(5);

/// The device was switched off — the bridge does not hang around forever: it
/// exits, and the status line starts it again once the Times Gate is back.
const GIVE_UP_AFTER: Duration = Duration::from_secs(10 * 60);

pub fn run(args: &[String]) -> Outcome {
    match args.first().map(String::as_str) {
        None => bridge(false),
        Some("once") => bridge(true),
        Some("on") => turn_on(&args[1..]),
        Some("off") => turn_off(),
        Some("screen") => screen(&args[1..]),
        Some("preview") => {
            let Some(file) = args.get(1) else {
                return Err(t("name a file: clistatus divoom preview panel.gif").into());
            };
            let frame = render::draw(&usage::read())?;
            std::fs::write(file, frame.bytes())
                .map_err(|err| tf!("could not write {0}: {1}", file, err))
        }
        Some("help" | "--help" | "-h") => {
            print!("{}", t(USAGE));
            Ok(())
        }
        Some(other) => Err(tf!("unknown command: {0}\n\n{1}", other, t(USAGE))),
    }
}

/// Keeps the panel updated, or sends it once and leaves.
fn bridge(once: bool) -> Outcome {
    let mut config = Config::load().map_err(|err| err.to_string())?;
    if !config.enabled() {
        return Err(Missing::NeverTurnedOn.to_string());
    }

    // The device is looked up before every run: its address comes from DHCP and
    // does not have to be the one of the last time.
    let device = locate(&mut config)?;

    let mut builder = Bridge::builder(device.clone())
        .screen(config.lcd_index)
        .port(config.port);
    if config.prev_clock_id != 0 {
        builder = builder.restore_to(ClockFace {
            id: config.prev_clock_id,
            independence: config.prev_independence,
        });
    }
    let bridge = builder.build().map_err(describe)?;

    // Remember the clock face that was there before we take the screen: come
    // uninstall time there would be nowhere else to get it from.
    if config.prev_clock_id == 0
        && let Some(face) = bridge.restores_to()
    {
        let _ = Config::update(|saved| {
            saved.prev_clock_id = face.id;
            saved.prev_independence = face.independence;
        });
    }

    if once {
        return bridge
            .show(&render::draw(&usage::read())?)
            .map_err(describe);
    }

    // A second bridge to the same device would interleave its frames with the
    // first one.
    daemon::take_lock()?;
    daemon::catch_signals();

    println!(
        "{}",
        tf!(
            "Panel on screen {0} of device {1}, updated every {2}",
            config.lcd_index + 1,
            device.ip(),
            "5s"
        )
    );

    let pacing = Pacing {
        poll: POLL,
        give_up_after: GIVE_UP_AFTER,
        ..Default::default()
    };
    let mut last_usage = String::new();

    let outcome = bridge.run(
        &pacing,
        || {
            // Someone else took over, or the panel was turned off: leave the
            // screen to whoever owns it now.
            if daemon::stopping() || !daemon::owns_lock() {
                return Ok(Tick::Stop);
            }

            let state = usage::read();
            let key = state.usage_key();
            let frame = render::draw(&state).map_err(Error::Malformed)?;

            // Growing percentages are why the panel hangs on the wall at all; a
            // countdown that moved by a minute can wait.
            let urgent = std::mem::replace(&mut last_usage, key.clone()) != key;
            Ok(if urgent {
                Tick::Now(frame)
            } else {
                Tick::WhenIdle(frame)
            })
        },
        |err| eprintln!("{}", describe_ref(err)),
    );

    daemon::drop_lock();
    outcome.map_err(|_| t("the device has been unreachable for too long, leaving").to_string())
}

/// Finds the chosen device again and writes its current address into the
/// config. The known address is tried first — a sweep of the network on every
/// start would be paid for on every run, while the address usually holds.
fn locate(config: &mut Config) -> Result<Device, String> {
    if let Ok(ip) = config.ip.parse() {
        let device = Device::at(ip);
        if device
            .command(serde_json::json!({"Command": "Channel/GetAllConf"}))
            .is_ok()
        {
            return Ok(device);
        }
    }

    let found = discover(Discover::new()).map_err(describe)?;
    // Nobody is at the keyboard here, so a device that is gone is replaced only
    // when the network has exactly one Divoom to offer.
    let picked = pick_known(&found, config)
        .or_else(|| found.first().filter(|_| found.len() == 1).cloned())
        .ok_or_else(|| {
            t("the chosen device is not on the network — choose again: clistatus divoom on")
                .to_string()
        })?;

    remember(config, &picked)?;
    Ok(picked)
}

/// The id and the MAC outlive a change of address, so they are asked first; a
/// config from an older version has neither and only knows the address.
fn pick_known(found: &[Device], config: &Config) -> Option<Device> {
    found
        .iter()
        .find(|device| config.device_id != 0 && device.id() == config.device_id)
        .or_else(|| {
            found
                .iter()
                .find(|device| !config.mac.is_empty() && device.mac() == config.mac)
        })
        .or_else(|| {
            (config.device_id == 0 && config.mac.is_empty())
                .then(|| {
                    found
                        .iter()
                        .find(|device| device.ip().to_string() == config.ip)
                })
                .flatten()
        })
        .cloned()
}

/// Only what the search really learned is written down: what the cloud once
/// told us must not be wiped by a sweep that went without it.
fn remember(config: &mut Config, device: &Device) -> Result<(), String> {
    config.ip = device.ip().to_string();
    if device.id() != 0 {
        config.device_id = device.id();
    }
    if !device.mac().is_empty() {
        config.mac = device.mac().to_string();
    }
    if !device.name().is_empty() {
        config.name = device.name().to_string();
    }

    let (ip, id, mac, name) = (
        config.ip.clone(),
        config.device_id,
        config.mac.clone(),
        config.name.clone(),
    );
    // Only the address is ours to write: the screen and the on/off flag may have
    // been changed by the human while we were looking.
    Config::update(move |saved| {
        saved.ip = ip;
        saved.device_id = id;
        saved.mac = mac;
        saved.name = name;
    })
}

/// Looks for the Divoom devices on the network and turns the panel on.
fn turn_on(args: &[String]) -> Outcome {
    let mut config = match Config::load() {
        Ok(config) => config,
        Err(Missing::NeverTurnedOn) => Config::default(),
        // A broken config is worth a word: silently starting from the defaults
        // would throw away the chosen screen.
        Err(broken) => return Err(broken.to_string()),
    };

    let found = discover(Discover::new()).map_err(describe)?;
    let picked = choose(&found, &config, args)?;

    config.ip = picked.ip().to_string();
    config.device_id = picked.id();
    config.mac = picked.mac().to_string();
    config.name = picked.name().to_string();
    config.on = Some(true);
    config.save()?;

    println!("{}", tf!("Device {0}: {1}", label(&picked), picked.ip()));

    daemon::ensure_running();
    // The bridge writes its pid file a moment after the fork, so a check right
    // away would report failure on a bridge that is in fact starting.
    for _ in 0..30 {
        if daemon::running() {
            println!(
                "{}",
                tf!("The panel is on, screen {0}", config.lcd_index + 1)
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Picks the device to hand the panel to: the number given on the command line
/// wins, then the one chosen before, and a single device on the network needs no
/// choosing at all. Everything else is a question to the human — with twenty
/// devices around, guessing would light up somebody else's screen.
fn choose(found: &[Device], config: &Config, args: &[String]) -> Result<Device, String> {
    if let Some(asked) = args.first() {
        return by_number(found, asked).ok_or_else(|| {
            list(found);
            tf!(
                "the device is a number from 1 to {0}, not {1}",
                found.len(),
                format!("{asked:?}")
            )
        });
    }
    if let Some(known) = pick_known(found, config) {
        return Ok(known);
    }
    if found.len() == 1 {
        return Ok(found[0].clone());
    }

    list(found);
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return Err(
            t("there is more than one device — name the number: clistatus divoom on N").into(),
        );
    }

    print!("{}", tf!("Which one gets the panel? 1–{0}: ", found.len()));
    let _ = std::io::stdout().flush();

    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|err| err.to_string())?;
    let answer = answer.trim();

    by_number(found, answer).ok_or_else(|| {
        tf!(
            "the device is a number from 1 to {0}, not {1}",
            found.len(),
            format!("{answer:?}")
        )
    })
}

fn by_number(found: &[Device], asked: &str) -> Option<Device> {
    let number: usize = asked.parse().ok()?;
    (number >= 1 && number <= found.len()).then(|| found[number - 1].clone())
}

fn list(found: &[Device]) {
    println!("{}", t("Divoom devices on the network:"));
    for (index, device) in found.iter().enumerate() {
        println!("  {}. {} — {}", index + 1, label(device), device.ip());
    }
}

fn label(device: &Device) -> String {
    if device.name().is_empty() {
        return t("Divoom device").into();
    }
    device.name().to_string()
}

fn turn_off() -> Outcome {
    if daemon::running() {
        daemon::stop();
    } else {
        daemon::restore();
    }

    // The config stays: the device and the screen chosen by the human are the
    // settings of the panel, not the state of a running bridge.
    match Config::load() {
        Err(Missing::NeverTurnedOn) => return Ok(()),
        Err(broken) => return Err(broken.to_string()),
        Ok(mut config) => {
            config.on = Some(false);
            config.save()?;
        }
    }

    // A bridge left over from an update or a second copy notices the cleared
    // flag only on its next tick, and until then it keeps drawing. Give it that
    // moment and put the clock face back once more.
    std::thread::sleep(POLL + Duration::from_secs(1));
    daemon::restore();

    println!(
        "{}",
        t("The panel is off, the screen got its clock face back")
    );
    Ok(())
}

fn screen(args: &[String]) -> Outcome {
    let mut config = Config::load().map_err(|err| err.to_string())?;
    let screens = divoomkit::SCREEN_COUNT;

    let Some(asked) = args.first() else {
        println!(
            "{}",
            tf!(
                "The panel is on screen {0} of {1}",
                config.lcd_index + 1,
                screens
            )
        );
        return Ok(());
    };

    let number: u8 = asked
        .parse()
        .ok()
        .filter(|number| (1..=screens).contains(number))
        .ok_or_else(|| {
            tf!(
                "the screen is a number from 1 to {0}, not {1}",
                screens,
                format!("{asked:?}")
            )
        })?;

    if number - 1 == config.lcd_index {
        println!("{}", tf!("The panel is on screen {0} already", number));
        return Ok(());
    }

    // The bridge holds the screen, so the old one is released first — stop waits
    // until the bridge has put the clock face back there.
    let was_running = daemon::running();
    if was_running {
        daemon::stop();
    } else {
        // With no bridge running nobody gives the old screen its clock face
        // back, and the last frame would stay there forever.
        daemon::restore();
    }

    config.lcd_index = number - 1;
    // What was on the new screen we do not know yet — the bridge remembers it at
    // startup.
    config.prev_clock_id = 0;
    config.prev_independence = 0;
    config.save()?;

    println!("{}", tf!("The panel moved to screen {0}", number));
    if was_running {
        daemon::ensure_running();
    }
    Ok(())
}

/// The crate speaks English and knows nothing of catalogs; the wording the user
/// reads is ours to pick.
fn describe(err: Error) -> String {
    describe_ref(&err)
}

fn describe_ref(err: &Error) -> String {
    match err {
        Error::Unreachable(what) => tf!("the device is unreachable: {0}", what),
        Error::Refused(code) => tf!("the device rejected the command: {0}", code),
        Error::Malformed(what) => tf!("the device answered with something unexpected: {0}", what),
        Error::NotFound => t("no Divoom devices are visible on this network").into(),
        Error::FrameNotFetched => t("the device did not take the frame").into(),
        Error::NoSuchScreen(index) => tf!("the device has no screen {0}", index),
        Error::Io(err) => err.to_string(),
    }
}
