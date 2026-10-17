#![cfg(feature = "test-hooks")]

use futures_util::StreamExt;
use koeinput_dbus_api::{BUS_NAME, Recognizer1Proxy};
use std::collections::HashMap;
use std::process::{Child, Command};
use std::time::Duration;
use tokio::time::timeout;

// Every daemon claims the same well-known name on the one test bus, so the tests take turns.
static BUS_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Daemon {
    child: Child,
    _turn: tokio::sync::MutexGuard<'static, ()>,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_koeinput-daemon"));
    cmd.env("KOEINPUT_TEST_FAKE", "1");
    for (k, v) in extra {
        cmd.env(k, v);
    }
    let child = Daemon { child: cmd.spawn().unwrap(), _turn: turn };
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
    let mut second = Command::new(env!("CARGO_BIN_EXE_koeinput-daemon")).env("KOEINPUT_TEST_FAKE", "1").spawn().unwrap();
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
    assert!(status.is_some_and(|s| !s.success()), "second daemon kept running: {status:?}");
    let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
    let pid = dbus.get_connection_unix_process_id(BUS_NAME.try_into().unwrap()).await.unwrap();
    assert_eq!(pid, d.child.id());
}
