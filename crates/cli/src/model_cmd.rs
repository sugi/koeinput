use anyhow::{Context, Result, bail};
use koeinput_core::config::{Config, Paths, load, set_final_model, set_model};
use koeinput_core::models::{
    Catalog, InstallSource, ModelEntry, VAD_MODEL_ID, install as install_model,
    remove as remove_model,
};
use std::path::Path;

fn catalog(paths: &Paths) -> Result<Catalog> {
    Catalog::load(&paths.catalog_file)
        .with_context(|| format!("reading {}", paths.catalog_file.display()))
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn entry<'a>(c: &'a Catalog, id: &str) -> Result<&'a ModelEntry> {
    c.get(id)
        .with_context(|| format!("unknown model {id}; see `koeinput model list`"))
}

fn config(paths: &Paths) -> Result<Config> {
    load(&paths.config_file).with_context(|| format!("reading {}", paths.config_file.display()))
}

pub fn list(paths: &Paths) -> Result<()> {
    let c = catalog(paths)?;
    let final_model = match config(paths) {
        Ok(cfg) => cfg.final_model,
        Err(e) => {
            eprintln!("koeinput: {e:#}; listing without the final model mark");
            None
        }
    };
    for m in &c.models {
        let state = if m.is_installed(&paths.models_dir) {
            "installed"
        } else {
            "not installed"
        };
        let default = if m.default { ", default" } else { "" };
        let is_final = if final_model.as_deref() == Some(m.id.as_str()) {
            ", final model"
        } else {
            ""
        };
        println!(
            "{:<20} {:>10}  {}{}{}  {} [{}]",
            m.id,
            mib(m.size),
            state,
            default,
            is_final,
            m.name,
            m.license
        );
    }
    Ok(())
}

fn install_one(paths: &Paths, m: &ModelEntry, file: Option<&Path>) -> Result<()> {
    let source = match file {
        Some(f) => InstallSource::File(f),
        None => InstallSource::Url,
    };
    let mut last = 0u64;
    install_model(m, source, &paths.models_dir, &mut |done, total| {
        if done - last >= 16 << 20 || done == total {
            last = done;
            eprint!("\r{}: {} / {}", m.id, mib(done), mib(total));
        }
    })
    .with_context(|| format!("installing {}", m.id))?;
    eprintln!(
        "\r{}: installed in {}",
        m.id,
        m.dir(&paths.models_dir).display()
    );
    Ok(())
}

pub fn install(paths: &Paths, id: &str, from_file: Option<&Path>) -> Result<()> {
    let c = catalog(paths)?;
    let m = entry(&c, id)?;
    if id != VAD_MODEL_ID {
        let vad = entry(&c, VAD_MODEL_ID)?;
        if !vad.is_installed(&paths.models_dir) {
            install_one(paths, vad, None)?;
        }
    }
    install_one(paths, m, from_file)?;
    if let Ok(cfg) = config(paths) {
        let main = main_model_id(&c, &cfg);
        if let Some(line) = install_hint(id, main, cfg.final_model.as_deref()) {
            println!("{line}");
        }
    }
    Ok(())
}

pub fn remove(paths: &Paths, id: &str) -> Result<()> {
    let c = catalog(paths)?;
    remove_model(entry(&c, id)?, &paths.models_dir)?;
    Ok(())
}

fn main_model_id<'a>(c: &'a Catalog, cfg: &'a Config) -> Option<&'a str> {
    cfg.model
        .as_deref()
        .or_else(|| c.default_model().map(|d| d.id.as_str()))
}

fn install_hint(id: &str, main: Option<&str>, final_model: Option<&str>) -> Option<String> {
    if id == VAD_MODEL_ID || main == Some(id) {
        Some("the daemon loads it at the next dictation; if it does not, restart it (systemctl --user restart koeinput)".into())
    } else if final_model == Some(id) {
        Some(format!("{RESTART_LOADS} it"))
    } else {
        None
    }
}

fn shared_model_note(main: Option<&str>, final_model: Option<&str>) -> Option<&'static str> {
    (main.is_some() && main == final_model).then_some(
        "the main and the final-stage model are the same, so the daemon uses the one model for both and skips the second decode",
    )
}

const RESTART_LOADS: &str = "restart the daemon (systemctl --user restart koeinput) to load";

const RESTART: &str = "restart the daemon to use it (systemctl --user restart koeinput)";

fn recognizer<'a>(c: &'a Catalog, id: &str) -> Result<&'a ModelEntry> {
    let m = entry(c, id)?;
    if m.kind == "vad" {
        bail!("{id} is a VAD model, not a recognizer");
    }
    Ok(m)
}

fn updating(paths: &Paths) -> String {
    format!("updating {}", paths.config_file.display())
}

pub fn use_model(paths: &Paths, id: &str) -> Result<()> {
    let c = catalog(paths)?;
    recognizer(&c, id)?;
    set_model(&paths.config_file, id).with_context(|| updating(paths))?;
    println!("model set to {id}; {RESTART}");
    let cfg = config(paths)?;
    if let Some(note) = shared_model_note(Some(id), cfg.final_model.as_deref()) {
        println!("{note}");
    }
    Ok(())
}

pub fn use_final(paths: &Paths, id: &str) -> Result<()> {
    if id == "none" {
        set_final_model(&paths.config_file, None).with_context(|| updating(paths))?;
        println!("final model turned off; {RESTART}");
        return Ok(());
    }
    let c = catalog(paths)?;
    let m = recognizer(&c, id)?;
    let cfg = config(paths)?;
    let main = main_model_id(&c, &cfg);
    if main == Some(id) {
        bail!("{id} is already the main model, which also decodes the final text");
    }
    set_final_model(&paths.config_file, Some(id)).with_context(|| updating(paths))?;
    println!("final model set to {id}; {RESTART}");
    if cfg.final_full_max_ms == 0 {
        println!(
            "final_full_max_ms is 0, so the final-stage model is not used until you set it above 0"
        );
    }
    if !m.is_installed(&paths.models_dir) {
        println!("{id} is not installed yet; install it with `koeinput model install {id}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mib_uses_one_decimal_place() {
        assert_eq!(mib(1572864), "1.5 MiB");
        assert_eq!(mib(0), "0.0 MiB");
        assert_eq!(mib(489389564), "466.7 MiB");
    }

    #[test]
    fn install_hint_depends_on_the_role_of_the_model() {
        let main = Some("parakeet-ja");
        let fin = Some("cohere-transcribe");
        assert!(
            install_hint("parakeet-ja", main, fin)
                .unwrap()
                .contains("next dictation")
        );
        assert!(
            install_hint(VAD_MODEL_ID, main, fin)
                .unwrap()
                .contains("next dictation")
        );
        let line = install_hint("cohere-transcribe", main, fin).unwrap();
        assert!(line.contains("restart the daemon") && !line.contains("next dictation"));
        assert_eq!(install_hint("other", main, fin), None);
    }

    #[test]
    fn shared_model_note_only_when_both_are_the_same() {
        assert!(shared_model_note(Some("a"), Some("a")).is_some());
        assert_eq!(shared_model_note(Some("a"), Some("b")), None);
        assert_eq!(shared_model_note(Some("a"), None), None);
    }
}
