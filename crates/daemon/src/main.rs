mod service;

use anyhow::Context;
use futures_util::StreamExt;
use koeinput_core::build::{BuildError, dictionary_path, load_final, load_main};
use koeinput_core::config::{Config, Paths, load};
use koeinput_core::engine::Engine;
use koeinput_core::models::DEFAULT_MODEL_ID;
use koeinput_core::postprocess::DictionaryWatch;
use koeinput_core::recognizer::Recognizer;
use koeinput_dbus_api::{BUS_NAME, OBJECT_PATH};
use service::{Readiness, Recognizer1, Shared};
use std::sync::{Arc, Mutex};
use zbus::object_server::InterfaceRef;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let paths = Paths::from_env();
    let cfg = load(&paths.config_file)
        .with_context(|| format!("reading {}", paths.config_file.display()))?;
    let test_fake =
        cfg!(feature = "test-hooks") && std::env::var("KOEINPUT_TEST_FAKE").as_deref() == Ok("1");
    // Stamped before the load, so a change while the model loads is picked up at the first Start.
    let dictionary = DictionaryWatch::new(dictionary_path(&paths, &cfg));
    let shared = Arc::new(Mutex::new(Shared {
        readiness: Readiness::Loading,
        current: None,
        next_id: 0,
        model: cfg
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL_ID.to_string()),
        final_model: cfg.final_model.clone().unwrap_or_default(),
        final_ready: false,
        source: cfg.source.clone(),
        test_source: test_fake,
        load_finished: tokio::sync::watch::channel(false).0,
        dictionary,
    }));
    // zbus would otherwise let a second daemon take the name from a running one.
    let conn = zbus::connection::Builder::session()
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .name(BUS_NAME)
        .serve_at(
            OBJECT_PATH,
            Recognizer1 {
                shared: shared.clone(),
            },
        )
        .build()
        .await?;
    tracing::info!("serving {BUS_NAME}");

    // Subscribed before the model can load, so no session can start before the watch is in place.
    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let mut changes = dbus.receive_name_owner_changed().await?;
    let iface = conn
        .object_server()
        .interface::<_, Recognizer1>(OBJECT_PATH)
        .await?;
    tokio::spawn(load_model(paths, cfg, test_fake, shared.clone(), iface));

    while let Some(sig) = changes.next().await {
        let Ok(args) = sig.args() else { continue };
        if args.new_owner().is_some() {
            continue;
        }
        let s = shared.lock().unwrap();
        if let Some(c) = &s.current
            && args.name().as_str() == c.owner.as_str()
        {
            tracing::info!(session = c.id, "client left; cancelling");
            c.handle.cancel();
        }
    }
    // A Start racing with this must find no model, or it would store a session after the cancel.
    // Otherwise the runtime drop waits for the forwarder thread, which only ends with the session.
    {
        let mut s = shared.lock().unwrap();
        s.readiness = Readiness::Missing("shutting down".into());
        if let Some(c) = s.current.take() {
            tracing::info!(session = c.id, "bus connection closed; cancelling");
            c.handle.cancel();
        }
    }
    // Exiting non-zero lets Restart=on-failure bring the daemon back once the bus is up again.
    anyhow::bail!("session bus connection closed")
}

async fn load_model(
    paths: Paths,
    cfg: Config,
    test_fake: bool,
    shared: Arc<Mutex<Shared>>,
    iface: InterfaceRef<Recognizer1>,
) {
    let (p, c) = (paths.clone(), cfg.clone());
    let r = tokio::task::spawn_blocking(move || load_for(&p, &c, test_fake)).await;
    let (ready, main_id) = {
        let mut s = shared.lock().unwrap();
        match r {
            Ok(Ok((engine, id))) => {
                s.model = id;
                s.readiness = Readiness::Ready(Arc::new(engine));
                tracing::info!(model = %s.model, "model loaded");
            }
            Ok(Err(BuildError::NoModel { hint, .. })) => {
                tracing::warn!("no model installed; run `{hint}`");
                s.readiness = Readiness::Missing(format!("no model installed; run `{hint}`"));
            }
            Ok(Err(e)) => {
                tracing::error!("{e}");
                s.readiness = Readiness::Missing(e.to_string());
            }
            Err(e) => {
                tracing::error!("model loading failed: {e}");
                s.readiness = Readiness::Missing(format!("model loading failed: {e}"));
            }
        }
        s.load_finished.send_replace(true);
        (matches!(s.readiness, Readiness::Ready(_)), s.model.clone())
    };
    // Proxies cache properties, so clients only see the new values through PropertiesChanged.
    // Model goes first so that a client reacting to Ready already has the right Model.
    // The interface guard is dropped here, before the final-stage load that may take several seconds.
    {
        let i = iface.get().await;
        let emitter = iface.signal_emitter();
        for r in [
            i.model_changed(emitter).await,
            i.ready_changed(emitter).await,
        ] {
            if let Err(e) = r {
                tracing::warn!("announcing property change: {e}");
            }
        }
    }
    let Some(id) = cfg.final_model.clone().filter(|_| ready) else {
        return;
    };
    // Loaded only after the main model, so dictation can start while this large model loads.
    let r = if id == main_id {
        tracing::info!(model = %id, "the final-stage model is the main model; using it for both");
        Ok(Ok(None))
    } else {
        tracing::info!(model = %id, "loading the final-stage model");
        tokio::task::spawn_blocking(move || load_final_for(&paths, &cfg, test_fake)).await
    };
    let rec = match r {
        Ok(Ok(Some(rec))) => Some(rec),
        Ok(Ok(None)) if id == main_id => None,
        Ok(Ok(None)) => return,
        Ok(Err(e)) => {
            tracing::error!(model = %id, "final-stage model: {e}; using the main model only");
            return;
        }
        Err(e) => {
            tracing::error!(model = %id, "final-stage model loading failed: {e}; using the main model only");
            return;
        }
    };
    {
        let mut s = shared.lock().unwrap();
        let Readiness::Ready(engine) = &s.readiness else {
            return;
        };
        let engine = Engine {
            final_rec: Some(rec.unwrap_or_else(|| engine.rec.clone())),
            ..Engine::clone(engine)
        };
        s.readiness = Readiness::Ready(Arc::new(engine));
        s.final_ready = true;
        tracing::info!(model = %id, "final-stage model loaded");
    }
    if let Err(e) = iface
        .get()
        .await
        .final_model_ready_changed(iface.signal_emitter())
        .await
    {
        tracing::warn!("announcing property change: {e}");
    }
}

fn load_for(paths: &Paths, cfg: &Config, test_fake: bool) -> Result<(Engine, String), BuildError> {
    #[cfg(feature = "test-hooks")]
    if test_fake {
        if std::env::var("KOEINPUT_TEST_NO_MODEL").as_deref() == Ok("1") {
            return Err(BuildError::NoModel {
                id: "parakeet-ja".into(),
                hint: "koeinput model install parakeet-ja".into(),
            });
        }
        if std::env::var("KOEINPUT_TEST_LOAD_PANIC").as_deref() == Ok("1") {
            panic!("test loader panic");
        }
        service::test_gate("KOEINPUT_TEST_LOAD_GATE");
        if let Some(d) = service::test_delay("KOEINPUT_TEST_LOAD_DELAY_MS") {
            std::thread::sleep(d);
        }
        use koeinput_core::testing::{FakeRecognizer, FakeVad};
        let rec = if std::env::var("KOEINPUT_TEST_PANIC").as_deref() == Ok("1") {
            FakeRecognizer::new(|_| panic!("test recognizer panic"))
        } else {
            FakeRecognizer::new(|s| Ok(format!("{}", s.len() / 16000)))
        };
        let engine = Engine {
            cfg: koeinput_core::session::SessionConfig::from_config(cfg),
            rec: Arc::new(rec),
            final_rec: None,
            vad: Arc::new(|| {
                Ok(Box::new(FakeVad::always_speech()) as Box<dyn koeinput_core::vad::Vad>)
            }),
            post: Arc::new(koeinput_core::postprocess::PostProcessor::empty()),
        };
        return Ok((engine, "fake".into()));
    }
    let _ = test_fake;
    load_main(paths, cfg)
}

fn load_final_for(
    paths: &Paths,
    cfg: &Config,
    test_fake: bool,
) -> Result<Option<Arc<dyn Recognizer>>, BuildError> {
    #[cfg(feature = "test-hooks")]
    if test_fake {
        service::test_gate("KOEINPUT_TEST_FINAL_LOAD_GATE");
        if let Some(d) = service::test_delay("KOEINPUT_TEST_FINAL_LOAD_DELAY_MS") {
            std::thread::sleep(d);
        }
        if std::env::var("KOEINPUT_TEST_FINAL_LOAD_FAIL").as_deref() == Ok("1") {
            return Err(BuildError::Load("test final-stage load failure".into()));
        }
        if std::env::var("KOEINPUT_TEST_FINAL_FAKE").as_deref() == Ok("1") {
            let rec = koeinput_core::testing::FakeRecognizer::new(|s| {
                Ok(format!("F{}", s.len() / 16000))
            });
            return Ok(Some(Arc::new(rec)));
        }
    }
    let _ = test_fake;
    load_final(paths, cfg)
}
