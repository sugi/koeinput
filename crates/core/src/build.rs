use crate::config::{Config, Paths};
use crate::engine::{Engine, VadFactory};
use crate::models::{Catalog, VAD_MODEL_ID};
use crate::postprocess::{PostProcessor, load_dictionary};
use crate::recognizer::{Recognizer, SherpaOffline};
use crate::session::SessionConfig;
use crate::vad::{SileroVad, Vad};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("model catalog: {0}")]
    Catalog(String),
    #[error("unknown model: {0}")]
    UnknownModel(String),
    #[error("model {id} is not installed; run `{hint}`")]
    NoModel { id: String, hint: String },
    #[error("cannot load model: {0}")]
    Load(String),
}

/// Loads the main model and, when configured, the final-stage model.
/// A final-stage model that cannot be loaded is logged; the engine then uses the main model only.
pub fn load_engine(paths: &Paths, cfg: &Config) -> Result<(Engine, String), BuildError> {
    let (mut engine, id) = load_main(paths, cfg)?;
    match load_final(paths, cfg) {
        Ok(rec) => engine.final_rec = rec,
        Err(e) => tracing::warn!("final-stage model: {e}; using {id} only"),
    }
    Ok((engine, id))
}

/// The configured dictionary file, or the default one.
pub fn dictionary_path(paths: &Paths, cfg: &Config) -> std::path::PathBuf {
    cfg.dictionary
        .clone()
        .unwrap_or_else(|| paths.dictionary_file.clone())
}

fn catalog(paths: &Paths) -> Result<Catalog, BuildError> {
    Catalog::load(&paths.catalog_file).map_err(|e| BuildError::Catalog(e.to_string()))
}

/// Whether the files of the configured (or default) main model and the VAD model are installed.
/// A catalog or configuration error counts as not installed; `load_main` reports the cause.
pub fn main_model_installed(paths: &Paths, cfg: &Config) -> bool {
    let Ok(catalog) = catalog(paths) else {
        return false;
    };
    let entry = match &cfg.model {
        Some(id) => catalog.get(id),
        None => catalog.default_model(),
    };
    [entry, catalog.get(VAD_MODEL_ID)]
        .into_iter()
        .all(|e| e.is_some_and(|e| e.is_installed(&paths.models_dir)))
}

/// The engine without the final-stage model.
pub fn load_main(paths: &Paths, cfg: &Config) -> Result<(Engine, String), BuildError> {
    let catalog = catalog(paths)?;
    let entry = match &cfg.model {
        Some(id) => catalog
            .get(id)
            .ok_or_else(|| BuildError::UnknownModel(id.clone()))?,
        None => catalog
            .default_model()
            .ok_or_else(|| BuildError::Catalog("no default model".into()))?,
    };
    let vad_entry = catalog
        .get(VAD_MODEL_ID)
        .ok_or_else(|| BuildError::Catalog("no silero-vad entry".into()))?;
    for e in [entry, vad_entry] {
        if !e.is_installed(&paths.models_dir) {
            return Err(BuildError::NoModel {
                id: e.id.clone(),
                hint: format!("koeinput model install {}", e.id),
            });
        }
    }
    let dict = load_dictionary(&dictionary_path(paths, cfg))
        .map_err(|e| BuildError::Load(e.to_string()))?;
    for w in &dict.warnings {
        tracing::warn!("{w}");
    }
    let rec = SherpaOffline::new(entry, &paths.models_dir, effective_threads(cfg.threads))
        .map_err(|e| BuildError::Load(e.to_string()))?;
    let vad_model = vad_entry
        .file(&paths.models_dir, "model")
        .map_err(|e| BuildError::Load(e.to_string()))?;
    let vad: VadFactory = Arc::new(move || {
        SileroVad::new(&vad_model)
            .map(|v| Box::new(v) as Box<dyn Vad>)
            .map_err(|e| e.to_string())
    });
    let engine = Engine {
        cfg: SessionConfig::from_config(cfg),
        rec: Arc::new(rec),
        final_rec: None,
        vad,
        post: Arc::new(PostProcessor::new(dict.pairs)),
    };
    Ok((engine, entry.id.clone()))
}

/// The configured final-stage recognizer, or `None` when no final-stage model is configured.
pub fn load_final(paths: &Paths, cfg: &Config) -> Result<Option<Arc<dyn Recognizer>>, BuildError> {
    let Some(id) = &cfg.final_model else {
        return Ok(None);
    };
    if cfg.final_full_max_ms == 0 {
        tracing::warn!(
            model = %id,
            "not loading the final-stage model: final_full_max_ms is 0"
        );
        return Ok(None);
    }
    let catalog = catalog(paths)?;
    let entry = catalog
        .get(id)
        .ok_or_else(|| BuildError::UnknownModel(id.clone()))?;
    if entry.kind == "vad" {
        return Err(BuildError::Load(format!(
            "{id} is a VAD model, not a recognizer"
        )));
    }
    if !entry.is_installed(&paths.models_dir) {
        return Err(BuildError::NoModel {
            id: id.clone(),
            hint: format!("koeinput model install {id}"),
        });
    }
    let rec = SherpaOffline::new(entry, &paths.models_dir, effective_threads(cfg.threads))
        .map_err(|e| BuildError::Load(e.to_string()))?;
    Ok(Some(Arc::new(rec)))
}

fn effective_threads(threads: i32) -> i32 {
    if threads < 1 {
        tracing::warn!("threads = {threads} is invalid; using 1");
        return 1;
    }
    threads
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn missing_vad_hint_names_the_vad_model() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_file: tmp.path().join("config.toml"),
            dictionary_file: tmp.path().join("dictionary.toml"),
            models_dir: tmp.path().join("models"),
            catalog_file: Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../data/models.toml"
            ))
            .into(),
        };
        let cfg = Config::default();
        let missing = |p: &Paths| match load_engine(p, &cfg) {
            Err(BuildError::NoModel { id, hint }) => (id, hint),
            other => panic!("expected NoModel, got {:?}", other.err()),
        };
        assert_eq!(
            missing(&paths),
            (
                "parakeet-ja".into(),
                "koeinput model install parakeet-ja".into()
            )
        );

        let catalog = Catalog::load(&paths.catalog_file).unwrap();
        let rec = catalog.default_model().unwrap();
        let dir = rec.dir(&paths.models_dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in rec.files.values() {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        assert_eq!(
            missing(&paths),
            (
                "silero-vad".into(),
                "koeinput model install silero-vad".into()
            )
        );
    }

    fn test_paths(dir: &Path) -> Paths {
        Paths {
            config_file: dir.join("config.toml"),
            dictionary_file: dir.join("dictionary.tsv"),
            models_dir: dir.join("models"),
            catalog_file: Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../data/models.toml"
            ))
            .into(),
        }
    }

    fn final_error(paths: &Paths, id: &str, final_full_max_ms: u64) -> BuildError {
        let cfg = Config {
            final_model: Some(id.into()),
            final_full_max_ms,
            ..Config::default()
        };
        match load_final(paths, &cfg) {
            Err(e) => e,
            Ok(r) => panic!("expected an error, got a recognizer: {}", r.is_some()),
        }
    }

    #[test]
    fn no_final_model_configured_loads_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = test_paths(tmp.path());
        assert!(load_final(&paths, &Config::default()).unwrap().is_none());
    }

    #[test]
    fn final_model_errors_name_the_model() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = test_paths(tmp.path());
        match final_error(&paths, "cohere-transcribe", 30_000) {
            BuildError::NoModel { id, hint } => {
                assert_eq!(id, "cohere-transcribe");
                assert_eq!(hint, "koeinput model install cohere-transcribe");
            }
            e => panic!("{e}"),
        }
        assert!(matches!(
            final_error(&paths, "nope", 30_000),
            BuildError::UnknownModel(id) if id == "nope"
        ));
        let e = final_error(&paths, VAD_MODEL_ID, 30_000).to_string();
        assert!(e.contains("not a recognizer"), "{e}");
    }

    #[test]
    fn final_model_is_skipped_when_the_whole_utterance_decode_is_off() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = test_paths(tmp.path());
        let cfg = Config {
            final_model: Some("cohere-transcribe".into()),
            final_full_max_ms: 0,
            ..Config::default()
        };
        assert!(load_final(&paths, &cfg).unwrap().is_none());
    }

    #[test]
    fn threads_below_one_become_one() {
        assert_eq!(effective_threads(0), 1);
        assert_eq!(effective_threads(-3), 1);
        assert_eq!(effective_threads(1), 1);
        assert_eq!(effective_threads(8), 8);
    }
}
