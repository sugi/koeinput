use crate::audio::capture::{Capture, CaptureMsg};
use crate::postprocess::PostProcessor;
use crate::recognizer::Recognizer;
use crate::session::{Event, Session, SessionConfig, State};
use crate::vad::Vad;
use crossbeam_channel::{Receiver, Sender, select, unbounded};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const CHUNK: usize = 320;
const AUDIO_TIMEOUT: Duration = Duration::from_secs(2);

pub enum Source {
    Microphone(Option<String>),
    File(PathBuf),
    Samples(Vec<f32>),
    #[cfg(any(test, feature = "test-hooks"))]
    Generator {
        level: f32,
    },
    #[cfg(test)]
    StallingGenerator {
        level: f32,
        chunks: usize,
    },
}

pub type VadFactory = Arc<dyn Fn() -> Result<Box<dyn Vad>, String> + Send + Sync>;

#[derive(Clone)]
pub struct Engine {
    pub cfg: SessionConfig,
    pub rec: Arc<dyn Recognizer>,
    pub final_rec: Option<Arc<dyn Recognizer>>,
    pub vad: VadFactory,
    pub post: Arc<PostProcessor>,
}

enum Command {
    Stop,
    Cancel,
}

pub struct SessionHandle {
    pub events: Receiver<Event>,
    cmd: Sender<Command>,
}

impl SessionHandle {
    pub fn stop(&self) {
        let _ = self.cmd.send(Command::Stop);
    }

    pub fn cancel(&self) {
        let _ = self.cmd.send(Command::Cancel);
    }
}

impl Engine {
    pub fn session(&self) -> Result<Session, String> {
        let vad = (self.vad)()?;
        Ok(
            Session::new(self.cfg, self.rec.clone(), vad, self.post.clone())
                .with_final_recognizer(self.final_rec.clone()),
        )
    }

    pub fn start(&self, source: Source) -> SessionHandle {
        let (ev_tx, ev_rx) = unbounded();
        let (cmd_tx, cmd_rx) = unbounded();
        let engine = self.clone();
        std::thread::Builder::new()
            .name("koeinput-session".into())
            .spawn(move || engine.run(source, ev_tx, cmd_rx))
            .expect("spawn session thread");
        SessionHandle {
            events: ev_rx,
            cmd: cmd_tx,
        }
    }

    fn run(&self, source: Source, ev: Sender<Event>, cmd: Receiver<Command>) {
        let send = |events: Vec<Event>| {
            for e in events {
                let _ = ev.send(e);
            }
        };
        send(vec![Event::State(State::Starting)]);
        let mut session = match self.session() {
            Ok(s) => s,
            Err(m) => return send(fail("model", m)),
        };
        let samples = match source {
            Source::Samples(s) => s,
            Source::File(p) => match crate::audio::file::read_wav_16k_mono(&p) {
                Ok(s) => s,
                Err(e) => return send(fail("audio", e.to_string())),
            },
            Source::Microphone(name) => {
                return self.run_live(&mut session, LiveSource::Mic(name), &send, &cmd);
            }
            #[cfg(any(test, feature = "test-hooks"))]
            Source::Generator { level } => {
                return self.run_live(
                    &mut session,
                    LiveSource::Generator(level, None),
                    &send,
                    &cmd,
                );
            }
            #[cfg(test)]
            Source::StallingGenerator { level, chunks } => {
                return self.run_live(
                    &mut session,
                    LiveSource::Generator(level, Some(chunks)),
                    &send,
                    &cmd,
                );
            }
        };
        for chunk in samples.chunks(CHUNK) {
            if let Ok(Command::Cancel) = cmd.try_recv() {
                return send(session.cancel());
            }
            send(session.push(chunk));
            if !session.wants_audio() {
                break;
            }
        }
        send(session.end_of_audio());
    }
}

fn fail(code: &str, message: String) -> Vec<Event> {
    vec![
        Event::Error {
            code: code.into(),
            message,
        },
        Event::State(State::Error),
    ]
}

enum LiveSource {
    Mic(Option<String>),
    #[cfg(any(test, feature = "test-hooks"))]
    Generator(f32, Option<usize>),
}

enum Running {
    Mic(Capture),
    #[cfg(any(test, feature = "test-hooks"))]
    Generator(Arc<std::sync::atomic::AtomicBool>),
}

impl Running {
    fn start(src: LiveSource, tx: Sender<CaptureMsg>) -> Running {
        match src {
            LiveSource::Mic(name) => Running::Mic(Capture::start(name, tx)),
            #[cfg(any(test, feature = "test-hooks"))]
            LiveSource::Generator(level, limit) => {
                let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let flag = stop.clone();
                std::thread::spawn(move || {
                    let mut sent = 0;
                    while !flag.load(std::sync::atomic::Ordering::SeqCst) {
                        if limit.is_some_and(|n| sent >= n) {
                            std::thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                        sent += 1;
                        if tx.send(CaptureMsg::Audio(vec![level; CHUNK])).is_err() {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    let _ = tx.send(CaptureMsg::Closed);
                });
                Running::Generator(stop)
            }
        }
    }

    fn stop(self) {
        match self {
            Running::Mic(c) => c.stop(),
            #[cfg(any(test, feature = "test-hooks"))]
            Running::Generator(flag) => flag.store(true, std::sync::atomic::Ordering::SeqCst),
        }
    }
}

fn fail_and_finish(session: &mut Session, code: &str, message: String) -> Vec<Event> {
    if session.abort() {
        fail(code, message)
    } else {
        Vec::new()
    }
}

fn apply_command(
    session: &mut Session,
    stop_requested: &mut bool,
    c: Option<Command>,
) -> Vec<Event> {
    match c {
        Some(Command::Stop) => {
            *stop_requested = true;
            session.stop()
        }
        Some(Command::Cancel) | None => session.cancel(),
    }
}

impl Engine {
    fn run_live(
        &self,
        session: &mut Session,
        src: LiveSource,
        send: &dyn Fn(Vec<Event>),
        cmd: &Receiver<Command>,
    ) {
        let (cap_tx, cap_rx) = unbounded();
        let mut running = Some(Running::start(src, cap_tx));
        let mut stop_requested = false;
        // Release the capture before forwarding the events that finish the session.
        let forward = |session: &Session, running: &mut Option<Running>, events: Vec<Event>| {
            if session.is_finished()
                && let Some(r) = running.take()
            {
                r.stop();
            }
            send(events);
        };
        while !session.is_finished() {
            let pending = match cmd.try_recv() {
                Ok(c) => Some(Some(c)),
                Err(crossbeam_channel::TryRecvError::Empty) => None,
                Err(crossbeam_channel::TryRecvError::Disconnected) => Some(None),
            };
            if let Some(c) = pending {
                let events = apply_command(session, &mut stop_requested, c);
                forward(session, &mut running, events);
                continue;
            }
            select! {
                recv(cmd) -> c => {
                    let events = apply_command(session, &mut stop_requested, c.ok());
                    forward(session, &mut running, events);
                },
                recv(cap_rx) -> m => {
                    let mut audio = Vec::new();
                    let mut next = m;
                    let terminal = loop {
                        match next {
                            Ok(CaptureMsg::Audio(a)) => audio.extend_from_slice(&a),
                            Ok(CaptureMsg::Error(msg)) => break Some(Some(msg)),
                            Ok(CaptureMsg::Closed) | Err(_) => break Some(None),
                        }
                        match cap_rx.try_recv() {
                            Ok(m) => next = Ok(m),
                            Err(_) => break None,
                        }
                    };
                    if !audio.is_empty() {
                        let mut events = session.push(&audio);
                        if !session.wants_audio() && !session.is_finished() {
                            events.extend(session.end_of_audio());
                        }
                        forward(session, &mut running, events);
                    }
                    if let Some(err) = terminal {
                        if session.is_finished() {
                            continue;
                        }
                        let events = if stop_requested {
                            session.end_of_audio()
                        } else {
                            fail_and_finish(session, "audio", err.unwrap_or_else(|| "microphone closed".into()))
                        };
                        forward(session, &mut running, events);
                    }
                },
                default(AUDIO_TIMEOUT) => {
                    let events = if stop_requested {
                        session.end_of_audio()
                    } else {
                        fail_and_finish(session, "audio", "no audio for 2 s".into())
                    };
                    forward(session, &mut running, events);
                },
            }
        }
        if let Some(r) = running.take() {
            r.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRecognizer, FakeVad};
    use std::time::Duration;

    fn engine() -> Engine {
        Engine {
            cfg: SessionConfig {
                partial_interval: 8000,
                tail: 4000,
                min_length: 8000,
                max_length: 160_000,
                final_full_max: 480_000,
                final_split_silence: 12_800,
            },
            rec: Arc::new(FakeRecognizer::new(|s| Ok(format!("{}", s.len() / 16000)))),
            final_rec: None,
            vad: Arc::new(|| Ok(Box::new(FakeVad::always_speech()) as Box<dyn Vad>)),
            post: Arc::new(PostProcessor::empty()),
        }
    }

    fn collect(h: &SessionHandle) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(e) = h.events.recv_timeout(Duration::from_secs(10)) {
            let done = matches!(
                e,
                Event::State(State::Done | State::Cancelled | State::Error)
            );
            out.push(e);
            if done {
                break;
            }
        }
        out
    }

    #[test]
    fn samples_source_runs_to_final() {
        let h = engine().start(Source::Samples(vec![0.1; 3 * 16000]));
        let ev = collect(&h);
        assert_eq!(ev.first(), Some(&Event::State(State::Starting)));
        assert!(ev.contains(&Event::State(State::Recording)));
        assert!(ev.iter().any(|e| matches!(e, Event::Partial(_))));
        assert!(ev.contains(&Event::Final("3。".into())), "{ev:?}");
        assert_eq!(ev.last(), Some(&Event::State(State::Done)));
    }

    #[test]
    fn final_recognizer_gives_the_final_text_and_the_main_one_partials() {
        let mut eng = engine();
        let f = Arc::new(FakeRecognizer::new(|s| Ok(format!("F{}", s.len() / 16000))));
        eng.final_rec = Some(f.clone());
        let ev = collect(&eng.start(Source::Samples(vec![0.1; 3 * 16000])));
        assert!(ev.contains(&Event::Final("F3。".into())), "{ev:?}");
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::Partial(t) if !t.starts_with('F'))),
            "{ev:?}"
        );
        assert!(
            !ev.iter()
                .any(|e| matches!(e, Event::Partial(t) if t.starts_with('F'))),
            "{ev:?}"
        );
        assert_eq!(f.calls(), 1);
    }

    #[test]
    fn generator_source_stops_and_cancels() {
        let h = engine().start(Source::Generator { level: 0.1 });
        std::thread::sleep(Duration::from_millis(700));
        h.stop();
        let ev = collect(&h);
        assert!(ev.iter().any(|e| matches!(e, Event::Final(_))), "{ev:?}");
        let h = engine().start(Source::Generator { level: 0.1 });
        std::thread::sleep(Duration::from_millis(300));
        h.cancel();
        assert_eq!(collect(&h).last(), Some(&Event::State(State::Cancelled)));
    }

    #[test]
    fn stall_after_stop_still_delivers_final() {
        let h = engine().start(Source::StallingGenerator {
            level: 0.1,
            chunks: 40,
        });
        std::thread::sleep(Duration::from_millis(1200));
        h.stop();
        let ev = collect(&h);
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::Final(t) if !t.is_empty())),
            "{ev:?}"
        );
        assert_eq!(ev.last(), Some(&Event::State(State::Done)), "{ev:?}");
    }

    #[test]
    fn stop_while_starting_finishes() {
        let h = engine().start(Source::Generator { level: 0.1 });
        h.stop();
        let ev = collect(&h);
        assert!(ev.contains(&Event::Final(String::new())), "{ev:?}");
        assert_eq!(ev.last(), Some(&Event::State(State::Done)), "{ev:?}");
    }

    #[test]
    fn unreachable_sound_server_reports_audio_error() {
        // SAFETY: see the capture tests; PULSE_SERVER is only read by capture threads.
        unsafe { std::env::set_var("PULSE_SERVER", "unix:/nonexistent/koeinput-test") };
        let h = engine().start(Source::Microphone(None));
        let ev = collect(&h);
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::Error { code, .. } if code == "audio")),
            "{ev:?}"
        );
        assert_eq!(ev.last(), Some(&Event::State(State::Error)));
    }

    #[test]
    fn missing_file_reports_audio_error() {
        let h = engine().start(Source::File("/nonexistent.wav".into()));
        let ev = collect(&h);
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::Error { code, .. } if code == "audio"))
        );
        assert_eq!(ev.last(), Some(&Event::State(State::Error)));
    }

    #[test]
    fn slow_recognizer_still_covers_all_audio() {
        let eng = Engine {
            cfg: SessionConfig {
                partial_interval: 1600,
                tail: 0,
                min_length: 1600,
                max_length: 1_600_000,
                final_full_max: 480_000,
                final_split_silence: 12_800,
            },
            rec: Arc::new(FakeRecognizer::new(|s| {
                std::thread::sleep(Duration::from_millis(300));
                Ok(s.len().to_string())
            })),
            final_rec: None,
            vad: Arc::new(|| Ok(Box::new(FakeVad::always_speech()) as Box<dyn Vad>)),
            post: Arc::new(PostProcessor::empty()),
        };
        let h = eng.start(Source::Generator { level: 0.1 });
        std::thread::sleep(Duration::from_millis(2500));
        h.stop();
        let ev = collect(&h);
        let n = ev
            .iter()
            .find_map(|e| match e {
                Event::Final(t) => t.strip_suffix('。')?.parse::<usize>().ok(),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no final: {ev:?}"));
        assert!(n >= 24_000, "final covered only {n} samples");
    }

    #[test]
    fn vad_factory_failure_reports_model_error() {
        let mut eng = engine();
        eng.vad = Arc::new(|| Err("no vad model".to_string()));
        let h = eng.start(Source::Samples(vec![0.1; 16000]));
        let ev = collect(&h);
        assert_eq!(ev.first(), Some(&Event::State(State::Starting)));
        assert!(
            ev.iter().any(|e| matches!(
                e,
                Event::Error { code, message } if code == "model" && message == "no vad model"
            )),
            "{ev:?}"
        );
        assert_eq!(ev.last(), Some(&Event::State(State::Error)));
        assert!(!ev.iter().any(|e| matches!(e, Event::Final(_))));
    }

    #[test]
    fn no_audio_for_two_seconds_reports_audio_error() {
        let started = std::time::Instant::now();
        let h = engine().start(Source::StallingGenerator {
            level: 0.1,
            chunks: 5,
        });
        let ev = collect(&h);
        let elapsed = started.elapsed();
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::Error { code, .. } if code == "audio")),
            "{ev:?}"
        );
        assert_eq!(ev.last(), Some(&Event::State(State::Error)), "{ev:?}");
        assert!(
            elapsed >= Duration::from_millis(1900) && elapsed < Duration::from_secs(4),
            "{elapsed:?}"
        );
    }
}
