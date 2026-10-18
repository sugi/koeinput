#![cfg(feature = "test-hooks")]

use futures_util::StreamExt;
use koeinput_dbus_api::{BUS_NAME, Recognizer1Proxy};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::time::timeout;

// Every daemon claims the same well-known name on the one test bus, so the tests take turns.
static BUS_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Daemon {
    child: Child,
    dir: PathBuf,
    _turn: tokio::sync::MutexGuard<'static, ()>,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if std::thread::panicking() {
            let log = std::fs::read_to_string(self.dir.join("stderr.log")).unwrap_or_default();
            eprintln!("--- daemon stderr ---\n{log}");
        } else {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Config, data and stderr of the daemon go to a fresh directory, away from the developer's own files.
fn daemon_command(extra: &[(&str, &str)], config: &str) -> (Command, PathBuf) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("dbus-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    if !config.is_empty() {
        let cfg_dir = dir.join("config/koeinput");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(cfg_dir.join("config.toml"), config).unwrap();
    }
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_koeinput-daemon"));
    cmd.env("HOME", &dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("KOEINPUT_TEST_FAKE", "1")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.join("stderr.log")).unwrap());
    for (k, v) in extra {
        cmd.env(k, v);
    }
    (cmd, dir)
}

/// A path the fake loader waits for; the test creates the file when the load may finish.
fn gate_path() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let gate =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("gate-{}-{n}", std::process::id()));
    // Process ids repeat across container runs, so a file left by an earlier run must not open this gate.
    let _ = std::fs::remove_file(&gate);
    gate
}

fn open_gate(gate: &std::path::Path) {
    std::fs::write(gate, b"").unwrap();
}

async fn start_daemon(extra: &[(&str, &str)]) -> (Daemon, zbus::Connection) {
    start_daemon_with_config(extra, "").await
}

async fn start_daemon_with_config(
    extra: &[(&str, &str)],
    config: &str,
) -> (Daemon, zbus::Connection) {
    let turn = BUS_TURN.lock().await;
    let conn = zbus::Connection::session().await.unwrap();
    let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
    for _ in 0..100 {
        if !dbus
            .name_has_owner(BUS_NAME.try_into().unwrap())
            .await
            .unwrap()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (mut cmd, dir) = daemon_command(extra, config);
    let child = Daemon {
        child: cmd.spawn().unwrap(),
        dir,
        _turn: turn,
    };
    for _ in 0..100 {
        let owner = dbus
            .get_connection_unix_process_id(BUS_NAME.try_into().unwrap())
            .await;
        if owner.is_ok_and(|pid| pid == child.child.id()) {
            return (child, conn);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("daemon did not own {BUS_NAME}");
}

async fn wait_ready(p: &Recognizer1Proxy<'_>) {
    for _ in 0..100 {
        if p.ready().await.unwrap() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("not ready");
}

#[tokio::test(flavor = "multi_thread")]
async fn start_stop_delivers_partial_and_final_to_caller_only() {
    let (_d, conn) = start_daemon(&[]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;

    let other = zbus::Connection::session().await.unwrap();
    let op = Recognizer1Proxy::new(&other).await.unwrap();
    let mut other_finals = op.receive_final_text().await.unwrap();

    let mut states = p.receive_state_changed().await.unwrap();
    let mut partials = p.receive_partial().await.unwrap();
    let mut finals = p.receive_final_text().await.unwrap();
    let id = p.start(HashMap::new()).await.unwrap();
    let mut seen = Vec::new();
    while let Ok(Some(s)) = timeout(Duration::from_secs(5), states.next()).await {
        let a = s.args().unwrap();
        assert_eq!(a.session, id);
        seen.push(a.state.clone());
        if a.state == "recording" {
            break;
        }
    }
    assert_eq!(seen, vec!["starting".to_string(), "recording".to_string()]);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    p.stop(id).await.unwrap();
    let f = timeout(Duration::from_secs(5), finals.next())
        .await
        .unwrap()
        .unwrap();
    let a = f.args().unwrap();
    assert_eq!(a.session, id);
    assert!(!a.text.is_empty());
    let partial = timeout(Duration::from_secs(1), partials.next())
        .await
        .expect("no Partial before Final")
        .unwrap();
    assert_eq!(partial.args().unwrap().session, id);
    assert!(
        timeout(Duration::from_millis(500), other_finals.next())
            .await
            .is_err(),
        "signal leaked to another client"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn second_start_is_busy_and_unknown_ids_are_rejected() {
    let (_d, conn) = start_daemon(&[]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    let id = p.start(HashMap::new()).await.unwrap();
    let err = p.start(HashMap::new()).await.unwrap_err();
    assert!(
        err.to_string().contains("org.nemui.KoeInput.Error.Busy"),
        "{err}"
    );
    let err = p.stop(id + 100).await.unwrap_err();
    assert!(
        err.to_string()
            .contains("org.nemui.KoeInput.Error.NoSuchSession"),
        "{err}"
    );
    p.cancel(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn owner_disconnect_cancels_session() {
    let (_d, conn) = start_daemon(&[]).await;
    {
        let first = zbus::Connection::session().await.unwrap();
        let p1 = Recognizer1Proxy::new(&first).await.unwrap();
        wait_ready(&p1).await;
        p1.start(HashMap::new()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    let mut ok = false;
    for _ in 0..40 {
        match p.start(HashMap::new()).await {
            Ok(id) => {
                p.cancel(id).await.unwrap();
                ok = true;
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    assert!(ok, "session of the disconnected client was not cancelled");
}

#[tokio::test(flavor = "multi_thread")]
async fn start_without_model_explains_install() {
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_NO_MODEL", "1")]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    assert!(!p.ready().await.unwrap());
    let mut msg = String::new();
    for _ in 0..40 {
        msg = p.start(HashMap::new()).await.unwrap_err().to_string();
        if !msg.contains("still loading") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(msg.contains("org.nemui.KoeInput.Error.NoModel"), "{msg}");
    assert!(msg.contains("koeinput model install"), "{msg}");
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_panic_ends_session_with_error_and_frees_the_daemon() {
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_PANIC", "1")]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    let mut states = p.receive_state_changed().await.unwrap();
    let mut errors = p.receive_error().await.unwrap();
    let id = p.start(HashMap::new()).await.unwrap();

    let e = timeout(Duration::from_secs(5), errors.next())
        .await
        .unwrap()
        .unwrap();
    let a = e.args().unwrap();
    assert_eq!((a.session, a.code.as_str()), (id, "internal"));
    let mut last = String::new();
    while let Ok(Some(s)) = timeout(Duration::from_secs(5), states.next()).await {
        let a = s.args().unwrap();
        assert_eq!(a.session, id);
        last = a.state.clone();
        if last == "error" {
            break;
        }
    }
    assert_eq!(last, "error");
    let next = p
        .start(HashMap::new())
        .await
        .expect("daemon still busy after the worker panicked");
    assert_ne!(next, id);
}

#[tokio::test(flavor = "multi_thread")]
async fn second_daemon_does_not_take_over_the_name() {
    let (d, conn) = start_daemon(&[]).await;
    let (mut cmd, dir) = daemon_command(&[], "");
    let mut second = cmd.spawn().unwrap();
    let mut status = None;
    for _ in 0..100 {
        status = second.try_wait().unwrap();
        if status.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = second.kill();
    let _ = second.wait();
    let log = std::fs::read_to_string(dir.join("stderr.log")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        status.is_some_and(|s| !s.success()),
        "second daemon kept running: {status:?}\n{log}"
    );
    assert!(log.contains("name already taken"), "{log}");
    let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
    let pid = dbus
        .get_connection_unix_process_id(BUS_NAME.try_into().unwrap())
        .await
        .unwrap();
    assert_eq!(pid, d.child.id());
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_and_model_changes_reach_an_existing_proxy() {
    let gate = gate_path();
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_LOAD_GATE", gate.to_str().unwrap())]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    assert!(!p.ready().await.unwrap());
    assert_eq!(p.model().await.unwrap(), "parakeet-ja");
    open_gate(&gate);
    wait_ready(&p).await;
    assert_eq!(p.model().await.unwrap(), "fake");
    assert_eq!(p.final_model().await.unwrap(), "");
    assert!(!p.final_model_ready().await.unwrap());
}

const FINAL_CONFIG: &str = "final_model = \"cohere-transcribe\"\n";

async fn wait_final_ready(p: &Recognizer1Proxy<'_>) {
    for _ in 0..100 {
        if p.final_model_ready().await.unwrap() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("final-stage model not ready");
}

/// Records for about a second and returns the session's partial texts and its final text.
async fn dictate(p: &Recognizer1Proxy<'_>) -> (Vec<String>, String) {
    let mut partials = p.receive_partial().await.unwrap();
    let mut finals = p.receive_final_text().await.unwrap();
    let id = p.start(HashMap::new()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    p.stop(id).await.unwrap();
    let f = timeout(Duration::from_secs(5), finals.next())
        .await
        .expect("no Final")
        .unwrap();
    let a = f.args().unwrap();
    assert_eq!(a.session, id);
    let mut texts = Vec::new();
    while let Ok(Some(s)) = timeout(Duration::from_millis(200), partials.next()).await {
        let a = s.args().unwrap();
        if a.session == id {
            texts.push(a.text.clone());
        }
    }
    (texts, a.text.clone())
}

#[tokio::test(flavor = "multi_thread")]
async fn final_text_comes_from_the_final_stage_model() {
    let (_d, conn) =
        start_daemon_with_config(&[("KOEINPUT_TEST_FINAL_FAKE", "1")], FINAL_CONFIG).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    assert_eq!(p.final_model().await.unwrap(), "cohere-transcribe");
    wait_final_ready(&p).await;
    assert!(p.ready().await.unwrap());
    let (partials, text) = dictate(&p).await;
    assert!(text.starts_with('F') && text.len() > 1, "{text}");
    assert!(!partials.is_empty());
    assert!(partials.iter().all(|t| !t.starts_with('F')), "{partials:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn final_stage_model_loads_after_the_main_model() {
    let gate = gate_path();
    let (_d, conn) = start_daemon_with_config(
        &[
            ("KOEINPUT_TEST_FINAL_FAKE", "1"),
            ("KOEINPUT_TEST_FINAL_LOAD_GATE", gate.to_str().unwrap()),
        ],
        FINAL_CONFIG,
    )
    .await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    assert!(!p.final_model_ready().await.unwrap());
    let (_, text) = dictate(&p).await;
    assert!(!text.is_empty() && !text.starts_with('F'), "{text}");
    open_gate(&gate);
    wait_final_ready(&p).await;
    let (_, text) = dictate(&p).await;
    assert!(text.starts_with('F'), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn final_model_load_failure_keeps_main_model() {
    let (d, conn) =
        start_daemon_with_config(&[("KOEINPUT_TEST_FINAL_LOAD_FAIL", "1")], FINAL_CONFIG).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    let log_path = d.dir.join("stderr.log");
    let error_line = |log: &str| {
        log.lines()
            .find(|l| l.contains("test final-stage load failure"))
            .map(str::to_string)
    };
    let mut log = String::new();
    for _ in 0..100 {
        log = std::fs::read_to_string(&log_path).unwrap_or_default();
        if error_line(&log).is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let line = error_line(&log).unwrap_or_else(|| panic!("no error line in\n{log}"));
    assert!(line.contains("ERROR"), "{line}");
    assert!(line.contains("cohere-transcribe"), "{line}");
    assert!(p.ready().await.unwrap());
    assert!(!p.final_model_ready().await.unwrap());
    assert_eq!(p.final_model().await.unwrap(), "cohere-transcribe");
    let (_, text) = dictate(&p).await;
    assert!(!text.is_empty() && !text.starts_with('F'), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn session_started_before_the_final_model_swap_keeps_the_main_model() {
    let gate = gate_path();
    let (_d, conn) = start_daemon_with_config(
        &[
            ("KOEINPUT_TEST_FINAL_FAKE", "1"),
            ("KOEINPUT_TEST_FINAL_LOAD_GATE", gate.to_str().unwrap()),
        ],
        FINAL_CONFIG,
    )
    .await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    let mut finals = p.receive_final_text().await.unwrap();
    let id = p.start(HashMap::new()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    open_gate(&gate);
    wait_final_ready(&p).await;
    p.stop(id).await.unwrap();
    let f = timeout(Duration::from_secs(5), finals.next())
        .await
        .expect("no Final")
        .unwrap();
    let a = f.args().unwrap();
    assert_eq!(a.session, id);
    assert!(!a.text.is_empty() && !a.text.starts_with('F'), "{}", a.text);
    let (_, text) = dictate(&p).await;
    assert!(text.starts_with('F'), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn final_model_equal_to_the_main_model_reuses_it() {
    let (d, conn) = start_daemon_with_config(
        &[("KOEINPUT_TEST_FINAL_LOAD_FAIL", "1")],
        "final_model = \"fake\"\n",
    )
    .await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    wait_final_ready(&p).await;
    assert_eq!(p.final_model().await.unwrap(), "fake");
    let (_, text) = dictate(&p).await;
    assert!(
        !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()),
        "{text}"
    );
    let log = std::fs::read_to_string(d.dir.join("stderr.log")).unwrap();
    assert!(!log.contains("test final-stage load failure"), "{log}");
}

#[tokio::test(flavor = "multi_thread")]
async fn client_leaving_during_start_does_not_leave_a_session_behind() {
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_START_DELAY_MS", "300")]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    {
        // The daemon is still inside this Start when the bus reports that the caller has gone.
        let first = zbus::Connection::session().await.unwrap();
        let p1 = Recognizer1Proxy::new(&first).await.unwrap();
        p1.inner()
            .call_noreply("Start", &(HashMap::<&str, zbus::Variant<'_>>::new(),))
            .await
            .unwrap();
        drop(p1);
        first.close().await.unwrap();
    }
    let mut ok = false;
    for _ in 0..10 {
        match p.start(HashMap::new()).await {
            Ok(id) => {
                p.cancel(id).await.unwrap();
                ok = true;
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    assert!(
        ok,
        "the session of a client that left during Start kept running"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn loader_panic_is_reported_instead_of_loading_forever() {
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_LOAD_PANIC", "1")]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    let mut msg = String::new();
    for _ in 0..40 {
        msg = p.start(HashMap::new()).await.unwrap_err().to_string();
        if !msg.contains("still loading") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(msg.contains("org.nemui.KoeInput.Error.NoModel"), "{msg}");
    assert!(msg.contains("model loading failed"), "{msg}");
}

#[tokio::test(flavor = "multi_thread")]
async fn start_waits_for_a_loading_model() {
    let gate = gate_path();
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_LOAD_GATE", gate.to_str().unwrap())]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    let starting = {
        let p = p.clone();
        tokio::spawn(async move { p.start(HashMap::new()).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!starting.is_finished(), "Start did not wait for the load");
    assert!(!p.ready().await.unwrap());
    open_gate(&gate);
    let id = timeout(Duration::from_secs(3), starting)
        .await
        .expect("Start did not return after the load")
        .unwrap()
        .unwrap();
    p.cancel(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn start_gives_up_after_four_seconds() {
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_LOAD_DELAY_MS", "6000")]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    let t = std::time::Instant::now();
    let msg = p.start(HashMap::new()).await.unwrap_err().to_string();
    let waited = t.elapsed();
    assert!(msg.contains("org.nemui.KoeInput.Error.NoModel"), "{msg}");
    assert!(msg.contains("still loading"), "{msg}");
    assert!(
        waited >= Duration::from_millis(3500) && waited <= Duration::from_secs(5),
        "{waited:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_answers_during_a_waiting_start() {
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_LOAD_DELAY_MS", "3000")]).await;
    let caller = zbus::Connection::session().await.unwrap();
    let caller_p = Recognizer1Proxy::new(&caller).await.unwrap();
    let waiting = {
        let p = caller_p.clone();
        tokio::spawn(async move { p.start(HashMap::new()).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    let ready = timeout(Duration::from_millis(500), p.ready())
        .await
        .expect("Ready did not answer during a waiting Start")
        .unwrap();
    assert!(!ready);
    let id = waiting.await.unwrap().unwrap();
    caller_p.cancel(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn dictionary_reload_between_sessions() {
    let (d, conn) = start_daemon(&[]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    let dict = d.dir.join("config/koeinput/dictionary.tsv");
    let words = |to: &str| -> String { (0..6).map(|n| format!("{n}\t{to}\n")).collect() };

    let (_, text) = dictate(&p).await;
    assert!(text.chars().all(|c| c.is_ascii_digit()), "{text}");

    std::fs::create_dir_all(dict.parent().unwrap()).unwrap();
    std::fs::write(&dict, words("one")).unwrap();
    let (_, text) = dictate(&p).await;
    assert_eq!(text, "one");

    // A session keeps the dictionary it started with.
    let mut finals = p.receive_final_text().await.unwrap();
    let id = p.start(HashMap::new()).await.unwrap();
    std::fs::write(&dict, words("twenty")).unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    p.stop(id).await.unwrap();
    let f = timeout(Duration::from_secs(5), finals.next())
        .await
        .expect("no Final")
        .unwrap();
    assert_eq!(f.args().unwrap().text, "one");
    let (_, text) = dictate(&p).await;
    assert_eq!(text, "twenty");

    std::fs::remove_file(&dict).unwrap();
    let (_, text) = dictate(&p).await;
    assert!(text.chars().all(|c| c.is_ascii_digit()), "{text}");
}

fn log_count(d: &Daemon, needle: &str) -> usize {
    std::fs::read_to_string(d.dir.join("stderr.log"))
        .unwrap_or_default()
        .matches(needle)
        .count()
}

/// The fake loader reports "no model" while this file is missing.
fn installed_marker() -> PathBuf {
    let marker = gate_path();
    let _ = std::fs::remove_file(&marker);
    marker
}

async fn start_error(p: &Recognizer1Proxy<'_>) -> String {
    p.start(HashMap::new()).await.unwrap_err().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn model_installed_after_start_loads_at_the_next_start() {
    let marker = installed_marker();
    let (d, conn) = start_daemon(&[
        ("KOEINPUT_TEST_NO_MODEL", "1"),
        ("KOEINPUT_TEST_NO_MODEL_FILE", marker.to_str().unwrap()),
    ])
    .await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    let msg = start_error(&p).await;
    assert!(msg.contains("koeinput model install"), "{msg}");
    assert!(!p.ready().await.unwrap());
    // A Start while the model is still missing must not start a load.
    assert!(start_error(&p).await.contains("koeinput model install"));
    assert_eq!(log_count(&d, "the model is installed now"), 0);

    let mut ready = p.receive_ready_changed().await;
    open_gate(&marker);
    let id = p.start(HashMap::new()).await.unwrap();
    let changed = timeout(Duration::from_secs(3), ready.next())
        .await
        .expect("no PropertiesChanged for Ready")
        .unwrap();
    assert!(changed.get().await.unwrap());
    assert!(p.ready().await.unwrap());
    assert_eq!(p.model().await.unwrap(), "fake");
    p.cancel(id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_starts_share_one_reload() {
    let marker = installed_marker();
    let gate = gate_path();
    let (d, conn) = start_daemon(&[
        ("KOEINPUT_TEST_NO_MODEL", "1"),
        ("KOEINPUT_TEST_NO_MODEL_FILE", marker.to_str().unwrap()),
        ("KOEINPUT_TEST_LOAD_GATE", gate.to_str().unwrap()),
    ])
    .await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    assert!(start_error(&p).await.contains("koeinput model install"));
    open_gate(&marker);
    let starts: Vec<_> = (0..2)
        .map(|_| {
            let p = p.clone();
            tokio::spawn(async move { p.start(HashMap::new()).await })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        starts.iter().all(|s| !s.is_finished()),
        "Start did not wait for the reload"
    );
    open_gate(&gate);
    let mut ok = 0;
    for s in starts {
        match timeout(Duration::from_secs(3), s).await.unwrap().unwrap() {
            Ok(id) => {
                ok += 1;
                p.cancel(id).await.unwrap();
            }
            Err(e) => {
                let e = e.to_string();
                assert!(e.contains("Busy") && !e.contains("NoModel"), "{e}");
            }
        }
    }
    assert_eq!(ok, 1);
    assert_eq!(log_count(&d, "model loaded model=fake"), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn reload_waits_four_seconds_then_reports_still_loading() {
    let marker = installed_marker();
    let gate = gate_path();
    let (_d, conn) = start_daemon(&[
        ("KOEINPUT_TEST_NO_MODEL", "1"),
        ("KOEINPUT_TEST_NO_MODEL_FILE", marker.to_str().unwrap()),
        ("KOEINPUT_TEST_LOAD_GATE", gate.to_str().unwrap()),
    ])
    .await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    assert!(start_error(&p).await.contains("koeinput model install"));
    open_gate(&marker);
    let t = std::time::Instant::now();
    let msg = start_error(&p).await;
    assert!(msg.contains("still loading"), "{msg}");
    assert!(
        t.elapsed() >= Duration::from_millis(3500),
        "{:?}",
        t.elapsed()
    );
    open_gate(&gate);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_load_error_is_not_retried() {
    let marker = installed_marker();
    let (d, conn) = start_daemon(&[
        ("KOEINPUT_TEST_NO_MODEL", "1"),
        ("KOEINPUT_TEST_NO_MODEL_FILE", marker.to_str().unwrap()),
        ("KOEINPUT_TEST_LOAD_PANIC", "1"),
    ])
    .await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    assert!(start_error(&p).await.contains("koeinput model install"));
    open_gate(&marker);
    let msg = start_error(&p).await;
    assert!(msg.contains("model loading failed"), "{msg}");
    for _ in 0..3 {
        assert!(start_error(&p).await.contains("model loading failed"));
    }
    assert_eq!(log_count(&d, "model loading failed"), 1);
}
