use crate::config::{Config, Paths};
use crate::engine::{Engine, VadFactory};
use crate::models::{Catalog, VAD_MODEL_ID};
use crate::postprocess::{PostProcessor, load_dictionary};
use crate::recognizer::SherpaOffline;
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

pub fn load_engine(paths: &Paths, cfg: &Config) -> Result<(Engine, String), BuildError> {
    let catalog = Catalog::load(&paths.catalog_file).map_err(|e| BuildError::Catalog(e.to_string()))?;
    let entry = match &cfg.model {
        Some(id) => catalog.get(id).ok_or_else(|| BuildError::UnknownModel(id.clone()))?,
        None => catalog.default_model().ok_or_else(|| BuildError::Catalog("no default model".into()))?,
    };
    let vad_entry = catalog.get(VAD_MODEL_ID).ok_or_else(|| BuildError::Catalog("no silero-vad entry".into()))?;
    for e in [entry, vad_entry] {
        if !e.is_installed(&paths.models_dir) {
            return Err(BuildError::NoModel { id: e.id.clone(), hint: format!("koeinput model install {}", entry.id) });
        }
    }
    let dict_path = cfg.dictionary.clone().unwrap_or_else(|| paths.dictionary_file.clone());
    let dict = load_dictionary(&dict_path).map_err(|e| BuildError::Load(e.to_string()))?;
    for w in &dict.warnings {
        tracing::warn!("{w}");
    }
    let rec = SherpaOffline::new(entry, &paths.models_dir, effective_threads(cfg.threads))
        .map_err(|e| BuildError::Load(e.to_string()))?;
    let vad_model = vad_entry.file(&paths.models_dir, "model").map_err(|e| BuildError::Load(e.to_string()))?;
    let vad: VadFactory = Arc::new(move || SileroVad::new(&vad_model).map(|v| Box::new(v) as Box<dyn Vad>).map_err(|e| e.to_string()));
    let engine = Engine {
        cfg: SessionConfig::from_config(cfg),
        rec: Arc::new(rec),
        vad,
        post: Arc::new(PostProcessor::new(dict.pairs)),
    };
    Ok((engine, entry.id.clone()))
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
    use super::effective_threads;

    #[test]
    fn threads_below_one_become_one() {
        assert_eq!(effective_threads(0), 1);
        assert_eq!(effective_threads(-3), 1);
        assert_eq!(effective_threads(1), 1);
        assert_eq!(effective_threads(8), 8);
    }
}
