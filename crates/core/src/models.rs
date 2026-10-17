use bzip2::read::BzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const VAD_MODEL_ID: &str = "silero-vad";
pub const DEFAULT_MODEL_ID: &str = "parakeet-ja";

const PROGRESS_STEP: u64 = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("invalid catalog: {0}")]
    Parse(String),
    #[error("download failed: {0}")]
    Download(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("unsafe archive entry: {0}")]
    UnsafeArchive(String),
    #[error("model file missing after install: {0}")]
    MissingFile(String),
    #[error("unknown model: {0}")]
    UnknownModel(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub license: String,
    pub format: String,
    pub archive_root: Option<String>,
    #[serde(default)]
    pub default: bool,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct Catalog {
    #[serde(rename = "model")]
    pub models: Vec<ModelEntry>,
}

impl Catalog {
    pub fn parse(text: &str) -> Result<Self, ModelError> {
        toml::from_str(text).map_err(|e| ModelError::Parse(e.to_string()))
    }

    pub fn load(path: &Path) -> Result<Self, ModelError> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn default_model(&self) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.default)
    }
}

impl ModelEntry {
    pub fn dir(&self, models_dir: &Path) -> PathBuf {
        models_dir.join(&self.id)
    }

    pub fn file(&self, models_dir: &Path, key: &str) -> Result<PathBuf, ModelError> {
        self.files
            .get(key)
            .map(|f| self.dir(models_dir).join(f))
            .ok_or_else(|| ModelError::MissingFile(format!("{}: no file role {key}", self.id)))
    }

    pub fn is_installed(&self, models_dir: &Path) -> bool {
        let d = self.dir(models_dir);
        d.is_dir() && self.files.values().all(|f| d.join(f).exists())
    }
}

pub enum InstallSource<'a> {
    Url,
    File(&'a Path),
}

pub fn install(
    entry: &ModelEntry,
    source: InstallSource,
    models_dir: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, ModelError> {
    std::fs::create_dir_all(models_dir)?;
    let partial = models_dir.join(format!(".{}.download", entry.id));
    let staging = models_dir.join(format!(".{}.staging", entry.id));
    remove_path(&partial)?;
    remove_path(&staging)?;

    let result = stage(entry, source, &partial, &staging, progress).and_then(|()| {
        let dest = entry.dir(models_dir);
        remove_path(&dest)?;
        std::fs::rename(&staging, &dest)?;
        Ok(dest)
    });
    let _ = remove_path(&partial);
    if result.is_err() {
        let _ = remove_path(&staging);
    }
    result
}

fn stage(
    entry: &ModelEntry,
    source: InstallSource,
    partial: &Path,
    staging: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), ModelError> {
    let actual = match source {
        InstallSource::File(path) => copy_hashed(File::open(path)?, partial, entry.size, progress)?,
        InstallSource::Url => {
            let response = ureq::get(&entry.url)
                .call()
                .map_err(|e| ModelError::Download(e.to_string()))?;
            let reader = response.into_body().into_reader();
            copy_hashed(reader, partial, entry.size, progress)?
        }
    };
    if !actual.eq_ignore_ascii_case(&entry.sha256) {
        return Err(ModelError::Checksum {
            expected: entry.sha256.clone(),
            actual,
        });
    }

    std::fs::create_dir_all(staging)?;
    match entry.format.as_str() {
        "file" => {
            let name = entry
                .files
                .get("model")
                .ok_or_else(|| ModelError::MissingFile(format!("{}: no file role model", entry.id)))?;
            let target = checked_join(staging, Path::new(name))?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(partial, target)?;
        }
        "tar.bz2" => extract(entry, partial, staging)?,
        other => return Err(ModelError::Parse(format!("unknown format: {other}"))),
    }

    for f in entry.files.values() {
        if !staging.join(f).exists() {
            return Err(ModelError::MissingFile(f.clone()));
        }
    }
    Ok(())
}

fn copy_hashed(
    mut reader: impl Read,
    dest: &Path,
    total: u64,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<String, ModelError> {
    let mut out = File::create(dest)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    let mut reported = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| ModelError::Download(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        done += n as u64;
        if done - reported >= PROGRESS_STEP {
            reported = done;
            progress(done, total);
        }
    }
    out.flush()?;
    progress(done, total);
    Ok(hex::encode(hasher.finalize()))
}

fn extract(entry: &ModelEntry, archive: &Path, staging: &Path) -> Result<(), ModelError> {
    let root = entry.archive_root.as_deref();
    let mut tar = tar::Archive::new(BzDecoder::new(File::open(archive)?));
    for item in tar.entries()? {
        let mut item = item?;
        let path = item.path()?.into_owned();
        let shown = path.display().to_string();
        let kind = item.header().entry_type();
        if !matches!(kind, tar::EntryType::Regular | tar::EntryType::Directory) {
            return Err(ModelError::UnsafeArchive(shown));
        }
        let mut comps = path.components().peekable();
        if let Some(root) = root {
            match comps.next() {
                Some(Component::Normal(first)) if first == root => {}
                _ => return Err(ModelError::UnsafeArchive(shown)),
            }
        }
        let rest: PathBuf = comps.collect();
        if rest.as_os_str().is_empty() {
            continue;
        }
        let target = checked_join(staging, &rest).map_err(|_| ModelError::UnsafeArchive(shown))?;
        if kind == tar::EntryType::Directory {
            std::fs::create_dir_all(&target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            item.unpack(&target)?;
        }
    }
    Ok(())
}

fn checked_join(base: &Path, rel: &Path) -> Result<PathBuf, ModelError> {
    for c in rel.components() {
        if !matches!(c, Component::Normal(_) | Component::CurDir) {
            return Err(ModelError::UnsafeArchive(rel.display().to_string()));
        }
    }
    Ok(base.join(rel))
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    let r = match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) => Err(e),
    };
    match r {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

pub fn remove(entry: &ModelEntry, models_dir: &Path) -> Result<(), ModelError> {
    match std::fs::remove_dir_all(entry.dir(models_dir)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => Ok(other?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::io::Write;

    fn bz_tar(entries: &[(&str, &[u8], tar::EntryType)]) -> Vec<u8> {
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        {
            let mut b = tar::Builder::new(&mut enc);
            for (path, data, kind) in entries {
                let mut h = tar::Header::new_gnu();
                h.set_entry_type(*kind);
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                let name = path.as_bytes();
                h.as_old_mut().name[..name.len()].copy_from_slice(name);
                if *kind == tar::EntryType::Symlink {
                    h.set_link_name("/etc/passwd").unwrap();
                }
                h.set_cksum();
                b.append(&h, *data).unwrap();
            }
            b.finish().unwrap();
        }
        enc.finish().unwrap()
    }

    fn entry_for(bytes: &[u8], format: &str) -> ModelEntry {
        let mut files = BTreeMap::new();
        files.insert("model".to_string(), "m.onnx".to_string());
        ModelEntry {
            id: "t".into(), name: "t".into(), kind: "nemo_ctc".into(), url: "file:///none".into(),
            sha256: hex::encode(Sha256::digest(bytes)), size: bytes.len() as u64,
            license: "x".into(), format: format.into(), archive_root: Some("root".into()),
            default: false, files,
        }
    }

    fn write_tmp(dir: &Path, bytes: &[u8]) -> PathBuf {
        let p = dir.join("a.tar.bz2");
        std::fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn repository_catalog_parses_and_has_defaults() {
        let c = Catalog::load(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/models.toml"))).unwrap();
        assert_eq!(c.default_model().unwrap().id, DEFAULT_MODEL_ID);
        assert!(c.get(VAD_MODEL_ID).is_some());
        for m in &c.models {
            assert_eq!(m.sha256.len(), 64, "{}", m.id);
            assert!(!m.files.is_empty(), "{}", m.id);
        }
    }

    #[test]
    fn installs_archive_and_strips_root() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/m.onnx", b"onnx", tar::EntryType::Regular), ("root/extra/x.txt", b"x", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        let dir = install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(dir.join("m.onnx")).unwrap(), b"onnx");
        assert!(dir.join("extra/x.txt").exists());
        assert!(e.is_installed(&models));
    }

    #[test]
    fn checksum_mismatch_leaves_nothing_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/m.onnx", b"onnx", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let mut e = entry_for(&bytes, "tar.bz2");
        e.sha256 = "0".repeat(64);
        let models = tmp.path().join("models");
        assert!(matches!(install(&e, InstallSource::File(&src), &models, &mut |_, _| {}), Err(ModelError::Checksum { .. })));
        assert!(!e.is_installed(&models));
        assert!(!e.dir(&models).exists());
        let leftovers: Vec<_> = std::fs::read_dir(&models).map(|d| d.collect()).unwrap_or_default();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn rejects_parent_dir_absolute_and_symlink_entries() {
        for bad in [
            vec![("root/../evil", &b"x"[..], tar::EntryType::Regular)],
            vec![("/abs", &b"x"[..], tar::EntryType::Regular)],
            vec![("root/link", &b""[..], tar::EntryType::Symlink)],
            vec![("other/m.onnx", &b"x"[..], tar::EntryType::Regular)],
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let bytes = bz_tar(&bad);
            let src = write_tmp(tmp.path(), &bytes);
            let e = entry_for(&bytes, "tar.bz2");
            let models = tmp.path().join("models");
            let r = install(&e, InstallSource::File(&src), &models, &mut |_, _| {});
            assert!(matches!(r, Err(ModelError::UnsafeArchive(_))), "{bad:?}: {r:?}");
            assert!(!e.dir(&models).exists());
        }
    }

    #[test]
    fn missing_required_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/other.onnx", b"x", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        assert!(matches!(install(&e, InstallSource::File(&src), &models, &mut |_, _| {}), Err(ModelError::MissingFile(_))));
        assert!(!e.dir(&models).exists());
    }

    #[test]
    fn installs_single_file_and_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = b"vad-bytes";
        let src = tmp.path().join("silero_vad.onnx");
        std::fs::write(&src, bytes).unwrap();
        let mut e = entry_for(bytes, "file");
        e.files.insert("model".into(), "silero_vad.onnx".into());
        let models = tmp.path().join("models");
        install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        assert!(e.is_installed(&models));
        remove(&e, &models).unwrap();
        assert!(!e.dir(&models).exists());
    }
}
