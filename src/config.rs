use std::cell::Cell;
use std::sync::OnceLock;

use glob::Pattern;
use serde::Deserialize;

fn deserialize_pattern<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Pattern, D::Error> {
    let s = String::deserialize(d)?;
    Pattern::new(&s).map_err(serde::de::Error::custom)
}

#[derive(Deserialize)]
pub struct Entry {
    #[serde(deserialize_with = "deserialize_pattern")]
    pub pattern: Pattern,
    pub url: String,
    #[serde(default)]
    pub readahead: usize,
}

pub struct Config {
    pub entries: Vec<Entry>,
}

impl Config {
    pub fn from_str(s: &str) -> Result<Self, String> {
        let entries = serde_json::from_str(s).map_err(|e| e.to_string())?;
        Ok(Config { entries })
    }

    pub fn find(&self, path: &str) -> Option<&Entry> {
        let name = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);
        self.entries.iter().find(|e| e.pattern.matches(name))
    }
}

static CONFIG: OnceLock<Option<Config>> = OnceLock::new();

thread_local! {
    // Guards against re-entering global() from the open() our own config load triggers.
    // Without it, OnceLock::get_or_init deadlocks on the same thread.
    static IN_INIT: Cell<bool> = const { Cell::new(false) };
}

pub fn global() -> Option<&'static Config> {
    if IN_INIT.with(|f| f.get()) {
        return None;
    }
    CONFIG
        .get_or_init(|| {
            IN_INIT.with(|f| f.set(true));
            let result = (|| {
                // Inline JSON wins if set — no temp file needed. Useful for wrappers
                // that construct the config in memory (smugmap-node, @smugmap/cdk).
                if let Ok(json) = std::env::var("SMUGMAP_CONFIG_JSON") {
                    return Config::from_str(&json)
                        .map_err(|e| eprintln!("[smugmap] invalid SMUGMAP_CONFIG_JSON: {e}"))
                        .ok();
                }
                let path = std::env::var("SMUGMAP_CONFIG").ok()?;
                let s = std::fs::read_to_string(&path)
                    .map_err(|e| eprintln!("[smugmap] cannot read config {path}: {e}"))
                    .ok()?;
                Config::from_str(&s)
                    .map_err(|e| eprintln!("[smugmap] invalid config {path}: {e}"))
                    .ok()
            })();
            IN_INIT.with(|f| f.set(false));
            result
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_entry() {
        let json = r#"[{"pattern":"*.db","url":"https://example.com/file.db"}]"#;
        let cfg = Config::from_str(json).unwrap();
        assert_eq!(cfg.entries.len(), 1);
        assert_eq!(cfg.entries[0].url, "https://example.com/file.db");
    }

    #[test]
    fn test_parse_with_readahead() {
        let json = r#"[{"pattern":"*.gguf","url":"https://example.com/m.gguf","readahead":4}]"#;
        let cfg = Config::from_str(json).unwrap();
        assert_eq!(cfg.entries[0].readahead, 4);
    }

    #[test]
    fn test_readahead_default_zero() {
        let json = r#"[{"pattern":"*.db","url":"https://example.com/file.db"}]"#;
        let cfg = Config::from_str(json).unwrap();
        assert_eq!(cfg.entries[0].readahead, 0);
    }

    #[test]
    fn test_match_by_pattern() {
        let json = r#"[{"pattern":"*.db","url":"https://example.com/file.db"}]"#;
        let cfg = Config::from_str(json).unwrap();
        assert!(cfg.find("/remote/analytics.db").is_some());
        assert!(cfg.find("/remote/model.gguf").is_none());
    }

    #[test]
    fn test_invalid_json_returns_err() {
        assert!(Config::from_str("not json").is_err());
    }
}
