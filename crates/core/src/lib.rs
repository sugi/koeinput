pub mod audio;
pub mod build;
pub mod config;
pub mod engine;
pub mod models;
pub mod postprocess;
pub mod recognizer;
pub mod session;
pub mod vad;
#[cfg(any(test, feature = "test-hooks"))]
pub mod testing;

pub const SAMPLE_RATE: u32 = 16000;

#[cfg(test)]
mod link_check {
    #[test]
    fn sherpa_onnx_links() {
        let r = sherpa_onnx::LinearResampler::create(48000, 16000).expect("resampler");
        let out = r.resample(&vec![0.0f32; 4800], true);
        assert!((1500..=1700).contains(&out.len()), "got {}", out.len());
    }

    #[test]
    fn pulse_simple_links() {
        let spec = libpulse_binding::sample::Spec {
            format: libpulse_binding::sample::Format::F32le,
            channels: 1,
            rate: super::SAMPLE_RATE,
        };
        assert!(spec.is_valid());
        let _ = libpulse_simple_binding::Simple::new;
    }
}
