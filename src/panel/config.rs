//! The settings of the panel: which device, which screen, which port.

use serde::{Deserialize, Serialize};

use crate::i18n::{t, tf};
use crate::paths;

const NAME: &str = "divoom.json";

/// The default port of the local server with the frames.
const DEFAULT_PORT: u16 = 8477;

/// The screen the panel takes unless told otherwise — the rightmost one.
const DEFAULT_SCREEN: u8 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Whether the panel is turned on. `off` clears the flag but keeps the
    /// file: the chosen device and screen have to survive it, or every `on`
    /// would start over from the defaults. A config written before the flag
    /// existed carries no value and counts as on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    /// The last known address of the device. DHCP moves it, so it is a hint for
    /// the search, never the answer.
    #[serde(default)]
    pub ip: String,
    /// Screen 0–4, the one we hand the panel to. The others are left alone.
    #[serde(default = "default_screen")]
    pub lcd_index: u8,
    #[serde(default = "default_port")]
    pub port: u16,
    /// The id of the device in the Divoom cloud — the screen layout is looked
    /// up by it, and it tells our device from the neighbouring ones after a
    /// move.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub device_id: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mac: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// What was on our screen before us. Remembered on the first run and given
    /// back on uninstall — otherwise the screen is left with a dead picture
    /// once the bridge is gone.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub prev_clock_id: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub prev_independence: u32,
}

fn default_screen() -> u8 {
    DEFAULT_SCREEN
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl Default for Config {
    fn default() -> Self {
        Self {
            on: None,
            ip: String::new(),
            lcd_index: DEFAULT_SCREEN,
            port: DEFAULT_PORT,
            device_id: 0,
            mac: String::new(),
            name: String::new(),
            prev_clock_id: 0,
            prev_independence: 0,
        }
    }
}

/// Why there is no config to work with. A file that was never written is told
/// apart from a broken one: the second has to be reported rather than quietly
/// replaced by the defaults.
#[derive(Debug)]
pub enum Missing {
    NeverTurnedOn,
    Broken(String),
}

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Missing::NeverTurnedOn => write!(
                f,
                "{}",
                t("the panel is not turned on — clistatus divoom on")
            ),
            Missing::Broken(what) => write!(f, "{what}"),
        }
    }
}

impl Config {
    pub fn load() -> Result<Config, Missing> {
        let path = paths::file(NAME).map_err(Missing::Broken)?;

        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(Missing::NeverTurnedOn);
            }
            Err(err) => return Err(Missing::Broken(err.to_string())),
        };

        let mut config: Config = serde_json::from_slice(&data)
            .map_err(|err| Missing::Broken(tf!("{0} does not parse: {1}", path.display(), err)))?;
        if config.port == 0 {
            config.port = DEFAULT_PORT;
        }
        Ok(config)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = paths::file(NAME)?;
        let mut data = serde_json::to_vec_pretty(self).map_err(|err| err.to_string())?;
        data.push(b'\n');
        std::fs::write(&path, data)
            .map_err(|err| tf!("could not write {0}: {1}", path.display(), err))
    }

    /// Re-reads the file and lets `change` touch only the fields it means to.
    ///
    /// The bridge lives for hours with a copy of the config in memory, while
    /// the human keeps giving orders — `screen`, `off` — through another
    /// process. Saving the whole copy would put a stale answer back over a
    /// fresh one; the last word belongs to whoever spoke last.
    pub fn update(change: impl FnOnce(&mut Config)) -> Result<(), String> {
        let mut config = Config::load().map_err(|err| err.to_string())?;
        change(&mut config);
        config.save()
    }

    pub fn enabled(&self) -> bool {
        self.on.unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_a_config_from_before_the_flag_as_on() {
        let config: Config =
            serde_json::from_str(r#"{"ip":"192.168.1.5","lcd_index":1,"port":8477}"#).unwrap();
        assert!(config.enabled());
    }

    #[test]
    fn remembers_being_turned_off() {
        let config: Config = serde_json::from_str(r#"{"ip":"192.168.1.5","on":false}"#).unwrap();
        assert!(!config.enabled());
    }

    #[test]
    fn fills_in_what_an_older_config_does_not_say() {
        let config: Config = serde_json::from_str(r#"{"ip":"192.168.1.5"}"#).unwrap();
        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.lcd_index, DEFAULT_SCREEN);
    }

    #[test]
    fn writes_nothing_it_does_not_know() {
        let written = serde_json::to_string(&Config {
            ip: "192.168.1.5".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(!written.contains("device_id"), "{written}");
        assert!(!written.contains("prev_clock_id"), "{written}");
        assert!(!written.contains("\"on\""), "{written}");
    }

    #[test]
    fn keeps_what_it_was_told_across_a_write_and_a_read() {
        let config = Config {
            on: Some(true),
            ip: "192.168.1.5".into(),
            lcd_index: 1,
            port: 4321,
            device_id: 300373815,
            mac: "aa:bb".into(),
            name: "Times Gate".into(),
            prev_clock_id: 104,
            prev_independence: 745289,
        };
        let read: Config = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();

        assert_eq!(read.lcd_index, 1);
        assert_eq!(read.device_id, 300373815);
        assert_eq!(
            read.prev_independence, 745289,
            "the grouping of the screens is not a flag"
        );
    }
}
