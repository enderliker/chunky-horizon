//! `config/chunky/config.json`.
//!
//! Chunky's own settings file, kept where Chunky keeps it and spelled the way
//! Chunky spells it, so an admin who already knows the mod finds what they
//! expect. Unknown keys are kept when the file is written back, and a value of
//! the wrong type falls back to the default rather than refusing to start.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The config format's own version, `ChunkyConfig.CONFIG_VERSION`.
pub const CONFIG_VERSION: i32 = 2;
/// The folder the config lives in, under the game's config directory.
pub const CONFIG_DIR: &str = "chunky";
/// The file itself.
pub const CONFIG_FILE: &str = "config.json";

/// What a fresh install starts with.
pub fn default_config() -> Config {
    Config {
        version: CONFIG_VERSION,
        language: "en".to_string(),
        continue_on_restart: false,
        force_load_existing_chunks: false,
        silent: false,
        update_interval: 1,
        player_pause_count: None,
        extra: Map::new(),
    }
}

/// The settings, with the same keys and defaults as Chunky's `config.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
    /// The config format version, written so an older Chunky can migrate.
    pub version: i32,
    /// The language the messages are printed in. Only `en` ships here.
    pub language: String,
    /// Start the tasks that were paused when the game stopped.
    #[serde(rename = "continueOnRestart")]
    pub continue_on_restart: bool,
    /// Load chunks that are already on disk, for every new task by default.
    #[serde(rename = "forceLoadExistingChunks")]
    pub force_load_existing_chunks: bool,
    /// Print nothing but errors.
    pub silent: bool,
    /// Seconds between progress reports, `updateInterval`.
    #[serde(rename = "updateInterval")]
    pub update_interval: u64,
    /// Pause the tasks once this many players are online, `null` for never.
    #[serde(rename = "playerPauseCount")]
    pub player_pause_count: Option<u32>,
    /// Anything else the file had, kept so a round trip does not eat it.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Config {
    fn default() -> Self {
        default_config()
    }
}

impl Config {
    /// Reads the config, falling back to the defaults for a missing file, an
    /// unreadable one, or a value whose type is not what the field wants.
    ///
    /// Chunky's own loader is Gson, which is just as forgiving; a config that
    /// someone hand-edited badly should not stop the server.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return default_config();
        };
        let mut config = default_config();
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return config;
        };
        let Value::Object(map) = value else {
            return config;
        };
        config.merge(map);
        config
    }

    /// Folds a parsed object into this config field by field.
    fn merge(&mut self, map: Map<String, Value>) {
        for (key, value) in map {
            match key.as_str() {
                "version" => self.version = int(&value).unwrap_or(self.version),
                "language" => {
                    if let Some(language) = string(&value) {
                        self.language = language;
                    }
                }
                "continueOnRestart" => {
                    self.continue_on_restart = boolean(&value).unwrap_or(self.continue_on_restart)
                }
                "forceLoadExistingChunks" => {
                    self.force_load_existing_chunks =
                        boolean(&value).unwrap_or(self.force_load_existing_chunks)
                }
                "silent" => self.silent = boolean(&value).unwrap_or(self.silent),
                "updateInterval" => {
                    self.update_interval = int(&value)
                        .map(|v| v.max(1) as u64)
                        .unwrap_or(self.update_interval)
                }
                "playerPauseCount" => {
                    self.player_pause_count = match value {
                        Value::Null => None,
                        other => int(&other).map(|v| v as u32),
                    }
                }
                _ => {
                    self.extra.insert(key, value);
                }
            }
        }
    }

    /// Writes the config, making its folder first.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        std::fs::write(path, text)
    }

    /// The path Chunky's config has, given the game's config directory.
    pub fn path_in(config_dir: &Path) -> PathBuf {
        config_dir.join(CONFIG_DIR).join(CONFIG_FILE)
    }

    /// Whether a task started now should load chunks that already exist.
    pub fn force_load(&self) -> bool {
        self.force_load_existing_chunks
    }
}

fn int(value: &Value) -> Option<i32> {
    value
        .as_i64()
        .map(|v| v as i32)
        .or_else(|| value.as_f64().map(|v| v as i32))
}

fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn boolean(value: &Value) -> Option<bool> {
    value.as_bool()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_chunkys_own() {
        let config = default_config();
        assert_eq!(config.version, 2);
        assert_eq!(config.language, "en");
        assert!(!config.continue_on_restart);
        assert!(!config.force_load_existing_chunks);
        assert!(!config.silent);
        assert_eq!(config.update_interval, 1);
        assert_eq!(config.player_pause_count, None);
    }

    #[test]
    fn a_round_trip_keeps_the_keys() {
        let dir = std::env::temp_dir().join("chunky-horizon-config-round-trip");
        let _ = std::fs::remove_dir_all(&dir);
        let path = Config::path_in(&dir);
        let mut config = default_config();
        config.silent = true;
        config.update_interval = 5;
        config.save(&path).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("continueOnRestart"), "{raw}");
        assert!(raw.contains("forceLoadExistingChunks"), "{raw}");
        assert!(raw.contains("updateInterval"), "{raw}");
        assert!(raw.contains("playerPauseCount"), "{raw}");

        let back = Config::load(&path);
        assert_eq!(back, config);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_keys_survive_a_round_trip() {
        let dir = std::env::temp_dir().join("chunky-horizon-config-unknown");
        let _ = std::fs::remove_dir_all(&dir);
        let path = Config::path_in(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"silent":true,"futureKey":42}"#).unwrap();

        let config = Config::load(&path);
        assert!(config.silent);
        config.save(&path).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("futureKey"), "{raw}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_broken_file_falls_back_instead_of_failing() {
        let dir = std::env::temp_dir().join("chunky-horizon-config-broken");
        let _ = std::fs::remove_dir_all(&dir);
        let path = Config::path_in(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ this is not json").unwrap();
        assert_eq!(Config::load(&path), default_config());

        std::fs::write(&path, r#"{"updateInterval":"nonsense","silent":7}"#).unwrap();
        let config = Config::load(&path);
        assert_eq!(config.update_interval, 1);
        assert!(!config.silent);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_the_default() {
        let path = std::env::temp_dir().join("chunky-horizon-config-absent/config.json");
        assert_eq!(Config::load(&path), default_config());
    }
}
