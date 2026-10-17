use bzip2::read::BzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const VAD_MODEL_ID: &str = "silero-vad";
pub const DEFAULT_MODEL_ID: &str = "parakeet-ja";

const PROGRESS_STEP: u64 = 4 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

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
        let catalog: Self = toml::from_str(text).map_err(|e| ModelError::Parse(e.to_string()))?;
        let mut ids = HashSet::new();
        for (i, m) in catalog.models.iter().enumerate() {
            let at = |problem: String| {
                ModelError::Parse(format!("model {} ({:?}): {problem}", i + 1, m.id))
            };
            m.check().map_err(at)?;
            if !ids.insert(m.id.as_str()) {
                return Err(at("duplicate `id`".into()));
            }
        }
        Ok(catalog)
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

    pub fn validate(&self) -> Result<(), ModelError> {
        self.check()
            .map_err(|problem| ModelError::Parse(format!("model {:?}: {problem}", self.id)))
    }

    fn check(&self) -> Result<(), String> {
        if !is_single_component(&self.id) {
            return Err("`id` must be a single path component".into());
        }
        if let Some(root) = &self.archive_root
            && !is_single_component(root)
        {
            return Err("`archive_root` must be a single path component".into());
        }
        for (role, f) in &self.files {
            if !is_relative_path(f) {
                return Err(format!(
                    "`files.{role}` must be a relative path without `.` or `..`"
                ));
            }
        }
        if !matches!(self.format.as_str(), "file" | "tar.bz2") {
            return Err(format!(
                "`format` must be `file` or `tar.bz2`, not {:?}",
                self.format
            ));
        }
        if self.sha256.len() != 64 || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("`sha256` must be 64 hex digits".into());
        }
        Ok(())
    }
}

fn is_relative_path(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('\0')
        && Path::new(s)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn is_single_component(s: &str) -> bool {
    is_relative_path(s) && !s.contains('/')
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
    entry.validate()?;
    std::fs::create_dir_all(models_dir)?;
    let partial = models_dir.join(format!(".{}.download", entry.id));
    let staging = models_dir.join(format!(".{}.staging", entry.id));
    let old = models_dir.join(format!(".{}.old", entry.id));
    for p in [&partial, &staging, &old] {
        remove_path(p)?;
    }

    let dest = entry.dir(models_dir);
    let result = stage(entry, source, &partial, &staging, progress)
        .and_then(|()| replace_dir(&staging, &dest, &old));
    let _ = remove_path(&partial);
    if result.is_err() {
        let _ = remove_path(&staging);
    }
    result.map(|()| dest)
}

fn replace_dir(new: &Path, dest: &Path, old: &Path) -> Result<(), ModelError> {
    let had_old = match std::fs::rename(dest, old) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    if let Err(e) = std::fs::rename(new, dest) {
        if had_old && let Err(back) = std::fs::rename(old, dest) {
            tracing::warn!("could not restore {}: {back}", dest.display());
        }
        return Err(e.into());
    }
    if had_old && let Err(e) = remove_path(old) {
        tracing::warn!("could not remove {}: {e}", old.display());
    }
    Ok(())
}

fn stage(
    entry: &ModelEntry,
    source: InstallSource,
    partial: &Path,
    staging: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), ModelError> {
    let actual = match source {
        InstallSource::File(path) => {
            let file = File::open(path)
                .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
            copy_hashed(file, partial, entry.size, ModelError::Io, progress)?
        }
        InstallSource::Url => {
            // No limit on the whole body: large models take minutes to download.
            let agent = ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .timeout_connect(Some(CONNECT_TIMEOUT))
                    .timeout_recv_response(Some(RESPONSE_TIMEOUT))
                    .build(),
            );
            let response = agent
                .get(&entry.url)
                .call()
                .map_err(|e| ModelError::Download(e.to_string()))?;
            let reader = response.into_body().into_reader();
            let to_download = |e: std::io::Error| ModelError::Download(e.to_string());
            copy_hashed(reader, partial, entry.size, to_download, progress)?
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
            let name = entry.files.get("model").ok_or_else(|| {
                ModelError::MissingFile(format!("{}: no file role model", entry.id))
            })?;
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
    reader: impl Read,
    dest: &Path,
    total: u64,
    read_err: fn(std::io::Error) -> ModelError,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<String, ModelError> {
    let mut reader = reader.take(total.saturating_add(1));
    let mut out = File::create(dest)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    let mut reported = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(read_err)?;
        if n == 0 {
            break;
        }
        if done + n as u64 > total {
            return Err(read_err(std::io::Error::other(format!(
                "larger than the catalog size ({total} bytes)"
            ))));
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
    entry.validate()?;
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
            id: "t".into(),
            name: "t".into(),
            kind: "nemo_ctc".into(),
            url: "file:///none".into(),
            sha256: hex::encode(Sha256::digest(bytes)),
            size: bytes.len() as u64,
            license: "x".into(),
            format: format.into(),
            archive_root: Some("root".into()),
            default: false,
            files,
        }
    }

    fn write_tmp(dir: &Path, bytes: &[u8]) -> PathBuf {
        let p = dir.join("a.tar.bz2");
        std::fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn repository_catalog_parses_and_has_defaults() {
        let c = Catalog::load(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/models.toml"
        )))
        .unwrap();
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
        let bytes = bz_tar(&[
            ("root/m.onnx", b"onnx", tar::EntryType::Regular),
            ("root/extra/x.txt", b"x", tar::EntryType::Regular),
        ]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        let dir = install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(dir.join("m.onnx")).unwrap(), b"onnx");
        assert!(dir.join("extra/x.txt").exists());
        assert!(e.is_installed(&models));
    }

    #[test]
    fn missing_source_file_error_names_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        let e = entry_for(b"x", "tar.bz2");
        let src = tmp.path().join("nope.tar.bz2");
        let err = install(
            &e,
            InstallSource::File(&src),
            &tmp.path().join("models"),
            &mut |_, _| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("nope.tar.bz2"), "{err}");
    }

    #[test]
    fn checksum_mismatch_leaves_nothing_installed() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/m.onnx", b"onnx", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let mut e = entry_for(&bytes, "tar.bz2");
        e.sha256 = "0".repeat(64);
        let models = tmp.path().join("models");
        assert!(matches!(
            install(&e, InstallSource::File(&src), &models, &mut |_, _| {}),
            Err(ModelError::Checksum { .. })
        ));
        assert!(!e.is_installed(&models));
        assert!(!e.dir(&models).exists());
        let leftovers: Vec<_> = std::fs::read_dir(&models)
            .map(|d| d.collect())
            .unwrap_or_default();
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
            assert!(
                matches!(r, Err(ModelError::UnsafeArchive(_))),
                "{bad:?}: {r:?}"
            );
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
        assert!(matches!(
            install(&e, InstallSource::File(&src), &models, &mut |_, _| {}),
            Err(ModelError::MissingFile(_))
        ));
        assert!(!e.dir(&models).exists());
    }

    fn model_toml(overrides: &[(&str, &str)]) -> String {
        let mut fields: Vec<(&str, String)> = vec![
            ("id", "\"t\"".into()),
            ("name", "\"T\"".into()),
            ("kind", "\"vad\"".into()),
            ("url", "\"https://example.invalid/t\"".into()),
            ("sha256", format!("\"{}\"", "aB".repeat(32))),
            ("size", "1".into()),
            ("license", "\"MIT\"".into()),
            ("format", "\"tar.bz2\"".into()),
            ("archive_root", "\"root\"".into()),
            ("files", "{ model = \"sub/m.onnx\" }".into()),
        ];
        for (k, v) in overrides {
            fields.iter_mut().find(|(f, _)| f == k).unwrap().1 = v.to_string();
        }
        let mut s = String::from("[[model]]\n");
        for (k, v) in fields {
            s.push_str(&format!("{k} = {v}\n"));
        }
        s
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|d| {
                d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    fn serve_once(body: Vec<u8>) -> String {
        use std::io::Read as _;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = Vec::new();
            let mut b = [0u8; 1];
            while !req.ends_with(b"\r\n\r\n") && s.read(&mut b).unwrap_or(0) == 1 {
                req.push(b[0]);
            }
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        });
        format!("http://{addr}/m.onnx")
    }

    #[test]
    fn catalog_rejects_unsafe_entries() {
        Catalog::parse(&model_toml(&[])).unwrap();
        Catalog::parse(&model_toml(&[
            ("archive_root", "\"r\""),
            ("format", "\"file\""),
        ]))
        .unwrap();
        let sha63 = format!("\"{}\"", "a".repeat(63));
        let sha_not_hex = format!("\"{}\"", "g".repeat(64));
        for (field, value) in [
            ("id", "\"\""),
            ("id", "\".\""),
            ("id", "\"..\""),
            ("id", "\"a/b\""),
            ("id", "\"a\\u0000\""),
            ("archive_root", "\"..\""),
            ("archive_root", "\"a/b\""),
            ("files", "{ model = \"/abs/m.onnx\" }"),
            ("files", "{ model = \"../x\" }"),
            ("files", "{ model = \"a/../../x\" }"),
            ("files", "{ model = \"\" }"),
            ("format", "\"zip\""),
            ("sha256", sha63.as_str()),
            ("sha256", sha_not_hex.as_str()),
        ] {
            match Catalog::parse(&model_toml(&[(field, value)])) {
                Err(ModelError::Parse(msg)) => {
                    assert!(
                        msg.contains(&format!("`{field}")),
                        "{field} = {value}: {msg}"
                    );
                    assert!(msg.contains("model 1"), "{field} = {value}: {msg}");
                }
                other => panic!("{field} = {value}: {other:?}"),
            }
        }
        let dup = format!("{}{}", model_toml(&[]), model_toml(&[]));
        match Catalog::parse(&dup) {
            Err(ModelError::Parse(msg)) => {
                assert!(msg.contains("model 2") && msg.contains("`id`"), "{msg}")
            }
            other => panic!("duplicate id: {other:?}"),
        }
    }

    #[test]
    fn download_over_catalog_size_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let body: Vec<u8> = (0..300_000u32).map(|i| i as u8).collect();
        let size = 1000;
        let mut e = entry_for(&body[..size], "file");
        e.url = serve_once(body);
        let models = tmp.path().join("models");
        let r = install(&e, InstallSource::Url, &models, &mut |_, _| {});
        match r {
            Err(ModelError::Download(msg)) => {
                assert!(msg.contains("larger than the catalog size"), "{msg}")
            }
            other => panic!("{other:?}"),
        }
        assert!(names_in(&models).is_empty(), "{:?}", names_in(&models));
    }

    #[test]
    fn installs_from_url() {
        let tmp = tempfile::tempdir().unwrap();
        let body = b"model-bytes".to_vec();
        let mut e = entry_for(&body, "file");
        e.url = serve_once(body.clone());
        let models = tmp.path().join("models");
        let mut last = (0, 0);
        let dir = install(&e, InstallSource::Url, &models, &mut |d, t| last = (d, t)).unwrap();
        assert_eq!(std::fs::read(dir.join("m.onnx")).unwrap(), body);
        assert_eq!(last, (body.len() as u64, body.len() as u64));
        assert!(e.is_installed(&models));
        assert_eq!(names_in(&models), ["t"]);
    }

    #[test]
    fn local_read_error_is_io() {
        let tmp = tempfile::tempdir().unwrap();
        let a_dir = tmp.path().join("a-dir");
        std::fs::create_dir(&a_dir).unwrap();
        let e = entry_for(b"x", "file");
        let models = tmp.path().join("models");
        for src in [a_dir, tmp.path().join("missing")] {
            let r = install(&e, InstallSource::File(&src), &models, &mut |_, _| {});
            assert!(matches!(r, Err(ModelError::Io(_))), "{src:?}: {r:?}");
            assert!(names_in(&models).is_empty(), "{:?}", names_in(&models));
        }
    }

    #[test]
    fn failed_reinstall_keeps_old_install() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/m.onnx", b"onnx", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        let mut bad = e.clone();
        bad.sha256 = "0".repeat(64);
        assert!(matches!(
            install(&bad, InstallSource::File(&src), &models, &mut |_, _| {}),
            Err(ModelError::Checksum { .. })
        ));
        assert!(e.is_installed(&models));
        assert_eq!(
            std::fs::read(e.dir(&models).join("m.onnx")).unwrap(),
            b"onnx"
        );
        assert_eq!(names_in(&models), ["t"]);
    }

    #[test]
    fn reinstall_replaces_files() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/m.onnx", b"onnx", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        let dir = install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        std::fs::write(dir.join("extra.txt"), b"old").unwrap();
        install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        assert!(!dir.join("extra.txt").exists());
        assert!(e.is_installed(&models));
        assert_eq!(names_in(&models), ["t"]);
    }

    #[test]
    fn leftover_old_install_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[("root/m.onnx", b"onnx", tar::EntryType::Regular)]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        std::fs::create_dir_all(models.join(".t.old/sub")).unwrap();
        std::fs::write(models.join(".t.old/sub/m.onnx"), b"stale").unwrap();
        install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        assert_eq!(names_in(&models), ["t"]);
    }

    #[test]
    fn cleanup_after_extraction_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[
            ("root/m.onnx", b"onnx", tar::EntryType::Regular),
            ("root/sub/x.txt", b"x", tar::EntryType::Regular),
            ("root/../evil", b"x", tar::EntryType::Regular),
        ]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        assert!(matches!(
            install(&e, InstallSource::File(&src), &models, &mut |_, _| {}),
            Err(ModelError::UnsafeArchive(_))
        ));
        assert!(names_in(&models).is_empty(), "{:?}", names_in(&models));
        assert!(!tmp.path().join("evil").exists());
    }

    #[test]
    fn archive_with_directory_entries_installs() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = bz_tar(&[
            ("root/", b"", tar::EntryType::Directory),
            ("root/sub/", b"", tar::EntryType::Directory),
            ("root/sub/empty/", b"", tar::EntryType::Directory),
            ("root/m.onnx", b"onnx", tar::EntryType::Regular),
            ("root/sub/x.txt", b"x", tar::EntryType::Regular),
        ]);
        let src = write_tmp(tmp.path(), &bytes);
        let e = entry_for(&bytes, "tar.bz2");
        let models = tmp.path().join("models");
        let dir = install(&e, InstallSource::File(&src), &models, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::read(dir.join("m.onnx")).unwrap(), b"onnx");
        assert_eq!(std::fs::read(dir.join("sub/x.txt")).unwrap(), b"x");
        assert!(dir.join("sub/empty").is_dir());
        assert!(e.is_installed(&models));
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
