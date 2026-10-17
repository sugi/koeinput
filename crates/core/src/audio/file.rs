use super::AudioError;
use crate::SAMPLE_RATE;
use std::path::Path;

/// Every error message starts with the path.
pub fn read_wav_16k_mono(path: &Path) -> Result<Vec<f32>, AudioError> {
    let mut r = hound::WavReader::open(path).map_err(|e| match e {
        hound::Error::IoError(io) => AudioError::Io(format!("{}: {io}", path.display())),
        other => AudioError::Format(format!("{}: {other}", path.display())),
    })?;
    let spec = r.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()
        }
    }
    .map_err(|e| AudioError::Format(format!("{}: {e}", path.display())))?;
    let ch = spec.channels.max(1) as usize;
    let mono: Vec<f32> = interleaved
        .chunks(ch)
        .map(|c| c.iter().sum::<f32>() / ch as f32)
        .collect();
    if spec.sample_rate == SAMPLE_RATE {
        return Ok(mono);
    }
    let rs = sherpa_onnx::LinearResampler::create(spec.sample_rate as i32, SAMPLE_RATE as i32)
        .ok_or_else(|| {
            AudioError::Format(format!(
                "{}: cannot resample from {} Hz",
                path.display(),
                spec.sample_rate
            ))
        })?;
    Ok(rs.resample(&mono, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_wav(path: &Path, rate: u32, channels: u16, secs: f32) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let n = (rate as f32 * secs) as usize;
        for i in 0..n {
            let v =
                ((i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16;
            for _ in 0..channels {
                w.write_sample(v).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    #[test]
    fn passes_through_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        write_wav(&p, 16000, 1, 1.0);
        let s = read_wav_16k_mono(&p).unwrap();
        assert_eq!(s.len(), 16000);
        assert!(s.iter().all(|v| v.abs() <= 1.0));
    }

    #[test]
    fn downmixes_and_resamples_48k_stereo() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("b.wav");
        write_wav(&p, 48000, 2, 1.0);
        let s = read_wav_16k_mono(&p).unwrap();
        assert!((15800..=16200).contains(&s.len()), "{}", s.len());
    }

    #[test]
    fn missing_file_is_io_error() {
        assert!(matches!(
            read_wav_16k_mono(Path::new("/nonexistent.wav")),
            Err(AudioError::Io(_))
        ));
    }

    #[test]
    fn reads_24bit_and_float_wav() {
        let dir = tempfile::tempdir().unwrap();
        let reference = dir.path().join("ref.wav");
        write_wav(&reference, 16000, 1, 0.25);
        let want = read_wav_16k_mono(&reference).unwrap();
        let sine = |i: usize| {
            (i as f32 / 16000.0 * 440.0 * std::f32::consts::TAU).sin() * 8000.0 / 32768.0
        };

        let p24 = dir.path().join("24.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&p24, spec).unwrap();
        for i in 0..want.len() {
            w.write_sample((sine(i) * 8_388_608.0) as i32).unwrap();
        }
        w.finalize().unwrap();

        let pf = dir.path().join("f32.wav");
        let spec = hound::WavSpec {
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
            ..spec
        };
        let mut w = hound::WavWriter::create(&pf, spec).unwrap();
        for i in 0..want.len() {
            w.write_sample(sine(i)).unwrap();
        }
        w.finalize().unwrap();

        for p in [p24, pf] {
            let got = read_wav_16k_mono(&p).unwrap();
            assert_eq!(got.len(), want.len());
            assert!(
                got.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-3),
                "{}",
                p.display()
            );
        }
    }
}
