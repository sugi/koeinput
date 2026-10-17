use std::process::Command;

fn koeinput(home: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_koeinput"));
    c.env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .env(
            "KOEINPUT_DATA_DIR",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"),
        );
    c
}

#[test]
fn model_list_shows_catalog_and_install_state() {
    let home = tempfile::tempdir().unwrap();
    let out = koeinput(home.path())
        .args(["model", "list"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("parakeet-ja"));
    assert!(text.contains("default"));
    assert!(text.contains("not installed"));
}

#[test]
fn model_use_writes_config() {
    let home = tempfile::tempdir().unwrap();
    let out = koeinput(home.path())
        .args(["model", "use", "zipformer-ja"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let cfg = std::fs::read_to_string(home.path().join(".config/koeinput/config.toml")).unwrap();
    assert!(cfg.contains("zipformer-ja"));
    let out = koeinput(home.path())
        .args(["model", "use", "nope"])
        .output()
        .unwrap();
    assert!(!out.status.success());
}

#[test]
fn transcribe_without_model_explains_how_to_install() {
    let home = tempfile::tempdir().unwrap();
    let out = koeinput(home.path())
        .args(["transcribe", "x.wav"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("koeinput model install parakeet-ja"));
}

#[test]
fn install_from_file_rejects_wrong_checksum() {
    let home = tempfile::tempdir().unwrap();
    let f = home.path().join("fake.onnx");
    std::fs::write(&f, b"not the vad").unwrap();
    let out = koeinput(home.path())
        .args(["model", "install", "silero-vad", "--from-file"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("checksum"));
}
