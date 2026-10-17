use koeinput_core::engine::{Engine, SessionHandle, Source};
use koeinput_core::session::{Event, State};
use koeinput_dbus_api::OBJECT_PATH;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::message::Header;
use zbus::names::{BusName, UniqueName};
use zbus::object_server::SignalEmitter;
use zbus::OwnedVariant;
use zbus::{Connection, interface};

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.nemui.KoeInput.Error")]
pub enum KoeError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Busy(String),
    NoModel(String),
    NoSuchSession(String),
}

pub enum Readiness {
    Loading,
    Ready(Arc<Engine>),
    Missing(String),
}

pub struct Current {
    pub id: u32,
    pub owner: UniqueName<'static>,
    pub handle: Arc<SessionHandle>,
}

pub struct Shared {
    pub readiness: Readiness,
    pub current: Option<Current>,
    pub next_id: u32,
    pub model: String,
    pub source: Option<String>,
    pub test_source: bool,
}

pub struct Recognizer1 {
    pub shared: Arc<Mutex<Shared>>,
}

#[interface(name = "org.nemui.KoeInput.Recognizer1")]
impl Recognizer1 {
    async fn start(
        &self,
        _options: HashMap<String, OwnedVariant>,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> Result<u32, KoeError> {
        let owner = hdr.sender().ok_or_else(|| KoeError::NoSuchSession("no sender".into()))?.to_owned();
        let emitter = SignalEmitter::new(conn, OBJECT_PATH)?.set_destination(BusName::Unique(owner.clone()));
        let mut s = self.shared.lock().unwrap();
        if s.current.is_some() {
            return Err(KoeError::Busy("a session is already running".into()));
        }
        let engine = match &s.readiness {
            Readiness::Ready(e) => e.clone(),
            Readiness::Loading => return Err(KoeError::NoModel("the model is still loading".into())),
            Readiness::Missing(hint) => return Err(KoeError::NoModel(hint.clone())),
        };
        s.next_id += 1;
        let id = s.next_id;
        let source = if s.test_source { test_source() } else { Source::Microphone(s.source.clone()) };
        let handle = Arc::new(engine.start(source));
        s.current = Some(Current { id, owner, handle: handle.clone() });
        drop(s);
        spawn_forwarder(emitter, self.shared.clone(), id, handle);
        Ok(id)
    }

    async fn stop(&self, session: u32) -> Result<(), KoeError> {
        self.with_session(session, |h| h.stop())
    }

    async fn cancel(&self, session: u32) -> Result<(), KoeError> {
        self.with_session(session, |h| h.cancel())
    }

    #[zbus(property)]
    async fn ready(&self) -> bool {
        matches!(self.shared.lock().unwrap().readiness, Readiness::Ready(_))
    }

    #[zbus(property)]
    async fn model(&self) -> String {
        self.shared.lock().unwrap().model.clone()
    }

    #[zbus(property)]
    async fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    #[zbus(signal)]
    pub async fn state_changed(emitter: &SignalEmitter<'_>, session: u32, state: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn partial(emitter: &SignalEmitter<'_>, session: u32, text: &str) -> zbus::Result<()>;
    #[zbus(signal, name = "Final")]
    pub async fn final_text(emitter: &SignalEmitter<'_>, session: u32, text: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn error(emitter: &SignalEmitter<'_>, session: u32, code: &str, message: &str) -> zbus::Result<()>;
}

impl Recognizer1 {
    fn with_session(&self, id: u32, f: impl FnOnce(&SessionHandle)) -> Result<(), KoeError> {
        let s = self.shared.lock().unwrap();
        match &s.current {
            Some(c) if c.id == id => {
                f(&c.handle);
                Ok(())
            }
            _ => Err(KoeError::NoSuchSession(format!("no session {id}"))),
        }
    }
}

#[cfg(feature = "test-hooks")]
fn test_source() -> Source {
    Source::Generator { level: 0.1 }
}

#[cfg(not(feature = "test-hooks"))]
fn test_source() -> Source {
    unreachable!("test source requested without the test-hooks feature")
}

fn clear_current(shared: &Mutex<Shared>, id: u32) {
    let mut s = shared.lock().unwrap();
    if s.current.as_ref().is_some_and(|c| c.id == id) {
        s.current = None;
    }
}

async fn emit(emitter: &SignalEmitter<'_>, id: u32, ev: &Event, log_text: bool) -> zbus::Result<()> {
    match ev {
        Event::State(st) => {
            tracing::info!(session = id, state = st.as_str());
            Recognizer1::state_changed(emitter, id, st.as_str()).await
        }
        Event::Partial(t) => Recognizer1::partial(emitter, id, t).await,
        Event::Final(t) => {
            if log_text {
                tracing::info!(session = id, text = %t, "final");
            }
            Recognizer1::final_text(emitter, id, t).await
        }
        Event::Error { code, message } => {
            tracing::warn!(session = id, code = %code, "{message}");
            Recognizer1::error(emitter, id, code, message).await
        }
    }
}

fn spawn_forwarder(emitter: SignalEmitter<'static>, shared: Arc<Mutex<Shared>>, id: u32, handle: Arc<SessionHandle>) {
    let rt = tokio::runtime::Handle::current();
    let log_text = std::env::var("KOEINPUT_LOG_TEXT").as_deref() == Ok("1");
    tokio::task::spawn_blocking(move || {
        let forward = |ev: &Event| {
            if let Err(e) = rt.block_on(emit(&emitter, id, ev, log_text)) {
                tracing::warn!("emitting signal: {e}");
            }
        };
        // The session is cleared before its terminal state goes out, so a client may start again on receipt.
        while let Ok(ev) = handle.events.recv() {
            if matches!(ev, Event::State(State::Done | State::Cancelled | State::Error)) {
                clear_current(&shared, id);
                forward(&ev);
                return;
            }
            forward(&ev);
        }
        // The worker dropped its sender without a terminal state, which means it panicked.
        clear_current(&shared, id);
        forward(&Event::Error { code: "internal".into(), message: "session ended unexpectedly".into() });
        forward(&Event::State(State::Error));
    });
}
