use anyhow::{Context, Result, anyhow};
use koeinput_core::SAMPLE_RATE;
use koeinput_core::audio::AudioError;
use koeinput_core::build::load_engine;
use koeinput_core::config::{Paths, load};
use koeinput_core::engine::Engine;
use koeinput_core::session::Event;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn run(paths: &Paths, partials: bool, files: &[PathBuf]) -> Result<()> {
    let cfg = load(&paths.config_file)
        .with_context(|| format!("reading {}", paths.config_file.display()))?;
    let (engine, _) = load_engine(paths, &cfg).map_err(|e| anyhow!("{e}"))?;
    run_batch(files, |f| transcribe_file(&engine, partials, f))
}

/// Why one file failed, and whether the message already starts with its path.
enum FileError {
    Audio(AudioError),
    Other(anyhow::Error),
}

/// Runs every file even when some fail, so one bad file does not hide the rest of the batch.
fn run_batch(
    files: &[PathBuf],
    mut each: impl FnMut(&Path) -> Result<(), FileError>,
) -> Result<()> {
    let mut failed = 0usize;
    for f in files {
        if let Err(e) = each(f) {
            failed += 1;
            eprintln!("{}", failure_line(f, &e));
        }
    }
    if failed > 0 {
        return Err(anyhow!("{failed} of {} files failed", files.len()));
    }
    Ok(())
}

fn failure_line(f: &Path, e: &FileError) -> String {
    match e {
        FileError::Audio(e) => e.to_string(),
        FileError::Other(e) => format!("{}: {e:#}", f.display()),
    }
}

fn transcribe_file(engine: &Engine, partials: bool, f: &Path) -> Result<(), FileError> {
    let samples = koeinput_core::audio::file::read_wav_16k_mono(f).map_err(FileError::Audio)?;
    decode(engine, partials, f, &samples).map_err(FileError::Other)
}

fn decode(engine: &Engine, partials: bool, f: &Path, samples: &[f32]) -> Result<()> {
    let mut s = engine.session().map_err(|e| anyhow!(e))?;
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
    fn failure_line_names_the_file_once() {
        let f = Path::new("x.wav");
        let named = FileError::Audio(AudioError::Io("x.wav: No such file".into()));
        assert_eq!(failure_line(f, &named), "x.wav: No such file");
        let bare = FileError::Other(anyhow!("boom"));
        assert_eq!(failure_line(f, &bare), "x.wav: boom");
    }

    #[test]
    fn failure_line_does_not_mistake_a_short_path_inside_another_message() {
        let f = Path::new("a");
        let other = FileError::Other(anyhow!("cannot start a session"));
        assert_eq!(failure_line(f, &other), "a: cannot start a session");
    }

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
                return Err(FileError::Other(anyhow!("cannot open")));
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
