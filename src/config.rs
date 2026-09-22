use glob::Pattern;
use serde::Deserialize;

#[derive(Deserialize)]
struct RawEntry {
    pattern: String,
    url: String,
    #[serde(default)]
    readahead: usize,
}

pub struct Entry {
    pub pattern: Pattern,
    pub url: String,
    pub readahead: usize,
}

pub struct Config {
    pub entries: Vec<Entry>,
}

impl Config {
    pub fn from_str(s: &str) -> Result<Self, String> {
        let raw: Vec<RawEntry> = serde_json::from_str(s).map_err(|e| e.to_string())?;
        let entries = raw
            .into_iter()
            .map(|r| {
                Pattern::new(&r.pattern)
                    .map(|p| Entry {
                        pattern: p,
                        url: r.url,
                        readahead: r.readahead,
                    })
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
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

use std::sync::OnceLock;

static CONFIG: OnceLock<Option<Config>> = OnceLock::new();

pub fn global() -> Option<&'static Config> {
    CONFIG
        .get_or_init(|| {
            let path = std::env::var("SMUGMAP_CONFIG").ok()?;
            let s = std::fs::read_to_string(&path)
                .map_err(|e| eprintln!("[smugmap] cannot read config {path}: {e}"))
                .ok()?;
            Config::from_str(&s)
                .map_err(|e| eprintln!("[smugmap] invalid config {path}: {e}"))
                .ok()
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
