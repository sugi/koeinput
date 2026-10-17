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
fn daemon_command(extra: &[(&str, &str)]) -> (Command, PathBuf) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("dbus-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
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

async fn start_daemon(extra: &[(&str, &str)]) -> (Daemon, zbus::Connection) {
    let turn = BUS_TURN.lock().await;
    let conn = zbus::Connection::session().await.unwrap();
    let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
    for _ in 0..100 {
        if !dbus.name_has_owner(BUS_NAME.try_into().unwrap()).await.unwrap() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (mut cmd, dir) = daemon_command(extra);
    let child = Daemon { child: cmd.spawn().unwrap(), dir, _turn: turn };
    for _ in 0..100 {
        let owner = dbus.get_connection_unix_process_id(BUS_NAME.try_into().unwrap()).await;
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
    let f = timeout(Duration::from_secs(5), finals.next()).await.unwrap().unwrap();
    let a = f.args().unwrap();
    assert_eq!(a.session, id);
    assert!(!a.text.is_empty());
    let partial = timeout(Duration::from_secs(1), partials.next()).await.expect("no Partial before Final").unwrap();
    assert_eq!(partial.args().unwrap().session, id);
    assert!(timeout(Duration::from_millis(500), other_finals.next()).await.is_err(), "signal leaked to another client");
}

#[tokio::test(flavor = "multi_thread")]
async fn second_start_is_busy_and_unknown_ids_are_rejected() {
    let (_d, conn) = start_daemon(&[]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    wait_ready(&p).await;
    let id = p.start(HashMap::new()).await.unwrap();
    let err = p.start(HashMap::new()).await.unwrap_err();
    assert!(err.to_string().contains("org.nemui.KoeInput.Error.Busy"), "{err}");
    let err = p.stop(id + 100).await.unwrap_err();
    assert!(err.to_string().contains("org.nemui.KoeInput.Error.NoSuchSession"), "{err}");
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

    let e = timeout(Duration::from_secs(5), errors.next()).await.unwrap().unwrap();
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
    let next = p.start(HashMap::new()).await.expect("daemon still busy after the worker panicked");
    assert_ne!(next, id);
}

#[tokio::test(flavor = "multi_thread")]
async fn second_daemon_does_not_take_over_the_name() {
    let (d, conn) = start_daemon(&[]).await;
    let (mut cmd, dir) = daemon_command(&[]);
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
    assert!(status.is_some_and(|s| !s.success()), "second daemon kept running: {status:?}\n{log}");
    assert!(log.contains("name already taken"), "{log}");
    let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
    let pid = dbus.get_connection_unix_process_id(BUS_NAME.try_into().unwrap()).await.unwrap();
    assert_eq!(pid, d.child.id());
}

#[tokio::test(flavor = "multi_thread")]
async fn ready_and_model_changes_reach_an_existing_proxy() {
    let (_d, conn) = start_daemon(&[("KOEINPUT_TEST_LOAD_DELAY_MS", "1000")]).await;
    let p = Recognizer1Proxy::new(&conn).await.unwrap();
    assert!(!p.ready().await.unwrap());
    assert_eq!(p.model().await.unwrap(), "parakeet-ja");
    wait_ready(&p).await;
    assert_eq!(p.model().await.unwrap(), "fake");
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
        p1.inner().call_noreply("Start", &(HashMap::<&str, zbus::Variant<'_>>::new(),)).await.unwrap();
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
    assert!(ok, "the session of a client that left during Start kept running");
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
