mod service;

use futures_util::StreamExt;
use koeinput_core::build::{BuildError, load_engine};
use koeinput_core::config::{Paths, load};
use koeinput_dbus_api::{BUS_NAME, OBJECT_PATH};
use service::{Readiness, Recognizer1, Shared};
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
    ).init();
    let paths = Paths::from_env();
    let cfg = load(&paths.config_file)?;
    let test_fake = cfg!(feature = "test-hooks") && std::env::var("KOEINPUT_TEST_FAKE").as_deref() == Ok("1");
    let shared = Arc::new(Mutex::new(Shared {
        readiness: Readiness::Loading,
        current: None,
        next_id: 0,
        model: cfg.model.clone().unwrap_or_default(),
        source: cfg.source.clone(),
        test_source: test_fake,
    }));
    // zbus would otherwise let a second daemon take the name from a running one.
    let conn = zbus::connection::Builder::session()
        .allow_name_replacements(false)
        .replace_existing_names(false)
        .name(BUS_NAME)
        .serve_at(OBJECT_PATH, Recognizer1 { shared: shared.clone() })
        .build()
        .await?;
    tracing::info!("serving {BUS_NAME}");

    let loader_shared = shared.clone();
    tokio::task::spawn_blocking(move || {
        let r = load_for(&paths, &cfg, test_fake);
        let mut s = loader_shared.lock().unwrap();
        match r {
            Ok((engine, id)) => {
                s.model = id;
                s.readiness = Readiness::Ready(Arc::new(engine));
                tracing::info!(model = %s.model, "model loaded");
            }
            Err(BuildError::NoModel { hint, .. }) => {
                tracing::warn!("no model installed; run `{hint}`");
                s.readiness = Readiness::Missing(format!("no model installed; run `{hint}`"));
            }
            Err(e) => {
                tracing::error!("{e}");
                s.readiness = Readiness::Missing(e.to_string());
            }
        }
    });

    let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
    let mut changes = dbus.receive_name_owner_changed().await?;
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
    Ok(())
}

fn load_for(
    paths: &Paths,
    cfg: &koeinput_core::config::Config,
    test_fake: bool,
) -> Result<(koeinput_core::engine::Engine, String), BuildError> {
    #[cfg(feature = "test-hooks")]
    if test_fake {
        if std::env::var("KOEINPUT_TEST_NO_MODEL").as_deref() == Ok("1") {
            return Err(BuildError::NoModel { id: "parakeet-ja".into(), hint: "koeinput model install parakeet-ja".into() });
        }
        use koeinput_core::testing::{FakeRecognizer, FakeVad};
        let rec = if std::env::var("KOEINPUT_TEST_PANIC").as_deref() == Ok("1") {
            FakeRecognizer::new(|_| panic!("test recognizer panic"))
        } else {
            FakeRecognizer::new(|s| Ok(format!("{}", s.len() / 16000)))
        };
        let engine = koeinput_core::engine::Engine {
            cfg: koeinput_core::session::SessionConfig::from_config(cfg),
            rec: Arc::new(rec),
            vad: Arc::new(|| Ok(Box::new(FakeVad::always_speech()) as Box<dyn koeinput_core::vad::Vad>)),
            post: Arc::new(koeinput_core::postprocess::PostProcessor::empty()),
        };
        return Ok((engine, "fake".into()));
    }
    let _ = test_fake;
    load_engine(paths, cfg)
}
