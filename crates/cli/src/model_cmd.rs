use anyhow::{Context, Result, bail};
use koeinput_core::config::{Paths, set_model};
use koeinput_core::models::{
    Catalog, InstallSource, ModelEntry, VAD_MODEL_ID, install as install_model,
    remove as remove_model,
};
use std::path::Path;

fn catalog(paths: &Paths) -> Result<Catalog> {
    Catalog::load(&paths.catalog_file)
        .with_context(|| format!("reading {}", paths.catalog_file.display()))
}

fn entry<'a>(c: &'a Catalog, id: &str) -> Result<&'a ModelEntry> {
    c.get(id)
        .with_context(|| format!("unknown model {id}; see `koeinput model list`"))
}

pub fn list(paths: &Paths) -> Result<()> {
    let c = catalog(paths)?;
    for m in &c.models {
        let state = if m.is_installed(&paths.models_dir) {
            "installed"
        } else {
            "not installed"
        };
        let default = if m.default { ", default" } else { "" };
        println!(
            "{:<20} {:>7.0} MB  {}{}  {} [{}]",
            m.id,
            m.size as f64 / 1e6,
            state,
            default,
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
            eprint!("\r{}: {} / {} MB", m.id, done >> 20, total >> 20);
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
    install_one(paths, m, from_file)
}

pub fn remove(paths: &Paths, id: &str) -> Result<()> {
    let c = catalog(paths)?;
    remove_model(entry(&c, id)?, &paths.models_dir)?;
    Ok(())
}

pub fn use_model(paths: &Paths, id: &str) -> Result<()> {
    let c = catalog(paths)?;
    let m = entry(&c, id)?;
    if m.kind == "vad" {
        bail!("{id} is a VAD model, not a recognizer");
    }
    set_model(&paths.config_file, id)?;
    println!("model set to {id}; restart the daemon to use it (systemctl --user restart koeinput)");
    Ok(())
}
