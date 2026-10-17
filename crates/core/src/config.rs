use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub model: Option<String>,
    pub source: Option<String>,
    pub threads: i32,
    pub partial_interval_ms: u64,
    pub tail_ms: u64,
    pub min_length_ms: u64,
    pub max_length_ms: u64,
    pub dictionary: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: None,
            source: None,
            threads: 8,
            partial_interval_ms: 500,
            tail_ms: 250,
            min_length_ms: 500,
            max_length_ms: 120_000,
            dictionary: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read or write config: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid config: {0}")]
    Parse(String),
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let cfg: Config =
                toml::from_str(&text).map_err(|e| ConfigError::Parse(e.to_string()))?;
            cfg.validate()?;
            Ok(cfg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}

impl Config {
    fn validate(&self) -> Result<(), ConfigError> {
        fn check(key: &str, v: i64, lo: i64, hi: i64) -> Result<(), ConfigError> {
            if (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(ConfigError::Parse(format!(
                    "{key} = {v} is out of range ({lo} to {hi})"
                )))
            }
        }
        let ms = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
        check("threads", self.threads.into(), 1, 256)?;
        check(
            "partial_interval_ms",
            ms(self.partial_interval_ms),
            100,
            10_000,
        )?;
        check("tail_ms", ms(self.tail_ms), 0, 2_000)?;
        check("min_length_ms", ms(self.min_length_ms), 0, 10_000)?;
        check("max_length_ms", ms(self.max_length_ms), 1_000, 600_000)?;
        if self.min_length_ms > self.max_length_ms {
            return Err(ConfigError::Parse(format!(
                "min_length_ms = {} must not exceed max_length_ms = {}",
                self.min_length_ms, self.max_length_ms
            )));
        }
        Ok(())
    }
}

pub fn set_model(path: &Path, model_id: &str) -> Result<(), ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| ConfigError::Parse(e.to_string()))?;
    doc["model"] = toml_edit::value(model_id);
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = parent.unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".tmp{}", std::process::id()));
    let tmp = dir.join(tmp_name);
    let result = std::fs::write(&tmp, doc.to_string()).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(result?)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paths {
    pub config_file: PathBuf,
    pub dictionary_file: PathBuf,
    pub models_dir: PathBuf,
    pub catalog_file: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Self {
        let get = |k: &str| get(k).filter(|v| !v.is_empty());
        let home = PathBuf::from(get("HOME").unwrap_or_else(|| "/".into()));
        let config_home = get("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".config"));
        let data_home = get("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".local/share"));
        let catalog_dir = get("KOEINPUT_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/share/koeinput"));
        let config_dir = config_home.join("koeinput");
        Self {
            config_file: config_dir.join("config.toml"),
            dictionary_file: config_dir.join("dictionary.tsv"),
            models_dir: data_home.join("koeinput/models"),
            catalog_file: catalog_dir.join("models.toml"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(&dir.path().join("none.toml")).unwrap();
        assert_eq!(cfg, Config::default());
        assert_eq!(cfg.threads, 8);
        assert_eq!(cfg.partial_interval_ms, 500);
        assert_eq!(cfg.tail_ms, 250);
        assert_eq!(cfg.min_length_ms, 500);
        assert_eq!(cfg.max_length_ms, 120_000);
    }

    #[test]
    fn values_override_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, "model = \"zipformer-ja\"\nthreads = 4\n").unwrap();
        let cfg = load(&p).unwrap();
        assert_eq!(cfg.model.as_deref(), Some("zipformer-ja"));
        assert_eq!(cfg.threads, 4);
        assert_eq!(cfg.tail_ms, 250);
    }

    #[test]
    fn unknown_key_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, "modle = \"x\"\n").unwrap();
        assert!(matches!(load(&p), Err(ConfigError::Parse(_))));
    }

    #[test]
    fn set_model_keeps_other_keys_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/c.toml");
        set_model(&p, "parakeet-ja").unwrap();
        assert_eq!(load(&p).unwrap().model.as_deref(), Some("parakeet-ja"));
        std::fs::write(&p, "# mine\nthreads = 4\nmodel = \"a\"\n").unwrap();
        set_model(&p, "b").unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("# mine"));
        assert!(text.contains("threads = 4"));
        assert_eq!(load(&p).unwrap().model.as_deref(), Some("b"));
    }

    #[test]
    fn paths_follow_xdg_and_data_dir_override() {
        let vars: HashMap<&str, &str> = [
            ("HOME", "/h"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("KOEINPUT_DATA_DIR", "/repo/data"),
        ]
        .into();
        let p = Paths::from_vars(|k| vars.get(k).map(|v| v.to_string()));
        assert_eq!(p.config_file, PathBuf::from("/cfg/koeinput/config.toml"));
        assert_eq!(
            p.dictionary_file,
            PathBuf::from("/cfg/koeinput/dictionary.tsv")
        );
        assert_eq!(
            p.models_dir,
            PathBuf::from("/h/.local/share/koeinput/models")
        );
        assert_eq!(p.catalog_file, PathBuf::from("/repo/data/models.toml"));
        let p = Paths::from_vars(|k| (k == "HOME").then(|| "/h".to_string()));
        assert_eq!(
            p.config_file,
            PathBuf::from("/h/.config/koeinput/config.toml")
        );
        assert_eq!(
            p.catalog_file,
            PathBuf::from("/usr/share/koeinput/models.toml")
        );
    }

    #[test]
    fn set_model_writes_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/b/c.toml");
        set_model(&p, "x").unwrap();
        assert_eq!(load(&p).unwrap().model.as_deref(), Some("x"));
        set_model(&p, "y").unwrap();
        assert_eq!(load(&p).unwrap().model.as_deref(), Some("y"));
        let names: Vec<_> = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("c.toml")]);
    }

    #[test]
    fn empty_env_values_are_ignored() {
        let vars: HashMap<&str, &str> = [
            ("HOME", "/h"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_DATA_HOME", ""),
            ("KOEINPUT_DATA_DIR", ""),
        ]
        .into();
        let p = Paths::from_vars(|k| vars.get(k).map(|v| v.to_string()));
        assert_eq!(
            p.config_file,
            PathBuf::from("/h/.config/koeinput/config.toml")
        );
        assert_eq!(
            p.models_dir,
            PathBuf::from("/h/.local/share/koeinput/models")
        );
        assert_eq!(
            p.catalog_file,
            PathBuf::from("/usr/share/koeinput/models.toml")
        );
        let p = Paths::from_vars(|k| (k == "HOME").then(String::new));
        assert_eq!(
            p.config_file,
            PathBuf::from("/.config/koeinput/config.toml")
        );
    }

    #[test]
    fn rejects_invalid_timing_values() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        let cases = [
            ("partial_interval_ms = 0", "partial_interval_ms"),
            ("partial_interval_ms = 10001", "partial_interval_ms"),
            ("tail_ms = 2001", "tail_ms"),
            ("max_length_ms = 0", "max_length_ms"),
            ("max_length_ms = 600001", "max_length_ms"),
            ("min_length_ms = 10001", "min_length_ms"),
            (
                "min_length_ms = 3000\nmax_length_ms = 2000",
                "min_length_ms",
            ),
            ("threads = 0", "threads"),
            ("threads = 257", "threads"),
        ];
        for (text, key) in cases {
            std::fs::write(&p, text).unwrap();
            match load(&p) {
                Err(ConfigError::Parse(m)) => assert!(m.contains(key), "{text}: {m}"),
                other => panic!("{text}: {other:?}"),
            }
        }
        std::fs::write(&p, "tail_ms = 0\nmin_length_ms = 0\nthreads = 1\n").unwrap();
        assert!(load(&p).is_ok());
    }
}
