use crate::SAMPLE_RATE;
use crate::models::ModelEntry;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum RecognizerError {
    #[error("unsupported model kind: {0}")]
    UnsupportedKind(String),
    #[error("model: {0}")]
    Model(String),
    #[error("decode: {0}")]
    Decode(String),
}

pub trait Recognizer: Send + Sync {
    fn decode(&self, samples: &[f32]) -> Result<String, RecognizerError>;
}

pub struct SherpaOffline {
    inner: sherpa_onnx::OfflineRecognizer,
}

impl SherpaOffline {
    pub fn new(
        entry: &ModelEntry,
        models_dir: &Path,
        threads: i32,
    ) -> Result<Self, RecognizerError> {
        let path = |key: &str| -> Result<Option<String>, RecognizerError> {
            let p = entry
                .file(models_dir, key)
                .map_err(|e| RecognizerError::Model(e.to_string()))?;
            if !p.exists() {
                return Err(RecognizerError::Model(format!(
                    "{} is missing; run `koeinput model install {}`",
                    p.display(),
                    entry.id
                )));
            }
            Ok(Some(p.to_string_lossy().into_owned()))
        };
        let mut c = sherpa_onnx::OfflineRecognizerConfig::default();
        let m = &mut c.model_config;
        match entry.kind.as_str() {
            "nemo_ctc" => {
                m.nemo_ctc.model = path("model")?;
                m.tokens = path("tokens")?;
            }
            "transducer" => {
                m.transducer.encoder = path("encoder")?;
                m.transducer.decoder = path("decoder")?;
                m.transducer.joiner = path("joiner")?;
                m.tokens = path("tokens")?;
            }
            "sense_voice" => {
                m.sense_voice.model = path("model")?;
                m.sense_voice.language = Some("ja".into());
                m.sense_voice.use_itn = true;
                m.tokens = path("tokens")?;
            }
            "qwen3" => {
                m.qwen3_asr.conv_frontend = path("conv_frontend")?;
                m.qwen3_asr.encoder = path("encoder")?;
                m.qwen3_asr.decoder = path("decoder")?;
                m.qwen3_asr.tokenizer = path("tokenizer")?;
            }
            "cohere" => {
                m.cohere_transcribe.encoder = path("encoder")?;
                m.cohere_transcribe.decoder = path("decoder")?;
                m.cohere_transcribe.language = Some("ja".into());
                m.cohere_transcribe.use_punct = true;
                m.cohere_transcribe.use_itn = true;
                m.tokens = path("tokens")?;
            }
            other => return Err(RecognizerError::UnsupportedKind(other.to_string())),
        }
        m.num_threads = threads;
        m.provider = Some("cpu".into());
        c.decoding_method = Some("greedy_search".into());
        let inner = sherpa_onnx::OfflineRecognizer::create(&c).ok_or_else(|| {
            RecognizerError::Model(format!("sherpa-onnx could not load {}", entry.id))
        })?;
        Ok(Self { inner })
    }
}

impl Recognizer for SherpaOffline {
    fn decode(&self, samples: &[f32]) -> Result<String, RecognizerError> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let s = self.inner.create_stream();
        s.accept_waveform(SAMPLE_RATE as i32, samples);
        self.inner.decode(&s);
        s.get_result()
            .map(|r| r.text.trim().to_string())
            .ok_or_else(|| RecognizerError::Decode("sherpa-onnx returned no result".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Catalog;
    use std::path::PathBuf;

    fn catalog() -> Catalog {
        Catalog::load(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/models.toml"
        )))
        .unwrap()
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let mut e = catalog().get("parakeet-ja").unwrap().clone();
        e.kind = "whisper".into();
        assert!(matches!(
            SherpaOffline::new(&e, Path::new("/x"), 1),
            Err(RecognizerError::UnsupportedKind(_))
        ));
    }

    #[test]
    fn missing_model_files_are_reported_before_loading() {
        let e = catalog().get("parakeet-ja").unwrap().clone();
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            SherpaOffline::new(&e, dir.path(), 1),
            Err(RecognizerError::Model(_))
        ));
    }

    #[test]
    fn parakeet_transcribes_sample_when_enabled() {
        if std::env::var("KOEINPUT_MODEL_TESTS").as_deref() != Ok("1") {
            return;
        }
        let models = PathBuf::from(
            std::env::var("KOEINPUT_TEST_MODELS_DIR").expect("KOEINPUT_TEST_MODELS_DIR"),
        );
        let e = catalog().get("parakeet-ja").unwrap().clone();
        let r = SherpaOffline::new(&e, &models, 8).unwrap();
        let wav = e.dir(&models).join("test_wavs/test_ja_1.wav");
        let samples = crate::audio::file::read_wav_16k_mono(&wav).unwrap();
        let text = r.decode(&samples).unwrap();
        assert!(text.chars().count() > 5, "{text}");
    }
}
