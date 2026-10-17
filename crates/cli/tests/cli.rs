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
fn model_use_final_sets_and_clears_the_final_stage_model() {
    let home = tempfile::tempdir().unwrap();
    let cfg_path = home.path().join(".config/koeinput/config.toml");
    let run = |args: &[&str]| koeinput(home.path()).args(args).output().unwrap();
    let out = run(&["model", "use", "parakeet-ja"]);
    assert!(out.status.success());

    let out = run(&["model", "use", "cohere-transcribe", "--final"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("koeinput model install cohere-transcribe"),
        "{stdout}"
    );
    let cfg = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(cfg.contains("final_model = \"cohere-transcribe\""), "{cfg}");
    assert!(cfg.contains("model = \"parakeet-ja\""), "{cfg}");

    let out = run(&["model", "list"]);
    let list = String::from_utf8(out.stdout).unwrap();
    let marked: Vec<_> = list.lines().filter(|l| l.contains("final model")).collect();
    assert_eq!(marked.len(), 1, "{list}");
    assert!(marked[0].starts_with("cohere-transcribe "), "{list}");

    let out = run(&["model", "use", "--final", "none"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cfg = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(!cfg.contains("cohere-transcribe"), "{cfg}");
    assert!(cfg.contains("model = \"parakeet-ja\""), "{cfg}");
    let out = run(&["model", "list"]);
    assert!(
        !String::from_utf8(out.stdout)
            .unwrap()
            .contains("final model")
    );

    for args in [
        &["model", "use", "none"][..],
        &["model", "use", "silero-vad", "--final"],
        &["model", "use", "nope", "--final"],
        &["model", "use", "--final"],
    ] {
        let out = run(args);
        assert!(!out.status.success(), "{args:?} succeeded");
    }
    let cfg = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(!cfg.contains("final_model = \"silero-vad\""), "{cfg}");
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

#[test]
fn manpage_is_hidden_roff_that_documents_the_commands() {
    let home = tempfile::tempdir().unwrap();
    let out = koeinput(home.path()).arg("manpage").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("\n.TH koeinput 1"),
        "{}",
        &text[..text.len().min(80)]
    );
    assert!(text.contains("model"));
    let plain = text.replace('\\', "");
    assert!(plain.contains("--from-file"));
    assert!(plain.contains("--partials"));
    assert!(plain.contains("--final"));
    assert!(!plain.contains("manpage"));
    for sub in ["listen", "status", "transcribe", "model", "help"] {
        assert!(!plain.contains(&format!("koeinput-{sub}(1)")), "{sub}");
    }
    let help = koeinput(home.path()).arg("--help").output().unwrap();
    assert!(!String::from_utf8(help.stdout).unwrap().contains("manpage"));
}
