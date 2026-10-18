use anyhow::{Result, anyhow};
use koeinput_core::build::dictionary_path;
use koeinput_core::config::{Paths, load};
use koeinput_core::postprocess::{AddOutcome, add_entry, load_dictionary, remove_entry};
use std::path::PathBuf;

fn path(paths: &Paths) -> Result<PathBuf> {
    let cfg = load(&paths.config_file)
        .map_err(|e| anyhow!("reading {}: {e}", paths.config_file.display()))?;
    Ok(dictionary_path(paths, &cfg))
}

pub fn add(paths: &Paths, wrong: &str, right: &str) -> Result<()> {
    let p = path(paths)?;
    match add_entry(&p, wrong, right)? {
        AddOutcome::Added => println!("added {wrong} -> {right}"),
        AddOutcome::Replaced => println!("replaced {wrong} -> {right}"),
    }
    Ok(())
}

pub fn remove(paths: &Paths, wrong: &str) -> Result<()> {
    if remove_entry(&path(paths)?, wrong)? {
        println!("removed {wrong}");
        Ok(())
    } else {
        Err(anyhow!("no entry for {wrong}"))
    }
}

pub fn list(paths: &Paths) -> Result<()> {
    let p = path(paths)?;
    let d = load_dictionary(&p).map_err(|e| anyhow!("reading {}: {e}", p.display()))?;
    for w in &d.warnings {
        eprintln!("{w}");
    }
    for (k, v) in &d.pairs {
        println!("{k}\t{v}");
    }
    Ok(())
}
