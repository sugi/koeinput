use crate::SAMPLE_RATE;
use crate::config::Config;
use crate::postprocess::{PostProcessor, join_segments};
use crate::recognizer::{Recognizer, RecognizerError};
use crate::vad::{Segment, Vad};
use std::sync::Arc;

const PAD: usize = 1600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Recording,
    Finalizing,
    Done,
    Cancelled,
    Error,
}

impl State {
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Recording => "recording",
            State::Finalizing => "finalizing",
            State::Done => "done",
            State::Cancelled => "cancelled",
            State::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    State(State),
    Partial(String),
    Final(String),
    Error { code: String, message: String },
}

#[derive(Debug, Clone, Copy)]
pub struct SessionConfig {
    pub partial_interval: usize,
    pub tail: usize,
    pub min_length: usize,
    pub max_length: usize,
}

impl SessionConfig {
    pub fn from_config(c: &Config) -> Self {
        let ms = |v: u64| (v as usize) * SAMPLE_RATE as usize / 1000;
        Self {
            partial_interval: ms(c.partial_interval_ms),
            tail: ms(c.tail_ms),
            min_length: ms(c.min_length_ms),
            max_length: ms(c.max_length_ms),
        }
    }
}

pub struct Session {
    cfg: SessionConfig,
    rec: Arc<dyn Recognizer>,
    vad: Box<dyn Vad>,
    post: Arc<PostProcessor>,
    audio: Vec<f32>,
    fixed: Vec<String>,
    last_partial: String,
    next_tick: usize,
    stop_at: Option<usize>,
    state: State,
}

impl Session {
    pub fn new(cfg: SessionConfig, rec: Arc<dyn Recognizer>, vad: Box<dyn Vad>, post: Arc<PostProcessor>) -> Self {
        Self {
            cfg,
            rec,
            vad,
            post,
            audio: Vec::new(),
            fixed: Vec::new(),
            last_partial: String::new(),
            next_tick: cfg.partial_interval,
            stop_at: None,
            state: State::Starting,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.state, State::Done | State::Cancelled | State::Error)
    }

    pub fn wants_audio(&self) -> bool {
        !self.is_finished() && self.stop_at.is_none_or(|t| self.audio.len() < t)
    }

    pub fn push(&mut self, samples: &[f32]) -> Vec<Event> {
        let mut ev = Vec::new();
        if self.is_finished() || samples.is_empty() {
            return ev;
        }
        if self.state == State::Starting {
            self.state = State::Recording;
            ev.push(Event::State(State::Recording));
        }
        self.audio.extend_from_slice(samples);
        self.vad.accept(samples);
        if let Err(e) = self.fix_finished() {
            return self.fail(ev, e);
        }
        if self.audio.len() >= self.cfg.max_length {
            if self.stop_at.is_none() {
                self.state = State::Finalizing;
                ev.push(Event::State(State::Finalizing));
            }
            ev.extend(self.finalize());
            return ev;
        }
        if let Some(t) = self.stop_at {
            if self.audio.len() >= t {
                ev.extend(self.finalize());
            }
            return ev;
        }
        if self.audio.len() >= self.next_tick {
            self.next_tick = self.audio.len() + self.cfg.partial_interval;
            match self.partial_text() {
                Ok(text) if !text.is_empty() && text != self.last_partial => {
                    self.last_partial = text.clone();
                    ev.push(Event::Partial(text));
                }
                Ok(_) => {}
                Err(e) => return self.fail(ev, e),
            }
        }
        ev
    }

    pub fn stop(&mut self) -> Vec<Event> {
        if self.is_finished() || self.stop_at.is_some() {
            return Vec::new();
        }
        self.state = State::Finalizing;
        let mut ev = vec![Event::State(State::Finalizing)];
        if self.audio.is_empty() {
            ev.extend(self.finalize());
        } else {
            self.stop_at = Some(self.audio.len() + self.cfg.tail);
        }
        ev
    }

    pub fn end_of_audio(&mut self) -> Vec<Event> {
        if self.is_finished() {
            return Vec::new();
        }
        let mut ev = Vec::new();
        if self.state != State::Finalizing {
            self.state = State::Finalizing;
            ev.push(Event::State(State::Finalizing));
        }
        ev.extend(self.finalize());
        ev
    }

    pub fn cancel(&mut self) -> Vec<Event> {
        if self.is_finished() {
            return Vec::new();
        }
        self.state = State::Cancelled;
        vec![Event::State(State::Cancelled)]
    }

    pub fn abort(&mut self) {
        self.state = State::Error;
    }

    fn decode(&self, seg: Segment) -> Result<String, RecognizerError> {
        let s = seg.start.saturating_sub(PAD);
        let e = (seg.end + PAD).min(self.audio.len());
        self.rec.decode(&self.audio[s..e])
    }

    fn fix_finished(&mut self) -> Result<(), RecognizerError> {
        for seg in self.vad.pop_finished() {
            let t = self.decode(seg)?;
            self.fixed.push(t);
        }
        Ok(())
    }

    fn partial_text(&self) -> Result<String, RecognizerError> {
        let mut parts = self.fixed.clone();
        if let Some(start) = self.vad.speech_start() {
            parts.push(self.decode(Segment { start, end: self.audio.len() })?);
        }
        Ok(self.post.apply(&join_segments(&parts)))
    }

    fn finalize(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        for seg in self.vad.finish() {
            match self.decode(seg) {
                Ok(t) => self.fixed.push(t),
                Err(e) => return self.fail(ev, e),
            }
        }
        let text = if self.audio.len() < self.cfg.min_length {
            String::new()
        } else {
            self.post.apply(&join_segments(&self.fixed))
        };
        self.state = State::Done;
        ev.push(Event::Final(text));
        ev.push(Event::State(State::Done));
        ev
    }

    fn fail(&mut self, mut ev: Vec<Event>, e: RecognizerError) -> Vec<Event> {
        self.state = State::Error;
        ev.push(Event::Error { code: "model".into(), message: e.to_string() });
        ev.push(Event::State(State::Error));
        ev
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeRecognizer, FakeVad};
    use crate::vad::Segment;

    const S: usize = 16000;

    fn cfg() -> SessionConfig {
        SessionConfig { partial_interval: S / 2, tail: S / 4, min_length: S / 2, max_length: 10 * S }
    }

    // Each test segment is filled with a distinct level; the fake maps the mean level to a word.
    fn rec() -> Arc<FakeRecognizer> {
        Arc::new(FakeRecognizer::new(|s| {
            let mean = s.iter().sum::<f32>() / s.len().max(1) as f32;
            Ok(if mean > 0.15 { "二".into() } else if mean > 0.05 { "一".into() } else { String::new() })
        }))
    }

    fn audio(parts: &[(usize, f32)]) -> Vec<f32> {
        parts.iter().flat_map(|(n, v)| std::iter::repeat_n(*v, *n)).collect()
    }

    fn session(r: Arc<FakeRecognizer>, segs: Vec<Segment>) -> Session {
        Session::new(cfg(), r, Box::new(FakeVad::new(segs, S / 4)), Arc::new(PostProcessor::empty()))
    }

    fn finals(ev: &[Event]) -> Vec<String> {
        ev.iter().filter_map(|e| if let Event::Final(t) = e { Some(t.clone()) } else { None }).collect()
    }

    #[test]
    fn first_audio_emits_recording_once() {
        let mut s = session(rec(), vec![]);
        assert_eq!(s.push(&[0.0; 320]), vec![Event::State(State::Recording)]);
        assert!(!s.push(&[0.0; 320]).contains(&Event::State(State::Recording)));
    }

    #[test]
    fn partials_follow_audio_time_and_never_queue() {
        let r = rec();
        let mut s = session(r.clone(), vec![Segment { start: 0, end: 4 * S }]);
        let mut ev = Vec::new();
        for chunk in audio(&[(4 * S, 0.1)]).chunks(320).take(S / 320) {
            ev.extend(s.push(chunk));
        }
        let before = r.calls();
        assert!(before <= 3, "{before} decodes in 1 s of audio");
        s.push(&audio(&[(2 * S, 0.1)]));
        assert_eq!(r.calls(), before + 1);
        assert!(ev.contains(&Event::Partial("一".into())));
    }

    #[test]
    fn finished_segments_are_fixed_and_ordered() {
        let mut s = session(rec(), vec![Segment { start: 0, end: S }, Segment { start: 2 * S, end: 3 * S }]);
        let mut ev = s.push(&audio(&[(S, 0.1), (S, 0.0), (S, 0.2), (S, 0.0)]));
        ev.extend(s.end_of_audio());
        assert_eq!(finals(&ev), vec!["一二".to_string()]);
        assert_eq!(ev.last(), Some(&Event::State(State::Done)));
    }

    #[test]
    fn stop_waits_for_tail() {
        let mut s = session(rec(), vec![Segment { start: 0, end: 2 * S }]);
        s.push(&audio(&[(S, 0.1)]));
        let ev = s.stop();
        assert_eq!(ev, vec![Event::State(State::Finalizing)]);
        assert!(s.wants_audio());
        assert!(finals(&s.push(&audio(&[(S / 8, 0.1)]))).is_empty());
        let ev = s.push(&audio(&[(S / 8, 0.1)]));
        assert_eq!(finals(&ev), vec!["一".to_string()]);
        assert!(!s.wants_audio());
        assert!(s.is_finished());
    }

    #[test]
    fn stop_before_audio_gives_empty_final() {
        let mut s = session(rec(), vec![]);
        let ev = s.stop();
        assert_eq!(ev, vec![Event::State(State::Finalizing), Event::Final(String::new()), Event::State(State::Done)]);
        assert!(s.is_finished());
        assert!(s.push(&[0.1; 320]).is_empty());
    }

    #[test]
    fn short_or_silent_recordings_give_empty_final() {
        let mut s = session(rec(), vec![Segment { start: 0, end: S / 4 }]);
        s.push(&audio(&[(S / 4, 0.1)]));
        assert_eq!(finals(&s.end_of_audio()), vec![String::new()]);
        let mut s = session(rec(), vec![]);
        s.push(&audio(&[(2 * S, 0.0)]));
        assert_eq!(finals(&s.end_of_audio()), vec![String::new()]);
    }

    #[test]
    fn cancel_emits_cancelled_and_no_final() {
        let mut s = session(rec(), vec![Segment { start: 0, end: 2 * S }]);
        s.push(&audio(&[(S, 0.1)]));
        assert_eq!(s.cancel(), vec![Event::State(State::Cancelled)]);
        assert!(s.end_of_audio().is_empty());
        assert!(s.is_finished());
    }

    #[test]
    fn max_length_finalizes_automatically() {
        let mut s = Session::new(
            SessionConfig { max_length: 2 * S, ..cfg() },
            rec(),
            Box::new(FakeVad::always_speech()),
            Arc::new(PostProcessor::empty()),
        );
        let ev = s.push(&audio(&[(3 * S, 0.1)]));
        assert_eq!(finals(&ev), vec!["一".to_string()]);
        assert!(s.is_finished());
    }

    #[test]
    fn recognizer_error_ends_session_with_error() {
        let r = Arc::new(FakeRecognizer::new(|_| Err(RecognizerError::Decode("boom".into()))));
        let mut s = session(r, vec![Segment { start: 0, end: S }]);
        let ev = s.push(&audio(&[(2 * S, 0.1)]));
        assert!(ev.iter().any(|e| matches!(e, Event::Error { code, .. } if code == "model")));
        assert_eq!(ev.last(), Some(&Event::State(State::Error)));
        assert!(s.is_finished());
    }

    #[test]
    fn abort_marks_finished_without_events() {
        let mut s = session(rec(), vec![]);
        s.push(&[0.1; 320]);
        s.abort();
        assert!(s.is_finished());
        assert_eq!(s.state(), State::Error);
        assert!(s.end_of_audio().is_empty());
    }

    #[test]
    fn final_text_is_post_processed() {
        let post = Arc::new(PostProcessor::new(vec![("一".into(), "壱".into())]));
        let mut s = Session::new(cfg(), rec(), Box::new(FakeVad::new(vec![Segment { start: 0, end: S }], S / 4)), post);
        s.push(&audio(&[(2 * S, 0.1)]));
        assert_eq!(finals(&s.end_of_audio()), vec!["壱".to_string()]);
    }
}
