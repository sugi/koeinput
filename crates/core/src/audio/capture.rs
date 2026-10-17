use crossbeam_channel::{Receiver, Sender};
use libpulse_binding::{
    def::BufferAttr,
    sample::{Format, Spec},
    stream::Direction,
};
use libpulse_simple_binding::Simple;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

const CHUNK_BYTES: usize = 1280;
const STOP_WAIT: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub enum CaptureMsg {
    Audio(Vec<f32>),
    Error(String),
    Closed,
}

pub struct Capture {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    done: Receiver<()>,
}

impl Capture {
    /// `tx` should be unbounded: a full bounded channel would block the capture thread.
    pub fn start(source: Option<String>, tx: Sender<CaptureMsg>) -> Capture {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let (done_tx, done) = crossbeam_channel::bounded(1);
        let handle = std::thread::Builder::new()
            .name("koeinput-capture".into())
            .spawn(move || {
                run(source, tx, flag);
                let _ = done_tx.send(());
            })
            .expect("spawn capture thread");
        Capture {
            stop,
            handle: Some(handle),
            done,
        }
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            join_or_detach(h, &self.done, STOP_WAIT);
        }
    }
}

/// A thread stuck in a blocking PulseAudio call cannot see the stop flag; detach it
/// instead of hanging the caller. It exits on its own after its next read or failed send.
fn join_or_detach(handle: JoinHandle<()>, done: &Receiver<()>, wait: Duration) {
    if done.recv_timeout(wait).is_ok() {
        let _ = handle.join();
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(source: Option<String>, tx: Sender<CaptureMsg>, stop: Arc<AtomicBool>) {
    let spec = Spec {
        format: Format::F32le,
        channels: 1,
        rate: crate::SAMPLE_RATE,
    };
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: CHUNK_BYTES as u32,
    };
    let s = match Simple::new(
        None,
        "KoeInput",
        Direction::Record,
        source.as_deref(),
        "Dictation",
        &spec,
        None,
        Some(&attr),
    ) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(CaptureMsg::Error(format!("cannot open microphone: {e}")));
            return;
        }
    };
    let mut buf = [0u8; CHUNK_BYTES];
    while !stop.load(Ordering::SeqCst) {
        if let Err(e) = s.read(&mut buf) {
            let _ = tx.send(CaptureMsg::Error(format!("microphone read failed: {e}")));
            break;
        }
        let samples = buf
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        if tx.send(CaptureMsg::Audio(samples)).is_err() {
            break;
        }
    }
    let _ = tx.send(CaptureMsg::Closed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn shutdown_does_not_hang_on_stalled_thread() {
        let (done_tx, done) = crossbeam_channel::bounded(1);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            let _ = done_tx.send(());
        });
        let start = std::time::Instant::now();
        join_or_detach(handle, &done, Duration::from_millis(100));
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn unreachable_server_reports_error() {
        // SAFETY: tests in this module do not run concurrently with other env readers of PULSE_SERVER.
        unsafe { std::env::set_var("PULSE_SERVER", "unix:/nonexistent/koeinput-test") };
        let (tx, rx) = crossbeam_channel::unbounded();
        let c = Capture::start(None, tx);
        let msg = rx.recv_timeout(Duration::from_secs(5)).expect("a message");
        assert!(matches!(msg, CaptureMsg::Error(_)), "{msg:?}");
        c.stop();
    }

    #[test]
    fn records_from_default_source_when_enabled() {
        if std::env::var("KOEINPUT_AUDIO_TESTS").as_deref() != Ok("1") {
            return;
        }
        let (tx, rx) = crossbeam_channel::unbounded();
        let c = Capture::start(None, tx);
        let mut chunks = 0;
        while chunks < 10 {
            match rx.recv_timeout(Duration::from_secs(3)).expect("audio") {
                CaptureMsg::Audio(a) => {
                    assert_eq!(a.len(), 320);
                    chunks += 1;
                }
                other => panic!("{other:?}"),
            }
        }
        c.stop();
    }
}
