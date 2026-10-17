use anyhow::{Result, anyhow};
use koeinput_core::SAMPLE_RATE;
use koeinput_core::build::load_engine;
use koeinput_core::config::{Paths, load};
use koeinput_core::engine::Engine;
use koeinput_core::session::{Event, Session};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn run(paths: &Paths, partials: bool, files: &[PathBuf]) -> Result<()> {
    let cfg = load(&paths.config_file)?;
    let (engine, _) = load_engine(paths, &cfg).map_err(|e| anyhow!("{e}"))?;
    run_batch(files, |f| transcribe_file(&engine, partials, f))
}

/// Runs every file even when some fail, so one bad file does not hide the rest of the batch.
fn run_batch(files: &[PathBuf], mut each: impl FnMut(&Path) -> Result<()>) -> Result<()> {
    let mut failed = 0usize;
    for f in files {
        if let Err(e) = each(f) {
            failed += 1;
            eprintln!("{}: {e:#}", f.display());
        }
    }
    if failed > 0 {
        return Err(anyhow!("{failed} of {} files failed", files.len()));
    }
    Ok(())
}

fn transcribe_file(engine: &Engine, partials: bool, f: &Path) -> Result<()> {
    let samples = koeinput_core::audio::file::read_wav_16k_mono(f)?;
    let vad = (engine.vad)().map_err(|e| anyhow!(e))?;
    let mut s = Session::new(engine.cfg, engine.rec.clone(), vad, engine.post.clone());
    println!("{}", f.display());
    let mut pushed = 0usize;
    let mut final_text = String::new();
    let handle = |events: Vec<Event>, at: usize, final_text: &mut String| -> Result<()> {
        for e in events {
            match e {
                Event::Partial(t) if partials => {
                    println!("  [{:.2} s] {t}", at as f64 / SAMPLE_RATE as f64)
                }
                Event::Final(t) => *final_text = t,
                Event::Error { code, message } => return Err(anyhow!("{code}: {message}")),
                _ => {}
            }
        }
        Ok(())
    };
    for chunk in samples.chunks(320) {
        pushed += chunk.len();
        handle(s.push(chunk), pushed, &mut final_text)?;
    }
    let t = Instant::now();
    let ev = s.end_of_audio();
    let after = t.elapsed();
    handle(ev, pushed, &mut final_text)?;
    println!("  {final_text}");
    println!(
        "  audio {:.2} s, final {:.2} s after end of audio",
        pushed as f64 / SAMPLE_RATE as f64,
        after.as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcribe_continues_after_bad_file() {
        let files: Vec<PathBuf> = ["a.wav", "missing.wav", "b.wav"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let mut seen = Vec::new();
        let r = run_batch(&files, |f| {
            seen.push(f.to_path_buf());
            if f.ends_with("missing.wav") {
                anyhow::bail!("cannot open");
            }
            Ok(())
        });
        assert_eq!(seen, files);
        assert!(r.unwrap_err().to_string().contains("1 of 3"));
    }

    #[test]
    fn batch_without_failures_succeeds() {
        let files = vec![PathBuf::from("a.wav")];
        assert!(run_batch(&files, |_| Ok(())).is_ok());
    }
}
