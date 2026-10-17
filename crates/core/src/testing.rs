use crate::recognizer::{Recognizer, RecognizerError};
use crate::vad::{Segment, Vad};
use std::sync::atomic::{AtomicUsize, Ordering};

type DecodeFn = dyn Fn(&[f32]) -> Result<String, RecognizerError> + Send + Sync;

pub struct FakeRecognizer {
    f: Box<DecodeFn>,
    calls: AtomicUsize,
}

impl FakeRecognizer {
    pub fn new(
        f: impl Fn(&[f32]) -> Result<String, RecognizerError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            f: Box::new(f),
            calls: AtomicUsize::new(0),
        }
    }
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Recognizer for FakeRecognizer {
    fn decode(&self, samples: &[f32]) -> Result<String, RecognizerError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        (self.f)(samples)
    }
}

pub struct FakeVad {
    segments: Vec<Segment>,
    min_silence: usize,
    fed: usize,
    returned: usize,
    always: bool,
}

impl FakeVad {
    pub fn new(segments: Vec<Segment>, min_silence: usize) -> Self {
        Self {
            segments,
            min_silence,
            fed: 0,
            returned: 0,
            always: false,
        }
    }
    pub fn always_speech() -> Self {
        Self {
            segments: Vec::new(),
            min_silence: 0,
            fed: 0,
            returned: 0,
            always: true,
        }
    }
}

impl Vad for FakeVad {
    fn accept(&mut self, samples: &[f32]) {
        self.fed += samples.len();
    }
    fn pop_finished(&mut self) -> Vec<Segment> {
        let mut out = Vec::new();
        while let Some(s) = self.segments.get(self.returned) {
            if s.end + self.min_silence <= self.fed {
                out.push(*s);
                self.returned += 1;
            } else {
                break;
            }
        }
        out
    }
    fn speech_start(&self) -> Option<usize> {
        if self.always {
            return (self.fed > 0).then_some(0);
        }
        self.segments
            .get(self.returned)
            .filter(|s| s.start < self.fed)
            .map(|s| s.start)
    }
    fn finish(&mut self) -> Vec<Segment> {
        if self.always {
            return if self.fed > 0 {
                vec![Segment {
                    start: 0,
                    end: self.fed,
                }]
            } else {
                Vec::new()
            };
        }
        let mut out = Vec::new();
        while let Some(s) = self.segments.get(self.returned) {
            if s.start >= self.fed {
                break;
            }
            out.push(Segment {
                start: s.start,
                end: s.end.min(self.fed),
            });
            self.returned += 1;
        }
        out
    }
}
