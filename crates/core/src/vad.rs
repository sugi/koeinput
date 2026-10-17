use crate::SAMPLE_RATE;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub start: usize,
    pub end: usize,
}

pub trait Vad: Send {
    fn accept(&mut self, samples: &[f32]);
    fn pop_finished(&mut self) -> Vec<Segment>;
    fn speech_start(&self) -> Option<usize>;
    fn finish(&mut self) -> Vec<Segment>;
}

#[derive(Debug, thiserror::Error)]
pub enum VadError {
    #[error("VAD model: {0}")]
    Model(String),
}

const MIN_SILENCE_S: f32 = 0.4;
const MIN_SPEECH_S: f32 = 0.25;

pub struct SileroVad {
    vad: sherpa_onnx::VoiceActivityDetector,
    fed: usize,
    last_end: usize,
    open_start: Option<usize>,
}

impl SileroVad {
    pub fn new(model: &Path) -> Result<Self, VadError> {
        if !model.exists() {
            return Err(VadError::Model(format!("{} is missing; run `koeinput model install silero-vad`", model.display())));
        }
        let mut c = sherpa_onnx::VadModelConfig::default();
        c.silero_vad.model = Some(model.to_string_lossy().into_owned());
        c.silero_vad.threshold = 0.5;
        c.silero_vad.min_silence_duration = MIN_SILENCE_S;
        c.silero_vad.min_speech_duration = MIN_SPEECH_S;
        c.silero_vad.window_size = 512;
        c.silero_vad.max_speech_duration = 20.0;
        c.sample_rate = SAMPLE_RATE as i32;
        c.num_threads = 1;
        c.provider = Some("cpu".into());
        let vad = sherpa_onnx::VoiceActivityDetector::create(&c, 130.0)
            .ok_or_else(|| VadError::Model("sherpa-onnx could not load the VAD".into()))?;
        Ok(Self { vad, fed: 0, last_end: 0, open_start: None })
    }

    // sherpa-onnx mis-segments when handed a large buffer in one call, so feed window-sized pieces.
    fn feed(&mut self, samples: &[f32]) {
        let lookback = ((MIN_SPEECH_S + 0.3) * SAMPLE_RATE as f32) as usize;
        for c in samples.chunks(512) {
            self.vad.accept_waveform(c);
            self.fed += c.len();
            if self.vad.detected() {
                if self.open_start.is_none() {
                    self.open_start = Some(self.fed.saturating_sub(lookback).max(self.last_end));
                }
            } else {
                self.open_start = None;
            }
        }
    }

    fn drain(&mut self, limit: usize) -> Vec<Segment> {
        let mut out = Vec::new();
        while let Some(seg) = self.vad.front() {
            let start = seg.start().max(0) as usize;
            let end = (start + seg.n().max(0) as usize).min(limit);
            if start < end {
                out.push(Segment { start, end });
                self.last_end = end;
            }
            self.vad.pop();
        }
        out
    }
}

impl Vad for SileroVad {
    fn accept(&mut self, samples: &[f32]) {
        self.feed(samples);
    }

    fn pop_finished(&mut self) -> Vec<Segment> {
        let out = self.drain(self.fed);
        if let (Some(open), Some(last)) = (self.open_start, out.last())
            && open < last.end
        {
            self.open_start = self.vad.detected().then_some(last.end);
        }
        out
    }

    fn speech_start(&self) -> Option<usize> {
        self.open_start
    }

    fn finish(&mut self) -> Vec<Segment> {
        let real = self.fed;
        let pad = vec![0.0f32; ((MIN_SILENCE_S + 0.25) * SAMPLE_RATE as f32) as usize];
        for c in pad.chunks(512) {
            self.vad.accept_waveform(c);
        }
        let out = self.drain(real);
        self.open_start = None;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeVad;

    #[test]
    fn fake_vad_reports_open_and_finished_segments() {
        let mut v = FakeVad::new(vec![Segment { start: 100, end: 200 }, Segment { start: 400, end: 500 }], 50);
        v.accept(&[0.0; 150]);
        assert_eq!(v.speech_start(), Some(100));
        assert!(v.pop_finished().is_empty());
        v.accept(&[0.0; 100]);
        assert_eq!(v.pop_finished(), vec![Segment { start: 100, end: 200 }]);
        assert_eq!(v.speech_start(), None);
        v.accept(&[0.0; 200]);
        assert_eq!(v.speech_start(), Some(400));
        assert_eq!(v.finish(), vec![Segment { start: 400, end: 450 }]);
    }

    #[test]
    fn fake_vad_always_speech() {
        let mut v = FakeVad::always_speech();
        v.accept(&[0.0; 10]);
        assert_eq!(v.speech_start(), Some(0));
        assert_eq!(v.finish(), vec![Segment { start: 0, end: 10 }]);
    }

    fn sample_wav() -> Option<(Vec<f32>, std::path::PathBuf)> {
        if std::env::var("KOEINPUT_MODEL_TESTS").as_deref() != Ok("1") {
            return None;
        }
        let models = std::path::PathBuf::from(std::env::var("KOEINPUT_TEST_MODELS_DIR").unwrap());
        let wav = models.join("parakeet-ja/test_wavs/test_ja_1.wav");
        let speech = crate::audio::file::read_wav_16k_mono(&wav).unwrap();
        Some((speech, models.join("silero-vad/silero_vad.onnx")))
    }

    #[test]
    fn silero_finds_speech_between_silences_when_enabled() {
        let Some((speech, model)) = sample_wav() else { return };
        let mut audio = vec![0.0; 16000];
        audio.extend_from_slice(&speech);
        audio.extend(std::iter::repeat_n(0.0, 16000));
        let mut v = SileroVad::new(&model).unwrap();
        let mut segs = Vec::new();
        for chunk in audio.chunks(320) {
            v.accept(chunk);
            segs.extend(v.pop_finished());
        }
        segs.extend(v.finish());
        assert!(!segs.is_empty());
        assert!(segs.iter().all(|s| s.start < s.end && s.end <= audio.len()));
        assert!(segs[0].start >= 12000, "{segs:?}");
    }

    // The buffer ends inside a second speech run (after a pause), so the reference is the open start
    // a chunked run reports at the same point; chunk granularity allows about two windows of drift.
    #[test]
    fn silero_speech_start_is_independent_of_buffer_size_when_enabled() {
        let Some((speech, model)) = sample_wav() else { return };
        let mut audio = vec![0.0; 16000];
        audio.extend_from_slice(&speech[..speech.len() / 2]);

        let mut chunked = SileroVad::new(&model).unwrap();
        for c in audio.chunks(320) {
            chunked.accept(c);
        }
        let reference = chunked.speech_start().expect("chunked run has an open segment");

        let mut one = SileroVad::new(&model).unwrap();
        one.accept(&audio);
        let s = one.speech_start().expect("single buffer has an open segment");
        assert!(s.abs_diff(reference) <= 1024, "single {s} vs chunked {reference}");
    }

    #[test]
    fn silero_finish_closes_segment_cut_mid_speech_when_enabled() {
        let Some((speech, model)) = sample_wav() else { return };
        let cut = &speech[..speech.len() / 2];
        let mut v = SileroVad::new(&model).unwrap();
        v.accept(cut);
        let mut segs = v.pop_finished();
        segs.extend(v.finish());
        assert!(segs.last().unwrap().end <= cut.len());
        assert!(segs.last().unwrap().end >= cut.len() - 16000);
    }
}
